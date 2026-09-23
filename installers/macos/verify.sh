#!/bin/sh
# D11：当前路线的候选核验（不再断言旧"禁止 /Applications"）。
set -eu
[ "$#" -eq 1 ] || { echo "usage: $0 <candidate.pkg>" >&2; exit 2; }
pkg=$1
tmp=$(mktemp -d "${TMPDIR:-/tmp}/natives-verify.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM
pkgutil --expand-full "$pkg" "$tmp/expanded" >/dev/null
payload="$tmp/expanded/Natives-component.pkg/Payload"
fail() { echo "VERIFY FAIL: $1" >&2; exit 1; }
app=$(find "$payload/Applications" -mindepth 1 -maxdepth 1 -name '*.app' | head -1)
[ -n "$app" ] || fail "no main .app"
[ -f "$app/Contents/Info.plist" ] || fail "Info.plist missing"
[ -x "$app/Contents/MacOS/Natives" ] || fail "main executable missing"
[ -f "$app/Contents/Resources/natives.icns" ] || fail "app icon missing"
[ -f "$app/Contents/Resources/onboarding/index.html" ] || fail "onboarding html missing"
if grep -rq "__VERSION__\|__EXTENSION_DIR__" "$app/Contents/Resources/onboarding/"; then fail "onboarding placeholders"; fi
# 模式隔离（plan §5 P1）：按主入口名称推断模式，核验对应唯一 .app、
# 系统源与模式专属 Host 注册名；两种模式不得同时出现在同一候选内。
case "$(basename "$app")" in
  "Natives.app") nm_core=com.natives.file_manager; nm_model=com.natives.model_host; nm_app_runtime=com.natives.app_runtime;;
  "Natives Local.app") nm_core=com.natives.local.file_manager; nm_model=com.natives.local.model_host; nm_app_runtime=com.natives.local.app_runtime;;
  *) fail "unexpected main .app name: $(basename "$app")";;
esac
other=$(find "$payload/Applications" -mindepth 1 -maxdepth 1 -name '*.app' ! -name "$(basename "$app")")
[ -z "$other" ] || fail "unexpected second .app in /Applications: $other"
[ -d "$payload/Library/Application Support/Natives-Local" ] || [ -d "$payload/Library/Application Support/Natives" ] || fail "system source missing"
src=$(find "$payload/Library/Application Support" -maxdepth 1 -type d \( -name 'Natives-Local' -o -name 'Natives' \) | head -1)
[ -f "$src/extension-id" ] || fail "extension ID receipt missing"
extension_id=$(cat "$src/extension-id")
for browser in "Library/Google/Chrome/NativeMessagingHosts" "Library/Application Support/Chromium/NativeMessagingHosts"; do
  core_manifest="$payload/$browser/$nm_core.json"
  model_manifest="$payload/$browser/$nm_model.json"
  app_runtime_manifest="$payload/$browser/$nm_app_runtime.json"
  [ -f "$core_manifest" ] || fail "core NM manifest missing ($browser)"
  [ -f "$model_manifest" ] || fail "model NM manifest missing ($browser)"
  [ -f "$app_runtime_manifest" ] || fail "app-runtime NM manifest missing ($browser)"
  grep -Fq "\"name\": \"$nm_core\"" "$core_manifest" || fail "core NM manifest name mismatch ($browser)"
  grep -Fq "\"name\": \"$nm_model\"" "$model_manifest" || fail "model NM manifest name mismatch ($browser)"
  grep -Fq "\"name\": \"$nm_app_runtime\"" "$app_runtime_manifest" || fail "app-runtime NM manifest name mismatch ($browser)"
  grep -Fq "chrome-extension://$extension_id/" "$core_manifest" || fail "core NM origin mismatch ($browser)"
  grep -Fq "chrome-extension://$extension_id/" "$model_manifest" || fail "model NM origin mismatch ($browser)"
  grep -Fq "chrome-extension://$extension_id/" "$app_runtime_manifest" || fail "app-runtime NM origin mismatch ($browser)"
done
[ -f "$src/native-file-host" ] || fail "core host missing"
[ -f "$src/model-host" ] || fail "model host missing"
[ -f "$src/hosts/natives-app-runtime" ] || fail "unified app runtime missing (ADR-0031)"
[ -f "$src/ChromeExtension/manifest.json" ] || fail "extension dir missing"
[ -f "$src/product-manifest.json" ] && [ -f "$src/product-manifest.sig" ] || fail "product manifest missing"

# Product Manifest Schema 2（ADR-0031）：唯一 appRuntime 声明，无 per-app artifact
grep -Fq '"schemaVersion": 2' "$src/product-manifest.json" || fail "product manifest must be schema 2"
grep -Fq '"appRuntime"' "$src/product-manifest.json" || fail "product manifest must declare unified appRuntime"

# 反向验证（计划 §35.2）：旧架构载荷必须为 0
fund_host_count=$(find "$payload" -name 'fund-host' -o -name 'fund-host-*' 2>/dev/null | wc -l | tr -d ' ')
[ "$fund_host_count" = "0" ] || fail "legacy fund-host payload found: $fund_host_count"
per_app_manifest_count=$(find "$payload" -path '*NativeMessagingHosts*' -name 'com.natives.app.*.json' ! -name 'com.natives.app_runtime.json' ! -name 'com.natives.local.app_runtime.json' 2>/dev/null | wc -l | tr -d ' ')
[ "$per_app_manifest_count" = "0" ] || fail "legacy per-app NM manifest found: $per_app_manifest_count"
runtime_payload_count=$(find "$payload" -path '*runtime/*/app' 2>/dev/null | wc -l | tr -d ' ')
[ "$runtime_payload_count" = "0" ] || fail "legacy runtime/<version>/app payload found: $runtime_payload_count"

[ ! -d "$payload/Library/Application Support/Google/Chrome/External Extensions" ] || fail "External Extensions must not ship"
echo "VERIFY OK: $pkg"
