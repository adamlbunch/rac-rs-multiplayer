# mp_sweep.ps1 - a host and a joiner on this PC in every level, one pair after another (this fork's test helper): both travel to
# the level, walk about and swing, and each game must run to its last field. Prints one line a level: whether both ended, drops,
# and what the run logs count. Logs stay in %TEMP%\racmp\sweep-host-LEVEL and sweep-join-LEVEL.
#
#   powershell -ExecutionPolicy Bypass -File tools\mp_sweep.ps1 [-Levels 0,1,3] [-Fields 6000] [-Together 2]
param([string]$Levels = '', [int]$Fields = 6000, [int]$Together = 2, [int]$Port = 3400, [string]$Env = '')
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
foreach ($kv in @($Env.Split(';') | Where-Object { $_ })) { $k, $v = $kv.Split('=', 2); if (-not $v) { $v = '1' }; Set-Item "Env:$k" $v }
# Play starts at about field 1530 on Veldin and about 2700 after a trip. From there: forward with a swing every 24 fields, a jump
# now and then, and a turn of the stick every 300 fields (`turn`: which way first), so the two do not walk the same line.
function Pad([int]$from, [int]$to, [int]$turn) {
	$dirs = @('80,00', 'ff,00', 'ff,80', '80,00', '00,00', '00,80')
	$out = @()
	$k = $turn
	for ($f = $from; $f + 300 -le $to; $f += 300) {
		$d = $dirs[$k % $dirs.Count]; $k++
		for ($g = $f; $g + 24 -le $f + 300; $g += 24) {
			$b = if ((($g - $from) / 24) % 5 -eq 4) { '40' } else { '80' }
			$out += "$g-$($g+5):${b}:$d"; $out += "$($g+6)-$($g+23):0:$d"
		}
	}
	$out -join ';'
}
function Count([string]$log, [string]$pat) { @(Select-String -LiteralPath $log -Pattern $pat -ErrorAction SilentlyContinue).Count }
function Last([string]$log, [string]$pat) { $m = Select-String -LiteralPath $log -Pattern $pat -ErrorAction SilentlyContinue | Select-Object -Last 1; if ($m) { $m.Line } else { '' } }
# -File hands "0,3" over as one string: split here.
$list = if ($Levels) { @($Levels -split '[,; ]+' | Where-Object { $_ -ne '' } | ForEach-Object { [int]$_ }) } else { @(0..18) }
$queue = [System.Collections.Queue]::new($list)
$running = @()
$results = @()
while ($queue.Count -gt 0 -or $running.Count -gt 0) {
	while ($queue.Count -gt 0 -and $running.Count -lt $Together) {
		$l = $queue.Dequeue()
		$p1 = Join-Path $env:TEMP "racmp\sweep-host-$l"; $p2 = Join-Path $env:TEMP "racmp\sweep-join-$l"
		Remove-Item -Recurse -Force $p1, $p2 -ErrorAction SilentlyContinue
		$start = if ($l -eq 0) { 1560 } else { 2700 }
		$a1 = @{ Mp = "host:127.0.0.1:$($Port + $l)"; Name = 'Host'; Player = $p1; Fields = $Fields; Pad = (Pad $start $Fields 0); Hidden = $true }
		$a2 = @{ Mp = "join:127.0.0.1:$($Port + $l)"; Name = 'Joiner'; Player = $p2; Fields = $Fields - 200; Pad = (Pad ($start + 60) ($Fields - 200) 1); Hidden = $true }
		if ($l -ne 0) { $a1.Travel = "${l}:1800"; $a2.Travel = "${l}:1800" }
		$a = & "$root\tools\mp_play.ps1" @a1
		Start-Sleep -Seconds 2
		$b = & "$root\tools\mp_play.ps1" @a2
		$ids = @($a, $b) | ForEach-Object { if ("$_" -match 'pid (\d+)') { [int]$Matches[1] } }
		$running += [pscustomobject]@{ Level = $l; Ids = $ids; Host = "$p1\log.txt"; Join = "$p2\log.txt"; Until = (Get-Date).AddSeconds($Fields / 20 + 240) }
	}
	Start-Sleep -Seconds 5
	$still = @()
	foreach ($r in $running) {
		$alive = @(Get-Process -Id $r.Ids -ErrorAction SilentlyContinue)
		if ($alive.Count -gt 0 -and (Get-Date) -lt $r.Until) { $still += $r; continue }
		$hung = $alive.Count
		$alive | Stop-Process -Force
		$line = "level {0,2}: host {1}, joiner {2}" -f $r.Level, $(if (Count $r.Host '^smoke: done') { 'ended' } else { 'DID NOT END (' + (Last $r.Host '^smoke: field') + ')' }), $(if (Count $r.Join '^smoke: done') { 'ended' } else { 'DID NOT END (' + (Last $r.Join '^smoke: field') + ')' })
		if ($hung) { $line += ", $hung still running at the time limit" }
		$line += "; drops {0}/{1}, could-not {2}/{3}, panics {4}/{5}" -f (Count $r.Host 'left or went silent|the host was given up'), (Count $r.Join 'left or went silent|the host was given up'), (Count $r.Host '^mp: could not'), (Count $r.Join '^mp: could not'), (Count $r.Host 'panicked'), (Count $r.Join 'panicked')
		$line += "; gone {0}/{1}, finished {2}/{3}" -f (Count $r.Host '^mp: gone here'), (Count $r.Join '^mp: gone here'), (Count $r.Host '^mp: finishing for another'), (Count $r.Join '^mp: finishing for another')
		$line
		$results += $line
	}
	$running = $still
}
$results | Set-Content (Join-Path $env:TEMP 'racmp\sweep.txt')
