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

# GOGCLI: use file-based encrypted keyring (no OS keyring in Docker)
export GOG_KEYRING_BACKEND=file
# Password derived from the agent's gRPC secret for deterministic unlock
export GOG_KEYRING_PASSWORD="${ZCGW_GRPC_SECRET:-zeroclaw}"

# Persist GOG config/keyring on the /data volume so tokens survive container recreation.
GOG_DATA="$ZCDIR/gogcli"
mkdir -p "$GOG_DATA" 2>/dev/null || true
GOG_DEFAULT="/root/.config/gogcli"
if [ ! -L "$GOG_DEFAULT" ]; then
    mkdir -p "$(dirname "$GOG_DEFAULT")" 2>/dev/null || true
    # Move any existing data (e.g. from a fresh auth) to persistent storage
    if [ -d "$GOG_DEFAULT" ]; then
        cp -a "$GOG_DEFAULT/." "$GOG_DATA/" 2>/dev/null || true
        rm -rf "$GOG_DEFAULT"
    fi
    ln -sf "$GOG_DATA" "$GOG_DEFAULT"
fi

exec "$@"
