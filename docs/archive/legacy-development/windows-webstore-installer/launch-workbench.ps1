[CmdletBinding()]
param([string] $ExtensionId)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($ExtensionId -notmatch '^[a-p]{32}$') { throw 'Pass the real 32-character Chrome Web Store extension ID.' }
$url = "chrome-extension://$ExtensionId/newtab.html"
$chrome = @(
  (Join-Path ${env:ProgramFiles} 'Google\Chrome\Application\chrome.exe'),
  (Join-Path ${env:LOCALAPPDATA} 'Google\Chrome\Application\chrome.exe')
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
if (-not $chrome) { throw 'Google Chrome was not found.' }
Start-Process -FilePath $chrome -ArgumentList $url
