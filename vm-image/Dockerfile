# Dockerfile for building libcrun-shim VM image
# 
# This creates a minimal Linux system with:
# - Linux kernel with virtio support
# - libcrun for container execution
# - libcrun-shim-agent for host communication
#
# Build: docker build -t libcrun-shim-builder .
# Extract: docker run --rm libcrun-shim-builder cat /output/kernel > kernel
#          docker run --rm libcrun-shim-builder cat /output/initramfs.img > initramfs.img

FROM alpine:3.19 AS kernel-builder

# Install kernel build dependencies
RUN apk add --no-cache \
    build-base \
    bc \
    bison \
    flex \
    elfutils-dev \
    openssl-dev \
    perl \
    linux-headers \
    xz \
    wget \
    cpio \
    gzip

# Download and build Linux kernel
ARG KERNEL_VERSION=6.6.10
WORKDIR /build

RUN wget -q https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-${KERNEL_VERSION}.tar.xz && \
    tar xf linux-${KERNEL_VERSION}.tar.xz && \
    mv linux-${KERNEL_VERSION} linux

WORKDIR /build/linux

# Create minimal kernel config for VM
RUN cat > /tmp/miniconfig << 'EOF'
# General setup
CONFIG_LOCALVERSION="-libcrun-shim"
CONFIG_DEFAULT_HOSTNAME="libcrun-vm"
CONFIG_SYSVIPC=y
CONFIG_POSIX_MQUEUE=y
CONFIG_AUDIT=y
CONFIG_CGROUPS=y
CONFIG_CGROUP_SCHED=y
CONFIG_CGROUP_PIDS=y
CONFIG_CGROUP_CPUACCT=y
CONFIG_CGROUP_DEVICE=y
CONFIG_CGROUP_FREEZER=y
CONFIG_CPUSETS=y
CONFIG_MEMCG=y
CONFIG_BLK_CGROUP=y
CONFIG_NAMESPACES=y
CONFIG_UTS_NS=y
CONFIG_IPC_NS=y
CONFIG_USER_NS=y
CONFIG_PID_NS=y
CONFIG_NET_NS=y
CONFIG_MULTIUSER=y
CONFIG_SYSFS=y
CONFIG_PROC_FS=y
CONFIG_TMPFS=y
CONFIG_DEVTMPFS=y
CONFIG_DEVTMPFS_MOUNT=y

# 64-bit kernel
CONFIG_64BIT=y

# Block layer
CONFIG_BLOCK=y
CONFIG_BLK_DEV=y
CONFIG_BLK_DEV_LOOP=y

# Virtio (required for Apple Virtualization.framework)
CONFIG_VIRTIO=y
CONFIG_VIRTIO_MENU=y
CONFIG_VIRTIO_PCI=y
CONFIG_VIRTIO_MMIO=y
CONFIG_VIRTIO_BLK=y
CONFIG_VIRTIO_NET=y
CONFIG_VIRTIO_CONSOLE=y
CONFIG_VIRTIO_VSOCK=y
CONFIG_VIRTIO_FS=y
CONFIG_HW_RANDOM_VIRTIO=y

# VSOCK for host communication
CONFIG_VSOCKETS=y
CONFIG_VSOCKETS_LOOPBACK=y
CONFIG_VIRTIO_VSOCKETS=y

# Networking
CONFIG_NET=y
CONFIG_PACKET=y
CONFIG_UNIX=y
CONFIG_INET=y
CONFIG_IP_MULTICAST=y
CONFIG_IP_ADVANCED_ROUTER=y
CONFIG_IP_MULTIPLE_TABLES=y
CONFIG_NET_IPIP=y
CONFIG_NET_IPGRE=y
CONFIG_SYN_COOKIES=y
CONFIG_NETFILTER=y
CONFIG_BRIDGE=y
CONFIG_VETH=y
CONFIG_MACVLAN=y
CONFIG_IPVLAN=y

# Filesystems
CONFIG_EXT4_FS=y
CONFIG_OVERLAY_FS=y
CONFIG_SQUASHFS=y
CONFIG_FUSE_FS=y

# Security
CONFIG_SECURITY=y
CONFIG_SECURITYFS=y
CONFIG_SECURITY_NETWORK=y
CONFIG_SECCOMP=y
CONFIG_SECCOMP_FILTER=y

# Required for containers
CONFIG_OVERLAY_FS=y
CONFIG_USER_NS=y
CONFIG_CGROUP_BPF=y
CONFIG_BPF_SYSCALL=y

# TTY/Console
CONFIG_TTY=y
CONFIG_VT=y
CONFIG_CONSOLE_TRANSLATIONS=y
CONFIG_VT_CONSOLE=y
CONFIG_HW_CONSOLE=y
CONFIG_UNIX98_PTYS=y
CONFIG_LEGACY_PTYS=n

# Serial console (for VM)
CONFIG_SERIAL_8250=y
CONFIG_SERIAL_8250_CONSOLE=y

# Initramfs support
CONFIG_BLK_DEV_INITRD=y
CONFIG_RD_GZIP=y

# Disable unnecessary features
CONFIG_MODULES=n
CONFIG_SOUND=n
CONFIG_USB=n
CONFIG_WIRELESS=n
CONFIG_DRM=n
CONFIG_FB=n

# Optimize for size
CONFIG_CC_OPTIMIZE_FOR_SIZE=y
CONFIG_KERNEL_XZ=y
EOF

# Generate minimal config
RUN make tinyconfig && \
    cat /tmp/miniconfig >> .config && \
    make olddefconfig

# Build kernel
RUN make -j$(nproc) bzImage

# ============================================================
# Stage 2: Build libcrun
# ============================================================
FROM alpine:3.19 AS libcrun-builder

RUN apk add --no-cache \
    build-base \
    autoconf \
    automake \
    libtool \
    pkgconfig \
    git \
    go \
    libcap-dev \
    libseccomp-dev \
    yajl-dev \
    argp-standalone \
    linux-headers \
    python3

WORKDIR /build

# Clone and build libcrun
RUN git clone --depth 1 https://github.com/containers/crun.git && \
    cd crun && \
    ./autogen.sh && \
    ./configure --enable-static --disable-systemd && \
    make -j$(nproc) && \
    strip src/crun

# ============================================================
# Stage 3: Build agent (cross-compiled from Rust)
# ============================================================
FROM rust:1.75-alpine AS agent-builder

RUN apk add --no-cache musl-dev

WORKDIR /build

# Copy the entire project
COPY . .

# Build the agent
RUN cargo build --package libcrun-shim-agent --release && \
    strip target/release/libcrun-shim-agent

# ============================================================
# Stage 4: Build initramfs
# ============================================================
FROM alpine:3.19 AS initramfs-builder

RUN apk add --no-cache \
    busybox-static \
    cpio \
    gzip \
    libcap \
    libseccomp \
    yajl

WORKDIR /initramfs

# Create directory structure
RUN mkdir -p \
    bin \
    sbin \
    etc \
    proc \
    sys \
    dev \
    tmp \
    run \
    var/run \
    var/log/containers \
    usr/bin \
    usr/lib \
    lib

# Copy busybox
RUN cp /bin/busybox bin/busybox && \
    cd bin && \
    for cmd in sh ls cat echo mount umount mkdir rm mv cp ln chmod chown \
               ps kill sleep true false test [ expr grep sed awk head tail \
               sort uniq wc tr cut date hostname id env; do \
        ln -s busybox $cmd; \
    done

# Copy libcrun from builder
COPY --from=libcrun-builder /build/crun/src/crun bin/crun
RUN chmod +x bin/crun

# Copy agent from builder
COPY --from=agent-builder /build/target/release/libcrun-shim-agent bin/libcrun-shim-agent
RUN chmod +x bin/libcrun-shim-agent

# Copy required libraries
COPY --from=libcrun-builder /usr/lib/libseccomp.so* lib/
COPY --from=libcrun-builder /usr/lib/libyajl.so* lib/
COPY --from=libcrun-builder /usr/lib/libcap.so* lib/

# Create init script
RUN cat > init << 'INIT_SCRIPT'
#!/bin/sh

# Mount essential filesystems
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev
mount -t tmpfs tmpfs /run
mount -t tmpfs tmpfs /tmp

# Create device nodes if needed
[ -c /dev/console ] || mknod /dev/console c 5 1
[ -c /dev/null ] || mknod /dev/null c 1 3
[ -c /dev/zero ] || mknod /dev/zero c 1 5
[ -c /dev/random ] || mknod /dev/random c 1 8
[ -c /dev/urandom ] || mknod /dev/urandom c 1 9
[ -c /dev/tty ] || mknod /dev/tty c 5 0
[ -c /dev/ptmx ] || mknod /dev/ptmx c 5 2

# Create pts directory
mkdir -p /dev/pts
mount -t devpts devpts /dev/pts

# Setup hostname
hostname libcrun-vm

# Setup network (basic loopback)
ip link set lo up 2>/dev/null || true

# Create log directory
mkdir -p /var/log/containers

# Print banner
echo "============================================"
echo "  libcrun-shim VM started"
echo "  Kernel: $(uname -r)"
echo "  Agent: $(bin/libcrun-shim-agent --version 2>/dev/null || echo 'unknown')"
echo "============================================"

# Start the agent
echo "Starting libcrun-shim-agent..."
exec /bin/libcrun-shim-agent --socket /run/shim.sock --vsock-port 1234

# If agent exits, drop to shell for debugging
exec /bin/sh
INIT_SCRIPT

RUN chmod +x init

# Create the initramfs archive
RUN find . | cpio -o -H newc | gzip > /initramfs.img

# ============================================================
# Stage 5: Final output
# ============================================================
FROM alpine:3.19 AS output

RUN mkdir /output

# Copy kernel
COPY --from=kernel-builder /build/linux/arch/x86_64/boot/bzImage /output/kernel

# Copy initramfs
COPY --from=initramfs-builder /initramfs.img /output/initramfs.img

# Also provide ARM64 instructions
RUN echo 'For ARM64 (Apple Silicon), rebuild kernel with:' > /output/README.txt && \
    echo 'ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- make' >> /output/README.txt

# Show output
RUN ls -la /output/

CMD ["sh", "-c", "echo 'Files in /output:' && ls -la /output/"]

