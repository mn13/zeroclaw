#!/bin/sh
set -e

# Set up ZeroClaw config directory inside the writable /data volume.
ZCDIR="/data/.zeroclaw"
mkdir -p "$ZCDIR/workspace"

# If a config file was mounted at /etc/zc/config.toml, always sync it
# so that gateway config edits take effect after container restart.
if [ -f /etc/zc/config.toml ]; then
    cp /etc/zc/config.toml "$ZCDIR/config.toml"
fi

# Point ZeroClaw at our writable config directory.
export ZEROCLAW_CONFIG_DIR="$ZCDIR"

exec "$@"
