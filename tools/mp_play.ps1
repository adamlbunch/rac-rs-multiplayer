# mp_play.ps1 - starts the game from your disc and walks it into play on Veldin with no hands on the pad (this fork's test
# helper). The title pilot (retail/title_boot.rs) presses Start and New Game, answers the memory card box with Yes (Circle) and
# picks slot 1; the key taps skip the two movies and the opening scene. Play starts at about field 1530 (USA disc).
#
#   powershell -ExecutionPolicy Bypass -File tools\mp_play.ps1 -Mp host:127.0.0.1:2410 -Name Host -Hidden -Fields 3000
#   powershell -ExecutionPolicy Bypass -File tools\mp_play.ps1 -Mp join:127.0.0.1:2410 -Name Joiner -Pad "1700-2400:0:80,00"
#
# Every run gets a fresh player folder (saves, run log log.txt) unless -Player names one. Prints the process id; -Wait waits
# for it.
param(
	[string]$Mp = '',          # RAC_MP: host, host:PORT, host:ADDR:PORT, join:ADDR[:PORT]; empty = multiplayer off
	[string]$Name = 'Ratchet', # RAC_MP_NAME
	[string]$Player = '',      # player folder; default: a new one under %TEMP%\racmp\
	[string]$Pad = '',         # RAC_RETAIL_PAD entries (from-to:buttons:lx,ly[:rx,ry], hex, fields from the start; ';' between)
	[int]$Fields = 0,          # quit after this many fields (RAC_RETAIL_SMOKE: off-screen window, muted); 0 = keep playing
	[int]$ShotField = 0,       # with -Shot: save the picture of this field, then quit
	[string]$Shot = '',
	[string]$Travel = '',      # RAC_RETAIL_TRAVEL, LEVEL:FIELD: travel to that level at that field (1 Novalis, 3 Kerwan, ...)
	[switch]$Hidden,           # off-screen window, muted, without -Fields
	[switch]$OnDesktop,        # with -Hidden or -Fields: run on the user's desktop anyway (default: a desktop of its own, desk_run.ps1)
	[switch]$Wait,
	[string]$Iso = ''           # another disc image than setup's (generated\play.cfg), e.g. the PAL one
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$exe = Join-Path $root 'build\release\rac-rs.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw "no game at ${exe}: run setup.ps1 first" }
$iso = if ($Iso) { $Iso } else { ((Get-Content -LiteralPath (Join-Path $root 'generated\play.cfg')) | Where-Object { $_ -like 'iso=*' } | Select-Object -First 1).Substring(4) }
if (-not $Player) { $Player = Join-Path $env:TEMP ("racmp\player-" + (Get-Date -Format 'HHmmss-fff')) }
New-Item -ItemType Directory -Force -Path $Player | Out-Null

$env:ORACLE_ISO = $iso
$env:ORACLE_STUBS = Join-Path $root 'oracle\stubs.txt'
$env:ORACLE_HAND_SKIP = Get-Content -LiteralPath (Join-Path $root 'oracle\hand\hand_skip.txt') -TotalCount 1
$env:RAC_PLAYER_DIR = $Player
$env:RAC_NO_UPDATE_CHECK = '1'
$env:RAC_TITLE_PILOT = 'title:30,start,cross,wait:120,circle,wait:90,cross,wait:120'
$env:RAC_RETAIL_KEYTAP = 'Escape@movie1+30;Escape@movie2+30;Escape@scene+30'
foreach ($v in 'RAC_MP', 'RAC_MP_NAME', 'RAC_RETAIL_PAD', 'RAC_RETAIL_SMOKE', 'RAC_RETAIL_TRAVEL', 'RAC_SHOT_FIELD', 'RAC_TEST_WINDOW', 'RAC_MUTE') { Remove-Item "Env:$v" -ErrorAction SilentlyContinue }
if ($Mp) { $env:RAC_MP = $Mp; $env:RAC_MP_NAME = $Name }
if ($Travel) { $env:RAC_RETAIL_TRAVEL = $Travel }
if ($Pad) { $env:RAC_RETAIL_PAD = $Pad }
if ($Fields -gt 0) { $env:RAC_RETAIL_SMOKE = "$Fields" }
if ($Hidden) { $env:RAC_TEST_WINDOW = 'hidden'; $env:RAC_MUTE = '1' }
$argv = @('view', "`"$iso`"", '0', '--retail-cold', '--modern-cam')
if ($Shot) { $argv += @('--shot', "`"$Shot`""); $env:RAC_SHOT_FIELD = "$ShotField" }
# Started detached (no redirection): a game holding this script's output pipe would keep whoever reads it waiting until the
# game ends. What the game prints goes to its run log, log.txt in the player folder (runlog.rs).
$env:RAC_RUN_LOG = '1'
# A test run (-Hidden or -Fields) goes to a desktop of its own, so its window never shows or takes the focus while the user works.
if (($Hidden -or $Fields -gt 0) -and -not $OnDesktop) {
	$id = & (Join-Path $PSScriptRoot 'desk_run.ps1') -Exe $exe -Args ($argv -join ' ') -Dir $root
	$p = Get-Process -Id $id
} else {
	$p = Start-Process -FilePath $exe -ArgumentList $argv -WorkingDirectory $root -PassThru
}
Write-Output "pid $($p.Id), player folder $Player (log: log.txt)"
if ($Wait) { $p | Wait-Process; Write-Output "exit $($p.ExitCode)" }
