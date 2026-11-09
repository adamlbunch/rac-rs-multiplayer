# hv_pair.ps1 - a Windows-to-Windows pair across machines: the host on this PC, the joiner in the Hyper-V test VM (hv_create.ps1,
# hv_game.ps1 -Copy), reaching the host at its address on Hyper-V's Default Switch, through this PC's firewall. The joiner draws on
# the CPU (about 1 field a second), so the host walks to Veldin's four crates first and breaks them; the joiner walks there later,
# and its game must finish the same four. Logs: DIR\hv-host-log.txt and DIR\vm-join-log.txt.
param([int]$JoinFields = 3000, [int]$HostFields = 120000, [int]$HostAtJoinField = 1300, [string]$HostAddr = '',
	[string]$Name = 'RacTest', [string]$Dir = "$env:USERPROFILE\vm\hyperv")
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent (Split-Path -Parent $here)
if (-not $HostAddr) { $HostAddr = (Get-NetIPAddress -AddressFamily IPv4 -InterfaceAlias 'vEthernet (Default Switch)').IPAddress }
function Walk([int]$at) {
	# Hold forward 860 fields; from 320 fields in, swing every 24 (the stick held through the swings).
	$out = @("$at-$($at+319):0:80,00")
	for ($f = $at + 320; $f + 24 -le $at + 860; $f += 24) { $out += "$f-$($f+5):80:80,00"; $out += "$($f+6)-$($f+23):0:80,00" }
	$out
}
function Swing([int]$from, [int]$to) { for ($f = $from; $f + 25 -le $to; $f += 25) { "$f-$($f+5):80:80,80" } }
$jpad = (@(Swing 1560 1640) + @(Walk 1640) + @(Swing 2504 $JoinFields)) -join ';'
$hpad = (@(Walk 3000)) -join ';'
$j = Start-Job -ScriptBlock { param($here, $mp, $f, $pad, $name, $dir) & powershell -ExecutionPolicy Bypass -File "$here\hv_game.ps1" -Mp $mp -Fields $f -Pad $pad -Log "$dir\vm-join-log.txt" -Name $name -Dir $dir -TimeoutMin 150 } -ArgumentList $here, "join:${HostAddr}:2410", $JoinFields, $jpad, $Name, $Dir
"joiner starting in the VM, joining ${HostAddr}:2410"
$kv = @{}; Get-Content "$Dir\vm-account.txt" | ForEach-Object { $k, $v = $_ -split '=', 2; $kv[$k] = $v }
$cred = New-Object System.Management.Automation.PSCredential("RACVM\$($kv.user)", (ConvertTo-SecureString $kv.password -AsPlainText -Force))
$t0 = Get-Date
do {
	Start-Sleep -Seconds 30
	$f = Invoke-Command -VMName $Name -Credential $cred -ScriptBlock {
		$l = Get-Content C:\rac\run\log.txt -ErrorAction SilentlyContinue | Select-String -Pattern '^smoke: field (\d+)' | Select-Object -Last 1
		if ($l) { [int]$l.Matches[0].Groups[1].Value } else { 0 }
	}
} while ($f -lt $HostAtJoinField -and ((Get-Date) - $t0).TotalMinutes -lt 60 -and $j.State -eq 'Running')
$hp = Join-Path $env:TEMP 'racmp\hv-host'
Remove-Item -Recurse -Force $hp -ErrorAction SilentlyContinue
$h = & "$root\tools\mp_play.ps1" -Mp host:2410 -Name Host -Player $hp -Fields $HostFields -Pad $hpad
"host here: $h (the joiner was at field $f)"
$hid = if ("$h" -match 'pid (\d+)') { [int]$Matches[1] } else { 0 }
$j | Wait-Job | Receive-Job
Get-Process -Id $hid -ErrorAction SilentlyContinue | Stop-Process -Force
Copy-Item "$hp\log.txt" "$Dir\hv-host-log.txt" -Force
"done"
