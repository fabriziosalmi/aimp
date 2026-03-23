#!/usr/bin/env bash
set -euo pipefail

# AIMP Edge Node Installer
# Usage: ./install.sh [path-to-binary]
#
# Installs the AIMP node as a systemd service with proper
# security hardening, user isolation, and state directories.

BINARY="${1:-./aimp_node}"
INSTALL_DIR="/usr/local/bin"
STATE_DIR="/var/lib/aimp"
CONFIG_DIR="/etc/aimp"
SERVICE_FILE="/etc/systemd/system/aimp-node.service"

echo "=== AIMP Edge Node Installer ==="

# Check root
if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: Must run as root (or with sudo)"
    exit 1
fi

# Check binary exists
if [ ! -f "$BINARY" ]; then
    echo "ERROR: Binary not found at $BINARY"
    echo "Usage: $0 <path-to-aimp_node-binary>"
    exit 1
fi

# Create service user (no login, no home)
if ! id -u aimp &>/dev/null; then
    echo "Creating aimp user..."
    useradd --system --no-create-home --shell /usr/sbin/nologin aimp
fi

# Install binary
echo "Installing binary to $INSTALL_DIR/aimp_node..."
install -m 755 "$BINARY" "$INSTALL_DIR/aimp_node"

# Create state directory
echo "Creating state directory $STATE_DIR..."
install -d -m 750 -o aimp -g aimp "$STATE_DIR"

# Create config directory
echo "Creating config directory $CONFIG_DIR..."
install -d -m 755 "$CONFIG_DIR"

# Install env file (don't overwrite existing)
if [ ! -f "$CONFIG_DIR/aimp.env" ]; then
    echo "Installing default environment file..."
    SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
    if [ -f "$SCRIPT_DIR/systemd/aimp.env" ]; then
        install -m 644 "$SCRIPT_DIR/systemd/aimp.env" "$CONFIG_DIR/aimp.env"
    fi
fi

# Install systemd service
echo "Installing systemd service..."
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
install -m 644 "$SCRIPT_DIR/systemd/aimp-node.service" "$SERVICE_FILE"

# Reload and enable
systemctl daemon-reload
systemctl enable aimp-node.service

echo ""
echo "=== Installation Complete ==="
echo ""
echo "  Binary:  $INSTALL_DIR/aimp_node"
echo "  State:   $STATE_DIR"
echo "  Config:  $CONFIG_DIR/aimp.env"
echo "  Service: aimp-node.service"
echo ""
echo "Commands:"
echo "  sudo systemctl start aimp-node     # Start the node"
echo "  sudo systemctl status aimp-node    # Check status"
echo "  sudo journalctl -u aimp-node -f    # Follow logs"
echo "  curl localhost:9090/health          # Health check"
echo "  curl localhost:9090/metrics         # Prometheus metrics"
echo ""
echo "Edit /etc/aimp/aimp.env to configure, then restart:"
echo "  sudo systemctl restart aimp-node"
