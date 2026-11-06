# vm_game_pair.ps1 - a pair across two machines: the host on this PC (Windows), the joiner on a second one, the WSL Linux VM,
# with its own Linux build of the game (setup.sh), its own copy of the disc and its own network address, reaching the host
# through this PC's firewall as a second PC on the network would. Both run hidden and muted (the joiner on a virtual screen,
# tools/vm_game.sh). Logs: %TEMP%\racmp\vmg-host-PORT\log.txt here, VMGAME/../join-PORT/log.txt in the VM (also copied next to
# the host's as join-log.txt).
#
#   powershell -ExecutionPolicy Bypass -File tools\vm_game_pair.ps1 [-Level 1] [-HostFields 6000] [-JoinFields 5000]
#
# The VM is slower (no GPU there: the software Vulkan driver draws), so the joiner starts first and the host when the joiner
# nears play; the joiner asks again until the host answers.
param(
	[int]$Port = 2410,
	[string]$HostAddr = '',          # the host as the VM reaches it; default: the VM's gateway, this PC
	[int]$HostFields = 6000,
	[int]$JoinFields = 5000,
	[string]$Level = '',             # travel both there at field 1800 (1 Novalis, 3 Kerwan, ...)
	[string]$HostPad = '',
	[string]$JoinPad = '',
	[string]$Distro = 'Ubuntu',
	[string]$VmGame = '~/racvm/game',
	[string]$VmIso = '~/racvm/usa.iso',
	[int]$HostAtJoinField = 1000     # start the host once the joiner's log reaches this field
)
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$p1 = Join-Path $env:TEMP "racmp\vmg-host-$Port"
Remove-Item -Recurse -Force $p1 -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $p1 | Out-Null
if (-not $HostAddr) {
	$route = (& wsl.exe -d $Distro -- ip -4 route show default) -join ' '
	if ($route -match 'via (\d+\.\d+\.\d+\.\d+)') { $HostAddr = $Matches[1] } else { throw "no default route in the VM: $route" }
}
$start = if ($Level) { 2700 } else { 1560 }
$swing = (0..400 | ForEach-Object { $f = $start + $_ * 25; "$f-$($f+5):80:80,80" }) -join ';'
$hpad = if ($HostPad) { $HostPad } else { $swing }
$jpad = if ($JoinPad) { $JoinPad } else { $swing }
$travel = if ($Level) { "--travel ${Level}:1800" } else { '' }
$vmPlayer = "$VmGame/../join-$Port"

# The joiner, in the VM.
$cmd = "rm -rf $vmPlayer; $VmGame/tools/vm_game.sh --mp join:${HostAddr}:$Port --name Joiner --player $vmPlayer --fields $JoinFields --pad '$jpad' --iso $VmIso $travel"
$joinOut = Join-Path $p1 'vm-joiner.txt'
$j = Start-Process -FilePath wsl.exe -ArgumentList @('-d', $Distro, '--', 'bash', '-lc', "`"$cmd`"") -WindowStyle Hidden -PassThru -RedirectStandardOutput $joinOut -RedirectStandardError (Join-Path $p1 'vm-joiner-err.txt')
Write-Output "joiner in the VM (wsl pid $($j.Id)), joining ${HostAddr}:$Port"

# The host here, once the joiner nears play.
$vmLog = "\\wsl.localhost\$Distro" + ((& wsl.exe -d $Distro -- bash -lc "realpath -m $vmPlayer") -join '') + '/log.txt'
$t0 = Get-Date
while ($true) {
	Start-Sleep -Seconds 5
	if ($j.HasExited) { break }
	$last = if (Test-Path -LiteralPath $vmLog) { Get-Content -LiteralPath $vmLog -Tail 200 | Select-String -Pattern 'smoke: field (\d+)' -AllMatches | Select-Object -Last 1 } else { $null }
	$f = if ($last) { [int]$last.Matches[-1].Groups[1].Value } else { 0 }
	if ($f -ge $HostAtJoinField -or ((Get-Date) - $t0).TotalMinutes -gt 15) { break }
}
$a1 = @{ Mp = "host:$Port"; Name = 'Host'; Player = $p1; Fields = $HostFields; Pad = $hpad; Hidden = $true }
if ($Level) { $a1.Travel = "${Level}:1800" }
$a = & "$root\tools\mp_play.ps1" @a1
$hid = if ("$a" -match 'pid (\d+)') { [int]$Matches[1] } else { 0 }
Write-Output "host here: $a"
Wait-Process -Id $hid -Timeout 1800 -ErrorAction SilentlyContinue
Get-Process -Id $hid -ErrorAction SilentlyContinue | Stop-Process -Force
$j | Wait-Process -Timeout 600 -ErrorAction SilentlyContinue
if (-not $j.HasExited) { & wsl.exe -d $Distro -- pkill -f 'build/release/rac-rs' | Out-Null; $j | Wait-Process -Timeout 30 -ErrorAction SilentlyContinue }
if (Test-Path -LiteralPath $vmLog) { Copy-Item -LiteralPath $vmLog (Join-Path $p1 'join-log.txt') }
"done" | Set-Content "$p1\done.txt"
