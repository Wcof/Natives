#!/bin/sh
set -eu
root=/
while [ "$#" -gt 0 ]; do
  case "$1" in
    --root) root=${2:-}; shift 2;;
    *) echo "usage: $0 [--root <test-root>]" >&2; exit 2;;
  esac
done
app_path="$root"__APP_PATH__
source_dir="$root"__SOURCE_ROOT__
for nm in __NM_CORE__ __NM_MODEL__; do
  for browser in "/Library/Google/Chrome/NativeMessagingHosts" "/Library/Application Support/Chromium/NativeMessagingHosts"; do
    rm -f "$root$browser/$nm.json"
  done
done
rm -rf "$app_path" "$source_dir"
echo "removed: $app_path"
echo "removed: $source_dir"
echo "user data (~/.natives, ~/.natives-local) is preserved."
