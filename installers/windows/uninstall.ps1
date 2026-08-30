[CmdletBinding(SupportsShouldProcess)]
param(
  [Parameter(Mandatory = $true)] [string] $ExtensionId,
  [switch] $DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$localAppData = [Environment]::GetFolderPath('LocalApplicationData')
$installRoot = Join-Path $localAppData 'Natives'
$chromeNativeKey = 'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.natives.file_manager'
$chromeExtensionKey = "HKCU:\Software\Google\Chrome\Extensions\$ExtensionId"
$startMenu = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Microsoft\Windows\Start Menu\Programs'
$launchShortcut = Join-Path $startMenu 'Natives.lnk'
$uninstallShortcut = Join-Path $startMenu 'Natives 卸载.lnk'
if ($ExtensionId -notmatch '^[a-p]{32}$') { throw 'ExtensionId must be the real 32-character Chrome Web Store ID (a-p only).' }

function Assert-InstallRoot([string] $Path) {
  $expected = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Natives'
  if ([IO.Path]::GetFullPath($Path) -ne [IO.Path]::GetFullPath($expected)) { throw "Refusing destructive path outside $expected" }
}

Assert-InstallRoot $installRoot
if ($DryRun) {
  @(
    "remove files: $installRoot (exact Natives install directory only)",
    "remove Start Menu shortcuts: $launchShortcut; $uninstallShortcut",
    "remove registry key: $chromeNativeKey",
    "remove registry key: $chromeExtensionKey"
  ) | ForEach-Object { "DRY-RUN $_" }
  exit 0
}

foreach ($shortcut in @($launchShortcut, $uninstallShortcut)) {
  if (Test-Path -LiteralPath $shortcut) { Remove-Item -LiteralPath $shortcut -Force }
}
if (Test-Path -LiteralPath $chromeNativeKey) { Remove-Item -LiteralPath $chromeNativeKey -Recurse -Force }
if (Test-Path -LiteralPath $chromeExtensionKey) { Remove-Item -LiteralPath $chromeExtensionKey -Recurse -Force }
if (Test-Path -LiteralPath $installRoot) { Remove-Item -LiteralPath $installRoot -Recurse -Force }
Write-Output 'Uninstalled Natives Windows assets.'
