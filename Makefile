# libcrun-shim Makefile
#
# Usage:
#   make              # Build everything
#   make build        # Build library and CLI
#   make agent        # Build agent for Linux
#   make vm-image     # Build VM image (kernel + initramfs)
#   make test         # Run tests
#   make install      # Install to system
#   make clean        # Clean build artifacts

.PHONY: all build agent vm-image test install clean help

# Detect architecture
UNAME_M := $(shell uname -m)
ifeq ($(UNAME_M),arm64)
    RUST_TARGET := aarch64-unknown-linux-musl
    DOCKER_PLATFORM := linux/arm64
else ifeq ($(UNAME_M),aarch64)
    RUST_TARGET := aarch64-unknown-linux-musl
    DOCKER_PLATFORM := linux/arm64
else
    RUST_TARGET := x86_64-unknown-linux-musl
    DOCKER_PLATFORM := linux/amd64
endif

# Directories
BUILD_DIR := target
RELEASE_DIR := $(BUILD_DIR)/release
OUTPUT_DIR := $(BUILD_DIR)/vm-image
INSTALL_DIR := /usr/local

# Colors
GREEN := \033[0;32m
YELLOW := \033[1;33m
NC := \033[0m

all: build

help:
	@echo "libcrun-shim build targets:"
	@echo ""
	@echo "  make build       Build library, CLI, and agent"
	@echo "  make release     Build optimized release binaries"
	@echo "  make agent       Build Linux agent (cross-compile)"
	@echo "  make vm-image    Build VM image using Docker"
	@echo "  make test        Run all tests"
	@echo "  make test-e2e    Run integration tests (requires agent)"
	@echo "  make install     Install to $(INSTALL_DIR)"
	@echo "  make clean       Clean build artifacts"
	@echo ""
	@echo "Detected architecture: $(UNAME_M)"
	@echo "Linux target: $(RUST_TARGET)"

# Build everything for development
build:
	@echo "$(GREEN)Building libcrun-shim...$(NC)"
	cargo build --workspace

# Build optimized release
release:
	@echo "$(GREEN)Building release...$(NC)"
	cargo build --workspace --release

# Build agent for Linux (for use inside VM)
agent:
	@echo "$(GREEN)Building agent for Linux ($(RUST_TARGET))...$(NC)"
	@if ! rustup target list --installed | grep -q "$(RUST_TARGET)"; then \
		echo "Installing Rust target: $(RUST_TARGET)"; \
		rustup target add $(RUST_TARGET); \
	fi
	cargo build --package libcrun-shim-agent --release --target $(RUST_TARGET)
	@mkdir -p $(OUTPUT_DIR)
	cp $(BUILD_DIR)/$(RUST_TARGET)/release/libcrun-shim-agent $(OUTPUT_DIR)/
	@echo "$(GREEN)Agent built: $(OUTPUT_DIR)/libcrun-shim-agent$(NC)"

# Build VM image using Docker
vm-image:
	@echo "$(GREEN)Building VM image using Docker...$(NC)"
	cd vm-image && ./build.sh --docker
	@echo "$(GREEN)VM image built in vm-image/output/$(NC)"

# Quick agent build (just for testing)
agent-quick:
	@echo "$(GREEN)Quick agent build...$(NC)"
	cd vm-image && ./build.sh --agent

# Run all tests
test:
	@echo "$(GREEN)Running tests...$(NC)"
	cargo test --workspace

# Run integration tests (requires agent running)
test-e2e:
	@echo "$(GREEN)Running integration tests...$(NC)"
	@echo "$(YELLOW)Note: Some tests require root privileges$(NC)"
	cargo test --test integration_tests -- --ignored --test-threads=1

# Run clippy lints
lint:
	@echo "$(GREEN)Running clippy...$(NC)"
	cargo clippy --workspace -- -D warnings

# Format code
fmt:
	@echo "$(GREEN)Formatting code...$(NC)"
	cargo fmt --all

# Check formatting
fmt-check:
	@echo "$(GREEN)Checking formatting...$(NC)"
	cargo fmt --all -- --check

# Install to system
install: release
	@echo "$(GREEN)Installing to $(INSTALL_DIR)...$(NC)"
	install -d $(INSTALL_DIR)/bin
	install -m 755 $(RELEASE_DIR)/crun-shim $(INSTALL_DIR)/bin/
	install -m 755 $(RELEASE_DIR)/libcrun-shim-agent $(INSTALL_DIR)/bin/
	@if [ -f vm-image/output/kernel ]; then \
		install -d $(INSTALL_DIR)/share/libcrun-shim; \
		install -m 644 vm-image/output/kernel $(INSTALL_DIR)/share/libcrun-shim/; \
		install -m 644 vm-image/output/initramfs.img $(INSTALL_DIR)/share/libcrun-shim/; \
	fi
	@echo "$(GREEN)Installed successfully$(NC)"

# Uninstall
uninstall:
	@echo "$(GREEN)Uninstalling...$(NC)"
	rm -f $(INSTALL_DIR)/bin/crun-shim
	rm -f $(INSTALL_DIR)/bin/libcrun-shim-agent
	rm -rf $(INSTALL_DIR)/share/libcrun-shim

# Clean build artifacts
clean:
	@echo "$(GREEN)Cleaning...$(NC)"
	cargo clean
	rm -rf vm-image/output

# Show version info
version:
	@cargo pkgid libcrun-shim | cut -d# -f2
	@echo "Agent target: $(RUST_TARGET)"

# Docker build for CI
docker-build:
	@echo "$(GREEN)Building in Docker for CI...$(NC)"
	docker build -t libcrun-shim-ci .

# Generate documentation
docs:
	@echo "$(GREEN)Generating documentation...$(NC)"
	cargo doc --workspace --no-deps --open

