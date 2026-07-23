#!/usr/bin/env bash
# Stage 0 gate check (Linux/WSL). On Linux the Tauri binary cannot link
# (webkitgtk), so we verify rewave-core standalone + workspace metadata.
set -euo pipefail
cd "$(dirname "$0")"
cargo build -p rewave-core
cargo metadata --no-deps --format-version 1 > /dev/null
npm run build --prefix rewave-ui
