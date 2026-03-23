#!/usr/bin/env bash
set -euo pipefail

# Build a minimal Alpine-based rootfs for Firecracker microVM.
#
# Usage: ./build-rootfs.sh <path-to-aimp-binary>
#
# Output: dist/aimp-rootfs.ext4 (~15-20MB)
#
# Requires: root (for mount), wget, e2fsprogs

BINARY="${1:?Usage: $0 <path-to-aimp-binary>}"
DIST_DIR="dist"
ROOTFS="$DIST_DIR/aimp-rootfs.ext4"
MOUNT_DIR="/tmp/aimp-rootfs-mount"
ALPINE_MINIROOTFS_URL="https://dl-cdn.alpinelinux.org/alpine/v3.20/releases/x86_64/alpine-minirootfs-3.20.0-x86_64.tar.gz"
ROOTFS_SIZE_MB=64

echo "=== AIMP Firecracker Rootfs Builder ==="

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: Must run as root (needed for mount/chroot)"
    exit 1
fi

if [ ! -f "$BINARY" ]; then
    echo "ERROR: Binary not found: $BINARY"
    exit 1
fi

mkdir -p "$DIST_DIR"

# Create empty ext4 image
echo "Creating ${ROOTFS_SIZE_MB}MB ext4 image..."
dd if=/dev/zero of="$ROOTFS" bs=1M count=$ROOTFS_SIZE_MB status=none
mkfs.ext4 -q -F "$ROOTFS"

# Mount and populate
mkdir -p "$MOUNT_DIR"
mount -o loop "$ROOTFS" "$MOUNT_DIR"

trap "umount '$MOUNT_DIR' 2>/dev/null; rm -rf '$MOUNT_DIR'" EXIT

# Download and extract Alpine minirootfs
echo "Downloading Alpine minirootfs..."
wget -q "$ALPINE_MINIROOTFS_URL" -O /tmp/alpine-minirootfs.tar.gz
tar xzf /tmp/alpine-minirootfs.tar.gz -C "$MOUNT_DIR"
rm /tmp/alpine-minirootfs.tar.gz

# Install the AIMP binary
echo "Installing AIMP binary..."
install -m 755 "$BINARY" "$MOUNT_DIR/usr/local/bin/aimp_node"

# Create state directory
mkdir -p "$MOUNT_DIR/var/lib/aimp"

# Create init script
cat > "$MOUNT_DIR/etc/init.d/aimp" << 'INITEOF'
#!/bin/sh
case "$1" in
    start)
        echo "Starting AIMP node..."
        /usr/local/bin/aimp_node --port 1337 --name "$(hostname)" &
        ;;
    stop)
        echo "Stopping AIMP node..."
        killall aimp_node 2>/dev/null
        ;;
    *)
        echo "Usage: $0 {start|stop}"
        ;;
esac
INITEOF
chmod +x "$MOUNT_DIR/etc/init.d/aimp"

# Create simple init that starts AIMP on boot
cat > "$MOUNT_DIR/sbin/init" << 'INITEOF'
#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev

# Configure networking (Firecracker tap interface)
ip addr add 172.16.0.2/24 dev eth0 2>/dev/null
ip link set eth0 up 2>/dev/null
ip route add default via 172.16.0.1 2>/dev/null

hostname aimp-edge

echo "=== AIMP MicroVM Booted ==="

# Start AIMP node in foreground
exec /usr/local/bin/aimp_node --port 1337 --name "microvm-$(cat /proc/sys/kernel/random/uuid | cut -d- -f1)"
INITEOF
chmod +x "$MOUNT_DIR/sbin/init"

# Cleanup unnecessary files to minimize size
rm -rf "$MOUNT_DIR/usr/share/man" "$MOUNT_DIR/usr/share/doc"
rm -rf "$MOUNT_DIR/var/cache/apk"

umount "$MOUNT_DIR"
trap - EXIT
rm -rf "$MOUNT_DIR"

ACTUAL_SIZE=$(du -h "$ROOTFS" | cut -f1)
echo ""
echo "=== Rootfs Built ==="
echo "  Image:  $ROOTFS ($ACTUAL_SIZE)"
echo "  Binary: aimp_node at /usr/local/bin/"
echo "  State:  /var/lib/aimp"
echo ""
echo "Run with Firecracker:"
echo "  firecracker --no-api --config-file deploy/firecracker/vm-config.json"
