#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TRIPLE="${TAURI_ENV_TARGET_TRIPLE:-${CARGO_BUILD_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}}"

cargo build -p natives-agent-daemon --release --target "$TRIPLE"
mkdir -p "$ROOT/src-tauri/binaries"
cp "$ROOT/target/$TRIPLE/release/natives-agent-daemon" \
  "$ROOT/src-tauri/binaries/natives-agent-daemon-$TRIPLE"
chmod 755 "$ROOT/src-tauri/binaries/natives-agent-daemon-$TRIPLE"
