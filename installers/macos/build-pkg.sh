#!/bin/sh
set -eu
# Unified Natives suite installer (ADR-0029).
#
# Modes (explicit, never inferred from flags):
#   local      — isolated local candidate; ad-hoc/development signing allowed per
#                managed-app contract §4.2. Seeds/model-host optional (dev builds).
#   production — complete suite candidate; REQUIRES Model Host binary, a seeds
#                directory containing a signed suite manifest, and all three
#                signing identities. Never builds from fixture seeds.
#
# The installer only writes the restricted system source, read-only seeds and
# the minimal browser registration (ADR-0029 §2). It never creates .app
# bundles, Dock/LaunchServices entries, or /Applications uninstall shortcuts.
usage() { echo "usage: $0 --mode local|production --host PATH --extension-id ID [--model-host PATH] [--seeds-dir PATH] [--version V] [--output PATH] [--app-sign ID] [--pkg-sign ID] [--product-sign ID]" >&2; exit 2; }
mode= host= extension_id= model_host= seeds_dir= version=1.0.0 output=./Natives-macos.pkg app_sign= pkg_sign= product_sign=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode) mode=${2:-}; shift 2;;
    --host) host=${2:-}; shift 2;;
    --model-host) model_host=${2:-}; shift 2;;
    --seeds-dir) seeds_dir=${2:-}; shift 2;;
    --extension-id) extension_id=${2:-}; shift 2;;
    --version) version=${2:-}; shift 2;;
    --output) output=${2:-}; shift 2;;
    --app-sign) app_sign=${2:-}; shift 2;;
    --pkg-sign) pkg_sign=${2:-}; shift 2;;
    --product-sign) product_sign=${2:-}; shift 2;;
    *) usage;;
  esac
done
case "$mode" in
  local) ;;
  production) ;;
  *) echo 'error: --mode must be explicitly local or production' >&2; exit 1;;
esac
[ -n "$host" ] && [ -x "$host" ] || { echo 'error: --host must be an executable prebuilt Host' >&2; exit 1; }
echo "$extension_id" | grep -Eq '^[a-p]{32}$' || { echo 'error: --extension-id must be the explicit 32-character Chrome ID' >&2; exit 1; }
echo "$version" | grep -Eq '^[0-9]+(\.[0-9]+){1,3}$' || { echo 'error: invalid --version' >&2; exit 1; }
command -v pkgbuild >/dev/null 2>&1 || { echo 'error: macOS pkgbuild is required' >&2; exit 1; }
command -v productbuild >/dev/null 2>&1 || { echo 'error: macOS productbuild is required' >&2; exit 1; }

# Complete-suite constraints (ADR-0029 R-APP-19): a production candidate is only
# complete with Model Host, signed seeds and full signing identities.
if [ "$mode" = production ]; then
  [ -n "$model_host" ] && [ -x "$model_host" ] || { echo 'error: production suite requires an executable --model-host' >&2; exit 1; }
  [ -n "$seeds_dir" ] && [ -f "$seeds_dir/suite-manifest.json" ] || { echo 'error: production suite requires --seeds-dir containing a signed suite-manifest.json' >&2; exit 1; }
  [ -n "$app_sign" ] || { echo 'error: production suite requires --app-sign' >&2; exit 1; }
  [ -n "$pkg_sign" ] || { echo 'error: production suite requires --pkg-sign' >&2; exit 1; }
  [ -n "$product_sign" ] || { echo 'error: production suite requires --product-sign' >&2; exit 1; }
fi

base=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/natives-pkg.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM
payload=$tmp/payload
mkdir -p "$payload/Library/Application Support/Natives/seeds" "$payload/Library/Google/Chrome/NativeMessagingHosts" \
  "$payload/Library/Application Support/Google/Chrome/External Extensions" "$payload/Library/Application Support/Chromium/NativeMessagingHosts" \
  "$payload/Library/Application Support/Chromium/External Extensions"
install -m 755 "$host" "$payload/Library/Application Support/Natives/native-file-host"
if [ -n "$model_host" ] && [ -x "$model_host" ]; then
  install -m 755 "$model_host" "$payload/Library/Application Support/Natives/model-host"
fi
if [ -n "$seeds_dir" ] && [ -d "$seeds_dir" ]; then
  cp -R "$seeds_dir/"* "$payload/Library/Application Support/Natives/seeds/"
fi
install -m 755 "$base/uninstall.sh" "$payload/Library/Application Support/Natives/uninstall.sh"
printf '%s\n' "$extension_id" > "$payload/Library/Application Support/Natives/extension-id"
for browser in "$payload/Library/Google/Chrome/NativeMessagingHosts" "$payload/Library/Application Support/Chromium/NativeMessagingHosts"; do
  sed "s/__EXTENSION_ID__/$extension_id/g" "$base/resources/native-host-manifest.json.in" > "$browser/com.natives.file_manager.json"
  if [ -n "$model_host" ] && [ -x "$model_host" ]; then
    cat > "$browser/com.natives.model_host.json" <<EOF
{
  "name": "com.natives.model_host",
  "description": "Natives model settings and local model proxy",
  "path": "/Library/Application Support/Natives/model-host",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://$extension_id/"
  ]
}
EOF
  fi
done
for browser in "$payload/Library/Application Support/Google/Chrome/External Extensions" "$payload/Library/Application Support/Chromium/External Extensions"; do
  cp "$base/resources/external-extension.json.in" "$browser/$extension_id.json"
done
# Hard prohibitions (ADR-0029 §2): no .app bundles, no /Applications entries at all.
if [ -d "$payload/Applications" ]; then
  echo "error: installer payload must not contain /Applications entries" >&2
  exit 1
fi
component=$tmp/Natives-component.pkg
set -- --root "$payload" --identifier com.natives.file-manager --version "$version" --install-location /
[ -z "$pkg_sign" ] || set -- "$@" --sign "$pkg_sign"
pkgbuild "$@" "$component"
set -- --package "$component"
[ -z "$product_sign" ] || set -- "$@" --sign "$product_sign"
mkdir -p "$(dirname "$output")"
productbuild "$@" "$output"
if [ -z "$pkg_sign" ] || [ -z "$product_sign" ]; then
  echo "created unsigned development package ($mode mode): $output (supply --pkg-sign and --product-sign for release signing)"
else
  echo "created signed package ($mode mode): $output"
fi
