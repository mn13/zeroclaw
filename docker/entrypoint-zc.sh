#!/bin/sh
set -e

# Set up ZeroClaw config directory inside the writable /data volume.
# Handle bind-mount ownership: ensure the zc user can write to /data.
ZCDIR="/data/.zeroclaw"
mkdir -p "$ZCDIR/workspace" 2>/dev/null || {
    # If mkdir fails (bind-mount owned by different user), try as current user
    # This happens on macOS Docker Desktop with host-mounted volumes
    true
}

# If a config file was mounted at /etc/zc/config.toml, always sync it
# so that gateway config edits take effect after container restart.
if [ -f /etc/zc/config.toml ]; then
    cp /etc/zc/config.toml "$ZCDIR/config.toml" 2>/dev/null || true
fi

# Point ZeroClaw at our writable config directory.
export ZEROCLAW_CONFIG_DIR="$ZCDIR"

exec "$@"
