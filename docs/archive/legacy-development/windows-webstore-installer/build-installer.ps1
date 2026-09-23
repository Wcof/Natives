[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)] [string] $ExtensionId,
  [string] $HostPath = (Join-Path $PSScriptRoot 'natives-native-host.exe'),
  [string] $OutputPath = (Join-Path $PSScriptRoot 'Natives-Setup.exe'),
  [switch] $DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($ExtensionId -notmatch '^[a-p]{32}$') { throw 'ExtensionId must be the real 32-character Chrome Web Store ID (a-p only).' }
$hostFullPath = [IO.Path]::GetFullPath($HostPath)
$packageRoot = [IO.Path]::GetFullPath($PSScriptRoot)
if (-not (Test-Path -LiteralPath $hostFullPath -PathType Leaf) -or [IO.Path]::GetExtension($hostFullPath) -ine '.exe') { throw "Prebuilt host .exe not found: $HostPath" }
if ([IO.Path]::GetDirectoryName($hostFullPath) -ne $packageRoot) { throw 'HostPath must be beside build-installer.ps1 for a deterministic one-directory package.' }
$outputFullPath = [IO.Path]::GetFullPath($OutputPath)
$sedPath = Join-Path $env:TEMP ("natives-" + $ExtensionId + '.sed')
$plan = @("IExpress package: $outputFullPath", "source directory: $packageRoot", "install command: install.ps1 -ExtensionId $ExtensionId")
if ($DryRun) { $plan | ForEach-Object { "DRY-RUN $_" }; exit 0 }
$iexpress = Get-Command iexpress.exe -ErrorAction SilentlyContinue
if (-not $iexpress) { throw 'IExpress is unavailable. Run this build on Windows with the inbox IExpress tool; no fallback installer is fabricated.' }

$sed = @"
[Version]
Class=IEXPRESS
SEDVersion=3
[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=0
HideExtractAnimation=1
UseLongFileName=1
InsideCompressed=1
CAB_FixedSize=0
RebootMode=I
InstallPrompt=
DisplayLicense=
FinishMessage=
TargetName=$outputFullPath
FriendlyName=Natives
AppLaunched=powershell.exe -NoProfile -ExecutionPolicy Bypass -File install.ps1 -ExtensionId $ExtensionId
PostInstallCmd=<None>
AdminQuietInstCmd=
UserQuietInstCmd=
SourceFiles=SourceFiles
[SourceFiles]
SourceFiles0=$packageRoot
[SourceFiles0]
build-installer.ps1=
install.ps1=
launch-workbench.ps1=
native-host-manifest.json=
natives-native-host.exe=
uninstall.ps1=
"@
$sed | Set-Content -LiteralPath $sedPath -Encoding ASCII
try {
  & $iexpress.Source /N /Q $sedPath
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $outputFullPath -PathType Leaf)) { throw 'IExpress failed to produce the installer.' }
} finally {
  Remove-Item -LiteralPath $sedPath -Force -ErrorAction SilentlyContinue
}
Write-Output "Built $outputFullPath"
