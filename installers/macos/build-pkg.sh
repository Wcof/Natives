#!/bin/sh
set -eu
usage() { echo "usage: $0 --host PATH --extension-id ID [--version V] [--output PATH] [--app-sign ID] [--pkg-sign ID] [--product-sign ID]" >&2; exit 2; }
host= extension_id= version=1.0.0 output=./Natives-macos.pkg app_sign= pkg_sign= product_sign=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --host) host=${2:-}; shift 2;;
    --extension-id) extension_id=${2:-}; shift 2;;
    --version) version=${2:-}; shift 2;;
    --output) output=${2:-}; shift 2;;
    --app-sign) app_sign=${2:-}; shift 2;;
    --pkg-sign) pkg_sign=${2:-}; shift 2;;
    --product-sign) product_sign=${2:-}; shift 2;;
    *) usage;;
  esac
done
[ -n "$host" ] && [ -x "$host" ] || { echo 'error: --host must be an executable prebuilt Host' >&2; exit 1; }
echo "$extension_id" | grep -Eq '^[a-p]{32}$' || { echo 'error: --extension-id must be the explicit 32-character Chrome ID' >&2; exit 1; }
echo "$version" | grep -Eq '^[0-9]+(\.[0-9]+){1,3}$' || { echo 'error: invalid --version' >&2; exit 1; }
command -v pkgbuild >/dev/null 2>&1 || { echo 'error: macOS pkgbuild is required' >&2; exit 1; }
command -v productbuild >/dev/null 2>&1 || { echo 'error: macOS productbuild is required' >&2; exit 1; }
base=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/natives-pkg.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM
payload=$tmp/payload
mkdir -p "$payload/Library/Application Support/Natives" "$payload/Library/Google/Chrome/NativeMessagingHosts" \
  "$payload/Library/Application Support/Google/Chrome/External Extensions" "$payload/Library/Application Support/Chromium/NativeMessagingHosts" \
  "$payload/Library/Application Support/Chromium/External Extensions" \
  "$payload/Applications/Natives Workbench.app/Contents/MacOS"
install -m 755 "$host" "$payload/Library/Application Support/Natives/native-file-host"
install -m 755 "$base/uninstall.sh" "$payload/Library/Application Support/Natives/uninstall.sh"
printf '%s\n' "$extension_id" > "$payload/Library/Application Support/Natives/extension-id"
for browser in "$payload/Library/Google/Chrome/NativeMessagingHosts" "$payload/Library/Application Support/Chromium/NativeMessagingHosts"; do
  sed "s/__EXTENSION_ID__/$extension_id/g" "$base/resources/native-host-manifest.json.in" > "$browser/com.natives.file_manager.json"
done
for browser in "$payload/Library/Application Support/Google/Chrome/External Extensions" "$payload/Library/Application Support/Chromium/External Extensions"; do
  cp "$base/resources/external-extension.json.in" "$browser/$extension_id.json"
done
sed "s/__EXTENSION_ID__/$extension_id/g" "$base/resources/natives-launch.sh.in" > "$payload/Applications/Natives Workbench.app/Contents/MacOS/natives-launch"
sed "s/__VERSION__/$version/g" "$base/resources/Info.plist.in" > "$payload/Applications/Natives Workbench.app/Contents/Info.plist"
chmod 755 "$payload/Applications/Natives Workbench.app/Contents/MacOS/natives-launch"
sed "s/__EXTENSION_ID__/$extension_id/g" "$base/resources/uninstall-wrapper.command.in" > "$payload/Applications/Natives Workbench Uninstall.command"
chmod 755 "$payload/Applications/Natives Workbench Uninstall.command"
[ -z "$app_sign" ] || { command -v codesign >/dev/null 2>&1 || { echo 'error: codesign is required with --app-sign' >&2; exit 1; }; codesign --force --deep --sign "$app_sign" "$payload/Applications/Natives Workbench.app"; }
component=$tmp/Natives-component.pkg
set -- --root "$payload" --identifier com.natives.file-manager --version "$version" --install-location /
[ -z "$pkg_sign" ] || set -- "$@" --sign "$pkg_sign"
pkgbuild "$@" "$component"
set -- --package "$component"
[ -z "$product_sign" ] || set -- "$@" --sign "$product_sign"
mkdir -p "$(dirname "$output")"
productbuild "$@" "$output"
if [ -z "$pkg_sign" ] || [ -z "$product_sign" ]; then
  echo "created unsigned development package: $output (supply --pkg-sign and --product-sign for release signing)"
else
  echo "created signed package: $output"
fi
