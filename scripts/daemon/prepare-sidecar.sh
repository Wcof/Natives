#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TRIPLE="${TAURI_ENV_TARGET_TRIPLE:-${CARGO_BUILD_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}}"
BUILD_TARGET_DIR="$ROOT/target"

CARGO_TARGET_DIR="$BUILD_TARGET_DIR" cargo build -p natives-agent-daemon --release --target "$TRIPLE"
mkdir -p "$ROOT/src-tauri/binaries"
cp "$BUILD_TARGET_DIR/$TRIPLE/release/natives-agent-daemon" \
  "$ROOT/src-tauri/binaries/natives-agent-daemon-$TRIPLE"
chmod 755 "$ROOT/src-tauri/binaries/natives-agent-daemon-$TRIPLE"
