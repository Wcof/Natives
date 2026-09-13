#!/bin/sh
set -eu
dir=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
sh -n "$dir/build-pkg.sh" "$dir/uninstall.sh"
grep -Fq 'pkgbuild' "$dir/build-pkg.sh"
grep -Fq 'productbuild' "$dir/build-pkg.sh"
grep -Fq '__EXTENSION_ID__' "$dir/resources/native-host-manifest.json.in"
grep -Fq 'https://clients2.google.com/service/update2/crx' "$dir/resources/external-extension.json.in"
# ADR-0029 §2: the installer never creates .app bundles or /Applications
# entries; the Workbench templates must not exist.
grep -Fq 'must not contain /Applications entries' "$dir/build-pkg.sh"
if grep -Fq 'Workbench' "$dir/build-pkg.sh"; then
  echo 'error: build-pkg.sh still references Workbench' >&2
  exit 1
fi
if [ -e "$dir/resources/natives-launch.sh.in" ] || [ -e "$dir/resources/Info.plist.in" ] || [ -e "$dir/resources/uninstall-wrapper.command.in" ]; then
  echo 'error: legacy Workbench templates must be removed' >&2
  exit 1
fi
# uninstall.sh still cleans legacy Workbench leftovers by known path.
tmp=$(mktemp -d "${TMPDIR:-/tmp}/natives-installer-check.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM
mkdir -p "$tmp/Library/Application Support/Natives" "$tmp/Library/Google/Chrome/NativeMessagingHosts" "$tmp/Applications/Natives Workbench.app/Contents/MacOS"
touch "$tmp/Library/Application Support/Natives/native-file-host" "$tmp/Library/Application Support/Natives/extension-id"
touch "$tmp/Library/Application Support/Natives/uninstall.sh"
touch "$tmp/Library/Google/Chrome/NativeMessagingHosts/com.natives.file_manager.json"
touch "$tmp/Applications/Natives Workbench.app/Contents/Info.plist" "$tmp/Applications/Natives Workbench.app/Contents/MacOS/natives-launch"
sh "$dir/uninstall.sh" --root "$tmp" >/dev/null
[ ! -e "$tmp/Library/Application Support/Natives/native-file-host" ]
[ ! -e "$tmp/Applications/Natives Workbench.app/Contents/Info.plist" ]
echo 'macOS installer static and temporary-layout checks passed.'
