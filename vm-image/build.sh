#!/bin/bash
#
# Build script for libcrun-shim VM image
#
# This builds:
# - Linux kernel with virtio support
# - Initramfs with libcrun and the agent
#
# Requirements:
# - Docker (for x86_64 builds)
# - Or: Linux with build tools (for native builds)
#
# Usage:
#   ./build.sh              # Build for current architecture
#   ./build.sh --docker     # Build using Docker (recommended)
#   ./build.sh --install    # Build and install to system paths
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="$SCRIPT_DIR/output"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Detect architecture
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)
            echo "x86_64"
            ;;
        arm64|aarch64)
            echo "arm64"
            ;;
        *)
            log_error "Unsupported architecture: $(uname -m)"
            exit 1
            ;;
    esac
}

# Build using Docker
build_docker() {
    local arch="$1"
    
    log_info "Building VM image using Docker..."
    log_info "Architecture: $arch"
    
    mkdir -p "$OUTPUT_DIR"
    
    # Build the Docker image
    docker build \
        --platform "linux/$arch" \
        -t libcrun-shim-builder \
        -f "$SCRIPT_DIR/Dockerfile" \
        "$PROJECT_ROOT"
    
    # Extract the files
    log_info "Extracting kernel and initramfs..."
    
    docker run --rm libcrun-shim-builder cat /output/kernel > "$OUTPUT_DIR/kernel"
    docker run --rm libcrun-shim-builder cat /output/initramfs.img > "$OUTPUT_DIR/initramfs.img"
    
    log_info "Build complete!"
    log_info "Output files:"
    ls -lah "$OUTPUT_DIR/"
}

# Quick build (just the agent)
build_agent_only() {
    log_info "Building agent only (for development)..."
    
    cd "$PROJECT_ROOT"
    
    # Determine target
    local target=""
    if [[ "$(uname)" == "Darwin" ]]; then
        # Cross-compile for Linux
        if [[ "$(uname -m)" == "arm64" ]]; then
            target="aarch64-unknown-linux-musl"
        else
            target="x86_64-unknown-linux-musl"
        fi
        
        log_info "Cross-compiling for Linux ($target)..."
        
        # Check if target is installed
        if ! rustup target list --installed | grep -q "$target"; then
            log_info "Installing Rust target: $target"
            rustup target add "$target"
        fi
        
        # Build
        cargo build --package libcrun-shim-agent --release --target "$target"
        
        mkdir -p "$OUTPUT_DIR"
        cp "target/$target/release/libcrun-shim-agent" "$OUTPUT_DIR/"
    else
        # Native Linux build
        cargo build --package libcrun-shim-agent --release
        
        mkdir -p "$OUTPUT_DIR"
        cp "target/release/libcrun-shim-agent" "$OUTPUT_DIR/"
    fi
    
    log_info "Agent built: $OUTPUT_DIR/libcrun-shim-agent"
}

# Install to system paths
install_vm_image() {
    local install_dir="/usr/local/share/libcrun-shim"
    
    log_info "Installing VM image to $install_dir..."
    
    if [[ ! -f "$OUTPUT_DIR/kernel" ]] || [[ ! -f "$OUTPUT_DIR/initramfs.img" ]]; then
        log_error "VM image not built. Run './build.sh --docker' first."
        exit 1
    fi
    
    sudo mkdir -p "$install_dir"
    sudo cp "$OUTPUT_DIR/kernel" "$install_dir/"
    sudo cp "$OUTPUT_DIR/initramfs.img" "$install_dir/"
    
    log_info "Installed to $install_dir"
    ls -la "$install_dir/"
}

# Create a minimal test initramfs (without full kernel build)
build_test_initramfs() {
    log_info "Building minimal test initramfs..."
    
    local tmpdir=$(mktemp -d)
    
    # Create structure
    mkdir -p "$tmpdir"/{bin,sbin,etc,proc,sys,dev,tmp,run,var/run,var/log/containers,lib}
    
    # Create a simple init
    cat > "$tmpdir/init" << 'EOF'
#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev

echo "libcrun-shim test VM"
echo "Kernel: $(uname -r)"

# Run agent if available
if [ -x /bin/libcrun-shim-agent ]; then
    exec /bin/libcrun-shim-agent
fi

exec /bin/sh
EOF
    chmod +x "$tmpdir/init"
    
    # Copy agent if built
    if [[ -f "$OUTPUT_DIR/libcrun-shim-agent" ]]; then
        cp "$OUTPUT_DIR/libcrun-shim-agent" "$tmpdir/bin/"
    fi
    
    # Create initramfs
    cd "$tmpdir"
    find . | cpio -o -H newc | gzip > "$OUTPUT_DIR/test-initramfs.img"
    
    rm -rf "$tmpdir"
    
    log_info "Test initramfs created: $OUTPUT_DIR/test-initramfs.img"
}

# Print usage
usage() {
    cat << EOF
Usage: $0 [OPTIONS]

Build libcrun-shim VM image.

Options:
    --docker        Build using Docker (recommended)
    --agent         Build agent binary only
    --test          Build minimal test initramfs
    --install       Install to system paths
    --help          Show this help

Examples:
    $0 --docker     # Full build using Docker
    $0 --agent      # Quick agent build for development
    $0 --install    # Install after building

EOF
}

# Main
main() {
    local arch=$(detect_arch)
    
    case "${1:-}" in
        --docker)
            build_docker "$arch"
            ;;
        --agent)
            build_agent_only
            ;;
        --test)
            build_agent_only
            build_test_initramfs
            ;;
        --install)
            install_vm_image
            ;;
        --help|-h)
            usage
            ;;
        "")
            log_info "No option specified. Use --docker for full build or --agent for quick build."
            usage
            exit 1
            ;;
        *)
            log_error "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
}

main "$@"

