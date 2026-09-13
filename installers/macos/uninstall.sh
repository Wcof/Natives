#!/bin/sh
set -eu
# Natives uninstall (plan §6): removes THIS system source directory (the
# script lives inside it) and the minimal browser Native Messaging
# registration. User data in ~/.natives and ~/.natives-local is preserved —
# clearing module data is a separate, explicitly confirmed action (§4.3).
src="$(cd "$(dirname "$0")" && pwd)"
case "$src" in
  "/Library/Application Support/Natives"|"Natives-Local") ;;
  "/Library/Application Support/Natives-Local") ;;
  *) echo "refusing to uninstall from unexpected location: $src" >&2; exit 1;;
esac
extension_id=$(cat "$src/extension-id" 2>/dev/null || true)
[ -z "$extension_id" ] || echo "$extension_id" | grep -Eq '^[a-p]{32}$' || { echo "invalid extension id" >&2; exit 1; }
for browser in "/Library/Google/Chrome/NativeMessagingHosts" "/Library/Application Support/Chromium/NativeMessagingHosts"; do
  rm -f "$browser/com.natives.file_manager.json" "$browser/com.natives.model_host.json"
done
# Legacy 2026-09-era leftovers are cleaned only when present (plan §5 P6).
rm -f "/Library/Application Support/Google/Chrome/External Extensions/$extension_id.json" 2>/dev/null || true
rm -f "/Library/Application Support/Chromium/External Extensions/$extension_id.json" 2>/dev/null || true
cd /
rm -rf "$src"
echo "Natives system source removed: $src"
echo "user data (~/.natives, ~/.natives-local) is preserved."
