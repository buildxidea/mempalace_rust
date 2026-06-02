#!/bin/bash
set -eu

DATA_DIR="${DATA_DIR:-/data}"
HMAC_FILE="${DATA_DIR}/.hmac"

# Ensure data directory exists and is writable
mkdir -p "$DATA_DIR"
chmod 755 "$DATA_DIR"

# Generate HMAC secret on first boot if not present
if [ ! -s "$HMAC_FILE" ]; then
    SECRET="$(openssl rand -hex 32)"
    umask 077
    printf '%s\n' "$SECRET" > "$HMAC_FILE"
    chmod 600 "$HMAC_FILE"
    echo "================================================================"
    echo "mempalace: generated HMAC secret on first boot"
    echo "MEMPALACE_HMAC_SECRET=$SECRET"
    echo "Copy this value now. It will not be printed again."
    echo "Stored at: $HMAC_FILE (chmod 600)"
    echo "To rotate: delete $HMAC_FILE on the persistent volume and restart."
    echo "================================================================"
fi

# Export HMAC secret
export MEMPALACE_HMAC_SECRET="$(cat "$HMAC_FILE")"

# Execute the command
exec "$@"