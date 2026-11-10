# mp_ride.ps1 - two players at one machine on this PC (this fork's test helper): both games travel to a level, each hero is put at
# a place at a field (RAC_MP_TEST_GOTO), and the watched classes are logged in both (RAC_MP_TEST_WATCH, mp/testaid.rs). Prints the
# watched object and each hero, side by side by field, from both run logs (left the host's, right the joiner's).
#
#   powershell -ExecutionPolicy Bypass -File tools\mp_ride.ps1 -Level 2 -Watch 651 -Id 0x103 -HostGoto "3200:172,276,34.6" -JoinGoto "3200:172,276,34.6"
param([int]$Level = 2, [string]$Watch = '651', [string]$Id = '', [string]$HostGoto = '', [string]$JoinGoto = '', [int]$Fields = 4400,
	[int]$Port = 3470, [int]$From = 3100, [int]$To = 4400)
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$p1 = Join-Path $env:TEMP "racmp\ride-host"; $p2 = Join-Path $env:TEMP "racmp\ride-join"
Remove-Item -Recurse -Force $p1, $p2 -ErrorAction SilentlyContinue
# Cross every 40 fields from 2950 to 3150: it closes the notice some levels show on arrival (which comes 50 fields earlier or later
# from run to run), and is over before a hero is put anywhere from 3160 on (a press after that makes him jump off).
$pad = ((0..5) | ForEach-Object { $f = 2950 + $_ * 40; "$f-$($f+5):40:80,80" }) -join ';'
$env:RAC_MP_TEST_WATCH = $Watch
$env:RAC_MP_TEST_GOTO = $HostGoto
$a = & "$root\tools\mp_play.ps1" -Mp "host:127.0.0.1:$Port" -Name Host -Player $p1 -Fields $Fields -Pad $pad -Travel "${Level}:1800" -Hidden
Start-Sleep -Seconds 2
$env:RAC_MP_TEST_GOTO = $JoinGoto
$b = & "$root\tools\mp_play.ps1" -Mp "join:127.0.0.1:$Port" -Name Joiner -Player $p2 -Fields ($Fields - 100) -Pad $pad -Travel "${Level}:1800" -Hidden
Remove-Item Env:RAC_MP_TEST_WATCH, Env:RAC_MP_TEST_GOTO -ErrorAction SilentlyContinue
$ids = @($a, $b) | ForEach-Object { if ("$_" -match 'pid (\d+)') { [int]$Matches[1] } }
$ids | ForEach-Object { Wait-Process -Id $_ -Timeout 600 -ErrorAction SilentlyContinue }
Get-Process -Id $ids -ErrorAction SilentlyContinue | Stop-Process -Force
function Rows([string]$log) {
	$t = @{}
	foreach ($l in (Get-Content -LiteralPath $log)) {
		if ($l -match '^mp test: f(\d+) (.*)$') {
			$f = [int]$Matches[1]; $rest = $Matches[2]
			if ($rest -match '^hero at \(([^)]*)\)') { $t["$f h"] = $Matches[1] }
			elseif ((-not $Id -or $rest -match "^id $Id ") -and $rest -match 'at \(([^)]*)\) state (0x[0-9a-f]+) (.*)$') { $t["$f o"] = "$($Matches[1]) s$($Matches[2]) $(if ($Matches[3] -match 'shown') { 'SHOWN' } else { 'own' })" }
			elseif ($rest -match 'is put at') { $t["$f p"] = 'put' }
		}
	}
	$t
}
$h = Rows "$p1\log.txt"; $j = Rows "$p2\log.txt"
for ($f = $From; $f -le $To; $f += 30) {
	"f{0}  host: obj {1,-34} hero {2,-22} | joiner: obj {3,-34} hero {4}" -f $f, $h["$f o"], $h["$f h"], $j["$f o"], $j["$f h"]
}
foreach ($p in $p1, $p2) { "== $p"; Select-String -LiteralPath "$p\log.txt" -Pattern 'is put at|^smoke: done|panicked|could not|went silent' | ForEach-Object { $_.Line.Substring(0, [Math]::Min(150, $_.Line.Length)) } }
