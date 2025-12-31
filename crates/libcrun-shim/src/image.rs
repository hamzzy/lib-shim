//! OCI Image handling
//!
//! This module provides functionality for pulling and managing OCI images.

use crate::types::{ImageInfo, ImageReference, PullProgress};
use crate::error::{Result, ShimError};
use std::path::{Path, PathBuf};
use std::collections::HashMap;

/// Image store for managing pulled images
pub struct ImageStore {
    /// Root directory for image storage
    root: PathBuf,
    /// Cached image list
    images: HashMap<String, ImageInfo>,
}

impl ImageStore {
    /// Create a new image store
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|e| {
            ShimError::runtime_with_context(
                format!("Failed to create image store directory: {}", e),
                format!("Path: {}", root.display()),
            )
        })?;

        Ok(Self {
            root,
            images: HashMap::new(),
        })
    }

    /// Get the default image store path
    pub fn default_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("/var/lib"))
            .join("libcrun-shim")
            .join("images")
    }

    /// Pull an image from a registry
    pub async fn pull(
        &mut self,
        reference: &str,
        progress_callback: Option<Box<dyn Fn(PullProgress) + Send>>,
    ) -> Result<ImageInfo> {
        let image_ref = ImageReference::parse(reference).ok_or_else(|| {
            ShimError::validation("reference", format!("Invalid image reference: {}", reference))
        })?;

        log::info!("Pulling image: {}", image_ref.full_name());

        // Notify progress
        if let Some(ref cb) = progress_callback {
            cb(PullProgress {
                current_layer: String::new(),
                total_layers: 0,
                completed_layers: 0,
                downloaded_bytes: 0,
                total_bytes: 0,
                status: format!("Pulling from {}", image_ref.registry),
            });
        }

        // Build registry URL
        let registry_url = get_registry_url(&image_ref.registry);

        // Get auth token (for Docker Hub)
        let token = if image_ref.registry == "docker.io" {
            get_docker_hub_token(&image_ref.repository).await?
        } else {
            None
        };

        // Fetch manifest
        let manifest = fetch_manifest(&registry_url, &image_ref.repository, &image_ref.reference, token.as_deref()).await?;

        // Parse manifest and get layers
        let (config_digest, layer_digests) = parse_manifest(&manifest)?;

        if let Some(ref cb) = progress_callback {
            cb(PullProgress {
                current_layer: String::new(),
                total_layers: layer_digests.len() as u32,
                completed_layers: 0,
                downloaded_bytes: 0,
                total_bytes: 0,
                status: format!("Found {} layers", layer_digests.len()),
            });
        }

        // Create image directory
        let image_id = &config_digest[7..19]; // Short ID from digest
        let image_dir = self.root.join(image_id);
        std::fs::create_dir_all(&image_dir)?;

        // Download layers
        let mut total_size: u64 = 0;
        for (i, layer_digest) in layer_digests.iter().enumerate() {
            if let Some(ref cb) = progress_callback {
                cb(PullProgress {
                    current_layer: layer_digest.clone(),
                    total_layers: layer_digests.len() as u32,
                    completed_layers: i as u32,
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    status: format!("Pulling layer {}/{}", i + 1, layer_digests.len()),
                });
            }

            let layer_path = image_dir.join(format!("layer_{}.tar.gz", i));
            if !layer_path.exists() {
                let size = download_blob(&registry_url, &image_ref.repository, layer_digest, &layer_path, token.as_deref()).await?;
                total_size += size;
            }
        }

        // Download config
        let config_path = image_dir.join("config.json");
        if !config_path.exists() {
            download_blob(&registry_url, &image_ref.repository, &config_digest, &config_path, token.as_deref()).await?;
        }

        // Parse config for image info
        let config_content = std::fs::read_to_string(&config_path)?;
        let config: serde_json::Value = serde_json::from_str(&config_content)?;

        let architecture = config["architecture"].as_str().unwrap_or("amd64").to_string();
        let os = config["os"].as_str().unwrap_or("linux").to_string();
        let created = config["created"].as_str()
            .and_then(|s| chrono_parse_timestamp(s))
            .unwrap_or(0);

        let labels = config["config"]["Labels"]
            .as_object()
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        // Extract layers to rootfs
        let rootfs_path = image_dir.join("rootfs");
        std::fs::create_dir_all(&rootfs_path)?;

        if let Some(ref cb) = progress_callback {
            cb(PullProgress {
                current_layer: String::new(),
                total_layers: layer_digests.len() as u32,
                completed_layers: layer_digests.len() as u32,
                downloaded_bytes: total_size,
                total_bytes: total_size,
                status: "Extracting layers".to_string(),
            });
        }

        for i in 0..layer_digests.len() {
            let layer_path = image_dir.join(format!("layer_{}.tar.gz", i));
            extract_layer(&layer_path, &rootfs_path)?;
        }

        let info = ImageInfo {
            reference: image_ref.clone(),
            id: image_id.to_string(),
            size: total_size,
            created,
            architecture,
            os,
            labels,
        };

        self.images.insert(image_id.to_string(), info.clone());

        if let Some(ref cb) = progress_callback {
            cb(PullProgress {
                current_layer: String::new(),
                total_layers: layer_digests.len() as u32,
                completed_layers: layer_digests.len() as u32,
                downloaded_bytes: total_size,
                total_bytes: total_size,
                status: "Pull complete".to_string(),
            });
        }

        log::info!("Image pulled successfully: {} ({})", image_ref.full_name(), image_id);

        Ok(info)
    }

    /// Get the rootfs path for an image
    pub fn get_rootfs(&self, image_id: &str) -> Option<PathBuf> {
        let rootfs_path = self.root.join(image_id).join("rootfs");
        if rootfs_path.exists() {
            Some(rootfs_path)
        } else {
            None
        }
    }

    /// List all images
    pub fn list(&self) -> Vec<ImageInfo> {
        self.images.values().cloned().collect()
    }

    /// Remove an image
    pub fn remove(&mut self, image_id: &str) -> Result<()> {
        let image_dir = self.root.join(image_id);
        if image_dir.exists() {
            std::fs::remove_dir_all(&image_dir)?;
        }
        self.images.remove(image_id);
        Ok(())
    }
}

fn get_registry_url(registry: &str) -> String {
    match registry {
        "docker.io" => "https://registry-1.docker.io".to_string(),
        r if r.starts_with("http://") || r.starts_with("https://") => r.to_string(),
        r => format!("https://{}", r),
    }
}

async fn get_docker_hub_token(repository: &str) -> Result<Option<String>> {
    let url = format!(
        "https://auth.docker.io/token?service=registry.docker.io&scope=repository:{}:pull",
        repository
    );

    // Simple HTTP GET using std (no external HTTP client dependency)
    // In production, you'd use reqwest or similar
    log::debug!("Fetching Docker Hub token for {}", repository);

    // For now, return None - anonymous pull
    // Full implementation would use an HTTP client
    Ok(None)
}

async fn fetch_manifest(
    registry_url: &str,
    repository: &str,
    reference: &str,
    _token: Option<&str>,
) -> Result<String> {
    let url = format!("{}/v2/{}/manifests/{}", registry_url, repository, reference);
    log::debug!("Fetching manifest from: {}", url);

    // Placeholder - would use HTTP client in production
    Err(ShimError::runtime_with_context(
        "Image pull not fully implemented",
        "Use 'podman pull' or 'docker pull' to download images, then extract rootfs manually",
    ))
}

fn parse_manifest(manifest: &str) -> Result<(String, Vec<String>)> {
    let v: serde_json::Value = serde_json::from_str(manifest)?;

    // Handle both Docker manifest v2 and OCI manifest
    let config_digest = v["config"]["digest"]
        .as_str()
        .ok_or_else(|| ShimError::runtime("Missing config digest in manifest"))?
        .to_string();

    let layers: Vec<String> = v["layers"]
        .as_array()
        .ok_or_else(|| ShimError::runtime("Missing layers in manifest"))?
        .iter()
        .filter_map(|l| l["digest"].as_str().map(String::from))
        .collect();

    Ok((config_digest, layers))
}

async fn download_blob(
    _registry_url: &str,
    _repository: &str,
    _digest: &str,
    _path: &Path,
    _token: Option<&str>,
) -> Result<u64> {
    // Placeholder - would use HTTP client in production
    Err(ShimError::runtime("Blob download not implemented"))
}

fn extract_layer(layer_path: &Path, rootfs_path: &Path) -> Result<()> {
    use std::process::Command;

    // Use tar to extract the layer
    let status = Command::new("tar")
        .args(&[
            "-xzf",
            layer_path.to_str().unwrap(),
            "-C",
            rootfs_path.to_str().unwrap(),
        ])
        .status()
        .map_err(|e| ShimError::runtime_with_context(
            format!("Failed to extract layer: {}", e),
            "tar command may not be available",
        ))?;

    if !status.success() {
        return Err(ShimError::runtime("Layer extraction failed"));
    }

    Ok(())
}

fn chrono_parse_timestamp(s: &str) -> Option<u64> {
    // Simple RFC3339 parsing
    // In production, use chrono crate
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_reference_parse() {
        let ref1 = ImageReference::parse("alpine").unwrap();
        assert_eq!(ref1.registry, "docker.io");
        assert_eq!(ref1.repository, "library/alpine");
        assert_eq!(ref1.reference, "latest");

        let ref2 = ImageReference::parse("alpine:3.18").unwrap();
        assert_eq!(ref2.registry, "docker.io");
        assert_eq!(ref2.repository, "library/alpine");
        assert_eq!(ref2.reference, "3.18");

        let ref3 = ImageReference::parse("ghcr.io/user/repo:v1.0").unwrap();
        assert_eq!(ref3.registry, "ghcr.io");
        assert_eq!(ref3.repository, "user/repo");
        assert_eq!(ref3.reference, "v1.0");

        let ref4 = ImageReference::parse("nginx").unwrap();
        assert_eq!(ref4.registry, "docker.io");
        assert_eq!(ref4.repository, "library/nginx");
        assert_eq!(ref4.reference, "latest");
    }
}

