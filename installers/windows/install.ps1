[CmdletBinding(SupportsShouldProcess)]
param(
  [Parameter(Mandatory = $true)] [string] $ExtensionId,
  [string] $HostPath = (Join-Path $PSScriptRoot 'natives-native-host.exe'),
  [switch] $DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-ExtensionId([string] $Value) {
  if ($Value -notmatch '^[a-p]{32}$') {
    throw 'ExtensionId must be the real 32-character Chrome Web Store ID (a-p only).'
  }
}

Assert-ExtensionId $ExtensionId
$localAppData = [Environment]::GetFolderPath('LocalApplicationData')
$installRoot = Join-Path $localAppData 'Natives'
$manifestPath = Join-Path $installRoot 'native-host-manifest.json'
$installedHost = Join-Path $installRoot 'natives-native-host.exe'
$installedLauncher = Join-Path $installRoot 'launch-workbench.ps1'
$installedUninstaller = Join-Path $installRoot 'uninstall.ps1'
$startMenu = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Microsoft\Windows\Start Menu\Programs'
$launchShortcut = Join-Path $startMenu 'Natives.lnk'
$uninstallShortcut = Join-Path $startMenu 'Natives 卸载.lnk'
$chromeNativeKey = 'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.natives.file_manager'
$chromeExtensionKey = "HKCU:\Software\Google\Chrome\Extensions\$ExtensionId"

if (-not (Test-Path -LiteralPath $HostPath -PathType Leaf)) {
  throw "Prebuilt host not found: $HostPath"
}
if ([IO.Path]::GetExtension($HostPath) -ine '.exe') {
  throw 'HostPath must point to a Windows .exe release host.'
}

$plan = @(
  "install host: $installedHost",
  "write manifest: $manifestPath",
  "install launcher: $installedLauncher",
  "install uninstaller: $installedUninstaller",
  "create Start Menu shortcuts: $launchShortcut; $uninstallShortcut",
  "register native host: $chromeNativeKey",
  "register Web Store extension: $chromeExtensionKey",
  'no service/startup/tray/background updater'
)
if ($DryRun) {
  $plan | ForEach-Object { "DRY-RUN $_" }
  exit 0
}

New-Item -ItemType Directory -Force -Path $installRoot | Out-Null
Copy-Item -LiteralPath $HostPath -Destination $installedHost -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'launch-workbench.ps1') -Destination $installedLauncher -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'uninstall.ps1') -Destination $installedUninstaller -Force
$manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'native-host-manifest.json') -Raw
$manifest = $manifest.Replace('__INSTALL_ROOT__', $installRoot.Replace('\', '\\')).Replace('__EXTENSION_ID__', $ExtensionId)
$manifest | Set-Content -LiteralPath $manifestPath -Encoding UTF8 -NoNewline

New-Item -Path $chromeNativeKey -Force | Out-Null
Set-Item -LiteralPath $chromeNativeKey -Value $manifestPath
New-Item -Path $chromeExtensionKey -Force | Out-Null
New-ItemProperty -LiteralPath $chromeExtensionKey -Name update_url -PropertyType String -Value 'https://clients2.google.com/service/update2/crx' -Force | Out-Null

New-Item -ItemType Directory -Force -Path $startMenu | Out-Null
$shell = New-Object -ComObject WScript.Shell
foreach ($shortcutSpec in @(
  @{ Path = $launchShortcut; Script = $installedLauncher; Description = 'Open Natives workbench' },
  @{ Path = $uninstallShortcut; Script = $installedUninstaller; Description = 'Uninstall Natives' }
)) {
  $shortcut = $shell.CreateShortcut($shortcutSpec.Path)
  $shortcut.TargetPath = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
  $shortcut.Arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$($shortcutSpec.Script)`" -ExtensionId $ExtensionId"
  $shortcut.WorkingDirectory = $installRoot
  $shortcut.Description = $shortcutSpec.Description
  $shortcut.Save()
}

# One-shot handoff to Chrome; the launcher starts Chrome and exits immediately.
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $installedLauncher -ExtensionId $ExtensionId

Write-Output "Installed Natives Windows assets in $installRoot"
