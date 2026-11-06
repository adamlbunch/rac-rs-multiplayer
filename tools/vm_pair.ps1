# Host on every network of this PC; the joiner reaches it only through a relay on another machine (the WSL VM), which delays,
# reorders and drops like the internet. Logs in %TEMP%\racmp\vm-host-PORT and vm-join-PORT.
param([int]$Port = 2410, [string]$Relay, [int]$RelayPort = 2411, [int]$HostFields = 5000, [int]$JoinFields = 4900, [string]$Env = '', [string]$Level = '', [string]$JoinPad = '', [string]$HostPad = '')
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$p1 = Join-Path $env:TEMP "racmp\vm-host-$Port"
$p2 = Join-Path $env:TEMP "racmp\vm-join-$Port"
Remove-Item -Recurse -Force $p1, $p2 -ErrorAction SilentlyContinue
$start = if ($Level) { 2700 } else { 1560 }
$swing = (0..400 | ForEach-Object { $f = $start + $_ * 25; "$f-$($f+5):80:80,80" }) -join ';'
foreach ($v in 'MPDBG_TOUR','MPDBG_GOTO','MPDBG_MAKE','MPDBG_FOLLOW','RAC_MP_FOLLOW','MPDBG_X') { Remove-Item "Env:$v" -ErrorAction SilentlyContinue }
foreach ($kv in @($Env.Split(';') | Where-Object { $_ })) { $k, $v = $kv.Split('=', 2); if (-not $v) { $v = '1' }; Set-Item "Env:$k" $v }
$hpad = if ($HostPad) { $HostPad } else { $swing }
$a1 = @{ Mp = "host:$Port"; Name = 'Host'; Player = $p1; Fields = $HostFields; Pad = $hpad; Hidden = $true }
$jpad = if ($JoinPad) { $JoinPad } else { $swing }
$a2 = @{ Mp = "join:${Relay}:$RelayPort"; Name = 'Joiner'; Player = $p2; Fields = $JoinFields; Pad = $jpad; Hidden = $true }
if ($Level) { $a1.Travel = "${Level}:1800"; $a2.Travel = "${Level}:1800" }
$a = & "$root\tools\mp_play.ps1" @a1
Start-Sleep -Seconds 2
$b = & "$root\tools\mp_play.ps1" @a2
$ids = @($a, $b) | ForEach-Object { if ("$_" -match 'pid (\d+)') { [int]$Matches[1] } }
$ids | ForEach-Object { Wait-Process -Id $_ -Timeout 600 -ErrorAction SilentlyContinue }
Get-Process -Id $ids -ErrorAction SilentlyContinue | Stop-Process -Force
"done" | Set-Content "$p1\done.txt"
