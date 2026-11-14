# ap_play.ps1 - starts the game with the Archipelago randomizer on (RAC_AP): it listens on 127.0.0.1:PORT for the
# "Ratchet & Clank Client" of the Archipelago Launcher (archipelago/rac1). Other multiplayer or launcher settings are untouched.
#
#   powershell -ExecutionPolicy Bypass -File tools\ap_play.ps1 [-Port 28111]
param([int]$Port = 28111)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$exe = Join-Path $root 'build\release\rac-rs.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw "no game at ${exe}: run setup.ps1 first" }
$env:RAC_AP = "$Port"
Start-Process -FilePath $exe -WorkingDirectory $root | Out-Null
Write-Output "started the game with Archipelago on port $Port; start the Ratchet & Clank Client in the Archipelago Launcher"
