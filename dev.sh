#!/bin/bash
# Development launcher that bypasses the OS keychain for the DB encryption key,
# eliminating the repeated permission prompts caused by binary cdhash changes
# on every recompilation.
#
# A persistent dev key is generated once and stored in .dev-db-key (gitignored).
# The key is passed via DISKDECK_DB_KEY env var so the keychain is never touched.
#
# Usage:
#   ./dev.sh                           # build + run (no keychain prompts)
#   RUST_LOG=info ./dev.sh             # with logging

set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/backend"

# --- Generate a persistent dev DB key (one-time) ---
# This key is used ONLY during development to avoid keychain prompts.
# It is stored with owner-only read permissions (chmod 600) and gitignored.
# In production builds, the key is stored in the OS keychain instead.
KEY_FILE="$SCRIPT_DIR/.dev-db-key"
if [ ! -f "$KEY_FILE" ]; then
  openssl rand -hex 32 > "$KEY_FILE"
  chmod 600 "$KEY_FILE"
  echo "Generated dev DB key in .dev-db-key (owner-read-only)"
fi
export DISKDECK_DB_KEY=$(cat "$KEY_FILE")

# --- Launch Tauri dev ---
exec cargo tauri dev
