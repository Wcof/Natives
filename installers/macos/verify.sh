#!/bin/sh
set -eu
dir=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
sh -n "$dir/build-pkg.sh" "$dir/uninstall.sh" "$dir/resources/natives-launch.sh.in" "$dir/resources/uninstall-wrapper.command.in"
grep -Fq 'pkgbuild' "$dir/build-pkg.sh"
grep -Fq 'productbuild' "$dir/build-pkg.sh"
grep -Fq '__EXTENSION_ID__' "$dir/resources/native-host-manifest.json.in"
grep -Fq 'https://clients2.google.com/service/update2/crx' "$dir/resources/external-extension.json.in"
grep -Fq 'osascript' "$dir/resources/uninstall-wrapper.command.in"
! grep -Fq 'rm ' "$dir/resources/uninstall-wrapper.command.in"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/natives-installer-check.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM
mkdir -p "$tmp/Library/Application Support/Natives" "$tmp/Library/Google/Chrome/NativeMessagingHosts" "$tmp/Applications/Natives Workbench.app/Contents/MacOS"
touch "$tmp/Library/Application Support/Natives/native-file-host" "$tmp/Library/Application Support/Natives/extension-id"
touch "$tmp/Library/Application Support/Natives/uninstall.sh"
touch "$tmp/Library/Google/Chrome/NativeMessagingHosts/com.natives.file_manager.json"
touch "$tmp/Applications/Natives Workbench.app/Contents/Info.plist" "$tmp/Applications/Natives Workbench.app/Contents/MacOS/natives-launch"
touch "$tmp/Applications/Natives Workbench Uninstall.command"
sh "$dir/uninstall.sh" --root "$tmp" >/dev/null
[ ! -e "$tmp/Library/Application Support/Natives/native-file-host" ]
[ ! -e "$tmp/Applications/Natives Workbench.app/Contents/Info.plist" ]
[ ! -e "$tmp/Applications/Natives Workbench Uninstall.command" ]
echo 'macOS installer static and temporary-layout checks passed.'
