#!/bin/bash
# Script to get an interactive Linux shell for testing libcrun-shim
# Usage: ./scripts/test-linux-interactive.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "🐳 Starting interactive Linux container for libcrun-shim testing..."
echo ""

# Build the test Docker image if it doesn't exist
if ! docker image inspect libcrun-shim-test &>/dev/null; then
    echo "📦 Building Docker test image..."
    docker build -t libcrun-shim-test -f "$PROJECT_ROOT/docker/Dockerfile.test" "$PROJECT_ROOT"
fi

# Run interactive container
echo ""
echo "🚀 Starting interactive shell..."
echo "   - Project is mounted at /workspace"
echo "   - libcrun should be available"
echo "   - Type 'exit' to leave"
echo ""

docker run -it --rm \
    -v "$PROJECT_ROOT:/workspace" \
    -w /workspace \
    libcrun-shim-test \
    bash

