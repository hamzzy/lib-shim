//! OCI Image handling
//!
//! This module provides functionality for pulling and managing OCI images.

use crate::error::{Result, ShimError};
use crate::types::{ImageInfo, ImageReference, PullProgress};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[cfg(feature = "image-pull")]
use futures_util::StreamExt;
#[cfg(feature = "image-pull")]
use sha2::{Digest, Sha256};

/// Image store for managing pulled images
pub struct ImageStore {
    /// Root directory for image storage
    root: PathBuf,
    /// Cached image list
    images: HashMap<String, ImageInfo>,
    /// HTTP client for registry requests
    #[cfg(feature = "image-pull")]
    client: reqwest::Client,
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

        // Load existing images
        let images = Self::scan_images(&root);

        Ok(Self {
            root,
            images,
            #[cfg(feature = "image-pull")]
            client: reqwest::Client::builder()
                .user_agent("libcrun-shim/0.1.0")
                .build()
                .map_err(|e| ShimError::runtime(format!("Failed to create HTTP client: {}", e)))?,
        })
    }

    /// Get the default image store path
    pub fn default_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("/var/lib"))
            .join("libcrun-shim")
            .join("images")
    }

    /// Scan existing images in the store
    fn scan_images(root: &Path) -> HashMap<String, ImageInfo> {
        let mut images = HashMap::new();

        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() {
                    let config_path = path.join("config.json");
                    if config_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(&config_path) {
                            if let Ok(info) = serde_json::from_str::<ImageInfo>(&content) {
                                images.insert(info.id.clone(), info);
                            }
                        }
                    }
                }
            }
        }

        images
    }

    /// Pull an image from a registry
    #[cfg(feature = "image-pull")]
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

        // Get auth token
        let token = self.get_auth_token(&image_ref).await?;

        // Fetch manifest
        let manifest = self
            .fetch_manifest(&image_ref, token.as_deref())
            .await?;

        // Parse manifest
        let (config_digest, layer_digests, total_size) = self.parse_manifest(&manifest)?;

        if let Some(ref cb) = progress_callback {
            cb(PullProgress {
                current_layer: String::new(),
                total_layers: layer_digests.len() as u32,
                completed_layers: 0,
                downloaded_bytes: 0,
                total_bytes: total_size,
                status: format!("Found {} layers", layer_digests.len()),
            });
        }

        // Create image directory using short ID
        let image_id = if config_digest.starts_with("sha256:") {
            config_digest[7..19].to_string()
        } else {
            config_digest[..12].to_string()
        };

        let image_dir = self.root.join(&image_id);
        std::fs::create_dir_all(&image_dir)?;

        // Download config blob
        let config_path = image_dir.join("config.json");
        if !config_path.exists() {
            self.download_blob(&image_ref, &config_digest, &config_path, token.as_deref())
                .await?;
        }

        // Download layers
        let mut downloaded_bytes: u64 = 0;
        for (i, (layer_digest, layer_size)) in layer_digests.iter().enumerate() {
            let layer_filename = layer_digest.replace("sha256:", "");
            let layer_path = image_dir.join(format!("{}.tar.gz", &layer_filename[..12]));

            if let Some(ref cb) = progress_callback {
                cb(PullProgress {
                    current_layer: layer_digest.clone(),
                    total_layers: layer_digests.len() as u32,
                    completed_layers: i as u32,
                    downloaded_bytes,
                    total_bytes: total_size,
                    status: format!("Downloading layer {}/{}", i + 1, layer_digests.len()),
                });
            }

            if !layer_path.exists() {
                self.download_blob_with_progress(
                    &image_ref,
                    layer_digest,
                    &layer_path,
                    token.as_deref(),
                    *layer_size,
                    &progress_callback,
                    downloaded_bytes,
                    total_size,
                )
                .await?;
            }

            downloaded_bytes += layer_size;
        }

        // Extract layers to rootfs
        let rootfs_path = image_dir.join("rootfs");
        if !rootfs_path.exists() {
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

            for (layer_digest, _) in &layer_digests {
                let layer_filename = layer_digest.replace("sha256:", "");
                let layer_path = image_dir.join(format!("{}.tar.gz", &layer_filename[..12]));
                self.extract_layer(&layer_path, &rootfs_path)?;
            }
        }

        // Parse config for image metadata
        let config_content = std::fs::read_to_string(&config_path)?;
        let config: serde_json::Value = serde_json::from_str(&config_content)?;

        let architecture = config["architecture"]
            .as_str()
            .unwrap_or("amd64")
            .to_string();
        let os = config["os"].as_str().unwrap_or("linux").to_string();
        let created = config["created"]
            .as_str()
            .and_then(parse_rfc3339_timestamp)
            .unwrap_or(0);

        let labels = config["config"]["Labels"]
            .as_object()
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let info = ImageInfo {
            reference: image_ref.clone(),
            id: image_id.clone(),
            size: total_size,
            created,
            architecture,
            os,
            labels,
        };

        // Save image info
        let info_path = image_dir.join("image_info.json");
        std::fs::write(&info_path, serde_json::to_string_pretty(&info)?)?;

        self.images.insert(image_id.clone(), info.clone());

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

        log::info!(
            "Image pulled successfully: {} ({})",
            image_ref.full_name(),
            image_id
        );

        Ok(info)
    }

    /// Pull without image-pull feature (stub)
    #[cfg(not(feature = "image-pull"))]
    pub async fn pull(
        &mut self,
        reference: &str,
        _progress_callback: Option<Box<dyn Fn(PullProgress) + Send>>,
    ) -> Result<ImageInfo> {
        Err(ShimError::runtime_with_context(
            "Image pull not available",
            format!(
                "Compile with 'image-pull' feature to enable. Reference: {}",
                reference
            ),
        ))
    }

    #[cfg(feature = "image-pull")]
    async fn get_auth_token(&self, image_ref: &ImageReference) -> Result<Option<String>> {
        if image_ref.registry == "docker.io" {
            // Docker Hub uses token-based auth
            let url = format!(
                "https://auth.docker.io/token?service=registry.docker.io&scope=repository:{}:pull",
                image_ref.repository
            );

            let response = self
                .client
                .get(&url)
                .send()
                .await
                .map_err(|e| ShimError::runtime(format!("Auth request failed: {}", e)))?;

            if response.status().is_success() {
                let json: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| ShimError::runtime(format!("Failed to parse auth response: {}", e)))?;

                if let Some(token) = json["token"].as_str() {
                    return Ok(Some(token.to_string()));
                }
            }
        }

        Ok(None)
    }

    #[cfg(feature = "image-pull")]
    async fn fetch_manifest(
        &self,
        image_ref: &ImageReference,
        token: Option<&str>,
    ) -> Result<serde_json::Value> {
        let registry_url = get_registry_url(&image_ref.registry);
        let url = format!(
            "{}/v2/{}/manifests/{}",
            registry_url, image_ref.repository, image_ref.reference
        );

        let mut request = self.client.get(&url).header(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json, application/vnd.oci.image.manifest.v1+json",
        );

        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .await
            .map_err(|e| ShimError::runtime(format!("Manifest request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ShimError::runtime(format!(
                "Failed to fetch manifest: HTTP {}",
                response.status()
            )));
        }

        response
            .json()
            .await
            .map_err(|e| ShimError::runtime(format!("Failed to parse manifest: {}", e)))
    }

    #[cfg(feature = "image-pull")]
    fn parse_manifest(
        &self,
        manifest: &serde_json::Value,
    ) -> Result<(String, Vec<(String, u64)>, u64)> {
        // Handle manifest list (multi-arch)
        if manifest["manifests"].is_array() {
            // For now, just pick the first linux/amd64 or linux/arm64 manifest
            let manifests = manifest["manifests"].as_array().unwrap();
            for m in manifests {
                let platform = &m["platform"];
                let os = platform["os"].as_str().unwrap_or("");
                let arch = platform["architecture"].as_str().unwrap_or("");

                if os == "linux" && (arch == "amd64" || arch == "arm64") {
                    // This is a manifest list, we need to fetch the actual manifest
                    return Err(ShimError::runtime(
                        "Manifest list detected - fetch specific architecture manifest",
                    ));
                }
            }
        }

        let config_digest = manifest["config"]["digest"]
            .as_str()
            .ok_or_else(|| ShimError::runtime("Missing config digest in manifest"))?
            .to_string();

        let layers: Vec<(String, u64)> = manifest["layers"]
            .as_array()
            .ok_or_else(|| ShimError::runtime("Missing layers in manifest"))?
            .iter()
            .filter_map(|l| {
                let digest = l["digest"].as_str()?.to_string();
                let size = l["size"].as_u64().unwrap_or(0);
                Some((digest, size))
            })
            .collect();

        let total_size: u64 = layers.iter().map(|(_, s)| s).sum();

        Ok((config_digest, layers, total_size))
    }

    #[cfg(feature = "image-pull")]
    async fn download_blob(
        &self,
        image_ref: &ImageReference,
        digest: &str,
        path: &Path,
        token: Option<&str>,
    ) -> Result<()> {
        let registry_url = get_registry_url(&image_ref.registry);
        let url = format!(
            "{}/v2/{}/blobs/{}",
            registry_url, image_ref.repository, digest
        );

        let mut request = self.client.get(&url);
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .await
            .map_err(|e| ShimError::runtime(format!("Blob download failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ShimError::runtime(format!(
                "Failed to download blob: HTTP {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ShimError::runtime(format!("Failed to read blob: {}", e)))?;

        // Verify digest
        let computed_digest = format!("sha256:{:x}", Sha256::digest(&bytes));
        if computed_digest != digest {
            return Err(ShimError::runtime(format!(
                "Digest mismatch: expected {}, got {}",
                digest, computed_digest
            )));
        }

        std::fs::write(path, &bytes)?;
        Ok(())
    }

    #[cfg(feature = "image-pull")]
    async fn download_blob_with_progress(
        &self,
        image_ref: &ImageReference,
        digest: &str,
        path: &Path,
        token: Option<&str>,
        _layer_size: u64,
        progress_callback: &Option<Box<dyn Fn(PullProgress) + Send>>,
        base_downloaded: u64,
        total_size: u64,
    ) -> Result<()> {
        let registry_url = get_registry_url(&image_ref.registry);
        let url = format!(
            "{}/v2/{}/blobs/{}",
            registry_url, image_ref.repository, digest
        );

        let mut request = self.client.get(&url);
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .await
            .map_err(|e| ShimError::runtime(format!("Blob download failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ShimError::runtime(format!(
                "Failed to download blob: HTTP {}",
                response.status()
            )));
        }

        let mut file = std::fs::File::create(path)?;
        let mut hasher = Sha256::new();
        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| ShimError::runtime(format!("Download stream error: {}", e)))?;

            std::io::Write::write_all(&mut file, &chunk)?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;

            if let Some(ref cb) = progress_callback {
                cb(PullProgress {
                    current_layer: digest.to_string(),
                    total_layers: 0,
                    completed_layers: 0,
                    downloaded_bytes: base_downloaded + downloaded,
                    total_bytes: total_size,
                    status: "Downloading".to_string(),
                });
            }
        }

        // Verify digest
        let computed_digest = format!("sha256:{:x}", hasher.finalize());
        if computed_digest != digest {
            std::fs::remove_file(path)?;
            return Err(ShimError::runtime(format!(
                "Digest mismatch: expected {}, got {}",
                digest, computed_digest
            )));
        }

        Ok(())
    }

    #[cfg(feature = "image-pull")]
    fn extract_layer(&self, layer_path: &Path, rootfs_path: &Path) -> Result<()> {
        use flate2::read::GzDecoder;
        use tar::Archive;

        let file = std::fs::File::open(layer_path)?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);

        // Handle whiteout files (OCI layer deletion markers)
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?;
            let path_str = path.to_string_lossy();

            // Skip whiteout files for now (they indicate deleted files)
            if path_str.contains(".wh.") {
                continue;
            }

            let dest = rootfs_path.join(&*path);

            // Create parent directories
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }

            entry.unpack(&dest).ok(); // Ignore permission errors
        }

        Ok(())
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

    /// Get image by ID
    pub fn get(&self, image_id: &str) -> Option<&ImageInfo> {
        self.images.get(image_id)
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

fn parse_rfc3339_timestamp(s: &str) -> Option<u64> {
    // Simple RFC3339 parsing: 2023-01-15T10:30:00Z
    let parts: Vec<&str> = s.split('T').collect();
    if parts.len() != 2 {
        return None;
    }

    let date_parts: Vec<u32> = parts[0]
        .split('-')
        .filter_map(|p| p.parse().ok())
        .collect();

    if date_parts.len() != 3 {
        return None;
    }

    // Approximate timestamp (days since epoch * seconds per day)
    let year = date_parts[0];
    let month = date_parts[1];
    let day = date_parts[2];

    // Simple approximation
    let days_since_epoch = (year - 1970) * 365 + (month - 1) * 30 + day;
    Some((days_since_epoch * 86400) as u64)
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

    #[test]
    fn test_parse_timestamp() {
        let ts = parse_rfc3339_timestamp("2024-01-15T10:30:00Z");
        assert!(ts.is_some());
        assert!(ts.unwrap() > 0);
    }
}
