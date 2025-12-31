#!/bin/bash
# Script to test libcrun-shim on Linux using Docker
# Usage: ./scripts/test-linux.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "🐳 Testing libcrun-shim on Linux via Docker..."
echo ""

# Build the test Docker image
echo "📦 Building Docker test image..."
docker build -t libcrun-shim-test -f "$PROJECT_ROOT/docker/Dockerfile.test" "$PROJECT_ROOT"

# Run tests in the container
echo ""
echo "🧪 Running tests in Docker container..."
docker run --rm \
    -v "$PROJECT_ROOT:/workspace" \
    -w /workspace \
    libcrun-shim-test \
    bash -c "
        echo '📋 Environment:'
        pkg-config --exists libcrun && echo '  ✓ libcrun found' || echo '  ✗ libcrun not found'
        pkg-config --modversion libcrun || true
        echo ''
        echo '🔨 Building...'
        cargo build --workspace
        echo ''
        echo '🧪 Running tests...'
        cargo test --workspace
        echo ''
        echo '✅ All tests passed!'
    "

echo ""
echo "✨ Linux testing complete!"

