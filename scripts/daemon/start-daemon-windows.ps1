# Windows: start natives-agent-daemon sidecar with secure bootstrap (not logged).
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Root) { $Root = (Get-Location).Path }

$RuntimeDir = if ($env:NATIVES_RUNTIME_DIR) { $env:NATIVES_RUNTIME_DIR } else { Join-Path $env:USERPROFILE ".natives\runtime" }
New-Item -ItemType Directory -Force -Path $RuntimeDir | Out-Null

$Socket = if ($env:NATIVES_DAEMON_SOCKET) { $env:NATIVES_DAEMON_SOCKET } else { Join-Path $RuntimeDir "natives-agent.sock" }
$PidFile = if ($env:NATIVES_DAEMON_PID) { $env:NATIVES_DAEMON_PID } else { Join-Path $RuntimeDir "natives-agent.pid" }
$BootstrapFile = if ($env:NATIVES_DAEMON_BOOTSTRAP_FILE) { $env:NATIVES_DAEMON_BOOTSTRAP_FILE } else { Join-Path $RuntimeDir "bootstrap.token" }
$DbPath = if ($env:NATIVES_DB_PATH) { $env:NATIVES_DB_PATH } else { Join-Path $env:USERPROFILE ".natives\natives.db" }

$Bin = $env:NATIVES_DAEMON_BIN
if (-not $Bin) {
  $rel = Join-Path $Root "target\release\natives-agent-daemon.exe"
  $dbg = Join-Path $Root "target\debug\natives-agent-daemon.exe"
  if (Test-Path $rel) { $Bin = $rel }
  elseif (Test-Path $dbg) { $Bin = $dbg }
  else { $Bin = "natives-agent-daemon.exe" }
}

if (-not $env:NATIVES_DAEMON_BOOTSTRAP) {
  $bytes = New-Object byte[] 32
  [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
  $bootstrap = ($bytes | ForEach-Object { $_.ToString("x2") }) -join ""
  $env:NATIVES_DAEMON_BOOTSTRAP = $bootstrap
}
Set-Content -Path $BootstrapFile -Value $env:NATIVES_DAEMON_BOOTSTRAP -NoNewline
# Restrict ACL best-effort
try {
  icacls $BootstrapFile /inheritance:r /grant:r "$env:USERNAME:(R)" | Out-Null
} catch {}

$env:NATIVES_DAEMON_MODE = if ($env:NATIVES_DAEMON_MODE) { $env:NATIVES_DAEMON_MODE } else { "uds" }
$env:NATIVES_DAEMON_SOCKET = $Socket
$env:NATIVES_RUNTIME_DIR = $RuntimeDir
$env:NATIVES_DB_PATH = $DbPath
$env:NATIVES_REQUIRE_UDS = if ($env:NATIVES_REQUIRE_UDS) { $env:NATIVES_REQUIRE_UDS } else { "1" }

$stdout = Join-Path $RuntimeDir "daemon.stdout.log"
$stderr = Join-Path $RuntimeDir "daemon.stderr.log"
# Daemon reads NATIVES_DAEMON_SOCKET / NATIVES_DAEMON_BOOTSTRAP from process env.
$proc = Start-Process -FilePath $Bin `
  -PassThru -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr
Set-Content -Path $PidFile -Value $proc.Id -NoNewline

$ready = $false
for ($i = 0; $i -lt 50; $i++) {
  if (Test-Path $Socket) { $ready = $true; break }
  Start-Sleep -Milliseconds 100
}
if ($ready) {
  Write-Host "daemon ready socket=$Socket pid=$($proc.Id)"
  Write-Host "bootstrap_file=$BootstrapFile (not printed)"
  exit 0
}
Write-Error "daemon failed to create socket within 5s"
exit 1
