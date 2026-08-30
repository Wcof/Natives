#!/bin/sh
set -eu
root=/
extension_id=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --root) root=${2:-}; shift 2;;
    --extension-id) extension_id=${2:-}; shift 2;;
    *) echo 'usage: uninstall.sh [--root <test-root>] [--extension-id <id>]' >&2; exit 2;;
  esac
done
[ -n "$extension_id" ] || extension_id=$(cat "$root/Library/Application Support/Natives/extension-id" 2>/dev/null || true)
[ -z "$extension_id" ] || echo "$extension_id" | grep -Eq '^[a-p]{32}$' || { echo 'error: invalid --extension-id' >&2; exit 1; }
remove() {
  path=$1
  [ -e "$root$path" ] || [ -L "$root$path" ] || return 0
  rm -f "$root$path"
}
remove '/Library/Application Support/Natives/native-file-host'
remove '/Library/Application Support/Natives/extension-id'
remove '/Library/Google/Chrome/NativeMessagingHosts/com.natives.file_manager.json'
if [ -n "$extension_id" ]; then
  remove "/Library/Application Support/Google/Chrome/External Extensions/$extension_id.json"
fi
remove '/Library/Application Support/Chromium/NativeMessagingHosts/com.natives.file_manager.json'
if [ -n "$extension_id" ]; then
  remove "/Library/Application Support/Chromium/External Extensions/$extension_id.json"
fi
remove '/Applications/Natives Workbench.app/Contents/MacOS/natives-launch'
remove '/Applications/Natives Workbench.app/Contents/Info.plist'
remove '/Applications/Natives Workbench Uninstall.command'
rmdir "$root/Applications/Natives Workbench.app/Contents/MacOS" 2>/dev/null || true
rmdir "$root/Applications/Natives Workbench.app/Contents" 2>/dev/null || true
rmdir "$root/Applications/Natives Workbench.app" 2>/dev/null || true
rmdir "$root/Library/Application Support/Natives" 2>/dev/null || true
echo 'Natives macOS files removed (user data untouched).'
