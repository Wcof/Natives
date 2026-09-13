#!/bin/sh
set -eu
# The single Natives system install engine (plan §5 P1, ADR-0029).
#
# Modes (explicit, never inferred from flags):
#   local      — isolated local candidate into
#                /Library/Application Support/Natives-Local/ (contract §3.2/§4.2);
#                ad-hoc/development signing and a development-built Host are
#                allowed here.
#   production — complete suite candidate into
#                /Library/Application Support/Natives/; REQUIRES the model Host,
#                the unpacked extension directory, the fixed module files, a
#                signed product composition manifest and all signing identities.
#
# The installer only writes the restricted root-owned system source and the
# minimal browser Native Messaging registration (contract §3.2). It never
# creates .app bundles, /Applications entries, Dock/LaunchServices products,
# External Extensions store entries, or user data/activation.
usage() {
  echo "usage: $0 --mode local|production --host PATH --extension-id ID --extension-dir PATH \
--modules-dir PATH --product-manifest PATH [--model-host PATH] [--launcher PATH] \
[--version V] [--output PATH] [--pkg-sign ID] [--product-sign ID]" >&2
  exit 2
}
mode= host= extension_id= extension_dir= modules_dir= product_manifest= model_host= launcher=
version=1.0.0 output= pkg_sign= product_sign=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode) mode=${2:-}; shift 2;;
    --host) host=${2:-}; shift 2;;
    --model-host) model_host=${2:-}; shift 2;;
    --launcher) launcher=${2:-}; shift 2;;
    --extension-id) extension_id=${2:-}; shift 2;;
    --extension-dir) extension_dir=${2:-}; shift 2;;
    --modules-dir) modules_dir=${2:-}; shift 2;;
    --product-manifest) product_manifest=${2:-}; shift 2;;
    --version) version=${2:-}; shift 2;;
    --output) output=${2:-}; shift 2;;
    --pkg-sign) pkg_sign=${2:-}; shift 2;;
    --product-sign) product_sign=${2:-}; shift 2;;
    *) usage;;
  esac
done
case "$mode" in
  local) source_name=Natives-Local;;
  production) source_name=Natives;;
  *) echo 'error: --mode must be explicitly local or production' >&2; exit 1;;
esac
[ -n "$host" ] && [ -x "$host" ] || { echo 'error: --host must be an executable prebuilt Host' >&2; exit 1; }
[ -n "$extension_dir" ] && [ -f "$extension_dir/manifest.json" ] || { echo 'error: --extension-dir must be the unpacked extension directory containing manifest.json' >&2; exit 1; }
[ -n "$modules_dir" ] && [ -d "$modules_dir" ] || { echo 'error: --modules-dir must contain the fixed module files' >&2; exit 1; }
[ -n "$product_manifest" ] && [ -f "$product_manifest" ] || { echo 'error: --product-manifest must be the signed product composition manifest' >&2; exit 1; }
[ -f "$product_manifest.sig" ] || { echo 'error: product manifest detached signature (.sig) is required' >&2; exit 1; }
echo "$extension_id" | grep -Eq '^[a-p]{32}$' || { echo 'error: --extension-id must be the explicit 32-character Chrome ID' >&2; exit 1; }
echo "$version" | grep -Eq '^[0-9]+(\.[0-9]+){1,3}$' || { echo 'error: invalid --version' >&2; exit 1; }
command -v pkgbuild >/dev/null 2>&1 || { echo 'error: macOS pkgbuild is required' >&2; exit 1; }
command -v productbuild >/dev/null 2>&1 || { echo 'error: macOS productbuild is required' >&2; exit 1; }
if [ "$mode" = production ]; then
  [ -n "$model_host" ] && [ -x "$model_host" ] || { echo 'error: production requires an executable --model-host' >&2; exit 1; }
  [ -n "$pkg_sign" ] || { echo 'error: production requires --pkg-sign' >&2; exit 1; }
  [ -n "$product_sign" ] || { echo 'error: production requires --product-sign' >&2; exit 1; }
fi

base=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/natives-pkg.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM
payload=$tmp/payload
source_root="$payload/Library/Application Support/$source_name"
mkdir -p "$source_root" "$payload/Library/Google/Chrome/NativeMessagingHosts" \
  "$payload/Library/Application Support/Chromium/NativeMessagingHosts"

install -m 755 "$host" "$source_root/native-file-host"
[ -z "$launcher" ] || install -m 755 "$launcher" "$source_root/natives-launcher"
if [ -n "$model_host" ] && [ -x "$model_host" ]; then
  install -m 755 "$model_host" "$source_root/model-host"
fi
# Fixed built-in module files and the UNPACKED extension directory: the
# complete product content (plan §3.1). No ZIP, no seeds, no second copy.
cp -R "$extension_dir/" "$source_root/ChromeExtension/"
cp -R "$modules_dir/" "$source_root/modules/"
install -m 644 "$product_manifest" "$source_root/product-manifest.json"
install -m 644 "$product_manifest.sig" "$source_root/product-manifest.sig"
install -m 755 "$base/uninstall.sh" "$source_root/uninstall.sh"
printf '%s\n' "$extension_id" > "$source_root/extension-id"

# Minimal browser Native Messaging registration only: the manifests point at
# the real installed system-source binaries and lock the fixed extension ID
# (contract §3.2). No External Extensions store entries (plan §5 P1).
for browser in "$payload/Library/Google/Chrome/NativeMessagingHosts" \
               "$payload/Library/Application Support/Chromium/NativeMessagingHosts"; do
  sed -e "s/__EXTENSION_ID__/$extension_id/g" -e "s|__SOURCE_ROOT__|/Library/Application Support/$source_name|g" \
    "$base/resources/native-host-manifest.json.in" > "$browser/com.natives.file_manager.json"
  if [ -n "$model_host" ] && [ -x "$model_host" ]; then
    sed -e "s/__EXTENSION_ID__/$extension_id/g" -e "s|__SOURCE_ROOT__|/Library/Application Support/$source_name|g" \
      "$base/resources/native-model-host-manifest.json.in" > "$browser/com.natives.model_host.json"
  fi
done

# Hard prohibitions (ADR-0029 §2 / plan §3.3): no .app, no /Applications.
if [ -d "$payload/Applications" ]; then
  echo "error: installer payload must not contain /Applications entries" >&2
  exit 1
fi
component=$tmp/Natives-component.pkg
set -- --root "$payload" --identifier "com.natives.$source_name" --version "$version" --install-location /
[ -z "$pkg_sign" ] || set -- "$@" --sign "$pkg_sign"
pkgbuild "$@" "$component"
set -- --package "$component"
[ -z "$product_sign" ] || set -- "$@" --sign "$product_sign"
mkdir -p "$(dirname "$output")"
productbuild "$@" "$output"
if [ -z "$pkg_sign" ] || [ -z "$product_sign" ]; then
  echo "created unsigned development package ($mode mode, source /Library/Application Support/$source_name): $output"
else
  echo "created signed package ($mode mode): $output"
fi
