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
--modules-dir PATH --product-manifest PATH --app-exec PATH --app-icon PATH \
--onboarding-dir PATH --conclusion-dir PATH [--model-host PATH] \
[--version V] [--output PATH] [--pkg-sign ID] [--product-sign ID]" >&2
  exit 2
}
mode= host= extension_id= extension_dir= modules_dir= product_manifest=
app_exec= app_icon= onboarding_dir= conclusion_dir= model_host= launcher= app_runtime=
version=1.0.0 output= pkg_sign= product_sign=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode) mode=${2:-}; shift 2;;
    --host) host=${2:-}; shift 2;;
    --app-runtime) app_runtime=${2:-}; shift 2;;
    --model-host) model_host=${2:-}; shift 2;;
    --launcher) launcher=${2:-}; shift 2;;
    --extension-id) extension_id=${2:-}; shift 2;;
    --extension-dir) extension_dir=${2:-}; shift 2;;
    --modules-dir) modules_dir=${2:-}; shift 2;;
    --product-manifest) product_manifest=${2:-}; shift 2;;
    --app-exec) app_exec=${2:-}; shift 2;;
    --app-icon) app_icon=${2:-}; shift 2;;
    --onboarding-dir) onboarding_dir=${2:-}; shift 2;;
    --conclusion-dir) conclusion_dir=${2:-}; shift 2;;
    --version) version=${2:-}; shift 2;;
    --output) output=${2:-}; shift 2;;
    --pkg-sign) pkg_sign=${2:-}; shift 2;;
    --product-sign) product_sign=${2:-}; shift 2;;
    *) usage;;
  esac
done
case "$mode" in
  local)
    source_name=Natives-Local; app_name="Natives Local.app"; setup_scheme=natives-setup-local
    # 端到端模式隔离（方案 §5 P1 第 2 条 / 契约 §4.2）：local 使用独立
    # bundle ID、显示名称、Native Messaging Host 名称与 URL scheme，
    # 绝不与 production 共用注册命名空间。
    bundle_id=com.natives.local.app; display_name="Natives Local"
    nm_core=com.natives.local.file_manager; nm_model=com.natives.local.model_host
    nm_app_runtime=com.natives.local.app_runtime
    url_name=com.natives.local.setup;;
  production)
    source_name=Natives; app_name="Natives.app"; setup_scheme=natives-setup
    bundle_id=com.natives.app; display_name="Natives"
    nm_core=com.natives.file_manager; nm_model=com.natives.model_host
    nm_app_runtime=com.natives.app_runtime
    url_name=com.natives.setup;;
  *) echo 'error: --mode must be explicitly local or production' >&2; exit 1;;
esac
[ -n "$host" ] && [ -x "$host" ] || { echo 'error: --host must be an executable prebuilt Host' >&2; exit 1; }
[ -n "$extension_dir" ] && [ -f "$extension_dir/manifest.json" ] || { echo 'error: --extension-dir must be the unpacked extension directory containing manifest.json' >&2; exit 1; }
[ -n "$modules_dir" ] && [ -d "$modules_dir" ] || { echo 'error: --modules-dir must contain the fixed module files' >&2; exit 1; }
[ -n "$product_manifest" ] && [ -f "$product_manifest" ] || { echo 'error: --product-manifest must be the signed product composition manifest' >&2; exit 1; }
[ -f "$product_manifest.sig" ] || { echo 'error: product manifest detached signature (.sig) is required' >&2; exit 1; }
[ -n "$app_exec" ] && [ -x "$app_exec" ] || { echo 'error: --app-exec must be the compiled main-entry wrapper' >&2; exit 1; }
[ -f "$app_icon" ] || { echo 'error: --app-icon (.icns) is required for the visible main entry' >&2; exit 1; }
[ -d "$onboarding_dir" ] && [ -f "$onboarding_dir/index.html" ] || { echo 'error: --onboarding-dir with index.html is required (offline guide)' >&2; exit 1; }
[ -f "$conclusion_dir/conclusion-zh.txt" ] && [ -f "$conclusion_dir/conclusion-en.txt" ] || { echo 'error: --conclusion-dir with conclusion-{zh,en}.txt is required' >&2; exit 1; }
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
mkdir -p "$source_root" "$source_root/hosts" "$payload/Library/Google/Chrome/NativeMessagingHosts" \
  "$payload/Library/Application Support/Chromium/NativeMessagingHosts"

install -m 755 "$host" "$source_root/native-file-host"
install -m 755 "$host" "$source_root/hosts/native-file-host"
if [ -n "$app_runtime" ] && [ -x "$app_runtime" ]; then
  install -m 755 "$app_runtime" "$source_root/hosts/natives-app-runtime"
fi
# 可见主入口 /Applications/Natives.app（用户决定 2026-09-13，ADR-0029 修订）：
# 正常 bundle、Info.plist、图标与引导资源；不设 Hidden/LSUIElement。
app_root="$payload/Applications/$app_name"
mkdir -p "$app_root/Contents/MacOS" "$app_root/Contents/Resources/onboarding"
install -m 755 "$app_exec" "$app_root/Contents/MacOS/Natives"
install -m 644 "$app_icon" "$app_root/Contents/Resources/natives.icns"
cp -R "$onboarding_dir/" "$app_root/Contents/Resources/onboarding/"
printf '%s\n' "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
<plist version=\"1.0\"><dict>
  <key>CFBundleName</key><string>__DISPLAY_NAME__</string>
  <key>CFBundleDisplayName</key><string>__DISPLAY_NAME__</string>
  <key>CFBundleIdentifier</key><string>__BUNDLE_ID__</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleExecutable</key><string>Natives</string>
  <key>CFBundleIconFile</key><string>natives</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleURLTypes</key><array><dict>
    <key>CFBundleURLName</key><string>__URL_NAME__</string>
    <key>CFBundleURLSchemes</key><array><string>__SETUP_SCHEME__</string></array>
  </dict></array>
</dict></plist>" > "$app_root/Contents/Info.plist"
sed -i '' -e "s/__SETUP_SCHEME__/$setup_scheme/" -e "s/__BUNDLE_ID__/$bundle_id/" -e "s/__DISPLAY_NAME__/$display_name/" -e "s/__URL_NAME__/$url_name/" "$app_root/Contents/Info.plist"
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
sed -e "s|__APP_PATH__|\"/Applications/$app_name\"|" -e "s|__SOURCE_ROOT__|\"/Library/Application Support/$source_name\"|" -e "s|__NM_CORE__|$nm_core|" -e "s|__NM_MODEL__|$nm_model|" "$base/resources/uninstall-template.sh" > "$source_root/uninstall.sh"
chmod 755 "$source_root/uninstall.sh"
printf '%s\n' "$extension_id" > "$source_root/extension-id"

# Minimal browser Native Messaging registration only: the manifests point at
# the real installed system-source binaries and lock the fixed extension ID
# (contract §3.2). No External Extensions store entries (plan §5 P1).
for browser in "$payload/Library/Google/Chrome/NativeMessagingHosts" \
               "$payload/Library/Application Support/Chromium/NativeMessagingHosts"; do
  sed -e "s/__NM_CORE__/$nm_core/g" -e "s/__EXTENSION_ID__/$extension_id/g" -e "s|__SOURCE_ROOT__|/Library/Application Support/$source_name|g" \
    "$base/resources/native-host-manifest.json.in" > "$browser/$nm_core.json"
  if [ -n "$model_host" ] && [ -x "$model_host" ]; then
    sed -e "s/__NM_MODEL__/$nm_model/g" -e "s/__EXTENSION_ID__/$extension_id/g" -e "s|__SOURCE_ROOT__|/Library/Application Support/$source_name|g" \
      "$base/resources/native-model-host-manifest.json.in" > "$browser/$nm_model.json"
  fi
  if [ -n "$app_runtime" ] && [ -x "$app_runtime" ]; then
    sed -e "s/__NM_APP_RUNTIME__/$nm_app_runtime/g" -e "s/__EXTENSION_ID__/$extension_id/g" -e "s|__SOURCE_ROOT__|/Library/Application Support/$source_name|g" \
      "$base/resources/native-app-runtime-manifest.json.in" > "$browser/$nm_app_runtime.json"
  fi
done

# 精确白名单（方案 §5 P0）：/Applications 下唯一允许的条目是主产品
# Natives.app；任何其他 .app/可执行入口/模块应用仍然禁止。
if [ -d "$payload/Applications" ]; then
  extra=$(find "$payload/Applications" -mindepth 1 -maxdepth 1 ! -name "$app_name")
  [ -z "$extra" ] || { echo "error: unexpected /Applications entries:" >&2; echo "$extra" >&2; exit 1; }
  nested=$(find "$app_root" -name '*.app' -mindepth 1)
  [ -z "$nested" ] || { echo "error: nested .app inside main bundle" >&2; exit 1; }
fi
component=$tmp/Natives-component.pkg
set -- --root "$payload" --identifier "com.natives.$source_name" --version "$version" --install-location /
[ -z "$pkg_sign" ] || set -- "$@" --sign "$pkg_sign"
pkgbuild "$@" "$component"

# Distribution XML：由 productbuild --synthesize 自动生成符合 Apple 规范的
# 骨架（包含正确的 choices-outline 与 hostArchitectures），确保 Installer.app
# 评估顺利通过，不触发架构错误或取消安装；注入同源 conclusion 资源与标题（§1.3/D06）。
resources="$tmp/resources"
mkdir -p "$resources"
cp "$conclusion_dir/conclusion-zh.txt" "$resources/conclusion-zh.txt"
cp "$conclusion_dir/conclusion-en.txt" "$resources/conclusion-en.txt"
productbuild --synthesize --package "$component" "$tmp/distribution.xml"
python3 -c "
import sys
p = '$tmp/distribution.xml'
s = open(p).read()
insert = '''    <title>$display_name</title>
    <conclusion lang=\"zh_CN\" file=\"conclusion-zh.txt\" mime-type=\"text/plain\"/>
    <conclusion lang=\"en\" file=\"conclusion-en.txt\" mime-type=\"text/plain\"/>
'''
s = s.replace('</installer-gui-script>', insert + '</installer-gui-script>')
open(p, 'w').write(s)
"

set -- --distribution "$tmp/distribution.xml" --resources "$resources" --package-path "$tmp"
[ -z "$product_sign" ] || set -- "$@" --sign "$product_sign"
mkdir -p "$(dirname "$output")"
productbuild "$@" "$output"
if [ -z "$pkg_sign" ] || [ -z "$product_sign" ]; then
  echo "created unsigned development package ($mode mode, source /Library/Application Support/$source_name): $output"
else
  echo "created signed package ($mode mode): $output"
fi
