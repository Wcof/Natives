[CmdletBinding()]
param([string] $ExtensionId)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if ($ExtensionId -and $ExtensionId -notmatch '^[a-p]{32}$') { throw 'Invalid extension ID.' }
foreach ($file in @('build-installer.ps1', 'install.ps1', 'uninstall.ps1', 'launch-workbench.ps1', 'native-host-manifest.json', 'README.md')) {
  if (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot $file) -PathType Leaf)) { throw "Missing asset: $file" }
}
$manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'native-host-manifest.json') -Raw | ConvertFrom-Json
if ($manifest.name -ne 'com.natives.file_manager' -or $manifest.type -ne 'stdio') { throw 'Manifest contract mismatch.' }
if ($manifest.allowed_origins.Count -ne 1 -or $manifest.allowed_origins[0] -notmatch '^chrome-extension://__EXTENSION_ID__/$') { throw 'Manifest ID placeholder mismatch.' }
if ((Get-Content -LiteralPath (Join-Path $PSScriptRoot 'install.ps1') -Raw) -match 'RunOnce|Services|Startup|schtasks|Electron|Tauri') { throw 'Forbidden persistence/runtime reference.' }
if ((Get-Content -LiteralPath (Join-Path $PSScriptRoot 'launch-workbench.ps1') -Raw) -notmatch 'chrome-extension://\$ExtensionId/newtab\.html') { throw 'Launcher must open the lightweight newtab surface.' }
if ((Get-Content -LiteralPath (Join-Path $PSScriptRoot 'install.ps1') -Raw) -notmatch 'CreateShortcut|Natives 卸载\.lnk') { throw 'Start Menu launch/uninstall shortcuts are missing.' }
if ((Get-Content -LiteralPath (Join-Path $PSScriptRoot 'uninstall.ps1') -Raw) -notmatch 'Natives\.lnk|Natives 卸载\.lnk') { throw 'Uninstall shortcut cleanup is missing.' }
Write-Output "PASS: Windows installer assets are present and statically valid ($root)"
