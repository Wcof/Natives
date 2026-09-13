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
for browser in "Library/Google/Chrome/NativeMessagingHosts" "Library/Application Support/Chromium/NativeMessagingHosts"; do
  [ -f "$payload/$browser/com.natives.file_manager.json" ] || fail "core NM manifest missing ($browser)"
done
[ -d "$payload/Library/Application Support/Natives-Local" ] || [ -d "$payload/Library/Application Support/Natives" ] || fail "system source missing"
src=$(find "$payload/Library/Application Support" -maxdepth 1 -type d -name 'Natives*Local' -o -maxdepth 1 -type d -name 'Natives' | head -1)
[ -f "$src/native-file-host" ] || fail "core host missing"
[ -f "$src/model-host" ] || fail "model host missing"
[ -f "$src/ChromeExtension/manifest.json" ] || fail "extension dir missing"
[ -f "$src/modules/fund/0.1.0/app" ] || fail "fund payload missing"
[ -f "$src/product-manifest.json" ] && [ -f "$src/product-manifest.sig" ] || fail "product manifest missing"
[ ! -d "$payload/Library/Application Support/Google/Chrome/External Extensions" ] || fail "External Extensions must not ship"
echo "VERIFY OK: $pkg"
