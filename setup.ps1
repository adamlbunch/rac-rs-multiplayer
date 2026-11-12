# setup.ps1 - builds the PC port of Ratchet & Clank (PAL / Europe or USA, v1.00) from YOUR OWN disc image. Windows PowerShell 5.1.
#
#   Double-click Setup.cmd: a small window opens (pick the .iso, Install, Play); no console window is shown.
#   For troubleshooting, the old console mode: Setup.cmd -Console, or
#   powershell -ExecutionPolicy Bypass -File .\setup.ps1 -Console -Iso "<your folder>\Ratchet & Clank (Europe) (En,Fr,De,Es,It) (v1.00).iso"
#   The window runs the same steps below as a hidden child (setup.ps1 -Console ...) and shows its progress.
#
# What it does, in order:
#   (a) downloads a private, pinned copy of Rust (GNU host, linker included), Python (embeddable) and the GNU assembler into .\toolchain
#       (once; nothing is installed on the PC, no admin rights, no PATH or registry change)
#   (b) checks that the disc image is Ratchet & Clank PAL / Europe v1.00 (SCES-50916) or USA v1.00 (SCUS-97199) (size and SHA-1)
#   (c) generates the game code from the disc image with tools\recgen_iso.py (boot code + 19 levels) into .\oracle\rec and .\oracle\game
#   (c2) generates the VU microcode translations (graphics code) from the disc image with tools\vugen_iso.py into
#        .\oracle\src\vu0_prog0.rs, vu0_prog1.rs, vu0_prog9hi.rs, skinff_ee.rs, skinff_vu.rs (the skinning fast path: tools\skinff_ee.py,
#        tools\skinff_gen.py, run by vugen_iso.py) and .\rac-rs\src\gsfull\vu1_prog*.rs + vu1aot_gen.rs
#   (d) builds the game with cargo (release) into .\build, guided by the speed profile for your disc in .\pgo when the release has one
#   (e) writes generated\play.cfg (the disc image path) and Play.cmd, which starts the game from your disc image (cold boot: logos, title screen, New Game, no savestate),
#       and a desktop shortcut to the game exe (the exe, started with no arguments, reads generated\play.cfg)
# Everything it makes stays inside this folder (toolchain\, oracle\rec, oracle\game, the generated .rs files named above, generated\
# for logs and stamps, build\, Play.cmd, and player\ once you play), plus the desktop shortcut. Downloads (internet needed once, about
# 220 MB): Rust, Python and the GNU assembler into toolchain\ (pinned versions, SHA-256 checked) and the Rust crates that cargo fetches for the build
# (into toolchain\cargo). Delete the folder to remove everything. The disc image is only read.
#
# Switches: -Console (no window: run the steps here in the console),
#           -SkipHash (size check only), -CheckOnly (do (a) and (b) only), -Regenerate (redo (c)), -Jobs N (parallel generators, default: one per logical CPU, at most 12),
#           -NoBuild (stop after (c2)), -NoShortcut (no desktop shortcut), -CargoJobs N (compiler jobs at once in (d); default: automatic), -NoSharedSlow (generate the memory-access slow paths inline instead of as shared helpers),
#           -NoPgo (build without the speed profile in .\pgo).
#           -NoFetchAhead (download the libraries in (d), not in the background during (b), (c) and (c2)).
#           -RustNoLinks (install Rust without rustup, as setup does by itself on a drive without hard links such as exFAT; test switch).
#           -NoLauncher / -AddToSteam: the two boxes of the setup window as switches (no launcher; add the game to Steam). -AddToSteam / -RemoveFromSteam alone only do that and exit.
param(
    [string]$Iso = '',
    [switch]$SkipHash,
    [switch]$CheckOnly,
    [switch]$Regenerate,
    [switch]$NoBuild,
    [switch]$NoSharedSlow,
    [switch]$NoShortcut,
    [switch]$NoPgo,                 # (d) without the speed profile (pgo\<serial>.profdata.gz)
    [switch]$NoFetchAhead,
    [switch]$RustNoLinks,           # install Rust from its packages, without rustup (done by itself when this drive cannot make hard links: exFAT, FAT32, some network shares)
    [int]$Jobs = 0,                 # parallel generators in (c); 0 = one per logical CPU, at most 12 (each needs about 120 MB)
    [int]$CargoJobs = 0,            # compiler jobs at once in (d); 0 = automatic
    [switch]$NoDxc,                 # do not fetch Microsoft's DirectX shader compiler (dxcompiler.dll) for the DirectX 12 renderer
    [switch]$Console,
    [string]$ShortcutDir = '',      # where the shortcut goes (default: the desktop)
    [switch]$TestNoShow,            # test only: build the launcher without showing it and save PNGs of its views
    [string]$TestShot = '',
    [string]$TestIso = '',
    [switch]$TestInstall,
    [switch]$Launch,                # started by the game exe: looks for an update; silent unless there is one
    [switch]$Together,              # [MP] open on "Play together" (host or join a multiplayer game): the second desktop shortcut
    [switch]$Update,                # the update window: waits for the game to close, then updates and rebuilds
    [switch]$ApplyUpdate,           # worker of the update window (with -Console): download + apply the update, then the normal steps
    [switch]$FinishOnly,            # with -Update: the files are already updated, only the rebuild is left
    [string]$NewVersion = '',
    [int]$ExePid = 0,               # the game exe that started -Launch (the update waits for it to close)
    [switch]$SkipUnblock,
    [switch]$GetLauncher,           # download the latest RAC-RS Launcher (github.com/AsobiCodes/rac-rs-launcher releases) into this folder; exit 0 = it is in place
    [string]$TestLauncherReleases = '',   # test only: a folder with latest.json + the asset instead of GitHub
    [switch]$NoLauncher,            # the setup window starts with "Also install the RAC-RS Launcher" unticked; with -Console it remembers "no launcher" (generated\setup_choice.txt). -Console never fetches the launcher itself (use -GetLauncher)
    [switch]$AddToSteam,            # add "Ratchet & Clank (rac-rs-multiplayer)" to Steam (the shortcuts.vdf of every Steam user; Steam must be closed). Alone: do just that and exit; with -Console -Iso: also at the end of the set-up
    [switch]$RemoveFromSteam,       # take our entry out of Steam's shortcuts.vdf again, then exit
    [string]$TestWorkerArgs = '',   # test only: more switches for the hidden worker of the window (for example -CheckOnly)
    [string]$WindowFlag = '',       # -Launch: a file it creates when it shows a window (the exe then waits for the answer)
    [string]$TestUpdateBase = '',   # test only: folder / file:// / http URL that serves VERSION, CHANGELOG.md and v<version>.zip instead of GitHub
    [string]$TestChoice = '',       # test only (-Launch -TestNoShow): 'update' or 'notnow' instead of a click; 'firstview' (launcher test): only the first view; 'dpiviews': every view with stub content (no install)
    [int]$TestDpi = 0,              # test only: pretend the Windows display scale is N percent (for example 300); the window lays itself out for that scale
    [string]$TestScreen = ''        # test only: pretend the screen is WxH pixels (for example 1920x1080; the taskbar takes 40 off the height)
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
# [MP] This fork's name, kept apart from upstream rac-rs ("Ratchet & Clank (rac-rs)") so that both can be installed: the window
# title, the desktop shortcut and the Steam entry. A shortcut of the old name that starts THIS folder's game is taken away.
$GameName = 'Ratchet & Clank (rac-rs-multiplayer)'
$GameLnk = "$GameName.lnk"
$OldLnk = 'Ratchet & Clank (rac-rs).lnk'
Set-Location -LiteralPath $root

# The supported discs (v1.00 of each): Ratchet & Clank (Europe) (En,Fr,De,Es,It) (v1.00), serial SCES-50916, and
# Ratchet & Clank (USA) (En,Fr,De,Es,It), serial SCUS-97199. They have different sizes; the SHA-1 then proves the copy.
$Discs = @(
    @{ Size = [long]4213997568; Sha1 = 'B0DFBD8EAC0D84F173B6FC25D8A02A5FF22C652F'; Name = 'PAL / Europe v1.00 (SCES-50916)'; Serial = 'SCES-50916' },
    @{ Size = [long]4214095872; Sha1 = 'AEE9275384F739C9B24CD5DE6F7E200292DB7B01'; Name = 'USA v1.00 (SCUS-97199)'; Serial = 'SCUS-97199' }
)
# The PAL values until a disc is picked; (b) sets them from the disc found.
$ExpectedSize = $Discs[0].Size
$ExpectedSha1 = $Discs[0].Sha1
$DiscName = $Discs[0].Name
$GenFlags = @('--no-comments', '--compact', '--shared-slow', '--outline-mem', '--direct-mem', '--skin-ff', '--vu-guard')
if ($NoSharedSlow) { $GenFlags = @('--no-comments', '--compact', '--outline-mem', '--direct-mem', '--skin-ff', '--vu-guard') }   # inline slow paths: bigger source and slower build, speed measured in SETUP_NOTES 7b
$Levels = 0..18

function Step($text) { Write-Host ''; Write-Host "== $text" -ForegroundColor Cyan }
function Fail($text) { Stop-LibsFetch; Write-Host "ERROR: $text" -ForegroundColor Red; Wait-RustJob; exit 1 }
$script:RustJob = $null
function Wait-RustJob {
    # the background Rust install of (a) (see Complete-Tools), when it still runs: let it finish, stopping it halfway could
    # leave toolchain\ broken
    $rj = $script:RustJob
    $script:RustJob = $null
    if ($rj -and -not $rj.Proc.HasExited) { Write-Host '  (letting the Rust download in the background finish first ...)'; [void]$rj.Proc.WaitForExit(900000) }
}
$script:LibsFetch = $null
function Stop-LibsFetch {
    # the background download of the libraries (see "libraries download" in (a)): only that process tree, when it still runs
    $lf = $script:LibsFetch
    $script:LibsFetch = $null
    if ($lf -and -not $lf.Proc.HasExited) {
        try { Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\taskkill.exe') -ArgumentList "/PID $($lf.Proc.Id) /T /F" -NoNewWindow -Wait } catch { }
    }
}
$timings = [ordered]@{}

# ================================================================ starting the game, desktop shortcut (shared by the launcher and -Console)
function Test-Installed {
    return ((Test-Path -LiteralPath (Join-Path $root 'build\release\rac-rs.exe')) -and (Test-Path -LiteralPath (Join-Path $root 'Play.cmd')) -and (Test-Path -LiteralPath (Join-Path $root 'generated\play.cfg')))
}

function Write-PlayCfg([string]$iso) {
    # generated\play.cfg: key=value lines (UTF-8, no BOM). The game exe, started with no arguments, reads it to find the disc image.
    $dir = Join-Path $root 'generated'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $dir 'play.cfg'), ("iso=$iso`n"), (New-Object System.Text.UTF8Encoding($false)))
}

function Get-InstalledIso {
    # the disc image path that setup wrote into generated\play.cfg (Play.cmd as a fallback)
    $cfg = Join-Path $root 'generated\play.cfg'
    if (Test-Path -LiteralPath $cfg) {
        foreach ($l in ([System.IO.File]::ReadAllLines($cfg, (New-Object System.Text.UTF8Encoding($false))))) { if ($l -match '^iso=(.+)$') { return $Matches[1].Trim() } }
    }
    $pc = Join-Path $root 'Play.cmd'
    if (Test-Path -LiteralPath $pc) {
        foreach ($l in (Get-Content -LiteralPath $pc)) { if ($l -match '^set "ORACLE_ISO=(.*)"\s*$') { return $Matches[1] } }
    }
    return ''
}

function Get-IconFile {
    # the game logo as an .ico (shipped in the folder), or '' when it is missing
    $ico = Join-Path $root 'rac-rs\assets\icon\rac-rs.ico'
    if (Test-Path -LiteralPath $ico -PathType Leaf) { return $ico }
    return ''
}

function Get-GamePsi {
    # The game exe itself, started with no arguments (it reads generated\play.cfg), no console window, working folder = this folder.
    $exe = Join-Path $root 'build\release\rac-rs.exe'
    if (-not (Test-Path -LiteralPath $exe)) { throw 'The game is not built yet. Run Setup.cmd first.' }
    $iso = Get-InstalledIso
    if (-not $iso) { throw 'The setup information was not found. Open Setup.cmd and use Repair / rebuild.' }
    if (-not (Test-Path -LiteralPath $iso -PathType Leaf)) { throw "Your disc image was not found at:`r`n$iso`r`n`r`nIf you moved it, open Setup.cmd and use Repair / rebuild to pick it again." }
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = ''
    $psi.WorkingDirectory = $root
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    return $psi
}

function Start-Game([bool]$noUpdateCheck = $false, [hashtable]$extraEnv = @{}) {
    $psi = Get-GamePsi
    if ($noUpdateCheck) { $psi.EnvironmentVariables['RAC_NO_UPDATE_CHECK'] = '1' }
    foreach ($k in $extraEnv.Keys) { $psi.EnvironmentVariables[$k] = [string]$extraEnv[$k] }      # [MP] RAC_MP and RAC_MP_NAME from "Play together"
    $pl = Join-Path $root 'player'
    if (-not (Test-Path -LiteralPath $pl)) { New-Item -ItemType Directory -Force -Path $pl | Out-Null }
    # safety copies of the save card before each start (the newest two starts), as Play.cmd does
    try {
        $c1 = Join-Path $pl 'card.start1.bin'; $c2 = Join-Path $pl 'card.start2.bin'; $c0 = Join-Path $pl 'card.bin'
        if (Test-Path -LiteralPath $c1) { Copy-Item -LiteralPath $c1 -Destination $c2 -Force }
        if (Test-Path -LiteralPath $c0) { Copy-Item -LiteralPath $c0 -Destination $c1 -Force }
    } catch { }
    $null = [System.Diagnostics.Process]::Start($psi)
}

function New-GameShortcut([string]$dir) {
    # Desktop shortcut: the game exe itself (no arguments, working folder = this folder).
    $exe = Join-Path $root 'build\release\rac-rs.exe'
    # With the launcher installed, the shortcut opens the launcher (it has the Play button); otherwise the game exe itself.
    $launcher = Join-Path $root 'RAC-RS Launcher.exe'
    $target = $(if (Test-Path -LiteralPath $launcher -PathType Leaf) { $launcher } else { $exe })
    $lnk = Join-Path $dir $GameLnk
    # [MP] the shortcut of the old name, when it starts this folder's game (or its launcher): replaced by the one of the new name
    try {
        $old = Join-Path $dir $OldLnk
        if (Test-Path -LiteralPath $old -PathType Leaf) {
            $ot = [string](New-Object -ComObject WScript.Shell).CreateShortcut($old).TargetPath
            if ($ot -and ($ot -eq $exe -or $ot -eq $launcher)) { Remove-Item -LiteralPath $old -Force }
        }
    } catch { }
    $sc = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
    $sc.TargetPath = $target
    $sc.Arguments = ''
    $sc.WorkingDirectory = $root
    $ico = Get-IconFile
    $sc.IconLocation = $(if ($ico) { $ico } else { "$target,0" })
    $sc.Description = $GameName
    $sc.Save()
    # [MP] A second shortcut opens this window on "Play together" (host or join). PowerShell starts minimized, then hides itself.
    try {
        $t = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $dir $TogetherLnk))
        $t.TargetPath = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
        $t.Arguments = '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + (Join-Path $root 'setup.ps1') + '" -Together'
        $t.WorkingDirectory = $root
        $t.WindowStyle = 7
        $t.IconLocation = $(if ($ico) { $ico } else { "$exe,0" })
        $t.Description = "${GameName}: host or join a multiplayer game"
        $t.Save()
    } catch { }
    return $lnk
}

# ================================================================ [MP] "Play together": this fork's multiplayer, set up from the window
# generated\multiplayer.cfg remembers the choices (key=value lines, UTF-8 without BOM): mode=host|join, join=<address>, name=<name>.
# generated\ is never touched by an update. The game gets them as RAC_MP and RAC_MP_NAME (docs\MULTIPLAYER.md).
$TogetherLnk = 'Ratchet & Clank - Play together.lnk'

function Get-MpSettings {
    $r = @{ mode = 'host'; join = ''; name = '' }
    $f = Join-Path $root 'generated\multiplayer.cfg'
    if (Test-Path -LiteralPath $f -PathType Leaf) {
        try { foreach ($l in ([System.IO.File]::ReadAllLines($f, (New-Object System.Text.UTF8Encoding($false))))) { if ($l -match '^(mode|join|name)=(.*)$') { $r[$Matches[1]] = $Matches[2].Trim() } } } catch { }
    }
    if (-not $r.name) { $r.name = [string]$env:USERNAME }
    return $r
}

function Set-MpSettings([string]$mode, [string]$join, [string]$name) {
    $dir = Join-Path $root 'generated'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $dir 'multiplayer.cfg'), "mode=$mode`njoin=$join`nname=$name`n", (New-Object System.Text.UTF8Encoding($false)))
}

function Get-MpName([string]$name) {
    # what the others see: at most 24 bytes of UTF-8 (the game's limit), no control characters
    $n = ($name -replace '[\x00-\x1f\x7f]', '').Trim()
    $u = New-Object System.Text.UTF8Encoding($false)
    while ($n.Length -gt 0 -and $u.GetByteCount($n) -gt 24) { $n = $n.Substring(0, $n.Length - 1) }
    return $n.Trim()
}

function Get-MpAddresses {
    # this PC's addresses others can join: its Tailscale one (100.64.0.0/10) and a home network one (private ranges, not a virtual switch)
    $ts = ''; $lan = ''
    try {
        foreach ($a in @(Get-NetIPAddress -AddressFamily IPv4 -ErrorAction Stop | Where-Object { [string]$_.AddressState -eq 'Preferred' })) {
            $ip = [string]$a.IPAddress
            if ($ip -match '^100\.(\d+)\.' -and [int]$Matches[1] -ge 64 -and [int]$Matches[1] -le 127) { if (-not $ts) { $ts = $ip } }
            elseif ($ip -match '^(192\.168\.|10\.|172\.(1[6-9]|2\d|3[01])\.)' -and [string]$a.InterfaceAlias -notmatch 'vEthernet|WSL|Hyper-V|VirtualBox|VMware|Loopback') { if (-not $lan) { $lan = $ip } }
        }
    } catch { }
    return @{ Tailscale = $ts; Lan = $lan }
}

function Get-MpGameEnv([bool]$join, [string]$addr, [string]$name) {
    # What the game is started with for "Play together": the session (RAC_MP, RAC_MP_NAME), and for a player who joins with no saves
    # yet (no player\card.bin) the way into the game without the title menu: the title pilot starts a new game (Start, New Game, Yes
    # to the memory card box, slot 1, as every pair test gets into play) and the intro movies and the opening scene are skipped. The
    # game then takes them to the host's planet. A card with saves is never touched by this: that player picks Load Game or New Game.
    $e = @{ RAC_MP = $(if ($join) { "join:$addr" } else { 'host' }); RAC_MP_NAME = $name }
    if ($join -and -not (Test-Path -LiteralPath (Join-Path $root 'player\card.bin'))) {
        $e['RAC_TITLE_PILOT'] = 'title:30,start,cross,wait:120,circle,wait:90,cross,wait:120'
        $e['RAC_RETAIL_KEYTAP'] = 'Escape@movie1+30;Escape@movie2+30;Escape@movie3+30;Escape@movie4+30;Escape@scene+30'
    }
    return $e
}

function Test-MpFirewallBlock {
    # whether Windows Firewall has an enabled rule that blocks the game exe from taking connections (Windows adds one when its
    # "allow" question was cancelled or never seen). Only reads the rules; changing them is the player's (the window opens the page).
    $exe = Join-Path $root 'build\release\rac-rs.exe'
    try {
        foreach ($r in @(Get-NetFirewallRule -Direction Inbound -Action Block -Enabled True -ErrorAction Stop)) {
            $p = [string](($r | Get-NetFirewallApplicationFilter -ErrorAction SilentlyContinue).Program)
            if ($p -and ([Environment]::ExpandEnvironmentVariables($p) -eq $exe)) { return $true }
        }
    } catch { }
    return $false
}

# [MP] Finding the games friends host (the Join list of "Play together"). Every PC Tailscale says is online, and the home network
# (a broadcast), is asked on the game's port whether a game is hosting there (mp\src\find.rs): a host of 0.4.1.4 or later answers
# with its player name, planet and players; an older one only with its protocol version (it answers a datagram of another version).
$MpPort = 2410
$MpPlanets = @('Veldin', 'Novalis', 'Aridia', 'Kerwan', 'Eudora', 'Rilgar', 'Blarg', 'Umbris', 'Batalia', 'Gaspar', 'Orxon', 'Pokitaru', 'Hoven', 'Gemlik', 'Oltanis', 'Quartu', 'Kalebo III', "Drek's Fleet", 'Veldin')
$MpQuery = [byte[]](@(0x52, 0x43, 0x4D, 0x51, 1) + @(0) * 43)      # RCMQ, format 1, zeros to 48 bytes
$MpProbe = [byte[]](0x52, 0x43, 0x4D, 0x50, 0, 0, 0)               # RCMP, protocol version 0, no message

function Get-MpProtocol {
    # this game's protocol version (mp\src\proto.rs): players join only a host of the same one. 0 when unknown.
    try { if ([System.IO.File]::ReadAllText((Join-Path $root 'mp\src\proto.rs')) -match 'pub const VERSION: u16 = (\d+);') { return [int]$Matches[1] } } catch { }
    return 0
}

function Read-MpAnswer([byte[]]$b) {
    # a host's answer, checked as find.rs checks it (it comes from someone else's PC); $null for anything else
    if (-not $b -or $b.Length -lt 8 -or $b[0] -ne 0x52 -or $b[1] -ne 0x43 -or $b[2] -ne 0x4D) { return $null }
    if ($b.Length -eq 8 -and $b[3] -eq 0x50 -and $b[6] -eq 3 -and $b[7] -eq 2) { return @{ info = $false; version = [int]$b[4] + 256 * [int]$b[5] } }
    if ($b.Length -lt 12 -or $b[3] -ne 0x49 -or $b[4] -lt 1) { return $null }
    $n = [int]$b[11]
    if ($n -gt 24 -or $b.Length -lt 12 + $n -or ($b[4] -eq 1 -and $b.Length -ne 12 + $n)) { return $null }
    $lv = [int]$b[10]; if ($lv -gt 127) { $lv -= 256 }
    if ($lv -lt -1 -or $lv -gt 18) { return $null }
    try { $name = (New-Object System.Text.UTF8Encoding($false, $true)).GetString($b, 12, $n) } catch { return $null }
    if ($name -match '[\x00-\x1f\x7f]') { return $null }
    return @{ info = $true; version = [int]$b[5] + 256 * [int]$b[6]; tick = [int]$b[7]; players = [int]$b[8]; max = [int]$b[9]; level = $lv; name = $name }
}

function Get-MpNetworks {
    # this PC's IPv4 addresses (its own answers are not a friend's game) and the broadcast address of each home network
    $mine = @{}; $bc = @('255.255.255.255')
    try {
        foreach ($nic in [System.Net.NetworkInformation.NetworkInterface]::GetAllNetworkInterfaces()) {
            if ($nic.OperationalStatus -ne 'Up') { continue }
            $virtual = ("$($nic.Name) $($nic.Description)" -match 'vEthernet|WSL|Hyper-V|VirtualBox|VMware|Loopback|Tailscale')
            foreach ($u in $nic.GetIPProperties().UnicastAddresses) {
                if ($u.Address.AddressFamily -ne 'InterNetwork') { continue }
                $ip = $u.Address.ToString(); $mine[$ip] = $true
                if ($virtual -or -not $u.IPv4Mask -or $ip -notmatch '^(192\.168\.|10\.|172\.(1[6-9]|2\d|3[01])\.)') { continue }
                $a = $u.Address.GetAddressBytes(); $m = $u.IPv4Mask.GetAddressBytes()
                $bc += (0..3 | ForEach-Object { [int]$a[$_] -bor (255 -bxor [int]$m[$_]) }) -join '.'
            }
        }
    } catch { }
    return @{ mine = $mine; broadcast = @($bc | Select-Object -Unique) }
}

function Find-Tailscale {
    foreach ($p in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
        if ($p -and (Test-Path -LiteralPath (Join-Path $p 'Tailscale\tailscale.exe') -PathType Leaf)) { return (Join-Path $p 'Tailscale\tailscale.exe') }
    }
    $c = Get-Command tailscale.exe -ErrorAction SilentlyContinue
    if ($c) { return [string]$c.Source }
    return ''
}

function Read-TailscalePeers([string]$json, [bool]$withSelf) {
    # `tailscale status --json`: whether it is on, and the PCs that are online (IPv4 address, device name, owner). A device of someone
    # this PC was shared with ("sharee") cannot be reached from here: sharing lets them in, not this PC out to them.
    $r = @{ running = $false; peers = @() }
    try { $j = $json | ConvertFrom-Json } catch { return $r }
    $r.running = ([string]$j.BackendState -eq 'Running')
    $nodes = @()
    if ($j.Peer) { $nodes = @($j.Peer.PSObject.Properties | ForEach-Object { $_.Value }) }
    if ($withSelf -and $j.Self) { $nodes += $j.Self }
    foreach ($v in $nodes) {
        if ($v.Online -eq $false) { continue }
        $ip = @($v.TailscaleIPs | Where-Object { "$_" -match '^\d+\.\d+\.\d+\.\d+$' }) | Select-Object -First 1
        if (-not $ip) { continue }
        $u = $null; if ($j.User -and $v.UserID) { $u = $j.User."$($v.UserID)" }
        $owner = [string]$u.DisplayName; if (-not $owner) { $owner = ([string]$u.LoginName -replace '@.*$', '') }
        $dev = [string]$v.HostName; $sharee = [bool]$v.ShareeNode
        if ($sharee -or $dev -eq 'device-of-shared-to-user' -or $dev -eq 'localhost') { $dev = '' }
        $dns = ([string]$v.DNSName).TrimEnd('.')
        $r.peers += @{ ip = [string]$ip; device = $dev; owner = $owner; sharee = $sharee; dns = $dns }
    }
    return $r
}

function Get-MpFoundText($e, [int]$proto) {
    # one line of the Join list
    $who = $(if ($e.info -and $e.name) { $e.name } elseif ($e.owner) { $e.owner } else { 'A friend' })
    if ($e.info) {
        $where = $(if ($e.level -ge 0) { 'on ' + $MpPlanets[$e.level] } else { 'at the title screen' })
        $t = "$who is hosting, $where ($($e.players) of $($e.max) players)"
        if ($e.players -ge $e.max) { $t += ', full' }
    } else { $t = "$who is hosting" }
    $pc = $(if ($e.device) { $e.device } elseif ($e.owner -and $e.info) { "$($e.owner)'s PC" } elseif ($e.lan) { "home network, $($e.ip)" } else { '' })
    if ($pc) { $t += "  -  $pc" }
    if ($proto -and $e.version -ne $proto) { $t += '  -  another version: both need the latest update' }
    return $t
}

# ================================================================ launcher choice and Steam shortcut (shared by the window, -Console and the launcher)
# generated\setup_choice.txt remembers what the player chose in the setup window (key=value lines, UTF-8 without BOM): launcher=yes|no.
# Setup.cmd reads it, so a later run follows the choice without asking. generated\ is never touched by an update.
$SteamAppName = $GameName      # [MP] upstream's entry ("Ratchet & Clank (RAC-RS)") is never touched

function Get-SetupChoice([string]$key) {
    $f = Join-Path $root 'generated\setup_choice.txt'
    if (Test-Path -LiteralPath $f -PathType Leaf) {
        try { foreach ($l in ([System.IO.File]::ReadAllLines($f))) { if ($l -match ('^' + [regex]::Escape($key) + '=(.*)$')) { return $Matches[1].Trim().ToLowerInvariant() } } } catch { }
    }
    return ''
}

function Set-SetupChoice([string]$key, [string]$value) {
    $f = Join-Path $root 'generated\setup_choice.txt'
    $lines = @()
    if (Test-Path -LiteralPath $f -PathType Leaf) {
        try { $lines = @([System.IO.File]::ReadAllLines($f) | Where-Object { $_.Trim() -and ($_ -notmatch ('^' + [regex]::Escape($key) + '=')) }) } catch { $lines = @() }
    }
    $lines += "$key=$value"
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $f) | Out-Null
    [System.IO.File]::WriteAllText($f, (($lines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))
}

# --- Steam: "Add to Steam" writes one non-Steam-game entry into the binary file userdata\<id>\config\shortcuts.vdf of every Steam user.
# Binary VDF: a section is  0  key  0  fields...  8 ; a field is  type  key  0  value  with type 1 = text (then 0), 2 = 32-bit number, 8 = end of section.
# The file is  0 "shortcuts" 0  [entry "0", entry "1", ...]  8  8.  Only our entry is added, updated or removed; every other entry is copied byte for byte.
function Find-VdfNul([byte[]]$b, [int]$pos) {
    $i = [Array]::IndexOf($b, [byte]0, $pos)
    if ($i -lt 0) { throw 'shortcuts.vdf: a text is not closed' }
    return ($i + 1)
}

function Skip-VdfValue([byte[]]$b, [int]$type, [int]$pos) {
    # the index just after a value of this type that starts at $pos (nested sections are skipped whole)
    switch ($type) {
        0 {
            while ($true) {
                if ($pos -ge $b.Length) { throw 'shortcuts.vdf: a section is not closed' }
                $t = [int]$b[$pos]; $pos++
                if ($t -eq 8) { return $pos }
                $pos = Find-VdfNul $b $pos
                $pos = Skip-VdfValue $b $t $pos
            }
        }
        1 { return (Find-VdfNul $b $pos) }
        { $_ -in 2, 3, 4, 6 } { $e = $pos + 4 }
        { $_ -in 7, 10 } { $e = $pos + 8 }
        default { throw ('shortcuts.vdf: unknown value type ' + $type) }
    }
    if ($e -gt $b.Length) { throw 'shortcuts.vdf: the file is cut off' }
    return $e
}

function Get-VdfFields([byte[]]$b, [int]$pos, [int]$end) {
    # the fields of one section body: from $pos to $end (= just after the closing 8). Each: Type, Key, From (type byte), VFrom (value start), To (just after the value)
    $list = New-Object System.Collections.ArrayList
    $limit = $end - 1
    while ($pos -lt $limit) {
        $from = $pos; $t = [int]$b[$pos]; $pos++
        $ks = $pos; $pos = Find-VdfNul $b $pos
        $key = [System.Text.Encoding]::UTF8.GetString($b, $ks, $pos - 1 - $ks)
        $vs = $pos; $pos = Skip-VdfValue $b $t $pos
        [void]$list.Add(@{ Type = $t; Key = $key; From = $from; VFrom = $vs; To = $pos })
    }
    return ,$list
}

function New-VdfEmptyShortcuts {
    $ms = New-Object System.IO.MemoryStream
    $ms.WriteByte(0); $kb = [System.Text.Encoding]::ASCII.GetBytes('shortcuts'); $ms.Write($kb, 0, $kb.Length); $ms.WriteByte(0); $ms.WriteByte(8); $ms.WriteByte(8)
    return ,$ms.ToArray()
}

function Read-ShortcutsVdf([byte[]]$b) {
    # Entries: Key, From (the entry's type byte), BodyFrom (after its key), To (after its closing 8). BodyFrom/DictEnd of "shortcuts" are indexes into $b.
    if ($b.Length -lt 13 -or $b[0] -ne 0) { throw 'shortcuts.vdf: not a shortcuts file' }
    $pos = Find-VdfNul $b 1
    if ([System.Text.Encoding]::ASCII.GetString($b, 1, $pos - 2) -ne 'shortcuts') { throw 'shortcuts.vdf: the first section is not "shortcuts"' }
    $bodyFrom = $pos
    $entries = New-Object System.Collections.ArrayList
    while ($true) {
        if ($pos -ge $b.Length) { throw 'shortcuts.vdf: the file is cut off' }
        $t = [int]$b[$pos]
        if ($t -eq 8) { break }
        if ($t -ne 0) { throw 'shortcuts.vdf: an entry is not a section' }
        $from = $pos; $pos++
        $ks = $pos; $pos = Find-VdfNul $b $pos
        $key = [System.Text.Encoding]::UTF8.GetString($b, $ks, $pos - 1 - $ks)
        $bs = $pos; $pos = Skip-VdfValue $b 0 $pos
        [void]$entries.Add(@{ Key = $key; From = $from; BodyFrom = $bs; To = $pos })
    }
    if (($pos + 2) -ne $b.Length -or $b[$pos + 1] -ne 8) { throw 'shortcuts.vdf: unexpected content after the shortcuts section' }
    return @{ BodyFrom = $bodyFrom; DictEnd = $pos; Entries = $entries }
}

function Get-VdfEntryInfo([byte[]]$b, $e) {
    # lower-case key -> value (text as string, numbers as [uint32]) of the fields of one entry
    $info = @{}
    foreach ($f in (Get-VdfFields $b $e.BodyFrom $e.To)) {
        $k = $f.Key.ToLowerInvariant()
        if ($f.Type -eq 1) { $info[$k] = [System.Text.Encoding]::UTF8.GetString($b, $f.VFrom, $f.To - 1 - $f.VFrom) }
        elseif ($f.Type -eq 2) { $info[$k] = [BitConverter]::ToUInt32($b, $f.VFrom) }
    }
    return $info
}

function Write-VdfField($ms, [string]$key, $value) {
    $isText = ($value -is [string])
    $ms.WriteByte($(if ($isText) { 1 } else { 2 }))
    $kb = [System.Text.Encoding]::UTF8.GetBytes($key); $ms.Write($kb, 0, $kb.Length); $ms.WriteByte(0)
    if ($isText) { $vb = [System.Text.Encoding]::UTF8.GetBytes([string]$value); $ms.Write($vb, 0, $vb.Length); $ms.WriteByte(0) }
    else { $vb = [BitConverter]::GetBytes([uint32]$value); $ms.Write($vb, 0, 4) }
}

function New-ShortcutEntry([string]$key, [hashtable]$d) {
    # a new entry with the fields Steam writes itself
    $ms = New-Object System.IO.MemoryStream
    $ms.WriteByte(0); $kb = [System.Text.Encoding]::UTF8.GetBytes($key); $ms.Write($kb, 0, $kb.Length); $ms.WriteByte(0)
    Write-VdfField $ms 'appid' $d.appid
    Write-VdfField $ms 'AppName' $d.AppName
    Write-VdfField $ms 'Exe' $d.Exe
    Write-VdfField $ms 'StartDir' $d.StartDir
    Write-VdfField $ms 'icon' $d.icon
    Write-VdfField $ms 'ShortcutPath' ''
    Write-VdfField $ms 'LaunchOptions' ''
    Write-VdfField $ms 'IsHidden' ([uint32]0)
    Write-VdfField $ms 'AllowDesktopConfig' ([uint32]1)
    Write-VdfField $ms 'AllowOverlay' ([uint32]1)
    Write-VdfField $ms 'OpenVR' ([uint32]0)
    Write-VdfField $ms 'Devkit' ([uint32]0)
    Write-VdfField $ms 'DevkitGameID' ''
    Write-VdfField $ms 'DevkitOverrideAppID' ([uint32]0)
    Write-VdfField $ms 'LastPlayTime' ([uint32]0)
    Write-VdfField $ms 'FlatpakAppID' ''
    $ms.WriteByte(0); $tb = [System.Text.Encoding]::ASCII.GetBytes('tags'); $ms.Write($tb, 0, $tb.Length); $ms.WriteByte(0); $ms.WriteByte(8)
    $ms.WriteByte(8)
    return ,$ms.ToArray()
}

function Update-ShortcutEntry([byte[]]$b, $e, [hashtable]$d) {
    # our entry exists: set only the five fields we own (appid, AppName, Exe, StartDir, icon); everything else in it (tags, play time, launch options ...) stays byte for byte
    $own = @{ 'appid' = $d.appid; 'appname' = $d.AppName; 'exe' = $d.Exe; 'startdir' = $d.StartDir; 'icon' = $d.icon }
    $seen = @{}
    $ms = New-Object System.IO.MemoryStream
    $ms.Write($b, $e.From, $e.BodyFrom - $e.From)
    foreach ($f in (Get-VdfFields $b $e.BodyFrom $e.To)) {
        $lk = $f.Key.ToLowerInvariant()
        if ($own.ContainsKey($lk) -and -not $seen.ContainsKey($lk)) { $seen[$lk] = $true; Write-VdfField $ms $f.Key $own[$lk] }
        else { $ms.Write($b, $f.From, $f.To - $f.From) }
    }
    foreach ($k in @('appid', 'AppName', 'Exe', 'StartDir', 'icon')) { if (-not $seen.ContainsKey($k.ToLowerInvariant())) { Write-VdfField $ms $k $own[$k.ToLowerInvariant()] } }
    $ms.WriteByte(8)
    return ,$ms.ToArray()
}

function Test-BytesEqual([byte[]]$a, [byte[]]$b) {
    if ($a.Length -ne $b.Length) { return $false }
    for ($i = 0; $i -lt $a.Length; $i++) { if ($a[$i] -ne $b[$i]) { return $false } }
    return $true
}

function Update-ShortcutsBytes([byte[]]$orig, [string]$mode, [hashtable]$d) {
    # mode 'add' (add our entry, or update it when it is there) or 'remove'. Returns Bytes (the new file), Changed, Action (added updated unchanged removed absent).
    # An entry is ours when its AppName is ours or its appid is ours. A removal from a file with consecutive keys 0,1,2 ... renumbers the keys after it (only the key text changes).
    $b = $(if ($orig.Length -eq 0) { New-VdfEmptyShortcuts } else { $orig })
    $v = Read-ShortcutsVdf $b
    $consecutive = $true; $idx = 0; $next = 0
    foreach ($e in $v.Entries) {
        if ($e.Key -ne [string]$idx) { $consecutive = $false }
        $idx++
        $n = 0; if ([int]::TryParse($e.Key, [ref]$n) -and ($n + 1) -gt $next) { $next = $n + 1 }
    }
    $ms = New-Object System.IO.MemoryStream
    $ms.Write($b, 0, $v.BodyFrom)
    $action = $(if ($mode -eq 'add') { 'added' } else { 'absent' })
    $matched = $false; $keep = 0
    foreach ($e in $v.Entries) {
        $info = Get-VdfEntryInfo $b $e
        $mine = (($info.ContainsKey('appname') -and ($info['appname'] -is [string]) -and ([string]$info['appname'] -ceq $d.AppName)) -or ($info.ContainsKey('appid') -and ($info['appid'] -is [uint32]) -and ([uint32]$info['appid'] -eq [uint32]$d.appid)))
        if ($mine -and $mode -eq 'remove') { $action = 'removed'; continue }
        if ($mine) {
            if ($matched) { $action = 'updated'; continue }       # a second copy of ours is dropped
            $matched = $true
            $nb = Update-ShortcutEntry $b $e $d
            $ms.Write($nb, 0, $nb.Length)
            $action = 'updated'
            $keep++
            continue
        }
        if ($mode -eq 'remove' -and $consecutive) {
            $kb = [System.Text.Encoding]::UTF8.GetBytes([string]$keep)
            $ms.WriteByte(0); $ms.Write($kb, 0, $kb.Length); $ms.WriteByte(0); $ms.Write($b, $e.BodyFrom, $e.To - $e.BodyFrom)
        } else { $ms.Write($b, $e.From, $e.To - $e.From) }
        $keep++
    }
    if ($mode -eq 'add' -and -not $matched) { $nb = New-ShortcutEntry ([string]$next) $d; $ms.Write($nb, 0, $nb.Length) }
    $ms.Write($b, $v.DictEnd, $b.Length - $v.DictEnd)
    $out = $ms.ToArray()
    $changed = -not (Test-BytesEqual $out $orig)
    if (-not $changed) { $action = $(if ($mode -eq 'add') { 'unchanged' } else { 'absent' }) }
    return @{ Bytes = $out; Changed = $changed; Action = $action }
}

$script:Crc32Table = $null
function Get-Crc32([byte[]]$data) {
    # the usual CRC-32 (zlib), in 64-bit integers so that PowerShell 5.1 never sees a negative number
    if (-not $script:Crc32Table) {
        $t = New-Object 'long[]' 256
        for ($i = 0; $i -lt 256; $i++) {
            $c = [long]$i
            for ($k = 0; $k -lt 8; $k++) { if (($c -band 1L) -ne 0) { $c = 0xEDB88320L -bxor ($c -shr 1) } else { $c = $c -shr 1 } }
            $t[$i] = $c
        }
        $script:Crc32Table = $t
    }
    $crc = 0xFFFFFFFFL
    foreach ($x in $data) { $crc = $script:Crc32Table[($crc -bxor [long]$x) -band 0xFFL] -bxor ($crc -shr 8) }
    return ($crc -bxor 0xFFFFFFFFL)
}

function Get-ShortcutAppId([string]$exeQuoted, [string]$name) {
    # the number Steam gives a non-Steam shortcut: CRC-32 of (the Exe field as written, with its quotes) + AppName, top bit set. Grid artwork files are named after it.
    $crc = Get-Crc32 ([System.Text.Encoding]::UTF8.GetBytes($exeQuoted + $name))
    return [uint32]($crc -bor 0x80000000L)
}

function Get-SteamEntryDef {
    $exe = '"' + (Join-Path $root 'build\release\rac-rs.exe') + '"'
    $d = @{ AppName = $SteamAppName; Exe = $exe; StartDir = ('"' + $root.TrimEnd('\') + '\"'); icon = [string](Get-IconFile) }
    $d.appid = Get-ShortcutAppId $exe $SteamAppName
    return $d
}

function Get-SteamRoot {
    # the Steam folder: HKCU\Software\Valve\Steam SteamPath; the test-only environment variable RAC_TEST_STEAM_DIR replaces it (tests never look at the real Steam)
    if ($env:RAC_TEST_STEAM_DIR) { if (Test-Path -LiteralPath $env:RAC_TEST_STEAM_DIR -PathType Container) { return $env:RAC_TEST_STEAM_DIR } else { return '' } }
    try {
        $p = [string](Get-ItemProperty -Path 'HKCU:\Software\Valve\Steam' -Name 'SteamPath' -ErrorAction Stop).SteamPath
        if ($p) { $p = $p.Replace('/', '\'); if (Test-Path -LiteralPath $p -PathType Container) { return $p } }
    } catch { }
    return ''
}

function Get-SteamUserDirs([string]$steam) {
    $ud = Join-Path $steam 'userdata'
    if (-not (Test-Path -LiteralPath $ud -PathType Container)) { return @() }
    return @(Get-ChildItem -LiteralPath $ud -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^\d+$' -and $_.Name -ne '0' } | ForEach-Object { $_.FullName })
}

function Test-SteamRunning {
    # Steam rewrites shortcuts.vdf when it closes, so it must be closed first. We only look, we never close it. (With RAC_TEST_STEAM_DIR only RAC_TEST_STEAM_RUNNING=1 says "running".)
    if ($env:RAC_TEST_STEAM_DIR) { return ($env:RAC_TEST_STEAM_RUNNING -eq '1') }
    try { return [bool](Get-Process -Name 'steam' -ErrorAction Stop) } catch { return $false }
}

function Set-SteamShortcutsFile([string]$path, [string]$mode, [hashtable]$d) {
    # one user's shortcuts.vdf. A .bak of the file as it was is written first; the new file is written beside it and moved into place. Returns the action.
    $exists = Test-Path -LiteralPath $path -PathType Leaf
    if (-not $exists -and $mode -eq 'remove') { return 'absent' }
    $orig = $(if ($exists) { [System.IO.File]::ReadAllBytes($path) } else { New-Object byte[] 0 })
    $r = Update-ShortcutsBytes $orig $mode $d
    if (-not $r.Changed) { return $r.Action }
    if ($exists) { Copy-Item -LiteralPath $path -Destination ($path + '.bak') -Force }
    else { New-Item -ItemType Directory -Force -Path (Split-Path -Parent $path) | Out-Null }
    $tmp = $path + '.rac-tmp'
    [System.IO.File]::WriteAllBytes($tmp, $r.Bytes)
    Move-Item -LiteralPath $tmp -Destination $path -Force
    return $r.Action
}

function Write-SteamGridArt([string]$userDir, [uint32]$appid) {
    # Artwork made from OUR logo only (rac-rs\assets\icon\rac-rs-256.png) on a plain dark card; an existing file is never replaced. Returns the files made.
    $logo = Join-Path $root 'rac-rs\assets\icon\rac-rs-256.png'
    $made = @()
    if (-not (Test-Path -LiteralPath $logo -PathType Leaf)) { return $made }
    Add-Type -AssemblyName System.Drawing
    $grid = Join-Path $userDir 'config\grid'
    $img = [System.Drawing.Image]::FromFile($logo)
    try {
        foreach ($a in @(@{ N = "${appid}p.png"; W = 600; H = 900 }, @{ N = "${appid}.png"; W = 920; H = 430 }, @{ N = "${appid}_hero.png"; W = 1920; H = 620 })) {
            $f = Join-Path $grid $a.N
            if (Test-Path -LiteralPath $f) { continue }
            New-Item -ItemType Directory -Force -Path $grid | Out-Null
            $bmp = New-Object System.Drawing.Bitmap($a.W, $a.H)
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            try {
                $g.Clear([System.Drawing.Color]::FromArgb(24, 28, 38))
                $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $s = [int][math]::Min($a.W * 0.8, $a.H * 0.9)
                $g.DrawImage($img, [int](($a.W - $s) / 2), [int](($a.H - $s) / 2), $s, $s)
            } finally { $g.Dispose() }
            try { $bmp.Save($f, [System.Drawing.Imaging.ImageFormat]::Png) } finally { $bmp.Dispose() }
            $made += $f
        }
    } finally { $img.Dispose() }
    return $made
}

function Invoke-SteamShortcut([string]$mode) {
    # mode 'add' or 'remove', for every Steam user. Status: ok, nosteam (Steam or its users not found), running (Steam is open: NOTHING was changed), error.
    $steam = Get-SteamRoot
    if (-not $steam) { return @{ Status = 'nosteam'; Detail = 'Steam was not found on this PC.'; Actions = @(); Errors = @() } }
    $users = @(Get-SteamUserDirs $steam)
    if ($users.Count -eq 0) { return @{ Status = 'nosteam'; Detail = 'Steam is here, but no Steam user has signed in yet. Start Steam once, sign in, close it, then try again.'; Actions = @(); Errors = @() } }
    if (Test-SteamRunning) { return @{ Status = 'running'; Detail = 'Steam is running. Close Steam first (it rewrites its shortcut list when it closes), then try again.'; Actions = @(); Errors = @() } }
    $d = Get-SteamEntryDef
    $artList = Join-Path $root 'generated\steam_art.txt'
    $acts = @(); $errs = @()
    foreach ($u in $users) {
        $vdf = Join-Path $u 'config\shortcuts.vdf'
        try {
            $acts += (Set-SteamShortcutsFile $vdf $mode $d)
            if ($mode -eq 'add') {
                $made = @(Write-SteamGridArt $u $d.appid)
                if ($made.Count -gt 0) {
                    # one row per picture: SHA-256, TAB, path (so -RemoveFromSteam can tell our picture from one the player swapped in)
                    $rows = @($made | ForEach-Object { (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash + "`t" + $_ })
                    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $artList) | Out-Null
                    [System.IO.File]::AppendAllText($artList, (($rows -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))
                }
            }
        } catch { $errs += ($vdf + ': ' + $_.Exception.Message) }
    }
    if ($mode -eq 'remove' -and (Test-Path -LiteralPath $artList -PathType Leaf)) {
        # the artwork this setup made (rows "SHA-256 TAB path" in generated\steam_art.txt): a file goes only while its hash is still ours; a picture the player swapped in stays
        foreach ($l in ([System.IO.File]::ReadAllLines($artList))) {
            $p = $l.Split("`t", 2)
            if ($p.Count -ne 2 -or -not $p[0].Trim() -or -not (Test-Path -LiteralPath $p[1] -PathType Leaf)) { continue }
            try { if ((Get-FileHash -LiteralPath $p[1] -Algorithm SHA256).Hash -eq $p[0].Trim()) { Remove-Item -LiteralPath $p[1] -Force } } catch { }
        }
        Remove-Item -LiteralPath $artList -Force -ErrorAction SilentlyContinue
    }
    $st = $(if ($acts.Count -eq 0) { 'error' } else { 'ok' })
    return @{ Status = $st; Detail = ''; Actions = $acts; Errors = $errs }
}

function Get-SteamResultText([string]$mode, $r) {
    switch ($r.Status) {
        'ok' {
            $t = $(if ($mode -eq 'add') { "Added `"$SteamAppName`" to Steam. Start Steam to see it in your library." } else { "Removed `"$SteamAppName`" from Steam." })
            if ($r.Errors.Count -gt 0) { $t += ' (Not every Steam user could be changed: ' + ($r.Errors -join '; ') + ')' }
            return $t
        }
        'error' { return ('Steam could not be changed: ' + ($r.Errors -join '; ')) }
        default { return $r.Detail }
    }
}

function Get-SteamState {
    # for the window: Found = Steam and at least one user are there; Has = our entry is in some user's shortcuts.vdf
    $found = $false; $has = $false
    $steam = Get-SteamRoot
    if ($steam) {
        $users = @(Get-SteamUserDirs $steam)
        $found = ($users.Count -gt 0)
        if ($found) {
            $d = Get-SteamEntryDef
            foreach ($u in $users) {
                $f = Join-Path $u 'config\shortcuts.vdf'
                if (Test-Path -LiteralPath $f -PathType Leaf) { try { if ((Update-ShortcutsBytes ([System.IO.File]::ReadAllBytes($f)) 'remove' $d).Action -eq 'removed') { $has = $true } } catch { } }
            }
        }
    }
    return @{ Found = $found; Has = $has }
}

# ================================================================ updates (shared by -Launch, the update window, the launcher and the -Console worker)
# Version = the one line in VERSION (missing = 0.1.0, the first public release). generated\built_version.txt = the version the game exe was built from.
# [MP] This fork (rac-rs-multiplayer) updates from its own GitHub releases, never from upstream rac-rs, whose code has no multiplayer:
# the newest published release is the version on offer, and its tag's files are the update. Its versions have a fourth number
# (0.4.1.1: upstream 0.4.1, this fork's first release on it). It does not offer the RAC-RS Launcher either: that updates from upstream.
$Repo = 'adamlbunch/rac-rs-multiplayer'
$RepoArchive = "https://github.com/$Repo/archive"
$OfferLauncher = $false
$script:RemoteTag = ''

function Get-LocalVersion {
    $f = Join-Path $root 'VERSION'
    if (Test-Path -LiteralPath $f -PathType Leaf) { try { $v = ([System.IO.File]::ReadAllText($f)).Trim(); if ($v -match '^\d+\.\d+\.\d+(\.\d+)?$') { return $v } } catch { } }
    return '0.1.0'
}

function Get-BuiltVersion {
    $f = Join-Path $root 'generated\built_version.txt'
    if (Test-Path -LiteralPath $f -PathType Leaf) { try { $v = ([System.IO.File]::ReadAllText($f)).Trim(); if ($v -match '^\d+\.\d+\.\d+(\.\d+)?$') { return $v } } catch { } }
    if (Test-Installed) { return '0.1.0' }      # built before versions existed
    return ''
}

function Compare-Version([string]$a, [string]$b) { return ([version]$a).CompareTo([version]$b) }

function Get-RemoteText([string]$url, [int]$timeoutMs) {
    # One text download (GitHub's API wants a User-Agent, which this sends). $null on any failure or after $timeoutMs; never throws.
    try {
        try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch { }
        $wc = New-Object System.Net.WebClient
        $wc.Headers['User-Agent'] = 'rac-rs-updater'
        # Started without the window's synchronization context: WebClient hands its result to the context it was started on, and the
        # window's thread is blocked in Wait below, so with a window open (the change list, "Check for updates") it always timed out.
        $ctx = [System.Threading.SynchronizationContext]::Current
        [System.Threading.SynchronizationContext]::SetSynchronizationContext($null)
        try { $task = $wc.DownloadStringTaskAsync($url) } finally { [System.Threading.SynchronizationContext]::SetSynchronizationContext($ctx) }
        if (-not $task.Wait($timeoutMs)) { try { $wc.CancelAsync() } catch { }; return $null }
        return [string]$task.Result
    } catch { return $null }
}

function Read-Remote([string]$name, [int]$timeoutMs = 4000) {
    # Text of VERSION / CHANGELOG.md of the newest release on GitHub (or from -TestUpdateBase: a folder, a file:// or an http URL).
    # VERSION is the newest release's tag (GitHub's "latest release"), CHANGELOG.md that tag's file. $null on any failure; never throws.
    try {
        if ($TestUpdateBase) {
            if ($TestUpdateBase -notmatch '^(https?|file)://') {
                $f = Join-Path $TestUpdateBase $name
                if (-not (Test-Path -LiteralPath $f -PathType Leaf)) { return $null }
                return [System.IO.File]::ReadAllText($f)
            }
            return (Get-RemoteText ($TestUpdateBase.TrimEnd('/') + '/' + $name) $timeoutMs)
        }
        if (-not $script:RemoteTag) {
            $rel = Get-RemoteText "https://api.github.com/repos/$Repo/releases/latest" $timeoutMs
            if (-not $rel) { return $null }
            $tag = [string](($rel | ConvertFrom-Json).tag_name)
            if ($tag -notmatch '^v\d+\.\d+\.\d+(\.\d+)?$') { return $null }
            $script:RemoteTag = $tag
        }
        if ($name -eq 'VERSION') { return $script:RemoteTag.Substring(1) }
        return (Get-RemoteText "https://raw.githubusercontent.com/$Repo/$($script:RemoteTag)/$name" $timeoutMs)
    } catch { return $null }
}

function Get-ChangelogFor([string]$text, [string]$from, [string]$to) {
    # CHANGELOG.md has one section per version, newest first, starting with "## v<version>". Returns the versions newer than $from up to $to.
    $out = New-Object System.Collections.ArrayList
    $take = $false
    foreach ($l in ($text -split "`r?`n")) {
        if ($l -match '^##\s+v?(\d+\.\d+\.\d+(?:\.\d+)?)') {
            $v = $Matches[1]
            $take = ((Compare-Version $v $from) -gt 0) -and ((Compare-Version $v $to) -le 0)
            if ($take) { if ($out.Count -gt 0) { [void]$out.Add('') }; [void]$out.Add('Version ' + $v) }
            continue
        }
        if ($take -and $l.Trim().Length -gt 0) { [void]$out.Add(($l -replace '^\s*[-*]\s+', ([string][char]0x2022 + ' '))) }
    }
    return ($out -join "`r`n")
}

function Get-FilesHash([string[]]$rels) {
    # SHA-256 over the files (path, NUL, content without CR) so that line-ending changes by git or an unzipper do not matter.
    $latin = [System.Text.Encoding]::GetEncoding(28591)
    $ms = New-Object System.IO.MemoryStream
    foreach ($r in $rels) {
        $nb = $latin.GetBytes($r + [string][char]0); $ms.Write($nb, 0, $nb.Length)
        $b = $latin.GetBytes($latin.GetString([System.IO.File]::ReadAllBytes((Join-Path $root $r))).Replace("`r", '')); $ms.Write($b, 0, $b.Length)
    }
    $ms.Position = 0
    $sha = [System.Security.Cryptography.SHA256]::Create()
    return ([System.BitConverter]::ToString($sha.ComputeHash($ms))).Replace('-', '').ToLowerInvariant()
}

function Get-VuGenFiles {
    $names = [string[]]@([System.IO.Directory]::GetFiles((Join-Path $root 'tools\vu')) | ForEach-Object { [System.IO.Path]::GetFileName($_) })
    [Array]::Sort($names, [System.StringComparer]::Ordinal)
    return @('tools/vugen_iso.py', 'tools/skinff_ee.py', 'tools/skinff_gen.py') + @($names | ForEach-Object { 'tools/vu/' + $_ })
}

function Test-ProtectedPath([string]$rel) {
    # What an update never touches: the toolchain, the build, the generated code, the player's saves, the disc image.
    $r = $rel.Replace('\', '/').TrimStart('/').ToLowerInvariant()
    foreach ($p in @('toolchain/', 'build/', 'generated/', 'player/', 'oracle/rec/', 'oracle/game/', 'test_shots/', 'mods/', 'profiles/')) { if ($r.StartsWith($p)) { return $true } }
    if ($r -in @('launcher.cfg', 'library.json', 'portable.txt', 'rac-rs launcher.exe', 'rac-rs launcher.exe.old')) { return $true }
    if ($r -in @('play.cmd', 'oracle/src/vu0_prog0.rs', 'oracle/src/vu0_prog1.rs', 'oracle/src/vu0_prog9hi.rs', 'oracle/src/skinff_ee.rs', 'oracle/src/skinff_vu.rs', 'rac-rs/src/gsfull/vu1aot_gen.rs', 'rac-rs/src/particles/orb_tables.rs')) { return $true }
    if ($r -match '^rac-rs/src/gsfull/vu1_prog\d+\.rs$') { return $true }
    return $r.EndsWith('.iso')
}

function Get-Sha256Hex([string]$path) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $fs = [System.IO.File]::OpenRead($path)
    try { return [System.BitConverter]::ToString($sha.ComputeHash($fs)) } finally { $fs.Dispose() }
}

function Save-Url([string]$url, [string]$file) {
    try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch { }
    if ($url -like 'file://*') { (New-Object System.Net.WebClient).DownloadFile($url, $file); return }
    $old = $ProgressPreference; $ProgressPreference = 'SilentlyContinue'
    try { Invoke-WebRequest -Uri $url -OutFile $file -UseBasicParsing -TimeoutSec 600 } finally { $ProgressPreference = $old }
}

function Get-UpdateZip([string]$ver, [string]$zip) {
    # v<ver>.zip from this fork's GitHub tag; with -TestUpdateBase from there. Throws on failure.
    if ($TestUpdateBase) {
        if ($TestUpdateBase -match '^(https?|file)://') { Save-Url ($TestUpdateBase.TrimEnd('/') + "/v$ver.zip") $zip; return }
        $f = Join-Path $TestUpdateBase "v$ver.zip"
        if (-not (Test-Path -LiteralPath $f -PathType Leaf)) { throw "not found: $f" }
        Copy-Item -LiteralPath $f -Destination $zip -Force
        return
    }
    $notFound = $false
    for ($try = 1; $try -le 3; $try++) {
        try { Save-Url "$RepoArchive/refs/tags/v$ver.zip" $zip; return }
        catch {
            $code = 0; try { $code = [int]$_.Exception.Response.StatusCode } catch { }
            if ($code -eq 404) { $notFound = $true; break }
            Write-Host "  download failed ($($_.Exception.Message)), retrying" -ForegroundColor Yellow
            if ($try -eq 3) { throw }
            Start-Sleep -Seconds (3 * $try)
        }
    }
    if ($notFound) { throw "version $ver was not found on GitHub ($Repo)" }      # [MP] never upstream's or a branch's code
}

function Expand-UpdateZip([string]$zip, [string]$outDir) {
    # Unpacks the zip without its single top folder (GitHub: rac-rs-<version>/). Refuses paths that leave $outDir.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $outFull = [System.IO.Path]::GetFullPath($outDir).TrimEnd('\') + '\'
    $za = [System.IO.Compression.ZipFile]::OpenRead($zip)
    $top = $null
    try {
        foreach ($e in $za.Entries) {
            $n = $e.FullName.Replace('\', '/')
            if ($n.StartsWith('/') -or $n -match '(^|/)\.\.(/|$)') { throw "unsafe path in the update file: $n" }
            $i = $n.IndexOf('/')
            if ($i -lt 1) { continue }
            $t = $n.Substring(0, $i)
            if ($null -eq $top) { $top = $t } elseif ($top -ne $t) { throw 'the update file has an unexpected layout' }
            $rel = $n.Substring($i + 1)
            if ($rel.Length -eq 0 -or $rel.EndsWith('/')) { continue }
            $dst = [System.IO.Path]::GetFullPath((Join-Path $outFull ($rel.Replace('/', '\'))))
            if (-not $dst.StartsWith($outFull, [System.StringComparison]::OrdinalIgnoreCase)) { throw "unsafe path in the update file: $n" }
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dst) | Out-Null
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($e, $dst, $true)
        }
    } finally { $za.Dispose() }
    foreach ($need in @('VERSION', 'setup.ps1', 'rac-rs\Cargo.toml')) { if (-not (Test-Path -LiteralPath (Join-Path $outFull $need))) { throw "the update file is not a rac-rs release ($need missing)" } }
}

function Sync-Update([string]$src, [string]$dst) {
    # Copies the new version over this install. Only files that are new or differ (SHA-256) are copied, and they get the CURRENT time
    # (cargo rebuilds by file time); unchanged files are not touched, so nothing that did not change is rebuilt. Files the old
    # release_files.txt lists and the new one does not are deleted (no old list = no deletions). Protected paths are never touched.
    # VERSION and release_files.txt come last: an interrupted copy leaves the old VERSION, so the update is offered again.
    $src = [System.IO.Path]::GetFullPath($src).TrimEnd('\') + '\'
    $dst = [System.IO.Path]::GetFullPath($dst).TrimEnd('\') + '\'
    $oldList = @(); $newList = @()
    if (Test-Path -LiteralPath (Join-Path $dst 'release_files.txt')) { $oldList = @(Get-Content -LiteralPath (Join-Path $dst 'release_files.txt') | ForEach-Object { $_.Trim() } | Where-Object { $_ }) }
    if (Test-Path -LiteralPath (Join-Path $src 'release_files.txt')) { $newList = @(Get-Content -LiteralPath (Join-Path $src 'release_files.txt') | ForEach-Object { $_.Trim() } | Where-Object { $_ }) }
    $last = @('VERSION', 'release_files.txt')
    $items = New-Object System.Collections.ArrayList
    foreach ($f in (Get-ChildItem -LiteralPath $src -Recurse -File)) {
        $rel = $f.FullName.Substring($src.Length).Replace('\', '/')
        if ((Test-ProtectedPath $rel) -or ($last -contains $rel)) { continue }
        [void]$items.Add([pscustomobject]@{ Rel = $rel; Full = $f.FullName; Len = $f.Length })
    }
    foreach ($rel in $last) { $p = Join-Path $src $rel; if (Test-Path -LiteralPath $p) { [void]$items.Add([pscustomobject]@{ Rel = $rel; Full = $p; Len = (Get-Item -LiteralPath $p).Length }) } }
    $changed = New-Object System.Collections.ArrayList
    foreach ($it in $items) {
        $d = Join-Path $dst ($it.Rel.Replace('/', '\'))
        $copy = $true
        if (Test-Path -LiteralPath $d -PathType Leaf) {
            if ((Get-Item -LiteralPath $d).Length -eq $it.Len -and (Get-Sha256Hex $d) -eq (Get-Sha256Hex $it.Full)) { $copy = $false }
        }
        if ($copy) {
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $d) | Out-Null
            [System.IO.File]::Copy($it.Full, $d, $true)
            [System.IO.File]::SetLastWriteTimeUtc($d, [DateTime]::UtcNow)
            [void]$changed.Add($it.Rel)
            Write-Host "  updated $($it.Rel)"
        }
    }
    $removed = New-Object System.Collections.ArrayList
    if ($oldList.Count -gt 0 -and $newList.Count -gt 0) {
        $keep = @{}; foreach ($n in $newList) { $keep[$n] = $true }
        foreach ($o in $oldList) {
            if ($keep.ContainsKey($o) -or ($last -contains $o) -or (Test-ProtectedPath $o) -or $o -match '(^|/)\.\.(/|$)' -or $o.StartsWith('/')) { continue }
            $d = Join-Path $dst ($o.Replace('/', '\'))
            if (Test-Path -LiteralPath $d -PathType Leaf) {
                Remove-Item -LiteralPath $d -Force
                [void]$removed.Add($o)
                Write-Host "  removed $o"
                $dir = Split-Path -Parent $d
                while ($dir.Length -gt $dst.Length -and @(Get-ChildItem -LiteralPath $dir -Force).Count -eq 0) { Remove-Item -LiteralPath $dir -Force; $dir = Split-Path -Parent $dir }
            }
        }
    }
    # Marker for the -ApplyUpdate worker: this version's files were put in place HERE (changed files carry the current time). The setup build removes it when it succeeds.
    $gdir = Join-Path $dst 'generated'
    New-Item -ItemType Directory -Force -Path $gdir | Out-Null
    $ver = $(try { ([System.IO.File]::ReadAllText((Join-Path $dst 'VERSION'))).Trim() } catch { '' })
    if ($ver -match '^\d+\.\d+\.\d+(\.\d+)?$') { [System.IO.File]::WriteAllText((Join-Path $gdir ("synced_$ver")), "$ver`n", (New-Object System.Text.UTF8Encoding($false))) }
    return [pscustomobject]@{ Changed = $changed; Removed = $removed }
}

function Set-ReleaseFileTimes {
    # Sets the time of every release file (release_files.txt) to now; returns how many. Used when the files of a new version were NOT copied by
    # Sync-Update but unzipped by hand: a ZIP keeps the times of its files (the commit time), and if the last build is newer than those, cargo would
    # think nothing changed and silently keep the old game. With the times set to now, the next build compiles everything once.
    $rels = @()
    $list = Join-Path $root 'release_files.txt'
    if (Test-Path -LiteralPath $list -PathType Leaf) { $rels = @(Get-Content -LiteralPath $list | ForEach-Object { $_.Trim() } | Where-Object { $_ }) }
    else {
        foreach ($d in @('oracle', 'rac-rs', 'tools')) {
            $dd = Join-Path $root $d
            if (Test-Path -LiteralPath $dd) { foreach ($f in (Get-ChildItem -LiteralPath $dd -Recurse -File)) { $rels += $f.FullName.Substring($root.Length).TrimStart('\').Replace('\', '/') } }
        }
    }
    $now = [DateTime]::UtcNow; $n = 0
    foreach ($r in $rels) {
        if ((Test-ProtectedPath $r) -or $r.StartsWith('/') -or $r -match '(^|/)\.\.(/|$)') { continue }
        $f = Join-Path $root ($r.Replace('/', '\'))
        if (Test-Path -LiteralPath $f -PathType Leaf) { [System.IO.File]::SetLastWriteTimeUtc($f, $now); $n++ }
    }
    return $n
}

function Get-GameProcs {
    $exe = Join-Path $root 'build\release\rac-rs.exe'
    $r = @()
    foreach ($p in @(Get-Process -Name 'rac-rs' -ErrorAction SilentlyContinue)) { try { if ($p.Path -eq $exe) { $r += $p } } catch { } }
    return $r
}

function Wait-GameClosed {
    # The build cannot replace rac-rs.exe while it runs: wait for the exe that started the update (and any other of this install).
    if ($ExePid -gt 0) { try { $p = [System.Diagnostics.Process]::GetProcessById($ExePid); [void]$p.WaitForExit(60000) } catch { } }
    $end = (Get-Date).AddSeconds(10)
    while (((Get-GameProcs).Count -gt 0) -and ((Get-Date) -lt $end)) { Start-Sleep -Milliseconds 300 }
}

function Start-UpdateProcess([string]$ver, [bool]$finish) {
    # A separate hidden setup.ps1 -Update: it outlives this process (which tells the game exe to exit) and shows the update window.
    $a = '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + (Join-Path $root 'setup.ps1') + '" -Update -NewVersion ' + $ver + ' -ExePid ' + $ExePid
    if ($finish) { $a += ' -FinishOnly' }
    if ($TestUpdateBase) { $a += ' -TestUpdateBase "' + $TestUpdateBase.TrimEnd('\') + '"' }
    if ($TestNoShow) { $a += ' -TestNoShow -TestShot "' + $(if ($TestShot) { $TestShot.TrimEnd('\') } else { Join-Path $root 'test_shots' }) + '"' }
    $null = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $a -WorkingDirectory $root -WindowStyle Hidden
}

# -GetLauncher (started by Setup.cmd when the launcher exe is missing): fetch the latest launcher RELEASE of AsobiCodes/rac-rs-launcher, check its
# SHA-256 digest, place "RAC-RS Launcher.exe" in this folder. Exit code 0 = placed; 1 = anything else (Setup.cmd then opens the old setup window).
if ($GetLauncher) {
    $code = 1
    $launcherExe = Join-Path $root 'RAC-RS Launcher.exe'
    try {
        $assetName = 'rac-rs-launcher-windows-x64.exe'
        try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch { }
        if ($TestLauncherReleases) { $relText = Get-Content -LiteralPath (Join-Path $TestLauncherReleases 'latest.json') -Raw }
        else {
            $old = $ProgressPreference; $ProgressPreference = 'SilentlyContinue'
            try { $relText = (Invoke-WebRequest -Uri 'https://api.github.com/repos/AsobiCodes/rac-rs-launcher/releases/latest' -UseBasicParsing -TimeoutSec 15 -Headers @{ 'Accept' = 'application/vnd.github+json'; 'User-Agent' = 'rac-rs-setup' }).Content } finally { $ProgressPreference = $old }
        }
        $rel = $relText | ConvertFrom-Json
        $asset = @($rel.assets | Where-Object { $_.name -eq $assetName })[0]
        if (-not $asset) { throw 'the release has no launcher exe' }
        if (-not ([string]$asset.digest -match '^sha256:([0-9a-fA-F]{64})$')) { throw 'the launcher exe has no checksum' }
        $want = $Matches[1].ToLower()
        $part = $launcherExe + '.part'
        if ($TestLauncherReleases) {
            $srcName = $(if ([string]$asset.browser_download_url -match '^https?://') { $asset.name } else { $asset.browser_download_url })
            Copy-Item -LiteralPath (Join-Path $TestLauncherReleases $srcName) -Destination $part -Force
        } else { Save-Url ([string]$asset.browser_download_url) $part }
        $got = (Get-Sha256Hex $part).Replace('-', '').ToLower()
        if ($got -ne $want) { Remove-Item -LiteralPath $part -Force; throw 'the checksum does not match' }
        Move-Item -LiteralPath $part -Destination $launcherExe -Force
        $code = 0
    } catch {
        try { Remove-Item -LiteralPath ($launcherExe + '.part') -Force -ErrorAction SilentlyContinue } catch { }
    }
    exit $code
}

# -AddToSteam / -RemoveFromSteam on their own (no -Iso; the launcher calls these): do just that, say what happened, exit. No window, no prompt, Steam is never closed.
# Exit code: 0 = done (or nothing to do), 1 = error, 2 = Steam (or a Steam user) not found, 3 = Steam is running (nothing was changed).
if (($AddToSteam -or $RemoveFromSteam) -and -not $Iso) {
    $steamMode = $(if ($RemoveFromSteam) { 'remove' } else { 'add' })
    $steamRes = Invoke-SteamShortcut $steamMode
    Write-Host (Get-SteamResultText $steamMode $steamRes)
    exit $(switch ($steamRes.Status) { 'ok' { 0 } 'nosteam' { 2 } 'running' { 3 } default { 1 } })
}

# -Launch (started by the game exe): is there something to update? Offline, an error or "not newer" = exit 0 at once, no window.
$launchMode = ''; $launchNew = ''
if ($Launch) {
    try {
        $lv = Get-LocalVersion; $bv = Get-BuiltVersion
        if ($bv -and ($bv -ne $lv)) { $launchMode = 'finish'; $launchNew = $lv }
        # Also when an update is unfinished: a newer version online replaces it (the unfinished one may be what fails to build, e.g. a bad release).
        $rv = Read-Remote 'VERSION'
        if ($rv) { $rv = $rv.Trim(); if (($rv -match '^\d+\.\d+\.\d+(\.\d+)?$') -and ((Compare-Version $rv $lv) -gt 0)) { $launchMode = 'offer'; $launchNew = $rv } }
    } catch { $launchMode = '' }
    if (-not $launchMode) { exit 0 }
    if ($WindowFlag) { try { [System.IO.File]::WriteAllText($WindowFlag, 'window') } catch { } }     # the exe now waits for the answer without a time limit
}

# ================================================================ launcher window (default; -Console skips it and runs the steps below in the console)
if (-not $Console) {
    $S = @{
        view = ''; discOk = $false; discPath = ''; check = $null; repair = $false
        proc = $null; outPath = ''; errPath = ''; outPos = [long]0; errPos = [long]0; outRest = ''; errRest = ''
        log = (New-Object System.Text.StringBuilder); step = 0; levels = 0; compiled = 0; progress = 0.0
        t0 = $null; lastError = ''; status = ''; shortcut = $false; ranOk = $false; finished = $false
        update = $false; newV = ''; finish = $false; finishPending = $false; workerExtra = ''; workerCode = 0; buildT0 = $null; expect = 0; exitCode = 0
        test = [bool]$TestNoShow; toolsSub = 0.0; vu = $false; suppress = $false; shortcutDir = ''; shortcutPath = ''
        fetch = $null; fetchErr = ''; wantShortcut = $false; handedOver = $false; steamWanted = $false; steamHas = $false; steamNote = ''; askAnswer = 'Cancel'; boxLauncher = $false; boxSteam = $false; launcherBtn = $true
        mpBack = 'installed'; mpAddr = ''; mpLoaded = $false; mpFresh = $false
        find = @{ udp = $null; proc = $null; out = $null; round = $false; next = [datetime]::MinValue; found = @{}; ts = @{}; tsOn = $null; sharees = @(); shown = @(); rounds = 0; self = $false }
        findPick = $false; findTouched = $false
        updTask = $null; updDone = $false; updAvail = ''
    }
    # test mode never looks at the real Steam: without a fake Steam folder (RAC_TEST_STEAM_DIR) Steam counts as not found
    if ($TestNoShow -and -not $env:RAC_TEST_STEAM_DIR) { $env:RAC_TEST_STEAM_DIR = Join-Path ([System.IO.Path]::GetTempPath()) 'rac_no_steam_here' }
    $StepNames = @('Download tools', 'Check disc', 'Prepare game code', 'Build game', 'Finish')
    $StepNamesInstall = $StepNames
    $StepNamesUpdate = @('Download update', 'Check tools', 'Prepare game code', 'Build game', 'Finish')
    $ExpectedCompiles = 563      # "Compiling" lines of a complete first build (measured); the bar never goes above 98% for the build
    $AppTitle = $GameName
    $psExe = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $UI = @{}

    try {
        Add-Type -AssemblyName System.Windows.Forms
        Add-Type -AssemblyName System.Drawing
        # sharp text on high-DPI screens (no compiler needed: a dynamic P/Invoke)
        try {
            $ab = [AppDomain]::CurrentDomain.DefineDynamicAssembly((New-Object Reflection.AssemblyName('RacDpi')), [Reflection.Emit.AssemblyBuilderAccess]::Run)
            $tb = $ab.DefineDynamicModule('m').DefineType('N', 'Public,Class')
            $null = $tb.DefinePInvokeMethod('SetProcessDPIAware', 'user32.dll', 'Public,Static,PinvokeImpl', 'Standard', [bool], [Type[]]@(), 'Winapi', 'Auto')
            $null = $tb.CreateType().GetMethod('SetProcessDPIAware').Invoke($null, $null)
        } catch { }
        [System.Windows.Forms.Application]::EnableVisualStyles()
        [System.Windows.Forms.Application]::SetCompatibleTextRenderingDefault($false)
    } catch {
        Write-Host "The launcher window could not start ($($_.Exception.Message)). Use Setup.cmd -Console instead." -ForegroundColor Red
        exit 1
    }
    $Mutex = $null
    if (-not $TestNoShow) {
        $created = $false
        $mname = $(if ($Launch) { 'Local\RacRsMpUpdateOffer' } elseif ($Update) { 'Local\RacRsMpUpdate' } else { 'Local\RacRsMpLauncher' })      # [MP] apart from upstream's
        $Mutex = New-Object System.Threading.Mutex($true, $mname, [ref]$created)
        if (-not $created) {
            if (-not $Launch -and -not $Update) { [void][System.Windows.Forms.MessageBox]::Show('The Ratchet & Clank launcher is already open.', $AppTitle, 'OK', 'Information') }
            exit 0
        }
    }

    # ---------------------------------------------------------------- display scale
    # The window is laid out in "design units" (96 DPI: 580 x 440 and so on). $LayoutK turns design units into pixels and is worked out ONCE, here:
    #   $want = the Windows display scale (125% = 1.25, 300% = 3.0; -TestDpi N pretends N percent),
    #   cut down so that the tallest view (580 units: Play together, with its Join list) and the width (580 units) fit the working area of the screen (taskbar excluded).
    # So at 300% on a 1920 x 1080 screen the window is smaller than 3.0 times (about 1.9) instead of bigger than the screen.
    # Fonts are set in points, which Windows already scales with the display DPI, so they only get the difference ($fMul; 1.0 on a real display
    # that is not cut down). There is no WinForms auto-scaling: one scale pass over the controls (Scale-Layout) and Set-FormHeight below.
    # Panels scroll as a last resort (a screen so small that even the cut-down window does not fit).
    $DesignW = 580; $DesignHMax = 580
    $dpiScale = 1.0
    try { $g0 = [System.Drawing.Graphics]::FromHwnd([IntPtr]::Zero); $dpiScale = [double]$g0.DpiX / 96.0; $g0.Dispose() } catch { $dpiScale = 1.0 }
    if ($dpiScale -lt 1.0 -or $dpiScale -gt 6.0) { $dpiScale = 1.0 }
    $want = $dpiScale
    if ($TestDpi -ge 50 -and $TestDpi -le 600) { $want = $TestDpi / 100.0 }
    $wa = $null
    if ($TestScreen -match '^(\d+)x(\d+)$') { $wa = New-Object System.Drawing.Rectangle(0, 0, [int]$Matches[1], ([int]$Matches[2] - 40)) }
    else { try { $wa = [System.Windows.Forms.Screen]::FromPoint([System.Windows.Forms.Cursor]::Position).WorkingArea } catch { $wa = $null } }
    if (-not $wa -or $wa.Width -lt 300 -or $wa.Height -lt 300) { $wa = New-Object System.Drawing.Rectangle(0, 0, 1280, 720) }
    # title bar + borders of this window style (measured on a throw-away form; scaled like the display when the scale is only pretended)
    $probe = New-Object System.Windows.Forms.Form
    $probe.FormBorderStyle = 'FixedDialog'; $probe.ClientSize = New-Object System.Drawing.Size(100, 100)
    $ncH = ($probe.Height - 100) * ($want / $dpiScale); $ncW = ($probe.Width - 100) * ($want / $dpiScale)
    $probe.Dispose()
    $LayoutK = [math]::Min($want, [math]::Min(($wa.Height - $ncH - 6) / $DesignHMax, ($wa.Width - $ncW - 6) / $DesignW))
    $LayoutK = [math]::Max($LayoutK, 0.8)
    $MaxClientH = [int][math]::Floor($wa.Height - $ncH - 6)
    $fMul = $LayoutK / $dpiScale
    function New-Font([string]$family, [double]$pt) { return (New-Object System.Drawing.Font($family, [single]($pt * $fMul))) }

    # ---------------------------------------------------------------- helpers
    # [MP] the look: a night sky with this project's own logo (rac-rs\assets\icon\rac-rs-256.png) and bolt-orange buttons. Everything is drawn
    # here (nothing from the game disc). Headings and the big buttons use Bahnschrift (Windows 10 and later), else Segoe UI Semibold.
    $HeadFamily = 'Bahnschrift SemiBold'
    try { $t0 = New-Object System.Drawing.Font($HeadFamily, [single]10); if ($t0.Name -ne $HeadFamily) { $HeadFamily = 'Segoe UI Semibold' }; $t0.Dispose() } catch { $HeadFamily = 'Segoe UI Semibold' }
    $fN = New-Font 'Segoe UI' 9.5
    $fSmall = New-Font 'Segoe UI' 8.75
    $fTitle = New-Font $HeadFamily 17
    $fBig = New-Font $HeadFamily 24
    $fBtn = New-Font $HeadFamily 10.5
    $fGlyph = New-Font 'Segoe UI Symbol' 11
    $fMono = New-Font 'Consolas' 8.5
    $cText = [System.Drawing.Color]::FromArgb(234, 239, 250)
    $cGrey = [System.Drawing.Color]::FromArgb(150, 164, 196)
    $cGreen = [System.Drawing.Color]::FromArgb(98, 214, 138)
    $cRed = [System.Drawing.Color]::FromArgb(255, 120, 102)
    $cBlue = [System.Drawing.Color]::FromArgb(96, 200, 255)
    $cAccent = [System.Drawing.Color]::FromArgb(247, 141, 36)         # bolt orange: the main buttons, the progress bar
    $cAccent2 = [System.Drawing.Color]::FromArgb(32, 150, 206)        # the second big button (Play together)
    $cBg = [System.Drawing.Color]::FromArgb(10, 15, 34)
    $cField = [System.Drawing.Color]::FromArgb(20, 30, 60)            # text boxes and the list
    $cBtn2 = [System.Drawing.Color]::FromArgb(38, 54, 96)             # the other buttons
    $cOff = [System.Drawing.Color]::FromArgb(50, 58, 84)              # a button that cannot be clicked yet

    function Get-Tint($c, [double]$t) {
        # t > 0: towards white, t < 0: towards black
        if ($t -ge 0) { return [System.Drawing.Color]::FromArgb($c.A, [int]($c.R + (255 - $c.R) * $t), [int]($c.G + (255 - $c.G) * $t), [int]($c.B + (255 - $c.B) * $t)) }
        $m = 1 + $t
        return [System.Drawing.Color]::FromArgb($c.A, [int]($c.R * $m), [int]($c.G * $m), [int]($c.B * $m))
    }
    function New-RoundPath([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
        $p = New-Object System.Drawing.Drawing2D.GraphicsPath
        $d = [single](2 * $r)
        $p.AddArc($x, $y, $d, $d, 180, 90); $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
        $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90); $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
        $p.CloseFigure()
        return $p
    }
    function New-ThemeBackground([int]$w, [int]$h) {
        # one picture behind every view (the panels show its top part): sky, glow, stars, the logo top right and an orange mark under the title
        $k = $LayoutK
        $bmp = New-Object System.Drawing.Bitmap($w, $h)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        try {
            $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
            $all = New-Object System.Drawing.Rectangle(0, 0, $w, $h)
            $br = New-Object System.Drawing.Drawing2D.LinearGradientBrush($all, [System.Drawing.Color]::FromArgb(8, 12, 30), [System.Drawing.Color]::FromArgb(22, 32, 66), [single]90)
            $g.FillRectangle($br, $all); $br.Dispose()
            foreach ($gl in @(@(540, 0, 300, 190, 240, 116, 36, 52), @(30, 470, 320, 230, 46, 92, 214, 46), @(300, 300, 420, 170, 112, 62, 178, 20))) {
                $path = New-Object System.Drawing.Drawing2D.GraphicsPath
                $path.AddEllipse([single](($gl[0] - $gl[2]) * $k), [single](($gl[1] - $gl[3]) * $k), [single](2 * $gl[2] * $k), [single](2 * $gl[3] * $k))
                $pg = New-Object System.Drawing.Drawing2D.PathGradientBrush($path)
                $pg.CenterColor = [System.Drawing.Color]::FromArgb($gl[7], $gl[4], $gl[5], $gl[6])
                $pg.SurroundColors = [System.Drawing.Color[]]@([System.Drawing.Color]::FromArgb(0, $gl[4], $gl[5], $gl[6]))
                $g.FillPath($pg, $path); $pg.Dispose(); $path.Dispose()
            }
            $rnd = New-Object System.Random(2002)
            $n = [int]($w * $h / (2400.0 * $k * $k))
            $sb = New-Object System.Drawing.SolidBrush($cText)
            for ($i = 0; $i -lt $n; $i++) {
                $x = $rnd.NextDouble() * $w; $y = $rnd.NextDouble() * $h
                $big = ($rnd.NextDouble() -lt 0.1)
                $s = [single]([math]::Max(1.0, $k) * $(if ($big) { 2.0 } else { 1.1 }))
                $tone = $(if ($rnd.NextDouble() -lt 0.3) { @(255, 214, 170) } else { @(205, 222, 255) })
                $sb.Color = [System.Drawing.Color]::FromArgb($(if ($big) { 220 } else { 40 + $rnd.Next(130) }), $tone[0], $tone[1], $tone[2])
                $g.FillEllipse($sb, [single]$x, [single]$y, $s, $s)
                if ($big) {
                    $gp = New-Object System.Drawing.Drawing2D.GraphicsPath; $rr = [single](5 * $k)
                    $gp.AddEllipse([single]($x + $s / 2 - $rr), [single]($y + $s / 2 - $rr), [single](2 * $rr), [single](2 * $rr))
                    $pg = New-Object System.Drawing.Drawing2D.PathGradientBrush($gp)
                    $pg.CenterColor = [System.Drawing.Color]::FromArgb(70, $tone[0], $tone[1], $tone[2]); $pg.SurroundColors = [System.Drawing.Color[]]@([System.Drawing.Color]::FromArgb(0, $tone[0], $tone[1], $tone[2]))
                    $g.FillPath($pg, $gp); $pg.Dispose(); $gp.Dispose()
                }
            }
            $sb.Dispose()
            # the orange mark under the title, and a fading line after it
            $mk = New-Object System.Drawing.RectangleF([single](28 * $k), [single](55 * $k), [single](46 * $k), [single]([math]::Max(2.0, 3 * $k)))
            $br = New-Object System.Drawing.Drawing2D.LinearGradientBrush($mk, [System.Drawing.Color]::FromArgb(255, 196, 92), $cAccent, [single]0)
            $mp = New-RoundPath $mk.X $mk.Y $mk.Width $mk.Height ([single]($mk.Height / 2))
            $g.FillPath($br, $mp); $br.Dispose(); $mp.Dispose()
            $ln = New-Object System.Drawing.RectangleF([single](80 * $k), [single](56 * $k), [single](350 * $k), [single]([math]::Max(1.0, $k)))
            $br = New-Object System.Drawing.Drawing2D.LinearGradientBrush($ln, [System.Drawing.Color]::FromArgb(120, $cAccent), [System.Drawing.Color]::FromArgb(0, $cAccent), [single]0)
            $g.FillRectangle($br, $ln); $br.Dispose()
            # the logo, top right (its drawn part: 246 x 94 of the 256 x 256 picture)
            $logo = Join-Path $root 'rac-rs\assets\icon\rac-rs-256.png'
            if (Test-Path -LiteralPath $logo -PathType Leaf) {
                $img = [System.Drawing.Image]::FromFile($logo)
                try {
                    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                    $g.DrawImage($img, (New-Object System.Drawing.RectangleF([single](444 * $k), [single](10 * $k), [single](116 * $k), [single](44 * $k))), (New-Object System.Drawing.RectangleF(5, 81, 246, 94)), [System.Drawing.GraphicsUnit]::Pixel)
                } finally { $img.Dispose() }
            }
        } finally { $g.Dispose() }
        return $bmp
    }
    $BgImage = $null
    # as tall as the tallest view can get (the install view with its details: 660 units; a long note adds a little), not the whole screen
    try { $BgImage = New-ThemeBackground ([int][math]::Round($DesignW * $LayoutK)) ([math]::Min($MaxClientH, [int][math]::Round(760 * $LayoutK))) } catch { $BgImage = $null }

    function Paint-ThemeButton($b, $e) {
        # every button: a rounded plate. The main ones (orange, or the colour they are given) have a white upper-case label with a shadow.
        $g = $e.Graphics; $w = $b.ClientSize.Width; $h = $b.ClientSize.Height
        if ($w -lt 6 -or $h -lt 6) { return }
        $k = $LayoutK
        $par = $b.Parent
        if ($par -and $par.BackgroundImage) { $g.DrawImage($par.BackgroundImage, (New-Object System.Drawing.Rectangle(0, 0, $w, $h)), (New-Object System.Drawing.Rectangle($b.Left, $b.Top, $w, $h)), [System.Drawing.GraphicsUnit]::Pixel) }
        else { $g.Clear($cBg) }
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $main = (-not $b.UseVisualStyleBackColor)
        $hot = $b.Enabled -and $b.ClientRectangle.Contains($b.PointToClient([System.Windows.Forms.Control]::MousePosition))
        $down = $hot -and (([System.Windows.Forms.Control]::MouseButtons -band [System.Windows.Forms.MouseButtons]::Left) -ne 0)
        $base = $(if (-not $b.Enabled) { $cOff } elseif ($main) { $b.BackColor } else { $cBtn2 })
        $top = Get-Tint $base $(if ($main) { 0.22 } else { 0.08 }); $bot = Get-Tint $base $(if ($main) { -0.18 } else { -0.12 })
        if ($hot) { $top = Get-Tint $top 0.14; $bot = Get-Tint $bot 0.12 }
        if ($down) { $top = Get-Tint $base -0.12; $bot = Get-Tint $base 0.04 }
        $r = [single][math]::Min($h / 2.0 - 1, [math]::Max(3.0, 7 * $k))
        $path = New-RoundPath 0.5 0.5 ([single]($w - 1.5)) ([single]($h - 1.5)) $r
        $br = New-Object System.Drawing.Drawing2D.LinearGradientBrush((New-Object System.Drawing.Rectangle(0, 0, $w, $h)), $top, $bot, [single]90)
        $g.FillPath($br, $path); $br.Dispose()
        if ($b.Enabled -and -not $down) {
            $hl = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb($(if ($main) { 110 } else { 50 }), 255, 255, 255), [single][math]::Max(1.0, $k))
            $g.DrawLine($hl, [single]($r), [single](1.5 * [math]::Max(1.0, $k)), [single]($w - $r - 1), [single](1.5 * [math]::Max(1.0, $k))); $hl.Dispose()
        }
        $edge = $(if (-not $b.Enabled) { Get-Tint $base 0.12 } elseif ($main) { Get-Tint $base -0.5 } elseif ($hot -or $b.Focused) { $cBlue } else { Get-Tint $base 0.28 })
        $pen = New-Object System.Drawing.Pen($edge, [single][math]::Max(1.0, $k)); $g.DrawPath($pen, $path); $pen.Dispose()
        if ($main -and $b.Focused -and $b.Enabled) {
            $fp = New-RoundPath ([single](3 * $k)) ([single](3 * $k)) ([single]($w - 1 - 6 * $k)) ([single]($h - 1 - 6 * $k)) ([single][math]::Max(2.0, $r - 3 * $k))
            $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(120, 255, 255, 255), [single][math]::Max(1.0, $k)); $g.DrawPath($pen, $fp); $pen.Dispose(); $fp.Dispose()
        }
        $path.Dispose()
        $txt = $(if ($main) { $b.Text.ToUpperInvariant() } else { $b.Text })
        $fl = [System.Windows.Forms.TextFormatFlags]::HorizontalCenter -bor [System.Windows.Forms.TextFormatFlags]::VerticalCenter -bor [System.Windows.Forms.TextFormatFlags]::NoPrefix -bor [System.Windows.Forms.TextFormatFlags]::SingleLine
        $dy = $(if ($down) { [int][math]::Max(1, $k) } else { 0 })
        if ($main -and $b.Enabled) {
            $sh = [int][math]::Max(1, [math]::Round(1.5 * $k))
            [System.Windows.Forms.TextRenderer]::DrawText($g, $txt, $b.Font, (New-Object System.Drawing.Rectangle(0, ($sh + $dy), $w, $h)), (Get-Tint $base -0.62), $fl)
        }
        $fc = $(if (-not $b.Enabled) { [System.Drawing.Color]::FromArgb(128, 140, 168) } elseif ($main) { [System.Drawing.Color]::White } else { $cText })
        [System.Windows.Forms.TextRenderer]::DrawText($g, $txt, $b.Font, (New-Object System.Drawing.Rectangle(0, $dy, $w, $h)), $fc, $fl)
    }

    function New-Label([string]$text, [int]$x, [int]$y, [int]$w, [int]$h, $font, $color) {
        $l = New-Object System.Windows.Forms.Label
        $l.AutoSize = $false; $l.UseMnemonic = $false; $l.BackColor = [System.Drawing.Color]::Transparent
        $l.Location = New-Object System.Drawing.Point($x, $y); $l.Size = New-Object System.Drawing.Size($w, $h)
        $l.Text = $text; $l.Font = $font; $l.ForeColor = $color
        return $l
    }
    function New-Button([string]$text, [int]$x, [int]$y, [int]$w, [int]$h, [bool]$primary) {
        $b = New-Object System.Windows.Forms.Button
        $b.Location = New-Object System.Drawing.Point($x, $y); $b.Size = New-Object System.Drawing.Size($w, $h)
        $b.Text = $text; $b.Font = $fN; $b.UseVisualStyleBackColor = $true
        $b.FlatStyle = 'Flat'; $b.FlatAppearance.BorderSize = 0; $b.BackColor = $cBtn2; $b.ForeColor = $cText
        if ($primary) { $b.BackColor = $cAccent; $b.ForeColor = [System.Drawing.Color]::White; $b.UseVisualStyleBackColor = $false; $b.Font = $fBtn }
        else { $b.UseVisualStyleBackColor = $true }      # (setting BackColor turned it off; Paint-ThemeButton tells the two kinds apart by it)
        $b.Add_Paint({ param($s, $e) try { Paint-ThemeButton $s $e } catch { } })
        foreach ($ev in @('MouseEnter', 'MouseLeave', 'MouseDown', 'MouseUp', 'GotFocus', 'LostFocus', 'EnabledChanged')) { $b."add_$ev"({ param($s, $e) $s.Invalidate() }) }
        return $b
    }
    function New-Link([string]$text, [int]$x, [int]$y, [int]$w, [int]$h) {
        $l = New-Object System.Windows.Forms.LinkLabel
        $l.AutoSize = $false; $l.Location = New-Object System.Drawing.Point($x, $y); $l.Size = New-Object System.Drawing.Size($w, $h)
        $l.Text = $text; $l.Font = $fN; $l.LinkColor = $cBlue; $l.VisitedLinkColor = $cBlue; $l.ActiveLinkColor = $cAccent
        $l.LinkBehavior = 'HoverUnderline'; $l.BackColor = [System.Drawing.Color]::Transparent
        return $l
    }
    function New-Panel() {
        $p = New-Object System.Windows.Forms.Panel
        $p.Dock = 'Fill'; $p.BackColor = $cBg; $p.Visible = $false
        if ($BgImage) { $p.BackgroundImage = $BgImage; $p.BackgroundImageLayout = 'None' }
        $p.AutoScroll = $true      # only shows a scroll bar when the content is taller than the window (a very small screen)
        return $p
    }
    function New-Bar([int]$x, [int]$y, [int]$w, [int]$h) {
        # a plain bar (track + fill) that draws the same everywhere
        $t = New-Object System.Windows.Forms.Panel
        $t.Location = New-Object System.Drawing.Point($x, $y); $t.Size = New-Object System.Drawing.Size($w, $h)
        $t.BackColor = [System.Drawing.Color]::FromArgb(36, 48, 88); $t.Tag = 0
        $f = New-Object System.Windows.Forms.Panel
        $f.Dock = 'Left'; $f.Width = 0; $f.BackColor = $cAccent
        $t.Controls.Add($f)
        return $t
    }
    function Set-ThemeControls($parent) {
        # the controls made with New-Object (boxes, list, check and radio buttons): colours of the dark window; the titles leave room for the logo
        foreach ($c in $parent.Controls) {
            if ($c -is [System.Windows.Forms.TextBox] -or $c -is [System.Windows.Forms.ListBox]) { $c.BackColor = $cField; $c.ForeColor = $cText }
            elseif ($c -is [System.Windows.Forms.CheckBox] -or $c -is [System.Windows.Forms.RadioButton]) { $c.BackColor = [System.Drawing.Color]::Transparent; $c.ForeColor = $cText }
            elseif ($c -is [System.Windows.Forms.Label] -and -not ($c -is [System.Windows.Forms.LinkLabel]) -and [object]::ReferenceEquals($c.Font, $fTitle)) { $c.Width = 404; $UI.titles += $c }
            if ($c.Controls.Count -gt 0 -and -not ($c -is [System.Windows.Forms.Panel] -and $c.Dock -eq 'Left')) { Set-ThemeControls $c }
        }
    }
    $UI.titles = @()
    function Set-Bar($bar, [int]$pct) {
        $bar.Tag = $pct
        $bar.Controls[0].Width = [int][math]::Floor($bar.ClientSize.Width * $pct / 100.0)
    }
    function Format-Elapsed($ts) { return ('{0:00}:{1:00}' -f [int][math]::Floor($ts.TotalMinutes), $ts.Seconds) }

    # ---------------------------------------------------------------- the window
    $form = New-Object System.Windows.Forms.Form
    $UI.form = $form
    $form.Text = $AppTitle
    $form.AutoScaleMode = 'None'      # no automatic scaling: see "display scale" above (the old setting scaled the window size on top of a size that already had the scale in it)
    $form.Font = $fN
    $form.ClientSize = New-Object System.Drawing.Size(580, 440)
    $form.FormBorderStyle = 'FixedDialog'
    $form.MaximizeBox = $false
    $form.StartPosition = 'CenterScreen'
    $form.BackColor = $cBg; $form.ForeColor = $cText
    # [MP] a dark title bar to match (Windows 10 2004 and later; Windows 11 also takes the bar colour). Only this window's own look; when it
    # fails the bar stays as it was.
    $Native = $null
    try {
        if (-not ('RacMp.Native' -as [type])) {
            Add-Type -Namespace RacMp -Name Native -MemberDefinition '[DllImport("dwmapi.dll")] public static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int value, int size);'
        }
        $Native = [type]'RacMp.Native'
    } catch { $Native = $null }
    if ($Native) {
        $form.Add_HandleCreated({
            param($s, $e)
            try {
                foreach ($a in @(@(20, 1), @(35, 0x00220F0A), @(36, 0x00FAEFEA))) { $v = [int]$a[1]; [void][RacMp.Native]::DwmSetWindowAttribute($s.Handle, $a[0], [ref]$v, 4) }
            } catch { }
        })
    }
    $exePath = Join-Path $root 'build\release\rac-rs.exe'
    $icoFile = Get-IconFile
    if ($icoFile) { try { $form.Icon = New-Object System.Drawing.Icon($icoFile) } catch { $icoFile = '' } }
    if (-not $icoFile -and (Test-Path -LiteralPath $exePath)) { try { $form.Icon = [System.Drawing.Icon]::ExtractAssociatedIcon($exePath) } catch { } }

    # --- view: pick the disc image
    $pPick = New-Panel; $UI.pPick = $pPick
    $pPick.Controls.Add((New-Label 'Set up Ratchet & Clank' 28 22 520 36 $fTitle $cText))
    $UI.lblPickText = New-Label 'Pick the .iso image of your own Ratchet & Clank disc (PAL / Europe or USA, v1.00)' 28 68 520 54 $fN $cText      # 3 lines (the repair text) need 54
    $pPick.Controls.Add($UI.lblPickText)
    $UI.txtIso = New-Object System.Windows.Forms.TextBox
    $UI.txtIso.Location = New-Object System.Drawing.Point(28, 124); $UI.txtIso.Size = New-Object System.Drawing.Size(404, 26); $UI.txtIso.Font = $fN
    $pPick.Controls.Add($UI.txtIso)
    $UI.btnBrowse = New-Button 'Browse...' 442 122 110 30 $false
    $pPick.Controls.Add($UI.btnBrowse)
    $UI.pbCheck = New-Bar 28 164 524 8
    $UI.pbCheck.Visible = $false
    $pPick.Controls.Add($UI.pbCheck)
    $UI.lblDisc = New-Label '' 28 180 524 62 $fN $cGrey
    $pPick.Controls.Add($UI.lblDisc)
    $UI.lblPickNote = New-Label 'Your disc image is only read. Nothing is uploaded.' 28 252 524 22 $fSmall $cGrey
    $pPick.Controls.Add($UI.lblPickNote)
    $UI.chkShortcut = New-Object System.Windows.Forms.CheckBox
    $UI.chkShortcut.Text = 'Create desktop shortcut'; $UI.chkShortcut.Checked = $true; $UI.chkShortcut.Font = $fN
    $UI.chkShortcut.Location = New-Object System.Drawing.Point(28, 276); $UI.chkShortcut.Size = New-Object System.Drawing.Size(300, 26)
    $pPick.Controls.Add($UI.chkShortcut)
    # the launcher choice: ticked = "Continue" gets the RAC-RS Launcher and hands over to it; unticked = this window does the whole set-up. Remembered in generated\setup_choice.txt.
    $launcherChoice = Get-SetupChoice 'launcher'
    $UI.chkLauncher = New-Object System.Windows.Forms.CheckBox
    $UI.chkLauncher.Text = 'Also install the RAC-RS Launcher (recommended: mods, texture packs, settings, updates)'
    $UI.chkLauncher.Checked = (($launcherChoice -ne 'no') -and -not $NoLauncher); $UI.chkLauncher.Font = $fN
    $UI.chkLauncher.Location = New-Object System.Drawing.Point(28, 306); $UI.chkLauncher.Size = New-Object System.Drawing.Size(524, 46)
    $pPick.Controls.Add($UI.chkLauncher)
    $UI.lnkGetLauncher = New-Link 'Install the RAC-RS Launcher instead' 28 310 300 24
    $UI.lnkGetLauncher.Visible = $false
    $pPick.Controls.Add($UI.lnkGetLauncher)
    $UI.chkSteam = New-Object System.Windows.Forms.CheckBox
    $UI.chkSteam.Text = 'Add the game to Steam (Steam must be closed; for the Steam Deck and Big Picture)'; $UI.chkSteam.Checked = $false; $UI.chkSteam.Font = $fN
    $UI.chkSteam.Location = New-Object System.Drawing.Point(28, 354); $UI.chkSteam.Size = New-Object System.Drawing.Size(524, 46)
    $pPick.Controls.Add($UI.chkSteam)
    $pPick.Controls.Add((New-Label 'Needs about 5 GB of free disk space and an internet connection (once).' 28 424 400 40 $fSmall $cGrey))
    $UI.btnInstall = New-Button 'Install' 442 416 110 42 $true
    $pPick.Controls.Add($UI.btnInstall)

    # --- view: installing
    $pInst = New-Panel; $UI.pInst = $pInst
    $UI.lblInstTitle = New-Label 'Installing' 28 22 520 36 $fTitle $cText
    $pInst.Controls.Add($UI.lblInstTitle)
    $UI.stepGlyph = @(); $UI.stepText = @()
    for ($i = 0; $i -lt $StepNames.Count; $i++) {
        $g = New-Label '' 30 (70 + 30 * $i) 26 26 $fGlyph $cGrey
        $t = New-Label $StepNames[$i] 62 (72 + 30 * $i) 300 24 $fN $cGrey
        $pInst.Controls.Add($g); $pInst.Controls.Add($t)
        $UI.stepGlyph += $g; $UI.stepText += $t
    }
    $UI.pb = New-Bar 28 234 524 14
    $pInst.Controls.Add($UI.pb)
    $UI.lblStatus = New-Label '' 28 256 524 20 $fSmall $cGrey
    $pInst.Controls.Add($UI.lblStatus)
    $UI.lblElapsed = New-Label 'Elapsed: 00:00' 28 284 300 22 $fN $cText
    $pInst.Controls.Add($UI.lblElapsed)
    $UI.lblPct = New-Label '0%' 452 284 100 22 $fN $cText
    $UI.lblPct.TextAlign = 'TopRight'
    $pInst.Controls.Add($UI.lblPct)
    $UI.lblInstNote = New-Label 'This takes about 15 minutes. You can keep using your PC.' 28 312 524 22 $fN $cText
    $pInst.Controls.Add($UI.lblInstNote)
    $UI.lnkDetails = New-Link 'Show details' 28 346 200 22
    $pInst.Controls.Add($UI.lnkDetails)
    $UI.txtLog = New-Object System.Windows.Forms.TextBox
    $UI.txtLog.Multiline = $true; $UI.txtLog.ReadOnly = $true; $UI.txtLog.ScrollBars = 'Both'; $UI.txtLog.WordWrap = $false; $UI.txtLog.Font = $fMono
    $UI.txtLog.Location = New-Object System.Drawing.Point(28, 376); $UI.txtLog.Size = New-Object System.Drawing.Size(524, 200); $UI.txtLog.Visible = $false
    $pInst.Controls.Add($UI.txtLog)

    # --- view: done / already installed
    $pDone = New-Panel; $UI.pDone = $pDone
    $UI.lblDoneTitle = New-Label 'Ready to play' 28 22 524 40 $fTitle $cText
    $pDone.Controls.Add($UI.lblDoneTitle)
    $UI.lblDoneText = New-Label '' 28 66 524 44 $fN $cText
    $pDone.Controls.Add($UI.lblDoneText)
    $UI.btnPlay = New-Button 'Play' 140 130 300 84 $true
    $UI.btnPlay.Font = $fBig
    $pDone.Controls.Add($UI.btnPlay)
    $UI.btnTogether = New-Button 'Play together' 140 222 300 42 $true      # [MP] host or join a multiplayer game
    $UI.btnTogether.Font = New-Font $HeadFamily 13; $UI.btnTogether.BackColor = $cAccent2
    $pDone.Controls.Add($UI.btnTogether)
    $UI.btnFolder = New-Button 'Open game folder' 200 272 180 30 $false
    $pDone.Controls.Add($UI.btnFolder)
    $UI.btnLauncher = New-Button 'Install the launcher' 200 270 180 34 $false     # shown while the RAC-RS Launcher is not installed (changing your mind later)
    $pDone.Controls.Add($UI.btnLauncher)
    $UI.lnkRepair = New-Link 'Repair / rebuild' 200 312 180 22
    $UI.lnkRepair.TextAlign = 'MiddleCenter'
    $pDone.Controls.Add($UI.lnkRepair)
    $UI.lnkSteam = New-Link 'Add to Steam' 200 336 180 22      # "Remove from Steam" while our entry is in Steam; hidden when Steam is not found
    $UI.lnkSteam.TextAlign = 'MiddleCenter'
    $pDone.Controls.Add($UI.lnkSteam)
    $UI.lblDoneNote = New-Label '' 28 362 524 44 $fSmall $cGrey
    $pDone.Controls.Add($UI.lblDoneNote)
    $UI.lblVersion = New-Label '' 28 408 200 20 $fSmall $cGrey
    $pDone.Controls.Add($UI.lblVersion)
    $UI.lnkCheck = New-Link 'Check for updates' 392 406 160 22
    $UI.lnkCheck.Font = $fSmall; $UI.lnkCheck.TextAlign = 'MiddleRight'
    $pDone.Controls.Add($UI.lnkCheck)
    # a small link at the end of a finished install / update (the "done" view only, never on errors); it opens the project's Ko-fi page in the browser when clicked, nothing else
    $UI.lnkSupport = New-Link 'Support the project' 392 406 160 22
    $UI.lnkSupport.Font = $fSmall; $UI.lnkSupport.TextAlign = 'MiddleRight'; $UI.lnkSupport.Visible = $false
    $pDone.Controls.Add($UI.lnkSupport)

    # --- view: error
    $pErr = New-Panel; $UI.pErr = $pErr
    $pErr.Controls.Add((New-Label 'Something went wrong' 28 22 524 36 $fTitle $cRed))
    $UI.lblErrMsg = New-Label '' 28 64 524 84 $fN $cText
    $pErr.Controls.Add($UI.lblErrMsg)
    $UI.txtErrLog = New-Object System.Windows.Forms.TextBox
    $UI.txtErrLog.Multiline = $true; $UI.txtErrLog.ReadOnly = $true; $UI.txtErrLog.ScrollBars = 'Both'; $UI.txtErrLog.WordWrap = $false; $UI.txtErrLog.Font = $fMono
    $UI.txtErrLog.Location = New-Object System.Drawing.Point(28, 154); $UI.txtErrLog.Size = New-Object System.Drawing.Size(524, 200)
    $pErr.Controls.Add($UI.txtErrLog)
    $UI.btnCopy = New-Button 'Copy details' 28 372 130 34 $false
    $pErr.Controls.Add($UI.btnCopy)
    $UI.btnRetry = New-Button 'Try again' 322 372 110 34 $true
    $pErr.Controls.Add($UI.btnRetry)
    $UI.btnErrClose = New-Button 'Close' 442 372 110 34 $false
    $pErr.Controls.Add($UI.btnErrClose)

    # --- view: an update is available (shown by -Launch)
    $pOffer = New-Panel; $UI.pOffer = $pOffer
    $UI.lblOfferTitle = New-Label 'Update available' 28 22 524 36 $fTitle $cText
    $pOffer.Controls.Add($UI.lblOfferTitle)
    $pOffer.Controls.Add((New-Label 'What is new' 28 66 300 22 $fN $cGrey))
    $UI.txtChanges = New-Object System.Windows.Forms.TextBox
    $UI.txtChanges.Multiline = $true; $UI.txtChanges.ReadOnly = $true; $UI.txtChanges.ScrollBars = 'Vertical'; $UI.txtChanges.Font = $fN
    $UI.txtChanges.Location = New-Object System.Drawing.Point(28, 90); $UI.txtChanges.Size = New-Object System.Drawing.Size(524, 130)
    $pOffer.Controls.Add($UI.txtChanges)
    $UI.lblOfferNote = New-Label '' 28 232 524 44 $fN $cText
    $pOffer.Controls.Add($UI.lblOfferNote)
    $UI.btnUpdNow = New-Button 'Update now' 312 292 140 40 $true
    $pOffer.Controls.Add($UI.btnUpdNow)
    $UI.btnNotNow = New-Button 'Not now' 462 292 90 40 $false
    $pOffer.Controls.Add($UI.btnNotNow)

    # --- view: play together ([MP] this fork's multiplayer: host or join; remembered in generated\multiplayer.cfg)
    $pMp = New-Panel; $UI.pMp = $pMp
    $pMp.Controls.Add((New-Label 'Play together' 28 22 524 36 $fTitle $cText))
    $UI.lblMpText = New-Label 'Each player needs this game set up on their own PC, from their own disc, at the same version. To play over the internet, every PC needs Tailscale (free), signed in to the same Tailscale network. On one home network it is not needed.' 28 60 524 58 $fN $cText
    $pMp.Controls.Add($UI.lblMpText)
    $UI.lnkTailscale = New-Link 'Get Tailscale (tailscale.com)' 28 118 260 22
    $pMp.Controls.Add($UI.lnkTailscale)
    $pMp.Controls.Add((New-Label 'Your name' 28 153 100 22 $fN $cText))
    $UI.txtMpName = New-Object System.Windows.Forms.TextBox
    $UI.txtMpName.Location = New-Object System.Drawing.Point(132, 150); $UI.txtMpName.Size = New-Object System.Drawing.Size(220, 26); $UI.txtMpName.Font = $fN; $UI.txtMpName.MaxLength = 24
    $pMp.Controls.Add($UI.txtMpName)
    $UI.rbHost = New-Object System.Windows.Forms.RadioButton
    $UI.rbHost.Text = 'Host a game: your friends join you'; $UI.rbHost.Font = $fN
    $UI.rbHost.Location = New-Object System.Drawing.Point(28, 184); $UI.rbHost.Size = New-Object System.Drawing.Size(524, 26)
    $pMp.Controls.Add($UI.rbHost)
    $UI.lblMpAddr = New-Label '' 48 212 404 40 $fN $cText
    $pMp.Controls.Add($UI.lblMpAddr)
    $UI.lnkMpCopy = New-Link 'Copy' 462 212 90 22
    $UI.lnkMpCopy.TextAlign = 'TopRight'; $UI.lnkMpCopy.Visible = $false
    $pMp.Controls.Add($UI.lnkMpCopy)
    $UI.lblMpFw = New-Label '' 48 254 504 34 $fSmall $cGrey
    $pMp.Controls.Add($UI.lblMpFw)
    $UI.lnkMpFw = New-Link 'Open the firewall settings' 48 288 300 22
    $UI.lnkMpFw.Visible = $false
    $pMp.Controls.Add($UI.lnkMpFw)
    $UI.rbJoin = New-Object System.Windows.Forms.RadioButton
    $UI.rbJoin.Text = "Join a friend's game"; $UI.rbJoin.Font = $fN
    $UI.rbJoin.Location = New-Object System.Drawing.Point(28, 316); $UI.rbJoin.Size = New-Object System.Drawing.Size(524, 26)
    $pMp.Controls.Add($UI.rbJoin)
    $UI.lblMpFound = New-Label 'Looking for games on Tailscale and your home network ...' 48 344 404 36 $fSmall $cGrey
    $pMp.Controls.Add($UI.lblMpFound)
    $UI.lnkMpFind = New-Link 'Look again' 462 344 90 20
    $UI.lnkMpFind.TextAlign = 'TopRight'
    $pMp.Controls.Add($UI.lnkMpFind)
    $UI.lstMpFound = New-Object System.Windows.Forms.ListBox      # the games found (`Show-MpFound`): a click picks one, a double-click joins it
    $UI.lstMpFound.Location = New-Object System.Drawing.Point(48, 382); $UI.lstMpFound.Size = New-Object System.Drawing.Size(504, 68)
    $UI.lstMpFound.Font = $fN; $UI.lstMpFound.IntegralHeight = $false; $UI.lstMpFound.HorizontalScrollbar = $true
    $pMp.Controls.Add($UI.lstMpFound)
    $pMp.Controls.Add((New-Label 'Their address' 48 463 110 22 $fN $cText))
    $UI.txtMpJoin = New-Object System.Windows.Forms.TextBox
    $UI.txtMpJoin.Location = New-Object System.Drawing.Point(160, 460); $UI.txtMpJoin.Size = New-Object System.Drawing.Size(392, 26); $UI.txtMpJoin.Font = $fN
    $pMp.Controls.Add($UI.txtMpJoin)
    $UI.lblMpJoinHint = New-Label 'Filled in when you click a game above, or type what their Play together window shows.' 48 490 504 20 $fSmall $cGrey
    $pMp.Controls.Add($UI.lblMpJoinHint)
    $UI.lnkMpBack = New-Link 'Back' 28 540 100 22
    $pMp.Controls.Add($UI.lnkMpBack)
    $UI.lnkMpUpdate = New-Link '' 140 540 262 22      # [MP] "Update to vX" once the window found a newer release (`Tick-UpdateLook`)
    $UI.lnkMpUpdate.Visible = $false
    $pMp.Controls.Add($UI.lnkMpUpdate)
    $UI.btnMpStart = New-Button 'Host' 412 524 140 42 $true
    $UI.btnMpStart.Font = New-Font $HeadFamily 12
    $pMp.Controls.Add($UI.btnMpStart)

    foreach ($p in @($pPick, $pInst, $pDone, $pErr, $pOffer, $pMp)) { $form.Controls.Add($p) }
    Set-ThemeControls $form

    # ---------------------------------------------------------------- views
    function Get-Scale { return $LayoutK }
    function Scale-Layout($parent) {
        # one pass, before the first view: design units -> pixels for every control that has a fixed place (docked ones follow their panel)
        foreach ($c in $parent.Controls) {
            if ($c.Dock -eq [System.Windows.Forms.DockStyle]::None) {
                # edges are rounded, not sizes, so two neighbours that touch in design units still touch (no 1 px overlaps)
                $x1 = [int][math]::Round($c.Left * $LayoutK); $y1 = [int][math]::Round($c.Top * $LayoutK)
                $c.SetBounds($x1, $y1, ([int][math]::Round(($c.Left + $c.Width) * $LayoutK) - $x1), ([int][math]::Round(($c.Top + $c.Height) * $LayoutK) - $y1))
            }
            if ($c.Controls.Count -gt 0) { Scale-Layout $c }
        }
    }
    function Fit-FormToScreen {
        # keep the whole window on the working area (the height changes from view to view; at big scales it must not slide off the bottom)
        $b = $form.Bounds
        $x = [math]::Min([math]::Max($b.X, $wa.Left), [math]::Max($wa.Left, $wa.Right - $b.Width))
        $y = [math]::Min([math]::Max($b.Y, $wa.Top), [math]::Max($wa.Top, $wa.Bottom - $b.Height))
        if ($x -ne $b.X -or $y -ne $b.Y) { $form.Location = New-Object System.Drawing.Point($x, $y) }
    }
    function Fit-LabelFont($lbl) {
        # a label whose text needs more lines than its box has (a scale where the text wraps differently): the font gets smaller in 0.5 pt steps (at most 6) until it fits
        if (-not $lbl.Text -or $lbl.ClientSize.Width -lt 20) { return }
        $fl = [System.Windows.Forms.TextFormatFlags]::WordBreak -bor [System.Windows.Forms.TextFormatFlags]::NoPrefix
        if ($lbl.Tag -isnot [System.Drawing.Font]) { $lbl.Tag = $lbl.Font }      # the normal font, kept in Tag
        $base = $lbl.Tag
        $f = $base
        for ($i = 0; $i -lt 6; $i++) {
            $m = [System.Windows.Forms.TextRenderer]::MeasureText($lbl.Text, $f, (New-Object System.Drawing.Size($lbl.ClientSize.Width, 100000)), $fl)
            if ($m.Height -le $lbl.ClientSize.Height) { break }
            $f = New-Object System.Drawing.Font($base.FontFamily, [single]([math]::Max(5.0, $f.Size - 0.5)), $base.Style)
        }
        if ($lbl.Font.Size -ne $f.Size) { $lbl.Font = $f }
    }
    function Fit-TextLabels { foreach ($l in @($UI.lblPickText, $UI.lblDisc, $UI.lblDoneText, $UI.lblOfferNote, $UI.lblMpText, $UI.lblMpAddr, $UI.lblMpFw, $UI.lblMpFound) + $UI.titles) { Fit-LabelFont $l } }
    function Set-FormHeight([int]$h96) {
        # the client size in pixels: width and height in design units times the scale; never taller than the screen has room for
        $h = [math]::Min([int][math]::Round($h96 * $LayoutK), $MaxClientH)
        $form.ClientSize = New-Object System.Drawing.Size([int][math]::Round($DesignW * $LayoutK), $h)
        if ($form.Visible) { Fit-FormToScreen }
    }
    Scale-Layout $form

    function Test-LauncherMode {
        # first view with "Also install the RAC-RS Launcher" ticked: the button says Continue and the disc is picked in the launcher
        return ($S.view -eq 'pick' -and $S.boxLauncher -and $UI.chkLauncher.Checked -and -not $S.repair)
    }

    function Update-PickMode {
        $lm = Test-LauncherMode
        $UI.txtIso.Enabled = (-not $lm); $UI.btnBrowse.Enabled = (-not $lm); $UI.lblDisc.Visible = (-not $lm)
        if ($lm) {
            $UI.lblPickText.Text = 'Continue gets the RAC-RS Launcher (a small download). It asks for your disc image (PAL / Europe or USA, v1.00) and does the whole set-up.'
            $UI.btnInstall.Text = 'Continue'; $UI.pbCheck.Visible = $false
        } elseif ($S.repair) {
            $UI.lblPickText.Text = 'Repair / rebuild: this makes the game code again from your disc and rebuilds the game (about 15 minutes). Pick the .iso image of your Ratchet & Clank disc (PAL / Europe or USA, v1.00).'
            $UI.btnInstall.Text = 'Repair'; $UI.pbCheck.Visible = [bool]$S.check
        } else {
            $UI.lblPickText.Text = 'Pick the .iso image of your own Ratchet & Clank disc (PAL / Europe or USA, v1.00)'
            $UI.btnInstall.Text = 'Install'; $UI.pbCheck.Visible = [bool]$S.check
        }
        Set-InstallEnabled $(if ($lm) { $true } else { $S.discOk })
        Fit-TextLabels
    }

    function Set-PickChoices {
        # the launcher and Steam boxes of the first view. Repair: neither. A remembered "no" (or no way to ask): no box, only the link to change your mind.
        $hasLauncher = Test-Path -LiteralPath (Join-Path $root 'RAC-RS Launcher.exe') -PathType Leaf
        $ask = ($OfferLauncher -and (-not $S.repair) -and (-not $hasLauncher))
        $no = ((Get-SetupChoice 'launcher') -eq 'no')
        $S.boxLauncher = ($ask -and -not $no); $S.boxSteam = ((-not $S.repair) -and (Get-SteamState).Found)      # the logic reads these, not .Visible (which is false while the window is not shown)
        $UI.chkLauncher.Visible = $S.boxLauncher
        $UI.lnkGetLauncher.Visible = ($ask -and $no)
        $UI.chkSteam.Visible = $S.boxSteam
        Update-PickMode
    }

    function Set-DoneExtras {
        # the done / installed view: "Install the launcher" while it is not installed, "Add to Steam" / "Remove from Steam" when Steam is found
        $st = Get-SteamState
        $S.steamHas = $st.Has
        $UI.lnkSteam.Visible = $st.Found
        $UI.lnkSteam.Text = $(if ($st.Has) { 'Remove from Steam' } else { 'Add to Steam' })
        $S.launcherBtn = ($OfferLauncher -and -not (Test-Path -LiteralPath (Join-Path $root 'RAC-RS Launcher.exe') -PathType Leaf))
        $UI.btnLauncher.Visible = $S.launcherBtn
    }

    function Update-DoneLayout {
        # done / installed view: the links and the note move up while the "Install the launcher" button is hidden; the note gets the height its text needs
        # (the window grows when that is more than the room). All positions are 96-DPI units times the window scale.
        $k = Get-Scale
        $shift = 0      # [MP] "Play together" and the folder button take the launcher button's room
        $UI.lnkRepair.Top = [int][math]::Round((312 - $shift) * $k)
        $UI.lnkSteam.Top = [int][math]::Round((336 - $shift) * $k)
        $UI.lblDoneNote.Top = [int][math]::Round((362 - $shift) * $k)
        $need = 22
        if ($UI.lblDoneNote.Text) {
            $fl = [System.Windows.Forms.TextFormatFlags]::WordBreak -bor [System.Windows.Forms.TextFormatFlags]::NoPrefix
            $m = [System.Windows.Forms.TextRenderer]::MeasureText($UI.lblDoneNote.Text, $UI.lblDoneNote.Font, (New-Object System.Drawing.Size($UI.lblDoneNote.ClientSize.Width, 100000)), $fl)
            $need = [int][math]::Ceiling($m.Height / $k) + 6
        }
        $extra = [math]::Max(0, $need - (404 - (362 - $shift)))
        $UI.lblDoneNote.Height = [int][math]::Round($need * $k)
        $UI.lblVersion.Top = [int][math]::Round((408 + $extra) * $k)
        $UI.lnkCheck.Top = [int][math]::Round((406 + $extra) * $k)
        $UI.lnkSupport.Top = $UI.lnkCheck.Top
        Set-FormHeight (440 + $extra)
    }

    function Update-ErrLayout {
        # error view: the message gets the height its text needs, the log box shrinks to make room (down to 110 units), then the window grows
        $k = Get-Scale
        $fl = [System.Windows.Forms.TextFormatFlags]::WordBreak -bor [System.Windows.Forms.TextFormatFlags]::NoPrefix
        $m = [System.Windows.Forms.TextRenderer]::MeasureText($UI.lblErrMsg.Text, $UI.lblErrMsg.Font, (New-Object System.Drawing.Size($UI.lblErrMsg.ClientSize.Width, 100000)), $fl)
        $need = [math]::Max(84, [int][math]::Ceiling($m.Height / $k) + 6)
        $extra = [math]::Max(0, $need - 84 - 90)
        $logTop = 64 + $need + 6
        $btnTop = 372 + $extra
        $UI.lblErrMsg.Height = [int][math]::Round($need * $k)
        $UI.txtErrLog.Top = [int][math]::Round($logTop * $k)
        $UI.txtErrLog.Height = [int][math]::Round(($btnTop - 18 - $logTop) * $k)
        foreach ($b in @($UI.btnCopy, $UI.btnRetry, $UI.btnErrClose)) { $b.Top = [int][math]::Round($btnTop * $k) }
        Set-FormHeight (440 + $extra)
    }

    function Show-View([string]$name) {
        $S.view = $name
        foreach ($p in @($pPick, $pInst, $pDone, $pErr, $pOffer, $pMp)) { $p.Visible = $false }
        Set-FormHeight $(if ($name -eq 'offer') { 350 } elseif ($name -eq 'pick') { 480 } elseif ($name -eq 'mp') { 580 } else { 440 })
        if ($name -ne 'mp') { Stop-MpFind }
        switch ($name) {
            'offer' { $pOffer.Visible = $true; $form.AcceptButton = $UI.btnUpdNow; $form.ActiveControl = $UI.btnUpdNow }      # [MP] not the list of changes (it would show all its text selected)
            'pick' { $pPick.Visible = $true; $form.AcceptButton = $UI.btnInstall; Set-PickChoices }
            'install' { $pInst.Visible = $true; $form.AcceptButton = $null; $UI.lnkDetails.Text = 'Show details'; $UI.txtLog.Visible = $false }
            'done'    { $pDone.Visible = $true; $form.AcceptButton = $UI.btnPlay }
            'installed' { $pDone.Visible = $true; $form.AcceptButton = $UI.btnPlay }
            'error'   { $pErr.Visible = $true; $form.AcceptButton = $UI.btnRetry }
            'mp'      { $pMp.Visible = $true; $form.AcceptButton = $UI.btnMpStart; Set-MpView }
        }
        if ($name -eq 'error') { Update-ErrLayout }
        if ($name -eq 'done' -or $name -eq 'installed') {
            $isDone = ($name -eq 'done')
            $UI.lblDoneTitle.Text = $(if ($isDone) { 'Ready to play' } else { 'Ratchet & Clank is installed' })
            $UI.lblDoneText.Text = $(if ($isDone) { 'Everything is set up. Click Play to start the game, or Play together to play with friends.' } else { 'Click Play to start the game, or Play together to play with friends.' })
            $UI.lnkRepair.Visible = (-not $isDone)
            $note = ''
            if ($isDone -and $S.shortcut) {
                $desk = [Environment]::GetFolderPath('Desktop')
                if ($S.shortcutPath -and ((Split-Path -Parent $S.shortcutPath) -ne $desk)) { $note = "A shortcut was created here: $($S.shortcutPath)" }
                else { $note = 'The shortcuts "' + $GameName + '" and "Ratchet & Clank - Play together" are on your desktop. They start this copy of the game (older shortcuts with the same names were replaced).' }
            }
            $iso = Get-InstalledIso
            if ($iso -and -not (Test-Path -LiteralPath $iso -PathType Leaf)) { $note = 'Your disc image was not found where it was during setup. Use Repair / rebuild to pick it again.' }
            if ($S.steamNote) { $note = ($note + "`r`n" + $S.steamNote).Trim() }
            $updNote = $false
            if ($S.updAvail -and -not $isDone -and -not ($note -like 'Your disc*')) {
                $note = "Version $($S.updAvail) is out (you have $(Get-LocalVersion)). Click `"Update to v$($S.updAvail)`" below: it takes a few minutes and keeps your saves. Friends play together on the same version."
                $updNote = $true
            }
            Set-DoneExtras
            $UI.lblDoneNote.Text = $note
            $UI.lblDoneNote.ForeColor = $(if ($note -like 'Your disc*') { $cRed } elseif ($updNote) { $cAccent } else { $cGrey })
            $S.finishPending = $false; $UI.btnPlay.Text = 'Play'
            if ($isDone -and $S.update) {
                $UI.lblDoneTitle.Text = 'Updated to v' + $S.newV; $UI.lblDoneText.Text = 'Click Play to start the game, or Play together to play with friends.'
                $UI.lblDoneNote.Text = 'Your saves and settings were kept.'; $UI.lblDoneNote.ForeColor = $cGrey
            }
            if (-not $isDone) {
                $bv = Get-BuiltVersion; $lv = Get-LocalVersion
                if ($bv -and ($bv -ne $lv)) {
                    $UI.lblDoneTitle.Text = 'Finish the update'
                    $UI.lblDoneText.Text = "Version $lv is here, but the game is still version $bv. Click Update to finish (about 2 to 10 minutes; your disc image is used as before)."
                    $UI.btnPlay.Text = 'Update'; $S.finishPending = $true; $S.newV = $lv
                }
            }
            $UI.lblVersion.Text = 'Version ' + (Get-LocalVersion)
            $UI.btnTogether.Visible = (-not $S.finishPending)
            Update-DoneLayout
        }
        $UI.lblVersion.Visible = ($name -eq 'done' -or $name -eq 'installed'); $UI.lnkCheck.Visible = ($name -eq 'installed'); $UI.lnkSupport.Visible = ($name -eq 'done')
        $UI.lnkCheck.Text = $(if ($S.updAvail) { "Update to v$($S.updAvail)" } else { 'Check for updates' })
        $UI.lnkMpUpdate.Visible = [bool]$S.updAvail; $UI.lnkMpUpdate.Text = $(if ($S.updAvail) { "Update to v$($S.updAvail)" } else { '' })
        Fit-TextLabels
        $form.PerformLayout()
    }

    # ---------------------------------------------------------------- disc check (background runspace: size, then SHA-1 with progress)
    $DiscScript = {
        param($path, $discs, $sync)
        try {
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { $sync.msg = 'That file could not be found.'; $sync.state = 'bad'; return }
            $len = (Get-Item -LiteralPath $path).Length
            $disc = $discs | Where-Object { $_.Size -eq $len } | Select-Object -First 1
            if (-not $disc) {
                $sz = $(if ($len -ge 1GB) { ('{0:N1} GB' -f ($len / 1GB)) } elseif ($len -ge 1MB) { ('{0:N0} MB' -f ($len / 1MB)) } else { ('{0:N0} KB' -f [math]::Ceiling($len / 1KB)) })
                $sync.msg = "This is not the right disc image. It must be Ratchet & Clank PAL / Europe v1.00 or USA v1.00, one .iso file of about 3.9 GB. This file is $sz. Other regions, other versions and Platinum / Greatest Hits re-releases do not work."
                $sync.state = 'bad'; return
            }
            $sync.total = $len
            $fs = [System.IO.File]::Open($path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
            try {
                $h = [System.Security.Cryptography.SHA1]::Create()
                $buf = New-Object byte[] (4MB)
                while ($true) {
                    if ($sync.cancel) { $sync.state = 'cancel'; return }
                    $n = $fs.Read($buf, 0, $buf.Length)
                    if ($n -le 0) { break }
                    $null = $h.TransformBlock($buf, 0, $n, $null, 0)
                    $sync.pos = $sync.pos + $n
                }
                $null = $h.TransformFinalBlock($buf, 0, 0)
                $hex = ([System.BitConverter]::ToString($h.Hash)).Replace('-', '')
            } finally { $fs.Dispose() }
            if ($hex -ne $disc.Sha1) {
                $sync.msg = "This file has the right size but it is not the $($disc.Name) disc (its checksum is different). Try making the .iso again from your disc, or use another copy."
                $sync.state = 'bad'; return
            }
            $sync.name = $disc.Name
            $sync.state = 'ok'
        } catch {
            $sync.msg = 'The file could not be read: ' + $_.Exception.Message
            $sync.state = 'bad'
        }
    }

    function Set-InstallEnabled([bool]$on) {
        if ($S.fetch) { $on = $false } elseif (Test-LauncherMode) { $on = $true }
        $UI.btnInstall.Enabled = $on
        $UI.btnInstall.BackColor = $(if ($on) { $cAccent } else { [System.Drawing.Color]::FromArgb(200, 200, 200) })
        $UI.btnInstall.ForeColor = $(if ($on) { [System.Drawing.Color]::White } else { [System.Drawing.Color]::FromArgb(245, 245, 245) })
    }
    function Set-DiscStatus([string]$kind, [string]$msg) {
        switch ($kind) {
            'none' { $UI.lblDisc.Text = ''; $UI.lblDisc.ForeColor = $cGrey }
            'run'  { $UI.lblDisc.Text = 'Checking the disc image ...'; $UI.lblDisc.ForeColor = $cGrey }
            'ok'   { $UI.lblDisc.Text = "Disc OK: Ratchet & Clank, $(if ($msg) { $msg } else { 'v1.00' })."; $UI.lblDisc.ForeColor = $cGreen }
            'bad'  { $UI.lblDisc.Text = $msg; $UI.lblDisc.ForeColor = $cRed }
        }
        $UI.pbCheck.Visible = ($kind -eq 'run')
        Fit-LabelFont $UI.lblDisc
    }

    function Stop-DiscCheck {
        if ($S.check) {
            $S.check.Sync.cancel = $true
            try { $null = $S.check.Ps.BeginStop($null, $null) } catch { }
            $S.check = $null
        }
    }

    function Start-DiscCheck([string]$path) {
        Stop-DiscCheck
        $path = $path.Trim().Trim('"')
        $S.discOk = $false; $S.discPath = ''
        Set-InstallEnabled $false
        if (-not $path) { Set-DiscStatus 'none' ''; return }
        $sync = [hashtable]::Synchronized(@{ pos = [long]0; total = [long]0; state = 'run'; msg = ''; name = ''; cancel = $false })
        $ps = [powershell]::Create()
        $null = $ps.AddScript($DiscScript).AddArgument($path).AddArgument($Discs).AddArgument($sync)
        $S.check = [pscustomobject]@{ Ps = $ps; Sync = $sync; Handle = $ps.BeginInvoke(); Path = $path }
        Set-Bar $UI.pbCheck 0
        Set-DiscStatus 'run' ''
    }

    function Tick-Disc {
        $c = $S.check
        if (-not $c) { return }
        $st = $c.Sync.state
        if ($st -eq 'run') {
            if ($c.Sync.total -gt 0) {
                $pct = [int][math]::Min(100, [math]::Floor(100.0 * $c.Sync.pos / $c.Sync.total))
                Set-Bar $UI.pbCheck $pct
                $UI.lblDisc.Text = "Checking the disc image ... $pct%"
            }
            return
        }
        if (-not $c.Handle.IsCompleted) { return }
        $S.check = $null
        try { $c.Ps.Dispose() } catch { }
        if ($st -eq 'ok') {
            $S.discOk = $true; $S.discPath = $c.Path
            Set-DiscStatus 'ok' $c.Sync.name
            Set-InstallEnabled $true
        } elseif ($st -eq 'bad') {
            Set-DiscStatus 'bad' $c.Sync.msg
        }
    }

    function Set-IsoPath([string]$path) {
        $S.suppress = $true
        $UI.txtIso.Text = $path
        $S.suppress = $false
        $UI.debounce.Stop()
        Start-DiscCheck $path
    }

    # ---------------------------------------------------------------- install (the console steps run in a hidden child, output read from files)
    function Reset-InstallState {
        $S.log = New-Object System.Text.StringBuilder
        $S.step = 0; $S.levels = 0; $S.compiled = 0; $S.progress = 0.0; $S.lastError = ''; $S.status = ''
        $S.shortcut = $false; $S.shortcutPath = ''; $S.ranOk = $false; $S.finished = $false
        $S.outPos = [long]0; $S.errPos = [long]0; $S.outRest = ''; $S.errRest = ''; $S.expect = 0; $S.buildT0 = $null
        $UI.txtLog.Clear(); $UI.txtErrLog.Clear()
        Set-Bar $UI.pb 0; $UI.lblPct.Text = '0%'; $UI.lblStatus.Text = ''; $UI.lblElapsed.Text = 'Elapsed: 00:00'
        Set-Step 0
    }

    function Set-Step([int]$idx, [bool]$failed = $false) {
        $S.step = $idx
        for ($i = 0; $i -lt $StepNames.Count; $i++) {
            $g = $UI.stepGlyph[$i]; $t = $UI.stepText[$i]
            if ($i -lt $idx) { $g.Text = [string][char]0x2713; $g.ForeColor = $cGreen; $t.ForeColor = $cText }
            elseif ($i -eq $idx) {
                if ($failed) { $g.Text = [string][char]0x2717; $g.ForeColor = $cRed; $t.ForeColor = $cRed }
                elseif ($S.finished) { $g.Text = [string][char]0x2713; $g.ForeColor = $cGreen; $t.ForeColor = $cText }
                else { $g.Text = [string][char]0x25B6; $g.ForeColor = $cBlue; $t.ForeColor = $cText }
            }
            else { $g.Text = [string][char]0x25CB; $g.ForeColor = $cGrey; $t.ForeColor = $cGrey }
        }
    }

    function Update-Progress {
        $v = 0.0
        switch ($S.step) {
            0 { $v = 1.0 + 7.0 * $S.toolsSub }
            1 { $v = 9.0 }
            2 { $v = 10.0 + 18.0 * [math]::Min(1.0, $S.levels / 19.0); if ($S.vu) { $v = 29.0 } }
            3 {
                $exp = $(if ($S.expect -gt 0) { $S.expect } else { $ExpectedCompiles })
                $done = $(if ($S.update) { [math]::Max(0, $S.compiled - 1) } else { $S.compiled })      # cargo prints "Compiling" when a crate STARTS
                $v = 30.0 + 68.0 * [math]::Min(1.0, $done / [double]$exp)
                if ($S.update -and $S.buildT0) { $v = [math]::Max($v, 30.0 + 60.0 * (1.0 - [math]::Exp(-((Get-Date) - $S.buildT0).TotalSeconds / 150.0))) }
            }
            4 { $v = 99.0 }
        }
        if ($S.finished) { $v = 100.0 }
        if ($v -gt $S.progress) { $S.progress = $v }
        $p = [int][math]::Min(100, [math]::Floor($S.progress))
        Set-Bar $UI.pb $p
        $UI.lblPct.Text = "$p%"
    }

    function Feed-Line([string]$line) {
        [void]$S.log.AppendLine($line)
        $UI.txtLog.AppendText($line + "`r`n")
        $t = $line.Trim()
        if ($t.Length -gt 0 -and $t -notlike '==*') { $S.status = $(if ($t.Length -gt 100) { $t.Substring(0, 100) } else { $t }) }
        if ($line -match '^== \((u|a|b|c|c2|d|e)\)') {
            $map = @{ 'u' = 0; 'a' = $(if ($S.update) { 1 } else { 0 }); 'b' = 1; 'c' = 2; 'c2' = 2; 'd' = 3; 'e' = 4 }
            $k = $Matches[1]
            if ($k -eq 'c2') { $S.vu = $true }
            $S.status = @{ 'u' = 'Downloading and applying the update ...'; 'a' = 'Getting the tools ready ...'; 'b' = 'Checking the disc image ...'; 'c' = 'Making the game code from your disc ...'; 'c2' = 'Making the graphics code from your disc ...'; 'd' = 'Building the game (the long part) ...'; 'e' = 'Finishing ...' }[$k]
            if ($map[$k] -ne $S.step) { Set-Step $map[$k] }
            if ($k -eq 'd' -and -not $S.buildT0) { $S.buildT0 = Get-Date }
        }
        elseif ($line -match '^expect-compiles: (\d+)') { $S.expect = [int]$Matches[1] }
        elseif ($line -match '^== done') { $S.finished = $true; Set-Step 4 }
        elseif ($line -match '^\s*level \d+ done') { $S.levels++ }
        elseif ($line -match '^\s+Compiling ') { $S.compiled++ }
        elseif ($line -match 'downloading Rust') { $S.toolsSub = 0.15 }
        elseif ($line -match 'downloading Python') { $S.toolsSub = 0.6 }
        elseif ($line -match 'downloading the GNU assembler') { $S.toolsSub = 0.8 }
        elseif ($line -match 'desktop shortcut created: (.+)$') { $S.shortcut = $true; $S.shortcutPath = $Matches[1].Trim() }
        if ($line -match '^ERROR: (.*)$') { $S.lastError = $Matches[1] }
        elseif ($line -match '^error(\[E\d+\])?: (.*)$' -and -not $S.lastError) { $S.lastError = $Matches[2] }
    }

    function Read-NewText([string]$which) {
        $path = $S["${which}Path"]
        if (-not $path -or -not (Test-Path -LiteralPath $path)) { return }
        $txt = ''
        $fs = $null
        try {
            $fs = New-Object System.IO.FileStream($path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
            $pos = $S["${which}Pos"]
            if ($fs.Length -le $pos) { return }
            [void]$fs.Seek($pos, [System.IO.SeekOrigin]::Begin)
            $buf = New-Object byte[] ([int]($fs.Length - $pos))
            $r = $fs.Read($buf, 0, $buf.Length)
            $S["${which}Pos"] = $pos + $r
            $txt = [System.Text.Encoding]::UTF8.GetString($buf, 0, $r)
        } finally { if ($fs) { $fs.Dispose() } }
        $parts = ($S["${which}Rest"] + $txt) -split '[\r\n]+'
        $S["${which}Rest"] = $parts[$parts.Count - 1]
        for ($i = 0; $i -lt $parts.Count - 1; $i++) { if ($parts[$i].Length -gt 0) { Feed-Line $parts[$i] } }
    }

    function Set-Mode([bool]$upd) {
        $S.update = $upd
        if (-not $upd) { $S.workerExtra = '' }
        $script:StepNames = $(if ($upd) { $StepNamesUpdate } else { $StepNamesInstall })
        for ($i = 0; $i -lt $StepNames.Count; $i++) { $UI.stepText[$i].Text = $StepNames[$i] }
        $UI.lblInstTitle.Text = $(if ($upd) { 'Updating to v' + $S.newV } else { 'Installing' })
        $UI.lblInstNote.Text = $(if ($upd) { 'This takes about 2 to 10 minutes. You can keep using your PC.' } else { 'This takes about 15 minutes. You can keep using your PC.' })
        $UI.btnErrClose.Text = $(if ($upd) { 'Play old version' } else { 'Close' })
    }

    function Start-UpdateRun {
        # the update runs as a hidden worker (setup.ps1 -Console -ApplyUpdate ...); this window shows its steps
        Set-Mode $true
        $iso = Get-InstalledIso
        $msg = ''
        if (@(Get-GameProcs).Count -gt 0) { $msg = "The game is still running.`r`n`r`nClose it, then click Try again." }
        elseif (-not $iso -or -not (Test-Path -LiteralPath $iso -PathType Leaf)) { $msg = "Your disc image was not found at:`r`n$iso`r`n`r`nPut it back where it was, or open Setup.cmd and use Repair / rebuild to pick it again." }
        if ($msg) {
            Reset-InstallState
            $S.workerCode = 1
            $UI.lblErrMsg.Text = $msg; $UI.txtErrLog.Text = ''
            Show-View 'error'
            return
        }
        $S.workerExtra = '-ApplyUpdate -NewVersion ' + $S.newV + $(if ($S.finish) { ' -FinishOnly' } else { '' }) + ' -SkipUnblock' + $(if ($TestUpdateBase) { ' -TestUpdateBase "' + $TestUpdateBase.TrimEnd('\') + '"' } else { '' })
        $deskDir = $(if ($S.shortcutDir) { $S.shortcutDir } else { [Environment]::GetFolderPath('Desktop') })
        $hasShortcut = Test-Path -LiteralPath (Join-Path $deskDir $GameLnk)
        Start-Install $iso $hasShortcut $false
    }

    function Set-OfferContent {
        $lv = Get-LocalVersion; $nv = $S.newV; $from = $lv
        $local = Join-Path $root 'CHANGELOG.md'
        if ($S.finish) {
            $from = Get-BuiltVersion
            $UI.lblOfferTitle.Text = "Finish the update to v$nv"
            $UI.lblOfferNote.Text = 'An earlier update was not finished. It takes about 2 to 10 minutes. Your saves and settings are kept.'
            $ct = $(if (Test-Path -LiteralPath $local) { [System.IO.File]::ReadAllText($local) } else { Read-Remote 'CHANGELOG.md' })
        } else {
            $bvx = Get-BuiltVersion; if ($bvx -and ((Compare-Version $bvx $lv) -lt 0)) { $from = $bvx }      # an unfinished update: the game you have is the built one
            $UI.lblOfferTitle.Text = "Update available: v$from " + [string][char]0x2192 + " v$nv"
            $UI.lblOfferNote.Text = 'This takes about 2 to 10 minutes. Your saves and settings are kept.'
            $ct = Read-Remote 'CHANGELOG.md'
        }
        $txt = $(if ($ct) { Get-ChangelogFor $ct $from $nv } else { '' })
        if (-not $txt) { $txt = '(The list of changes could not be loaded.)' }
        $UI.txtChanges.Text = $txt
    }

    function Start-Install([string]$iso, [bool]$shortcut, [bool]$regen) {
        Reset-InstallState
        $S.toolsSub = 0.0; $S.vu = $false
        $tmp = Join-Path ([System.IO.Path]::GetTempPath()) 'rac-rs-setup'
        New-Item -ItemType Directory -Force -Path $tmp | Out-Null
        $S.outPath = Join-Path $tmp 'out.log'; $S.errPath = Join-Path $tmp 'err.log'
        Remove-Item -LiteralPath $S.outPath, $S.errPath -Force -ErrorAction SilentlyContinue
        $a = '-NoProfile -ExecutionPolicy Bypass -File "' + (Join-Path $root 'setup.ps1') + '" -Console -Iso "' + $iso + '" -SkipHash'
        if ($S.workerExtra) { $a += ' ' + $S.workerExtra }
        if ($TestWorkerArgs) { $a += ' ' + $TestWorkerArgs }
        if (-not $shortcut) { $a += ' -NoShortcut' }
        if ($S.shortcutDir) { $a += ' -ShortcutDir "' + $S.shortcutDir + '"' }
        if ($regen) { $a += ' -Regenerate' }
        $S.t0 = Get-Date
        Show-View 'install'
        Set-Step 0
        $S.proc = Start-Process -FilePath $psExe -ArgumentList $a -WorkingDirectory $root -WindowStyle Hidden -PassThru -RedirectStandardOutput $S.outPath -RedirectStandardError $S.errPath
        $null = $S.proc.Handle
    }

    function Stop-Install {
        $p = $S.proc
        if ($p -and -not $p.HasExited) {
            # only the child we started and its descendants
            try { Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\taskkill.exe') -ArgumentList "/PID $($p.Id) /T /F" -WindowStyle Hidden -Wait } catch { }
        }
    }

    function Get-ErrorText {
        if ($S.update -and $S.workerCode -eq 3) { return "The update could not be downloaded.`r`n`r`nPlease check your internet connection and click Try again. Nothing on your PC was changed." }
        $all = $S.log.ToString()
        $name = $StepNames[[math]::Min($S.step, $StepNames.Count - 1)]
        $m = "The step '$name' did not finish."
        $noCompile = (($all -split "`r?`n") | Where-Object { $_ -notmatch '^\s+Compiling ' }) -join "`n"
        $net = ($S.step -eq 0) -or ($noCompile -match '(?i)could not download|failed to download|download of .* failed|couldn.t resolve|could not resolve|resolve host|timed out|unable to connect|spurious network|network failure|failed to get ')
        $disk = ($all -match '(?i)no space left|not enough space|os error 112|there is not enough space')
        if ($S.step -eq 1) { $m = "The disc image was not accepted.`r`n`r`nIt must be Ratchet & Clank PAL / Europe v1.00 or USA v1.00 as one .iso file. Click Try again and pick the right file." }
        elseif ($disk) { $m += "`r`n`r`nThere may not be enough free disk space (about 5 GB are needed). Free some space and click Try again." }
        elseif ($net) { $m += "`r`n`r`nPlease check your internet connection, then click Try again. Setup continues where it stopped." }
        else {
            if ($S.lastError) { $m += "`r`n`r`n" + $S.lastError }
            $m += "`r`n`r`nClick Try again. If it keeps failing, click Copy details and share them when you ask for help."
        }
        if ($S.update) { $m += "`r`n`r`nYour old version is still installed. Click Play old version to keep playing it." }
        return $m
    }

    function Get-DetailsText {
        $lines = $S.log.ToString() -split "`r?`n"
        $tail = $lines | Select-Object -Last 200
        $name = $StepNames[[math]::Min($S.step, $StepNames.Count - 1)]
        return ("$GameName setup - problem report`r`nStep: $name`r`nError: $($S.lastError)`r`nWindows: $([Environment]::OSVersion.VersionString)`r`n`r`nLog (last 200 lines):`r`n" + ($tail -join "`r`n"))
    }

    function Show-Error {
        $UI.lblErrMsg.Text = Get-ErrorText
        $UI.btnCopy.Visible = (-not ($S.update -and $S.workerCode -eq 3)); $UI.btnCopy.Text = 'Copy details'
        $UI.txtErrLog.Text = $S.log.ToString()
        Show-View 'error'
        $UI.txtErrLog.SelectionStart = $UI.txtErrLog.TextLength
        $UI.txtErrLog.ScrollToCaret()
    }

    function Tick-Install {
        $p = $S.proc
        if (-not $p) { return }
        if ($S.t0) { $UI.lblElapsed.Text = 'Elapsed: ' + (Format-Elapsed ((Get-Date) - $S.t0)) }
        Read-NewText 'out'; Read-NewText 'err'
        if ($S.status) { $UI.lblStatus.Text = $S.status }
        Update-Progress
        if ($p.HasExited) {
            Read-NewText 'out'; Read-NewText 'err'
            foreach ($w in @('out', 'err')) { if ($S["${w}Rest"].Length -gt 0) { Feed-Line $S["${w}Rest"]; $S["${w}Rest"] = '' } }
            $S.proc = $null
            $code = $p.ExitCode
            $S.workerCode = $code
            if ($code -eq 0) {
                $S.finished = $true; $S.ranOk = $true
                Set-Step 4; Update-Progress
                if ($S.steamWanted) { $S.steamWanted = $false; [void](Invoke-SteamFromWindow 'add') }      # "Add to Steam" was ticked: now that the game exists
                Show-View 'done'
            } else {
                Set-Step $S.step $true
                Show-Error
            }
        }
    }

    function Ask-CloseSteam {
        # Steam is open: it would overwrite our change when it closes. We never close it; the player does, then clicks Retry.
        if ($S.test) { return $S.askAnswer }
        $r = [System.Windows.Forms.MessageBox]::Show($form, "Steam is running.`r`n`r`nPlease close Steam first (right-click its icon next to the clock, then Exit), because Steam rewrites its shortcut list when it closes. Then click Retry.", $AppTitle, 'RetryCancel', 'Information')
        return $(if ($r -eq [System.Windows.Forms.DialogResult]::Retry) { 'Retry' } else { 'Cancel' })
    }

    function Invoke-SteamFromWindow([string]$mode) {
        # add or remove our Steam entry; while Steam runs: ask the player to close it and click Retry. The result text goes into $S.steamNote.
        while ($true) {
            $r = Invoke-SteamShortcut $mode
            if ($r.Status -ne 'running') { break }
            if ((Ask-CloseSteam) -ne 'Retry') { break }
        }
        $what = $(if ($mode -eq 'add') { 'Add to Steam' } else { 'Remove from Steam' })
        $S.steamNote = $(if ($r.Status -eq 'running') { "Steam was still running, so it was not changed. Close Steam, then use `"$what`" again." } else { Get-SteamResultText $mode $r })
        return $r
    }

    function Set-FetchBusy([bool]$on) {
        foreach ($c in @($UI.chkLauncher, $UI.lnkGetLauncher, $UI.btnLauncher, $UI.btnPlay, $UI.btnFolder, $UI.lnkRepair, $UI.lnkSteam, $UI.chkShortcut, $UI.chkSteam)) { $c.Enabled = (-not $on) }
        if ($on) {
            if ($S.view -eq 'pick') { $UI.lblPickText.Text = 'Getting the RAC-RS Launcher ... (a few seconds)'; $UI.btnInstall.Text = 'Please wait'; $UI.btnInstall.Enabled = $false; $UI.btnInstall.BackColor = [System.Drawing.Color]::FromArgb(200, 200, 200); $UI.btnInstall.ForeColor = [System.Drawing.Color]::FromArgb(245, 245, 245) }
            else { $UI.lblDoneNote.Text = 'Getting the RAC-RS Launcher ... (a few seconds)' }
        }
    }

    function Start-LauncherHandOver {
        # ticked box / "Install the launcher": a hidden child runs setup.ps1 -GetLauncher (GitHub, SHA-256 digest checked); Tick-Fetch continues when it is done
        if ($S.fetch) { return }
        $S.steamWanted = (($S.view -eq 'pick') -and $S.boxSteam -and $UI.chkSteam.Checked)
        $S.wantShortcut = (($S.view -eq 'pick') -and $UI.chkShortcut.Checked)
        $a = '-NoProfile -ExecutionPolicy Bypass -File "' + (Join-Path $root 'setup.ps1') + '" -GetLauncher'
        if ($TestLauncherReleases) { $a += ' -TestLauncherReleases "' + $TestLauncherReleases.TrimEnd('\') + '"' }
        Set-FetchBusy $true
        $S.fetch = Start-Process -FilePath $psExe -ArgumentList $a -WorkingDirectory $root -WindowStyle Hidden -PassThru
        $null = $S.fetch.Handle
    }

    function Complete-LauncherHandOver {
        # the launcher is in place: remember the choice, make or re-point the desktop shortcut (it opens the launcher now), Steam when ticked, start the launcher, close this window
        Set-SetupChoice 'launcher' 'yes'
        $deskDir = $(if ($S.shortcutDir) { $S.shortcutDir } else { [Environment]::GetFolderPath('Desktop') })
        try { if ($S.wantShortcut -or (Test-Path -LiteralPath (Join-Path $deskDir $GameLnk))) { $S.shortcutPath = New-GameShortcut $deskDir } } catch { }
        if ($S.steamWanted) { $S.steamWanted = $false; [void](Invoke-SteamFromWindow 'add') }
        $S.handedOver = $true
        if ($S.test) { return }
        try { $null = Start-Process -FilePath (Join-Path $root 'RAC-RS Launcher.exe') -WorkingDirectory $root }
        catch { [void][System.Windows.Forms.MessageBox]::Show($form, "The launcher could not be started.`r`n`r`n" + $_.Exception.Message, $AppTitle, 'OK', 'Warning'); Set-FetchBusy $false; Show-View $S.view; return }
        $form.Close()
    }

    function Tick-Fetch {
        $p = $S.fetch
        if (-not $p -or -not $p.HasExited) { return }
        $S.fetch = $null
        if ($p.ExitCode -eq 0 -and (Test-Path -LiteralPath (Join-Path $root 'RAC-RS Launcher.exe') -PathType Leaf)) { Complete-LauncherHandOver; return }
        $S.fetchErr = 'The RAC-RS Launcher could not be downloaded.'
        Set-FetchBusy $false
        Show-View $S.view
        if (-not $S.test) {
            $more = $(if ($S.view -eq 'pick') { 'Check your internet connection and try again, or untick "Also install the RAC-RS Launcher" to set up the game with this window.' } else { 'Check your internet connection and try again. The game works without it.' })
            [void][System.Windows.Forms.MessageBox]::Show($form, "The RAC-RS Launcher could not be downloaded.`r`n`r`n$more", $AppTitle, 'OK', 'Information')
        }
    }

    function Invoke-InstallClick {
        if ($S.fetch) { return }
        if (Test-LauncherMode) { Start-LauncherHandOver; return }
        if (-not $S.discOk) { return }
        if ($S.boxLauncher -and -not $S.repair) { Set-SetupChoice 'launcher' 'no' }      # unticked: remembered, so a later Setup.cmd does not push the launcher again
        $S.steamWanted = ((-not $S.repair) -and $S.boxSteam -and $UI.chkSteam.Checked)
        Set-Mode $false
        Start-Install $S.discPath $UI.chkShortcut.Checked $S.repair
    }

    # [MP] The window looks for a newer release by itself when it opens (in the background: the timer picks the answer up), so
    # nobody needs to visit GitHub: the home view says so and its bottom-right link and Play together's link become "Update to vX".
    # The game's own start does the same check (-Launch). Offline or any failure: nothing is said.
    function Start-UpdateLook([bool]$test = $false) {
        if ($S.updTask -or $S.updDone -or (($TestNoShow -or $Launch -or $Update) -and -not $test)) { return }
        try {
            if ($TestUpdateBase -and $TestUpdateBase -notmatch '^(https?|file)://') {
                $S.updTask = [pscustomobject]@{ IsCompleted = $true; IsFaulted = $false; IsCanceled = $false; Result = [string](Read-Remote 'VERSION') }
                return
            }
            try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch { }
            $wc = New-Object System.Net.WebClient
            $wc.Headers['User-Agent'] = 'rac-rs-updater'
            $url = $(if ($TestUpdateBase) { $TestUpdateBase.TrimEnd('/') + '/VERSION' } else { "https://api.github.com/repos/$Repo/releases/latest" })
            $ctx = [System.Threading.SynchronizationContext]::Current
            [System.Threading.SynchronizationContext]::SetSynchronizationContext($null)
            try { $S.updTask = $wc.DownloadStringTaskAsync($url) } finally { [System.Threading.SynchronizationContext]::SetSynchronizationContext($ctx) }
        } catch { $S.updDone = $true }
    }

    function Tick-UpdateLook {
        $t = $S.updTask
        if ($S.updDone -or -not $t -or -not $t.IsCompleted) { return }
        $S.updDone = $true
        try {
            if ($t.IsFaulted -or $t.IsCanceled) { return }
            $txt = [string]$t.Result
            $rv = ''
            if ($TestUpdateBase) { $rv = $txt.Trim() }
            else {
                $tag = [string](($txt | ConvertFrom-Json).tag_name)
                if ($tag -match '^v(\d+\.\d+\.\d+(\.\d+)?)$') { $rv = $Matches[1]; $script:RemoteTag = $tag }
            }
            if ($rv -match '^\d+\.\d+\.\d+(\.\d+)?$' -and (Compare-Version $rv (Get-LocalVersion)) -gt 0) {
                $S.updAvail = $rv
                if ($S.view -in @('installed', 'mp') -and -not $S.proc) { Show-View $S.view }
            }
        } catch { }
    }

    function Start-UpdateFromLink {
        # "Update to vX": as "Check for updates" does once it found one (the same update run in this window)
        if (-not $S.updAvail -or $S.proc) { return }
        $lv = Get-LocalVersion
        $r = [System.Windows.Forms.MessageBox]::Show($form, "Update from version $lv to $($S.updAvail) now?`r`n`r`nIt takes about 2 to 10 minutes. Your saves and settings are kept.", $AppTitle, 'YesNo', 'Question')
        if ($r -eq [System.Windows.Forms.DialogResult]::Yes) { $S.newV = $S.updAvail; $S.finish = $false; Start-UpdateRun }
    }

    function Tick-All { Tick-Disc; Tick-Install; Tick-Fetch; Tick-UpdateLook; Tick-MpFind }

    function Switch-Details {
        if ($UI.txtLog.Visible) {
            $UI.txtLog.Visible = $false; $UI.lnkDetails.Text = 'Show details'; Set-FormHeight 440
        } else {
            Set-FormHeight 660; $UI.txtLog.Size = New-Object System.Drawing.Size($UI.txtLog.Width, [int][math]::Max(40 * (Get-Scale), $form.ClientSize.Height - $UI.txtLog.Top - 18 * (Get-Scale)))
            $UI.txtLog.Visible = $true; $UI.lnkDetails.Text = 'Hide details'
            $UI.txtLog.SelectionStart = $UI.txtLog.TextLength; $UI.txtLog.ScrollToCaret()
        }
    }

    # ---------------------------------------------------------------- [MP] play together
    function Update-MpMode {
        $h = $UI.rbHost.Checked
        $UI.btnMpStart.Text = $(if ($h) { 'Host' } else { 'Join' })
        foreach ($c in @($UI.lblMpAddr, $UI.lnkMpCopy, $UI.lblMpFw, $UI.lnkMpFw)) { $c.Enabled = $h }
        $UI.lblMpJoinHint.Enabled = (-not $h)
    }

    function Set-MpView {
        # the remembered choices (once), then this PC's address and the firewall, looked up just after the view shows (a second or two)
        if (-not $S.mpLoaded) {
            $S.mpLoaded = $true
            $S.mpFresh = -not (Test-Path -LiteralPath (Join-Path $root 'generated\multiplayer.cfg') -PathType Leaf)
            $m = Get-MpSettings
            $UI.txtMpName.Text = Get-MpName $m.name
            $UI.txtMpJoin.Text = $m.join
            if ($m.mode -eq 'join') { $UI.rbJoin.Checked = $true } else { $UI.rbHost.Checked = $true }
        }
        Update-MpMode
        $form.ActiveControl = $UI.btnMpStart
        $UI.lblMpAddr.Text = 'Looking up the address your friends join ...'; $UI.lnkMpCopy.Visible = $false
        $UI.lblMpFw.Text = ''; $UI.lnkMpFw.Visible = $false
        if (-not $S.test) { $UI.mpInfo.Stop(); $UI.mpInfo.Start() }
        if (-not $S.find.round -and $S.find.rounds -eq 0) { Start-MpFind }
    }

    function Update-MpInfo {
        $form.Refresh()
        $a = Get-MpAddresses
        if ($a.Tailscale) { $S.mpAddr = $a.Tailscale; $UI.lblMpAddr.Text = "Friends join this address: $($a.Tailscale)`r`n(this PC's Tailscale address)" }
        elseif ($a.Lan) { $S.mpAddr = $a.Lan; $UI.lblMpAddr.Text = "Friends on your home network join: $($a.Lan)`r`n(Tailscale is not running on this PC)" }
        else { $S.mpAddr = ''; $UI.lblMpAddr.Text = 'No network address was found. Is this PC online?' }
        $UI.lnkMpCopy.Text = 'Copy'; $UI.lnkMpCopy.Visible = [bool]$S.mpAddr
        if (Test-MpFirewallBlock) {
            $UI.lblMpFw.Text = 'Windows Firewall blocks the game, so nobody can join you. Open the firewall settings, click Change settings, tick both boxes of rac-rs.exe, then OK.'
            $UI.lblMpFw.ForeColor = $cRed; $UI.lnkMpFw.Visible = $true
        } else {
            $UI.lblMpFw.Text = 'The first time you host, Windows may ask whether the game may use the network: click Allow.'
            $UI.lblMpFw.ForeColor = $cGrey; $UI.lnkMpFw.Visible = $false
        }
        Fit-TextLabels
    }

    # [MP] The Join list: a round asks every online Tailscale PC and the home network (Get-MpNetworks) whether a game hosts there, both
    # ways (`$MpQuery` for 0.4.1.4 and later, `$MpProbe` for older hosts), and asks again once for a lost datagram; about two seconds
    # later the list shows what answered. Rounds repeat every few seconds while this view shows, so a friend who starts hosting
    # appears by itself. Nothing here blocks the window: `tailscale status` runs as a process read when done, the socket only when
    # something is waiting (Tick-MpFind, from the window's timer).
    function Send-MpFind([string[]]$ips) {
        foreach ($ip in $ips) {
            try {
                $ep = New-Object System.Net.IPEndPoint([System.Net.IPAddress]::Parse($ip), $MpPort)
                [void]$S.find.udp.Send($MpQuery, $MpQuery.Length, $ep); [void]$S.find.udp.Send($MpProbe, $MpProbe.Length, $ep)
            } catch { }
        }
    }

    function Start-MpFind {
        $f = $S.find
        if ($f.round) { return }
        try {
            if (-not $f.udp) {
                $f.udp = New-Object System.Net.Sockets.UdpClient(0, [System.Net.Sockets.AddressFamily]::InterNetwork)
                $f.udp.EnableBroadcast = $true
                try { [void]$f.udp.Client.IOControl([int]-1744830452, [byte[]](0, 0, 0, 0), $null) } catch { }      # SIO_UDP_CONNRESET off: a PC with nothing on the port is no error
            }
        } catch { $f.udp = $null; return }
        $f.round = $true; $f.found = @{}; $f.t0 = Get-Date; $f.again = $false; $f.tsAt = $null; $f.peers = @()
        $f.net = Get-MpNetworks
        Send-MpFind $f.net.broadcast
        $f.proc = $null; $f.out = $null
        $ts = Find-Tailscale
        if ($ts) {
            try {
                $psi = New-Object System.Diagnostics.ProcessStartInfo
                $psi.FileName = $ts; $psi.Arguments = 'status --json'; $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
                $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true; $psi.StandardOutputEncoding = [System.Text.Encoding]::UTF8
                $f.proc = [System.Diagnostics.Process]::Start($psi)
                $f.out = $f.proc.StandardOutput.ReadToEndAsync(); [void]$f.proc.StandardError.ReadToEndAsync()
            } catch { $f.proc = $null; $f.out = $null }
        }
        if (-not $f.out) { $f.tsOn = $false; $f.tsAt = Get-Date }
    }

    function Stop-MpFind {
        $f = $S.find
        try { if ($f.proc -and -not $f.proc.HasExited) { $f.proc.Kill() } } catch { }
        try { if ($f.udp) { $f.udp.Close() } } catch { }
        $f.udp = $null; $f.proc = $null; $f.out = $null; $f.round = $false; $f.next = [datetime]::MinValue
    }

    function Tick-MpFind {
        $f = $S.find
        if ($S.view -ne 'mp' -or $S.proc) { return }
        $now = Get-Date
        if (-not $f.round) { if ($now -ge $f.next) { Start-MpFind }; return }
        try {
            while ($f.udp.Available -gt 0) {
                $ep = New-Object System.Net.IPEndPoint([System.Net.IPAddress]::Any, 0)
                $b = $null; try { $b = $f.udp.Receive([ref]$ep) } catch { continue }
                $a = Read-MpAnswer $b
                $ip = $ep.Address.ToString()
                if (-not $a -or ($f.net.mine.ContainsKey($ip) -and -not $f.self)) { continue }
                $old = $f.found[$ip]
                if ($old -and $old.info -and -not $a.info) { continue }      # the query's answer says more than the probe's
                $a.ip = $ip; $f.found[$ip] = $a
            }
        } catch { }
        if ($f.out -and -not $f.tsAt) {
            if ($f.out.IsCompleted) {
                $t = Read-TailscalePeers $(if ($f.out.IsFaulted) { '' } else { [string]$f.out.Result }) $f.self
                $f.tsOn = $t.running; $f.peers = @($t.peers | Where-Object { -not $_.sharee })
                $f.sharees = @($t.peers | Where-Object { $_.sharee } | ForEach-Object { $_.owner } | Where-Object { $_ } | Select-Object -Unique)
                $f.ts = @{}; foreach ($p in $t.peers) { $f.ts[$p.ip] = $p }
                Send-MpFind @($t.peers | ForEach-Object { $_.ip })
                $f.tsAt = $now
            } elseif (($now - $f.t0).TotalSeconds -gt 5) {
                try { $f.proc.Kill() } catch { }
                $f.tsOn = $false; $f.tsAt = $now
            }
        }
        if (-not $f.again -and ($now - $f.t0).TotalMilliseconds -ge 800) {
            # once more, for a datagram lost on the way
            $f.again = $true
            Send-MpFind $f.net.broadcast
            if ($f.tsAt) { Send-MpFind @($f.ts.Keys) }
        }
        if ($f.tsAt -and ($now - $f.tsAt).TotalMilliseconds -ge 1600 -and ($now - $f.t0).TotalMilliseconds -ge 2000) {
            $f.round = $false; $f.rounds++; $f.next = $now.AddSeconds(4)
            Show-MpFound
        }
    }

    function Show-MpFound {
        # the list from the last round: Tailscale names attached, one line per PC (a PC found both on Tailscale and the home network once)
        $f = $S.find
        $proto = Get-MpProtocol
        $list = @()
        foreach ($a in $f.found.Values) {
            $e = @{} + $a
            $p = $f.ts[$a.ip]
            if ($p) { $e.device = $p.device; $e.owner = $p.owner; $e.dns = $p.dns; $e.lan = $false } else { $e.device = ''; $e.owner = ''; $e.dns = ''; $e.lan = $true }
            $list += $e
        }
        $list = @($list | Sort-Object @{ e = { [int]$_.lan } }, @{ e = { $(if ($_.info) { $_.name } else { "~$($_.owner)" }) } }, @{ e = { $_.ip } })
        $keep = @(); $seen = @{}
        foreach ($e in $list) {
            $k = $(if ($e.info) { "$($e.name)|$($e.version)|$($e.tick)|$($e.players)|$($e.level)" } else { $null })
            if ($k -and $seen.ContainsKey($k)) { continue }
            if ($k) { $seen[$k] = $true }
            $e.text = Get-MpFoundText $e $proto
            $keep += $e
        }
        $f.shown = $keep
        $S.findPick = $true
        try {
            $lines = @($keep | ForEach-Object { $_.text })
            if (($lines -join "`n") -ne (@($UI.lstMpFound.Items) -join "`n")) {      # redrawn only when something changed
                $UI.lstMpFound.BeginUpdate(); $UI.lstMpFound.Items.Clear()
                foreach ($t in $lines) { [void]$UI.lstMpFound.Items.Add($t) }
                $UI.lstMpFound.EndUpdate()
            }
            # the game in the address box stays picked; a first visit with one game found picks it (and Join, unless they chose Host)
            $cur = $UI.txtMpJoin.Text.Trim().TrimEnd('.').ToLowerInvariant()
            $i = -1
            for ($n = 0; $n -lt $keep.Count; $n++) {
                $e = $keep[$n]
                if ($cur -and ($cur -eq $e.ip -or ($e.dns -and ($cur -eq $e.dns.ToLowerInvariant() -or $cur -eq ($e.dns -split '\.')[0].ToLowerInvariant())))) { $i = $n; break }
            }
            if ($i -ge 0) { $UI.lstMpFound.SelectedIndex = $i }
            elseif (-not $cur -and $keep.Count -eq 1 -and -not $S.findTouched) {
                $UI.lstMpFound.SelectedIndex = 0; $UI.txtMpJoin.Text = $keep[0].ip
                if ($S.mpFresh) { $UI.rbJoin.Checked = $true }
            }
        } finally { $S.findPick = $false }
        if ($keep.Count -gt 0) { $UI.lblMpFound.Text = 'Games you can join now: click one, then Join.' }
        else {
            $t = 'No game found yet. Is your friend hosting? This list looks again every few seconds; you can also type their address below.'
            if ($f.tsOn -eq $false) { $t = 'No game found on your home network. Tailscale is not running on this PC: start it to find friends who play over the internet.' }
            elseif ($f.sharees.Count -gt 0) { $t = "No game found yet. $($f.sharees[0]) can join games you host; to see the games they host here, they share their PC with you in Tailscale." }
            $UI.lblMpFound.Text = $t
        }
        $UI.lblMpFound.ForeColor = $cGrey
        Fit-LabelFont $UI.lblMpFound
    }

    function Start-Together {
        $name = Get-MpName $UI.txtMpName.Text
        if (-not $name) { [void][System.Windows.Forms.MessageBox]::Show($form, 'Type the name your friends will see above your Ratchet.', $AppTitle, 'OK', 'Information'); $UI.txtMpName.Focus(); return }
        $join = $UI.rbJoin.Checked
        $addr = ($UI.txtMpJoin.Text.Trim() -replace '^[a-z]+://', '').TrimEnd('/')
        if ($join -and -not $addr) { [void][System.Windows.Forms.MessageBox]::Show($form, "Type your friend's address: their Play together window shows it.", $AppTitle, 'OK', 'Information'); $UI.txtMpJoin.Focus(); return }
        if ($join -and $addr -match '\s') { [void][System.Windows.Forms.MessageBox]::Show($form, "An address has no spaces, for example 100.101.102.103.", $AppTitle, 'OK', 'Information'); $UI.txtMpJoin.Focus(); return }
        $mode = $(if ($join) { 'join' } else { 'host' })
        try { Set-MpSettings $mode $(if ($join) { $addr } else { $UI.txtMpJoin.Text.Trim() }) $name } catch { }
        $envs = Get-MpGameEnv $join $addr $name
        if ($S.test) { $S.mpStarted = $envs; return }
        try { Start-Game $false $envs; $form.Close() }
        catch { [void][System.Windows.Forms.MessageBox]::Show($form, "The game could not be started.`r`n`r`n" + $_.Exception.Message, $AppTitle, 'OK', 'Warning') }
    }

    # ---------------------------------------------------------------- events
    $UI.mpInfo = New-Object System.Windows.Forms.Timer
    $UI.mpInfo.Interval = 60
    $UI.mpInfo.Add_Tick({ $UI.mpInfo.Stop(); try { Update-MpInfo } catch { } })
    $UI.btnTogether.Add_Click({ $S.mpBack = $S.view; Show-View 'mp' })
    $UI.lnkMpBack.Add_LinkClicked({ Show-View $(if ($S.mpBack -eq 'done') { 'done' } else { 'installed' }) })
    $UI.rbHost.Add_CheckedChanged({ Update-MpMode })
    $UI.rbJoin.Add_CheckedChanged({ Update-MpMode })
    $UI.txtMpJoin.Add_Enter({ $UI.rbJoin.Checked = $true })
    $UI.txtMpJoin.Add_TextChanged({
        if ($S.findPick) { return }
        $S.findTouched = $true
        $i = $UI.lstMpFound.SelectedIndex
        if ($i -ge 0 -and $i -lt $S.find.shown.Count -and $UI.txtMpJoin.Text.Trim() -ne $S.find.shown[$i].ip) { $S.findPick = $true; $UI.lstMpFound.ClearSelected(); $S.findPick = $false }
    })
    $UI.lstMpFound.Add_SelectedIndexChanged({
        if ($S.findPick) { return }
        $i = $UI.lstMpFound.SelectedIndex
        if ($i -lt 0 -or $i -ge $S.find.shown.Count) { return }
        $S.findPick = $true; $S.findTouched = $true
        try { $UI.txtMpJoin.Text = $S.find.shown[$i].ip; $UI.rbJoin.Checked = $true } finally { $S.findPick = $false }
    })
    $UI.lstMpFound.Add_DoubleClick({ if ($UI.lstMpFound.SelectedIndex -ge 0) { Start-Together } })
    $UI.lnkMpFind.Add_LinkClicked({ if (-not $S.find.round) { $UI.lblMpFound.Text = 'Looking for games on Tailscale and your home network ...'; Start-MpFind } })
    $UI.btnMpStart.Add_Click({ Start-Together })
    $UI.lnkMpUpdate.Add_LinkClicked({ Start-UpdateFromLink })
    $UI.lnkMpCopy.Add_LinkClicked({ try { [System.Windows.Forms.Clipboard]::SetText($S.mpAddr); $UI.lnkMpCopy.Text = 'Copied' } catch { } })
    $UI.lnkMpFw.Add_LinkClicked({ try { Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\control.exe') -ArgumentList '/name Microsoft.WindowsFirewall /page pageConfigureApps' } catch { } })
    $UI.lnkTailscale.Add_LinkClicked({ try { Start-Process 'https://tailscale.com/download' } catch { } })
    $UI.debounce = New-Object System.Windows.Forms.Timer
    $UI.debounce.Interval = 700
    $UI.debounce.Add_Tick({ $UI.debounce.Stop(); Start-DiscCheck $UI.txtIso.Text })
    $UI.txtIso.Add_TextChanged({
        if ($S.suppress) { return }
        Stop-DiscCheck
        $S.discOk = $false; Set-InstallEnabled $false; Set-DiscStatus 'none' ''
        $UI.debounce.Stop(); $UI.debounce.Start()
    })
    $UI.btnBrowse.Add_Click({
        $dlg = New-Object System.Windows.Forms.OpenFileDialog
        $dlg.Title = 'Pick your Ratchet & Clank (Europe or USA, v1.00) disc image'
        $dlg.Filter = 'Disc image (*.iso)|*.iso|All files (*.*)|*.*'
        if ($UI.txtIso.Text -and (Test-Path -LiteralPath (Split-Path -Parent $UI.txtIso.Text) -ErrorAction SilentlyContinue)) { $dlg.InitialDirectory = Split-Path -Parent $UI.txtIso.Text }
        if ($dlg.ShowDialog($form) -eq [System.Windows.Forms.DialogResult]::OK) { Set-IsoPath $dlg.FileName }
    })
    $UI.btnInstall.Add_Click({ Invoke-InstallClick })
    $UI.chkLauncher.Add_CheckedChanged({ Update-PickMode })
    $UI.lnkGetLauncher.Add_LinkClicked({ Start-LauncherHandOver })
    $UI.btnLauncher.Add_Click({ Start-LauncherHandOver })
    $UI.lnkSteam.Add_LinkClicked({
        [void](Invoke-SteamFromWindow $(if ($S.steamHas) { 'remove' } else { 'add' }))
        Show-View $S.view
    })
    $UI.lnkDetails.Add_LinkClicked({ Switch-Details })
    $UI.lnkSupport.Add_LinkClicked({ try { Start-Process 'https://ko-fi.com/asobicodes' } catch { } })
    $UI.btnPlay.Add_Click({
        if ($S.finishPending) { $S.finish = $true; Start-UpdateRun; return }
        try { Start-Game $S.update; $form.Close() }
        catch { [void][System.Windows.Forms.MessageBox]::Show($form, "The game could not be started.`r`n`r`n" + $_.Exception.Message, $AppTitle, 'OK', 'Warning') }
    })
    $UI.btnFolder.Add_Click({ try { Start-Process -FilePath 'explorer.exe' -ArgumentList ('"' + $root + '"') } catch { } })
    $UI.lnkRepair.Add_LinkClicked({
        $S.repair = $true
        Show-View 'pick'
        $iso = Get-InstalledIso
        if ($iso -and (Test-Path -LiteralPath $iso -PathType Leaf)) { Set-IsoPath $iso } else { Set-IsoPath '' }
    })
    $UI.btnCopy.Add_Click({
        try { [System.Windows.Forms.Clipboard]::SetText((Get-DetailsText)); $UI.btnCopy.Text = 'Copied' } catch { $UI.btnCopy.Text = 'Copy failed' }
    })
    $UI.btnRetry.Add_Click({
        if ($S.update) { Start-UpdateRun; return }
        Show-View 'pick'
        if ($S.discOk) { Set-InstallEnabled $true }
    })
    $UI.btnErrClose.Add_Click({
        if ($S.update) { try { Start-Game $true } catch { } }
        $form.Close()
    })
    $UI.btnUpdNow.Add_Click({ try { Start-UpdateProcess $S.newV $S.finish; $S.exitCode = 10 } catch { $S.exitCode = 0 }; $form.Close() })      # could not start the update window: play instead
    $UI.btnNotNow.Add_Click({ $S.exitCode = 0; $form.Close() })
    $UI.lnkCheck.Add_LinkClicked({
        if ($S.updAvail) { Start-UpdateFromLink; return }
        $UI.lnkCheck.Text = 'Checking ...'; $form.Refresh()
        $lv = Get-LocalVersion; $rv = Read-Remote 'VERSION'
        $UI.lnkCheck.Text = 'Check for updates'
        if (-not $rv -or ($rv.Trim() -notmatch '^\d+\.\d+\.\d+(\.\d+)?$')) { [void][System.Windows.Forms.MessageBox]::Show($form, 'Could not check for updates. Are you online?', $AppTitle, 'OK', 'Information'); return }
        $rv = $rv.Trim()
        if ((Compare-Version $rv $lv) -le 0) { [void][System.Windows.Forms.MessageBox]::Show($form, "You have the latest version (v$lv).", $AppTitle, 'OK', 'Information'); return }
        $r = [System.Windows.Forms.MessageBox]::Show($form, "Version $rv is available (you have $lv).`r`n`r`nUpdate now? It takes about 2 to 10 minutes. Your saves and settings are kept.", $AppTitle, 'YesNo', 'Question')
        if ($r -eq [System.Windows.Forms.DialogResult]::Yes) { $S.newV = $rv; $S.finish = $false; Start-UpdateRun }
    })
    $form.Add_FormClosing({
        param($sender, $e)
        if ($S.proc -and -not $S.proc.HasExited) {
            if ($S.test) { Stop-Install }
            else {
                $r = [System.Windows.Forms.MessageBox]::Show($form, "Setup is still running.`r`n`r`nStop it and close this window? You can start it again later and it will continue.", $AppTitle, 'YesNo', 'Question')
                if ($r -ne [System.Windows.Forms.DialogResult]::Yes) { $e.Cancel = $true; return }
                Stop-Install
            }
        }
        Stop-DiscCheck
        Stop-MpFind
    })
    $UI.timer = New-Object System.Windows.Forms.Timer
    $UI.timer.Interval = 400
    $UI.timer.Add_Tick({ try { Tick-All } catch { } })
    $UI.timer.Start()

    Set-InstallEnabled $false
    # first view: -Launch = the update offer, -Update = the update window, otherwise installed -> Play, or pick the disc
    if ($Launch) { $S.newV = $launchNew; $S.finish = ($launchMode -eq 'finish'); Set-OfferContent; Show-View 'offer' }
    elseif ($Update) {
        $S.newV = $NewVersion; $S.finish = [bool]$FinishOnly
        if (-not $TestNoShow) { Wait-GameClosed; Start-UpdateRun }
    }
    elseif (Test-Installed) { if ($Together) { $S.mpBack = 'installed'; Show-View 'mp' } else { Show-View 'installed' } } else { Show-View 'pick' }

    # ---------------------------------------------------------------- hidden test mode: nothing is shown, the views are saved as PNG
    function Wait-Until([scriptblock]$cond, [int]$sec) {
        $end = (Get-Date).AddSeconds($sec)
        while (-not (& $cond)) {
            Tick-All
            if ((Get-Date) -gt $end) { return $false }
            Start-Sleep -Milliseconds 150
        }
        Tick-All
        return $true
    }
    function Save-Shot([string]$name) {
        $form.PerformLayout()
        $bmp = New-Object System.Drawing.Bitmap($form.Width, $form.Height)
        try {
            $form.DrawToBitmap($bmp, (New-Object System.Drawing.Rectangle(0, 0, $form.Width, $form.Height)))
            $f = Join-Path $TestShot "$name.png"
            $bmp.Save($f, [System.Drawing.Imaging.ImageFormat]::Png)
        } finally { $bmp.Dispose() }
        Write-Host "  shot $name.png"
    }
    function Test-Check([string]$what, [bool]$ok) {
        $script:TestFails += $(if ($ok) { 0 } else { 1 })
        Write-Host ('  {0}: {1}' -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $what)
    }

    if ($TestNoShow) {
        $script:TestFails = 0
        if (-not $TestShot) { $TestShot = Join-Path $root 'test_shots' }
        New-Item -ItemType Directory -Force -Path $TestShot | Out-Null
        $null = $form.Handle
        # a window that is never shown has no child windows yet: create them (without showing anything) so they can be drawn
        try { [void]$form.GetType().GetMethod('CreateControl', [Reflection.BindingFlags]'NonPublic,Instance', $null, [Type[]]@([bool]), $null).Invoke($form, @($true)) } catch { }
        $S.shortcutDir = Join-Path $TestShot 'desk'
        New-Item -ItemType Directory -Force -Path $S.shortcutDir | Out-Null
        if ($Launch) {
            # -Launch test: the offer window is built but never shown; the choice comes from -TestChoice
            Write-Host ('  title: ' + $UI.lblOfferTitle.Text); Write-Host ('  changes: ' + ($UI.txtChanges.Text -replace "`r`n", ' | '))
            Save-Shot $(if ($S.finish) { 'launch_finish' } else { 'launch_offer' })
            $code = $(if ($TestChoice -eq 'update') { 10 } else { 0 })
            if ($code -eq 10) { Start-UpdateProcess $S.newV $S.finish }
            Write-Host "  choice '$TestChoice' -> exit $code"
            exit $code
        }
        if ($Update) {
            # -Update test: the whole update runs hidden; the window is built but never shown
            Wait-GameClosed
            Save-Shot 'upd_00_start'
            Start-UpdateRun
            [void](Wait-Until { $S.step -ge 3 -or -not $S.proc } 1800)
            if ($S.proc) { Save-Shot 'upd_01_building' }
            [void](Wait-Until { -not $S.proc } 3600)
            Save-Shot $(if ($S.view -eq 'done') { 'upd_02_done' } else { 'upd_02_error' })
            Write-Host ("  update view: {0}, worker exit code {1}" -f $S.view, $S.workerCode)
            if ($S.view -ne 'done') { Write-Host ('  error text: ' + ($UI.lblErrMsg.Text -replace "`r`n", ' | ')) }
            Set-Content -LiteralPath (Join-Path $TestShot 'upd_log.txt') -Value $S.log.ToString() -Encoding UTF8
            Stop-Install
            exit $(if ($S.view -eq 'done') { 0 } else { 1 })
        }
        if ($TestChoice -eq 'firstview') {
            # test only: just the first view of the launcher (the installed view says "Finish the update" after a ZIP was unzipped over an older folder)
            Save-Shot 'first_view'
            Write-Host ('  view: ' + $S.view + '; title: ' + $UI.lblDoneTitle.Text + '; text: ' + $UI.lblDoneText.Text + '; button: ' + $UI.btnPlay.Text)
            exit 0
        }
        if ($TestChoice -eq 'together') {
            # [MP] test only: the home view with "Play together", then that view as host (this PC's real addresses and firewall rules, only
            # read) and as join. Writes nothing but the shots.
            Show-View 'installed'; Save-Shot 'mp_00_installed'
            $S.mpBack = 'installed'; Show-View 'mp'; Update-MpInfo; Save-Shot 'mp_01_host'
            Write-Host ('  host: ' + ($UI.lblMpAddr.Text -replace "`r`n", ' | ') + ' / firewall: ' + $UI.lblMpFw.Text + ' / button: ' + $UI.btnMpStart.Text)
            $UI.rbJoin.Checked = $true; $UI.txtMpJoin.Text = '100.101.102.103'; Save-Shot 'mp_02_join'
            Write-Host ('  join: button: ' + $UI.btnMpStart.Text + ', name: ' + $UI.txtMpName.Text)
            foreach ($j in @($false, $true)) {
                $e = Get-MpGameEnv $j '100.101.102.103' 'Raj'
                Write-Host ('  game started with (' + $(if ($j) { 'join' } else { 'host' }) + '): ' + (($e.Keys | Sort-Object | ForEach-Object { "$_=$($e[$_])" }) -join ' | '))
            }
            Show-View $S.mpBack; Write-Host ('  back: ' + $S.view)
            Start-UpdateLook $true
            [void](Wait-Until { $S.updDone } 20)
            Write-Host ('  update look: newer ' + $(if ($S.updAvail) { $S.updAvail } else { 'none' }) + ' / link: ' + $UI.lnkCheck.Text + ' / note: ' + $UI.lblDoneNote.Text)
            Show-View 'installed'; Save-Shot 'mp_03_installed_update'
            Show-View 'mp'; Save-Shot 'mp_04_together_update'
            Write-Host ('  together link: ' + $UI.lnkMpUpdate.Text)
            # the Join list: a round as players get it (this PC's own game left out), then one with this PC included (a game hosted here
            # for the test is found), and its lines
            Stop-MpFind; Start-MpFind
            [void](Wait-Until { -not $S.find.round } 15)
            Write-Host ('  find (this PC left out): ' + $S.find.shown.Count + ' games')
            Stop-MpFind; $S.find.self = $true; $S.find.rounds = 0; $UI.txtMpJoin.Text = ''; $S.findTouched = $false; $S.mpFresh = $true; $UI.rbHost.Checked = $true
            Start-MpFind
            [void](Wait-Until { -not $S.find.round } 15)
            Write-Host ('  find: tailscale ' + $(if ($S.find.tsOn) { 'on' } else { 'off' }) + ', ' + $S.find.peers.Count + ' online PCs asked, ' + $S.find.found.Count + ' answers')
            foreach ($e in $S.find.shown) { Write-Host ('  found: ' + $e.text + '  [' + $e.ip + ']') }
            Write-Host ('  find status: ' + $UI.lblMpFound.Text + ' / address: ' + $UI.txtMpJoin.Text + ' / join: ' + $UI.rbJoin.Checked + ' / button: ' + $UI.btnMpStart.Text)
            Save-Shot 'mp_05_found'
            $bottom = $UI.btnMpStart.Bottom; $ph = $UI.pMp.ClientSize.Height
            Write-Host ('  fit: Join button bottom ' + $bottom + ' of ' + $ph + ', window ' + $form.Width + 'x' + $form.Height + ' in ' + $wa.Width + 'x' + $wa.Height + ' -> ' + $(if ($bottom -le $ph -and $form.Height -le $wa.Height -and $form.Width -le $wa.Width) { 'ok' } else { 'DOES NOT FIT' }))
            exit 0
        }
        if ($TestChoice -in @('launcherviews', 'flow_unticked', 'flow_ticked', 'dpiviews')) {
            # test only: the launcher box / Steam box views (shots 30..) and the two click paths of the first view, headless, on a COPY of the folder
            # (it writes generated\, the launcher exe and a Steam entry). Steam is a fake folder inside the shot folder; the real Steam is never looked at.
            if ($root -notlike '*\scratch_setup*\*') { Write-Host 'refused: run this on a copy of the folder under scratch_setup'; exit 2 }
            $simK = $LayoutK      # the window itself is laid out for -TestDpi N (the old RAC_TEST_SIM_SCALE second look is gone: the real layout is tested instead)
            $getState = [System.Windows.Forms.Control].GetMethod('GetState', [System.Reflection.BindingFlags]'NonPublic,Instance', $null, [Type[]]@([int]), $null)
            function Own-Visible($c) { return [bool]$getState.Invoke($c, @(2)) }      # a control's own Visible flag (.Visible reads false while the window is not shown)
            $fake = Join-Path $TestShot 'fake_steam'
            if ($fake.StartsWith($TestShot) -and (Test-Path -LiteralPath $fake)) { Remove-Item -LiteralPath $fake -Recurse -Force }
            New-Item -ItemType Directory -Force -Path (Join-Path $fake 'userdata\1001\config') | Out-Null
            $env:RAC_TEST_STEAM_DIR = $fake; $env:RAC_TEST_STEAM_RUNNING = '0'
            $noSteam = Join-Path $TestShot 'no_steam_here'
            $choiceFile = Join-Path $root 'generated\setup_choice.txt'
            $launcherExe = Join-Path $root 'RAC-RS Launcher.exe'
            Write-Host ('  layout scale of this window: {0:N2} (Windows display scale {1:N2})' -f $LayoutK, $want)
            function Test-ViewFit([string]$tag) {
                # every visible control of the current view: inside the panel, its text fits its box (word-wrapped), no two boxes overlap
                $panel = @($form.Controls | Where-Object { (Own-Visible $_) -and ($_ -is [System.Windows.Forms.Panel]) })[0]
                $kids = @($panel.Controls | Where-Object { Own-Visible $_ })
                $bad = 0
                foreach ($c in $kids) {
                    $nm = $(if ($c.Text) { $c.Text } else { $c.GetType().Name })
                    if ($c.Bottom -gt $panel.ClientSize.Height -or $c.Right -gt $panel.ClientSize.Width) { Test-Check "$tag : '$nm' is inside the window ($($c.Bottom) of $($panel.ClientSize.Height))" $false; $bad++ }
                    if (($c -is [System.Windows.Forms.Label] -or $c -is [System.Windows.Forms.ButtonBase]) -and $c.Text) {
                        $isBtn = ($c -is [System.Windows.Forms.Button]); $isChk = ($c -is [System.Windows.Forms.CheckBox])
                        $pad = $(if ($isChk) { [int](22 * $simK) } elseif ($isBtn) { [int](10 * $simK) } else { 0 })
                        $fl = [System.Windows.Forms.TextFormatFlags]::NoPrefix; if (-not $isBtn) { $fl = $fl -bor [System.Windows.Forms.TextFormatFlags]::WordBreak }
                        $m = [System.Windows.Forms.TextRenderer]::MeasureText($c.Text, $c.Font, (New-Object System.Drawing.Size(($c.ClientSize.Width - $pad), 100000)), $fl)
                        if (($isBtn -and $m.Width -gt ($c.ClientSize.Width - $pad)) -or (-not $isBtn -and $m.Height -gt $c.ClientSize.Height)) { Test-Check "$tag : text of '$nm' fits its box (needs $($m.Width)x$($m.Height), box $($c.ClientSize.Width)x$($c.ClientSize.Height))" $false; $bad++ }
                    }
                }
                for ($i = 0; $i -lt $kids.Count; $i++) { for ($j = $i + 1; $j -lt $kids.Count; $j++) {
                    if ($kids[$i].Bounds.IntersectsWith($kids[$j].Bounds)) { Test-Check "$tag : '$($kids[$i].Text)' and '$($kids[$j].Text)' do not overlap" $false; $bad++ }
                } }
                Test-Check "$tag : every control fits (inside, text not clipped, no overlap)" ($bad -eq 0)
            }
            if ($TestChoice -eq 'launcherviews') {
                if (Test-Path -LiteralPath $choiceFile) { Remove-Item -LiteralPath $choiceFile -Force }
                if (Test-Path -LiteralPath $launcherExe) { Remove-Item -LiteralPath $launcherExe -Force }
                $S.repair = $false
                $UI.chkLauncher.Checked = $true; $UI.chkSteam.Checked = $false
                Show-View 'pick'
                Test-Check 'pick, ticked: launcher box and Steam box shown, button says Continue, disc controls greyed' ((Own-Visible $UI.chkLauncher) -and (Own-Visible $UI.chkSteam) -and ($UI.btnInstall.Text -eq 'Continue') -and $UI.btnInstall.Enabled -and -not $UI.txtIso.Enabled -and -not (Own-Visible $UI.lnkGetLauncher))
                Test-ViewFit 'pick ticked'; Save-Shot '30_pick_ticked'
                $UI.chkLauncher.Checked = $false
                Test-Check 'pick, unticked: button says Install and waits for a checked disc, disc controls usable' (($UI.btnInstall.Text -eq 'Install') -and -not $UI.btnInstall.Enabled -and $UI.txtIso.Enabled -and $UI.btnBrowse.Enabled)
                Test-ViewFit 'pick unticked'; Save-Shot '31_pick_unticked'
                $UI.chkLauncher.Checked = $true; $UI.chkSteam.Checked = $true
                Test-ViewFit 'pick Steam ticked'; Save-Shot '32_pick_steam_ticked'
                $UI.chkSteam.Checked = $false
                $env:RAC_TEST_STEAM_DIR = $noSteam; Show-View 'pick'
                Test-Check 'pick, Steam not found: no Steam box' (-not (Own-Visible $UI.chkSteam) -and (Own-Visible $UI.chkLauncher))
                Test-ViewFit 'pick no Steam'; Save-Shot '33_pick_no_steam'
                $env:RAC_TEST_STEAM_DIR = $fake
                Set-SetupChoice 'launcher' 'no'; Show-View 'pick'
                Test-Check 'pick, remembered no: no box, the link "Install the RAC-RS Launcher instead", button says Install' ((-not (Own-Visible $UI.chkLauncher)) -and (Own-Visible $UI.lnkGetLauncher) -and ($UI.btnInstall.Text -eq 'Install'))
                Test-ViewFit 'pick remembered no'; Save-Shot '34_pick_remembered_no'
                Remove-Item -LiteralPath $choiceFile -Force
                $UI.chkLauncher.Checked = $true; Show-View 'pick'
                Set-FetchBusy $true
                Test-Check 'fetching: boxes and button disabled, text says getting the launcher' ((-not $UI.chkLauncher.Enabled) -and (-not $UI.btnInstall.Enabled) -and ($UI.lblPickText.Text -like 'Getting the RAC-RS Launcher*'))
                Test-ViewFit 'pick fetching'; Save-Shot '35_pick_fetching'
                Set-FetchBusy $false
                Show-View 'pick'
                Set-IsoPath (Join-Path $TestShot 'dummy.iso'); $UI.chkLauncher.Checked = $false
                [void](Wait-Until { -not $S.check } 60)
                Test-ViewFit 'pick unticked, bad disc'; Save-Shot '36_pick_unticked_bad_disc'
                Set-IsoPath ''; $UI.chkLauncher.Checked = $true
                $S.repair = $true; Show-View 'pick'
                Test-Check 'repair: neither launcher nor Steam box' ((-not (Own-Visible $UI.chkLauncher)) -and (-not (Own-Visible $UI.chkSteam)) -and ($UI.btnInstall.Text -eq 'Repair'))
                Test-ViewFit 'pick repair'; Save-Shot '37_pick_repair'
                $S.repair = $false
                # installed view
                Show-View 'installed'
                Test-Check 'installed, Steam found and not in it: "Install the launcher" button and the link "Add to Steam"' ((Own-Visible $UI.btnLauncher) -and (Own-Visible $UI.lnkSteam) -and ($UI.lnkSteam.Text -eq 'Add to Steam'))
                Test-ViewFit 'installed add'; Save-Shot '38_installed_add_steam'
                $null = Invoke-SteamShortcut 'add'; Show-View 'installed'
                Test-Check 'installed, our entry is in Steam: the link says Remove from Steam' ($UI.lnkSteam.Text -eq 'Remove from Steam')
                Test-ViewFit 'installed remove'; Save-Shot '39_installed_remove_steam'
                $env:RAC_TEST_STEAM_DIR = $noSteam; Show-View 'installed'
                Test-Check 'installed, Steam not found: no Steam link' (-not (Own-Visible $UI.lnkSteam))
                Test-ViewFit 'installed no Steam'; Save-Shot '40_installed_no_steam'
                $env:RAC_TEST_STEAM_DIR = $fake
                Set-Content -LiteralPath $launcherExe -Value 'x'
                Show-View 'installed'
                Test-Check 'installed, launcher present: no "Install the launcher" button' (-not (Own-Visible $UI.btnLauncher))
                Test-ViewFit 'installed with launcher'; Save-Shot '41_installed_launcher_present'
                Remove-Item -LiteralPath $launcherExe -Force
                # done view with the longest notes
                $S.shortcut = $true; $S.shortcutPath = ''; $S.steamNote = (Get-SteamResultText 'add' @{ Status = 'ok'; Errors = @(); Actions = @('added') })
                Show-View 'done'
                Test-ViewFit 'done shortcut + Steam note'; Save-Shot '42_done_notes'
                $S.steamNote = "Steam was still running, so it was not changed. Close Steam, then use `"Add to Steam`" again."
                Show-View 'done'
                Test-ViewFit 'done Steam running note'; Save-Shot '43_done_steam_running'
                $S.steamNote = (Get-SteamResultText 'add' @{ Status = 'ok'; Errors = @('C:\Program Files (x86)\Steam\userdata\123456789\config\shortcuts.vdf: Access to the path is denied.'); Actions = @('added') })
                Show-View 'done'
                Test-ViewFit 'done Steam error note'; Save-Shot '44_done_steam_error'
                $S.shortcut = $false; $S.steamNote = ''
                $isoGone = 'C:\no_such_folder\Ratchet and Clank.iso'
                [System.IO.File]::WriteAllText((Join-Path $root 'generated\play.cfg'), "iso=$isoGone`n")
                $S.steamNote = (Get-SteamResultText 'remove' @{ Status = 'ok'; Errors = @('C:\Program Files (x86)\Steam\userdata\123456789\config\shortcuts.vdf: Access to the path is denied.'); Actions = @('removed') })
                Show-View 'installed'
                Test-ViewFit 'installed, disc gone + Steam error note'; Save-Shot '45_installed_longest_note'
                Remove-Item -LiteralPath (Join-Path $root 'generated\play.cfg') -Force
                Stop-DiscCheck
                Write-Host ("launcher views finished, failures: {0}" -f $script:TestFails)
                if ($Mutex) { $Mutex.ReleaseMutex() }
                exit $script:TestFails
            }
            if ($TestChoice -eq 'dpiviews') {
                # every view with stub content at the scale given by -TestDpi / -TestScreen: the window must fit the screen, every control must be inside
                # its panel with its text not clipped. Nothing is installed or downloaded. Shots: dpi_*.png
                Write-Host ('  screen {0}x{1} (work area), display scale {2:N2} -> layout scale {3:N2}, window client {4}x{5} max {6}' -f $wa.Width, $wa.Height, $want, $LayoutK, $form.ClientSize.Width, $form.ClientSize.Height, $MaxClientH)
                function Test-Window([string]$tag) {
                    Test-Check "$tag : window $($form.Width)x$($form.Height) fits the work area $($wa.Width)x$($wa.Height)" (($form.Width -le $wa.Width) -and ($form.Height -le $wa.Height))
                }
                function Show-DpiView([string]$tag, [string]$view) {
                    Show-View $view; Test-Window $tag; Test-ViewFit $tag; Save-Shot ('dpi_' + $tag)
                }
                if (Test-Path -LiteralPath $choiceFile) { Remove-Item -LiteralPath $choiceFile -Force }
                if (Test-Path -LiteralPath $launcherExe) { Remove-Item -LiteralPath $launcherExe -Force }
                New-Item -ItemType Directory -Force -Path (Join-Path $root 'generated') | Out-Null
                $builtFile = Join-Path $root 'generated\built_version.txt'
                $bakBuilt = $null; if (Test-Path -LiteralPath $builtFile) { $bakBuilt = [System.IO.File]::ReadAllText($builtFile) }
                $playCfg = Join-Path $root 'generated\play.cfg'
                $bakCfg = $null; if (Test-Path -LiteralPath $playCfg) { $bakCfg = [System.IO.File]::ReadAllText($playCfg) }
                # first view
                $S.repair = $false; $UI.chkLauncher.Checked = $true; $UI.chkSteam.Checked = $true
                Show-DpiView '01_pick_launcher_ticked' 'pick'
                $UI.chkLauncher.Checked = $false
                Set-DiscStatus 'bad' 'This is not the right disc image. It must be Ratchet & Clank PAL / Europe v1.00 or USA v1.00, one .iso file of about 3.9 GB. This file is 4.0 KB. Other regions, other versions and Platinum / Greatest Hits re-releases do not work.'
                $UI.txtIso.Text = 'C:\Users\Someone\Games\Ratchet & Clank (Europe) (En,Fr,De,Es,It) (v1.00).iso'
                Show-DpiView '02_pick_unticked_bad_disc' 'pick'
                $S.repair = $true
                Show-DpiView '03_pick_repair' 'pick'
                $S.repair = $false; $UI.chkLauncher.Checked = $true
                # installing, then the details box
                Set-Mode $false
                Reset-InstallState; $S.toolsSub = 0.0; $S.vu = $false; $S.t0 = (Get-Date).AddMinutes(-7).AddSeconds(-12)
                Show-View 'install'
                foreach ($l in @('== (a) tools', '== (b) disc image', '== (c) generating game code', '== (c2) VU', '== (d) building the game')) { Feed-Line $l }
                foreach ($i in 1..237) { Feed-Line ('   Compiling crate-{0} v0.1.0' -f $i) }
                Feed-Line '   Compiling rec-l4-03 v0.1.0 (C:\games\rac-rs\oracle\rec\l4\rec-l4-03)'
                $UI.lblElapsed.Text = 'Elapsed: 07:12'; $UI.lblStatus.Text = $S.status; Update-Progress
                Test-Window '04_install'; Test-ViewFit '04_install'; Save-Shot 'dpi_04_install'
                Switch-Details
                Test-Window '05_install_details'; Test-ViewFit '05_install_details'; Save-Shot 'dpi_05_install_details'
                Switch-Details
                # the same for an update
                $S.newV = '0.4.1'; Set-Mode $true
                Reset-InstallState; $S.t0 = (Get-Date).AddMinutes(-1)
                Show-View 'install'; Feed-Line '== (u) updating'
                Update-Progress
                Test-Window '06_install_update'; Test-ViewFit '06_install_update'; Save-Shot 'dpi_06_install_update'
                # done after an install (shortcut note + Steam note), then the done view of an update
                Set-Mode $false; Reset-InstallState
                foreach ($l in @('== (e) play.cfg and Play.cmd', '== done', ('  desktop shortcut created: C:\Users\Someone\Desktop\' + $GameLnk))) { Feed-Line $l }
                $S.finished = $true; Set-Step 4; Update-Progress
                $S.steamNote = (Get-SteamResultText 'add' @{ Status = 'ok'; Errors = @(); Actions = @('added') })
                Show-DpiView '07_done_install' 'done'
                Test-Check '07 done: the support link is visible and says what it is' ((Own-Visible $UI.lnkSupport) -and ($UI.lnkSupport.Text -eq 'Support the project') -and (Own-Visible $UI.btnPlay))
                $S.steamNote = (Get-SteamResultText 'add' @{ Status = 'ok'; Errors = @('C:\Program Files (x86)\Steam\userdata\123456789\config\shortcuts.vdf: Access to the path is denied.'); Actions = @('added') })
                Show-DpiView '08_done_install_longest_note' 'done'
                $S.steamNote = ''
                $S.newV = '0.4.1'; Set-Mode $true; Reset-InstallState
                $S.finished = $true; Set-Step 4; Update-Progress
                Show-DpiView '09_done_update' 'done'
                Test-Check '09 done (update): the support link is visible' (Own-Visible $UI.lnkSupport)
                Set-Mode $false; Reset-InstallState
                # installed (home) view, and the "Finish the update" variant of it
                [System.IO.File]::WriteAllText($playCfg, "iso=C:\no_such_folder\Ratchet and Clank.iso`n")
                Show-DpiView '10_installed_disc_gone' 'installed'
                Test-Check '10 installed: no support link' (-not (Own-Visible $UI.lnkSupport))
                [System.IO.File]::WriteAllText($playCfg, "iso=$($MyInvocation.MyCommand.Path)`n")
                [System.IO.File]::WriteAllText($builtFile, '0.3.1')
                Show-DpiView '11_finish_the_update' 'installed'
                Test-Check '11 finish the update: the button says Update' ($UI.btnPlay.Text -eq 'Update')
                if ($null -ne $bakBuilt) { [System.IO.File]::WriteAllText($builtFile, $bakBuilt) } else { Remove-Item -LiteralPath $builtFile -Force }
                Show-DpiView '12_installed' 'installed'
                # error (install and update), with a long log
                Reset-InstallState
                foreach ($l in @('== (a) tools', '== (b) disc image', '== (c) generating game code', '== (d) building the game')) { Feed-Line $l }
                foreach ($i in 1..40) { Feed-Line ('   Compiling crate-{0} v0.1.0' -f $i) }
                Feed-Line 'error[E0463]: can not find crate for `core` (a long line that makes the horizontal scroll bar of the log box appear: 0123456789 0123456789 0123456789 0123456789 0123456789)'
                $S.lastError = 'could not compile `rec-l4-03` (lib) due to 1 previous error'
                Set-Step 3 $true; Show-Error
                Test-Window '13_error'; Test-ViewFit '13_error'; Save-Shot 'dpi_13_error'
                Test-Check '13 error: no support link anywhere on the error view' (-not (Own-Visible $UI.lnkSupport))
                $S.newV = '0.4.1'; Set-Mode $true; Show-Error
                Test-Window '14_error_update'; Test-ViewFit '14_error_update'; Save-Shot 'dpi_14_error_update'
                Set-Mode $false
                # update offer (shown by the game exe)
                $UI.lblOfferTitle.Text = 'Update available: v0.4.0 ' + [string][char]0x2192 + ' v0.4.1'
                $UI.lblOfferNote.Text = 'This takes about 2 to 10 minutes. Your saves and settings are kept.'
                $UI.txtChanges.Text = (@(1..14 | ForEach-Object { "- Change number $_ : something got better in the game" }) -join "`r`n")
                Show-DpiView '15_offer' 'offer'
                $S.finish = $true
                $UI.lblOfferTitle.Text = 'Finish the update to v0.4.1'
                $UI.lblOfferNote.Text = 'An earlier update was not finished. It takes about 2 to 10 minutes. Your saves and settings are kept.'
                Show-DpiView '16_offer_finish' 'offer'
                $S.finish = $false
                # restore what the test changed in the folder
                if ($null -ne $bakCfg) { [System.IO.File]::WriteAllText($playCfg, $bakCfg) } else { Remove-Item -LiteralPath $playCfg -Force -ErrorAction SilentlyContinue }
                Stop-DiscCheck
                Write-Host ("dpi views finished (display scale {0:N2}, layout scale {1:N2}), failures: {2}" -f $want, $LayoutK, $script:TestFails)
                if ($Mutex) { $Mutex.ReleaseMutex() }
                exit $script:TestFails
            }
            # ---- the two click paths of the first view
            if (Test-Path -LiteralPath $choiceFile) { Remove-Item -LiteralPath $choiceFile -Force }
            $S.repair = $false; $S.discOk = $true; $S.discPath = $TestIso
            $UI.chkShortcut.Checked = $true
            Show-View 'pick'
            if ($TestChoice -eq 'flow_unticked') {
                $UI.chkLauncher.Checked = $false; $UI.chkSteam.Checked = $true
                Test-Check 'unticked: Install is enabled with the test disc' ($UI.btnInstall.Enabled -and ($UI.btnInstall.Text -eq 'Install'))
                Invoke-InstallClick
                Test-Check 'unticked: the choice "no" is written at once' ((Get-SetupChoice 'launcher') -eq 'no')
                [void](Wait-Until { -not $S.proc } 900)
                Write-Host ('  worker exit code {0}; view {1}; steam note: {2}' -f $S.workerCode, $S.view, ($S.steamNote -replace "`r`n", ' | '))
                Test-Check 'unticked: the old install ran (worker exit 0) and the done view is shown' (($S.workerCode -eq 0) -and ($S.view -eq 'done'))
                Test-Check 'unticked: no launcher exe was fetched' (-not (Test-Path -LiteralPath $launcherExe))
                Test-Check 'unticked: Steam was added AFTER the install (entry in the fake Steam, note says so)' ((Get-SteamState).Has -and ($S.steamNote -like 'Added*'))
                Test-Check 'unticked: the done view note names the Steam result' ($UI.lblDoneNote.Text -like '*Steam*')
                Test-ViewFit 'flow_unticked done'; Save-Shot '50_flow_unticked_done'
            } else {
                $UI.chkLauncher.Checked = $true; $UI.chkSteam.Checked = $true
                Test-Check 'ticked: the button says Continue' ($UI.btnInstall.Text -eq 'Continue')
                Invoke-InstallClick
                Test-Check 'ticked: the click starts the hidden fetch and greys the window' ($S.fetch -and (-not $UI.btnInstall.Enabled))
                [void](Wait-Until { $S.handedOver } 120)
                Write-Host ('  handed over: {0}; fetch error: "{1}"; steam note: {2}' -f $S.handedOver, $S.fetchErr, ($S.steamNote -replace "`r`n", ' | '))
                Test-Check 'ticked: the launcher exe is in place' (Test-Path -LiteralPath $launcherExe -PathType Leaf)
                Test-Check 'ticked: the choice "yes" is written' ((Get-SetupChoice 'launcher') -eq 'yes')
                $lnkPath = Join-Path $S.shortcutDir $GameLnk
                $tgt = $(if (Test-Path -LiteralPath $lnkPath) { ((New-Object -ComObject WScript.Shell).CreateShortcut($lnkPath)).TargetPath } else { '' })
                Test-Check 'ticked: the shortcut opens the launcher' ($tgt -eq $launcherExe)
                Test-Check 'ticked: Steam was added at the hand-over' ((Get-SteamState).Has -and ($S.steamNote -like 'Added*'))
                Test-Check 'ticked: no install worker ran (no game build started)' (-not $S.proc -and ($S.workerCode -eq 0))
                Test-Check 'ticked: handed over (in test mode the launcher is not started and the window not closed)' ($S.handedOver)
            }
            Write-Host ("flow test finished, failures: {0}" -f $script:TestFails)
            Stop-DiscCheck
            if ($Mutex) { $Mutex.ReleaseMutex() }
            exit $script:TestFails
        }
        Write-Host "launcher test (nothing is shown); shots in $TestShot"

        Test-Check 'first view matches the installed check' (($S.view -eq 'installed') -eq (Test-Installed))
        Save-Shot '01_first_view'
        # play.cfg and the game start (the game is NOT launched)
        if ($TestIso) {
            Write-PlayCfg $TestIso
            $b = [System.IO.File]::ReadAllBytes((Join-Path $root 'generated\play.cfg'))
            Test-Check 'play.cfg is UTF-8 without BOM, iso= line' (($b[0] -ne 0xEF) -and ([System.Text.Encoding]::UTF8.GetString($b) -eq ("iso=$TestIso`n")))
            Test-Check 'installed check passes once play.cfg exists' (Test-Installed)
            $psi = Get-GamePsi
            Write-Host ('  game: {0} [args: "{1}"] cwd={2}' -f $psi.FileName, $psi.Arguments, $psi.WorkingDirectory)
            Test-Check 'game start: exe, no arguments, no console, working dir = root' (($psi.FileName -like '*\build\release\rac-rs.exe') -and ($psi.Arguments -eq '') -and $psi.CreateNoWindow -and -not $psi.UseShellExecute -and ($psi.WorkingDirectory -eq $root))
            Test-Check 'iso read back from play.cfg' ((Get-InstalledIso) -eq $TestIso)
        }
        # a stale shortcut with our name that points to another exe (like an old dev shortcut) must be replaced
        $lnk = Join-Path $S.shortcutDir $GameLnk
        $old = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
        $old.TargetPath = Join-Path $env:SystemRoot 'System32\notepad.exe'; $old.Arguments = '--old-dev-arg'; $old.WorkingDirectory = $env:SystemRoot; $old.Save()
        Test-Check 'stale shortcut prepared (points elsewhere, has arguments)' (((New-Object -ComObject WScript.Shell).CreateShortcut($lnk)).Arguments -eq '--old-dev-arg')
        $lnk = New-GameShortcut $S.shortcutDir
        $sc = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
        Write-Host ('  shortcut: {0} [args: "{1}"] cwd={2} icon={3}' -f $sc.TargetPath, $sc.Arguments, $sc.WorkingDirectory, $sc.IconLocation)
        $icoShipped = (Get-IconFile).Length -gt 0
        $icoUsed = $sc.IconLocation -like '*rac-rs.ico*'
        Write-Host ("  icon shipped={0} used={1}; target ok={2} args empty={3}" -f $icoShipped, $icoUsed, ($sc.TargetPath -like '*\build\release\rac-rs.exe'), ($sc.Arguments -eq ''))
        Test-Check 'shortcut: the exe, no arguments, icon is the .ico when shipped' (($sc.TargetPath -like '*\build\release\rac-rs.exe') -and ($sc.Arguments -eq '') -and ($icoShipped -eq $icoUsed))
        Test-Check 'shortcut written to the test folder (not the desktop)' (Test-Path -LiteralPath $lnk)

        # picker: empty, then a dummy small file, then the real disc
        $S.repair = $false
        Show-View 'pick'; Set-IsoPath ''
        Save-Shot '02_pick_empty'
        $dummy = Join-Path $TestShot 'dummy.iso'
        [System.IO.File]::WriteAllBytes($dummy, (New-Object byte[] 4096))
        Set-IsoPath $dummy
        [void](Wait-Until { -not $S.check } 60)
        Test-Check 'dummy file: red error, Install stays disabled' ((-not $UI.btnInstall.Enabled) -and ($UI.lblDisc.ForeColor -eq $cRed))
        Write-Host ('  message: ' + $UI.lblDisc.Text)
        Save-Shot '03_pick_bad'
        Set-IsoPath (Join-Path $TestShot 'does_not_exist.iso')
        [void](Wait-Until { -not $S.check } 60)
        Test-Check 'missing file: red error' ((-not $UI.btnInstall.Enabled) -and ($UI.lblDisc.ForeColor -eq $cRed))
        if ($TestIso) {
            Set-IsoPath $TestIso
            [void](Wait-Until { $S.check -and $S.check.Sync.pos -gt 300MB } 120)
            Save-Shot '04_pick_checking'
            [void](Wait-Until { -not $S.check } 300)
            Test-Check 'real disc: Disc OK and Install enabled' ($UI.btnInstall.Enabled -and ($UI.lblDisc.ForeColor -eq $cGreen))
            Save-Shot '05_pick_ok'
        }

        # installing view with a simulated log (the bar, the ticks and the details box)
        Reset-InstallState; $S.toolsSub = 0.0; $S.vu = $false; $S.t0 = (Get-Date).AddMinutes(-7).AddSeconds(-12)
        Show-View 'install'
        foreach ($l in @('== (a) tools', '== (b) disc image', '== (c) generating game code', '== (c2) VU', '== (d) building the game')) { Feed-Line $l }
        foreach ($i in 1..237) { Feed-Line ('   Compiling crate-{0} v0.1.0' -f $i) }
        Feed-Line '   Compiling rec-l4-03 v0.1.0 (...)'
        $UI.lblElapsed.Text = 'Elapsed: ' + (Format-Elapsed ((Get-Date) - $S.t0)); $UI.lblStatus.Text = $S.status; Update-Progress
        Test-Check 'simulated build: step 4 active, bar between 30 and 98' (($S.step -eq 3) -and ($UI.pb.Tag -gt 30) -and ($UI.pb.Tag -lt 98))
        Save-Shot '06_installing'
        Switch-Details
        Save-Shot '07_installing_details'
        foreach ($i in 238..2000) { Feed-Line ('   Compiling crate-{0} v0.1.0' -f $i) }
        Update-Progress
        Test-Check 'never above 98 percent before the end' ($UI.pb.Tag -le 98)

        # a real run in this (already built) folder: a dummy file as disc -> the error view; the real disc -> the done view
        Reset-InstallState
        Start-Install $dummy $false $false
        [void](Wait-Until { -not $S.proc } 600)
        Test-Check 'wrong disc through the installer: error view' ($S.view -eq 'error')
        Write-Host ('  error text: ' + ($UI.lblErrMsg.Text -replace "`r`n", ' | '))
        Save-Shot '08_error'
        $d = Get-DetailsText
        Set-Content -LiteralPath (Join-Path $TestShot 'copy_details.txt') -Value $d -Encoding UTF8
        Test-Check 'copy details text built' ($d.Length -gt 50)
        if ($TestIso -and $TestInstall) {
            Start-Install $TestIso $false $false
            [void](Wait-Until { $S.step -ge 3 } 600)
            Save-Shot '09_installing_real'
            [void](Wait-Until { -not $S.proc } 1500)
            Test-Check 'real run (everything already built): done view' ($S.view -eq 'done')
            Save-Shot '10_done'
        }
        # done view as after a successful run (simulated log lines, as the worker prints them)
        Reset-InstallState
        foreach ($l in @('== (e) play.cfg and Play.cmd', '== done', ('  desktop shortcut created: ' + $lnk))) { Feed-Line $l }
        $S.finished = $true; Set-Step 4; Update-Progress
        Show-View 'done'
        Test-Check 'done view names the shortcut' ($UI.lblDoneNote.Text -like '*shortcut*')
        Write-Host ('  done note: ' + $UI.lblDoneNote.Text)
        Save-Shot '10_done'
        Show-View 'installed'
        Save-Shot '11_installed_again'
        Stop-DiscCheck
        Stop-Install
        Write-Host ("launcher test finished, failures: {0}" -f $script:TestFails)
        if ($Mutex) { $Mutex.ReleaseMutex() }
        exit $script:TestFails
    }

    Start-UpdateLook
    try {
        [System.Windows.Forms.Application]::Run($form)
    } catch {
        try { [void][System.Windows.Forms.MessageBox]::Show("The launcher stopped unexpectedly:`r`n`r`n" + $_.Exception.Message + "`r`n`r`nYou can use Setup.cmd -Console instead.", $AppTitle, 'OK', 'Error') } catch { }
        Stop-Install
        exit 1
    }
    if ($Mutex) { try { $Mutex.ReleaseMutex() } catch { } }
    exit $S.exitCode
}


# Files from a downloaded ZIP carry the 'from the internet' mark; clear it so the scripts and build run without prompts.
if ($ApplyUpdate) {
    # Worker of the update window. Downloads the new version, copies it over this install (Sync-Update), then runs the normal steps below with
    # the NEW setup.ps1 as a fresh process (the tools, stamps and build of the new version decide what is redone).
    Step "(u) updating to v$NewVersion"
    if (-not $FinishOnly) {
        $utmp = Join-Path ([System.IO.Path]::GetTempPath()) "rac-rs-update\$NewVersion"
        try {
            if (Test-Path -LiteralPath $utmp) { Remove-Item -LiteralPath $utmp -Recurse -Force }
            New-Item -ItemType Directory -Force -Path $utmp | Out-Null
            Write-Host "  downloading v$NewVersion ..."
            Get-UpdateZip $NewVersion (Join-Path $utmp 'update.zip')
            Write-Host '  unpacking ...'
            Expand-UpdateZip (Join-Path $utmp 'update.zip') (Join-Path $utmp 'x')
        } catch {
            Write-Host "ERROR: could not download the update: $($_.Exception.Message)" -ForegroundColor Red
            Remove-Item -LiteralPath $utmp -Recurse -Force -ErrorAction SilentlyContinue
            exit 3
        }
        try {
            $got = ([System.IO.File]::ReadAllText((Join-Path $utmp 'x\VERSION'))).Trim()
            if ($got -notmatch '^\d+\.\d+\.\d+(\.\d+)?$' -or (Compare-Version $got (Get-LocalVersion)) -le 0) { throw "the downloaded version ($got) is not newer than this one ($(Get-LocalVersion))" }
            Write-Host "  applying v$got ..."
            $res = Sync-Update (Join-Path $utmp 'x') $root
        } catch {
            Write-Host "ERROR: could not apply the update: $($_.Exception.Message)" -ForegroundColor Red
            Remove-Item -LiteralPath $utmp -Recurse -Force -ErrorAction SilentlyContinue
            exit 1
        }
        Remove-Item -LiteralPath $utmp -Recurse -Force -ErrorAction SilentlyContinue
        $ch = @($res.Changed)
        # compiles to expect (progress bar): rac-rs only: the game library, the two build-order crates and the exe; the
        # generated crates depend on oracle/core (ee-core) only, so they rebuild when it or the generators change
        $exp = 4
        if (@($ch | Where-Object { $_ -like 'oracle/core/*' -or $_ -like 'tools/*' }).Count -gt 0) { $exp = 563 }
        elseif (@($ch | Where-Object { $_ -like 'oracle/src/*' -or $_ -eq 'oracle/Cargo.toml' }).Count -gt 0) { $exp = 8 }
        elseif (@($ch | Where-Object { $_ -like 'oracle/hand/*' }).Count -gt 0) { $exp = 3 }
        Write-Host "  $($ch.Count) files updated, $(@($res.Removed).Count) removed"
        Write-Host "expect-compiles: $exp"
    } else {
        Write-Host '  the files are already updated; only the rebuild is left'
        Write-Host 'expect-compiles: 563'
    }
    # Files that were not copied by Sync-Update (no generated\synced_<version>: a ZIP unzipped over the folder) carry the ZIP's times: stamp them, or cargo may keep the old game.
    $syncMark = Join-Path $root ('generated\synced_' + (Get-LocalVersion))
    if (-not (Test-Path -LiteralPath $syncMark -PathType Leaf)) {
        $nst = Set-ReleaseFileTimes
        Write-Host "  the files were put in place by hand: set the time of $nst files to now, so that everything is rebuilt once"
        New-Item -ItemType Directory -Force -Path (Join-Path $root 'generated') | Out-Null
        [System.IO.File]::WriteAllText($syncMark, ((Get-LocalVersion) + "`n"), (New-Object System.Text.UTF8Encoding($false)))
    }
    $a2 = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $root 'setup.ps1'), '-Console', '-Iso', $Iso, '-SkipHash', '-SkipUnblock')
    if ($NoShortcut) { $a2 += '-NoShortcut' }
    if ($ShortcutDir) { $a2 += @('-ShortcutDir', $ShortcutDir) }
    & (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') @a2
    exit $LASTEXITCODE
}

if (-not $SkipUnblock) { try { Get-ChildItem -LiteralPath $root -Recurse -File -ErrorAction SilentlyContinue | Unblock-File -ErrorAction SilentlyContinue } catch { } }

# ---------------------------------------------------------------- (a) tools
# A private, pinned copy of Rust, Python and the GNU assembler is downloaded INTO this folder (.\toolchain, about 1.2 GB on disk): nothing is installed
# on the PC, no admin rights, no PATH or registry change. Rust uses the GNU host (x86_64-pc-windows-gnu): its linker ships with
# Rust itself, so the Microsoft C++ build tools are not needed. Python is the official embeddable package from python.org.
# Rust or Python already installed on the PC are not used and not touched. Running setup again reuses .\toolchain (no new download).
# On a drive that cannot make hard links (exFAT, FAT32, some network shares) rustup cannot work (its cargo.exe / rustc.exe are hard links
# to rustup.exe): there the same pinned Rust is unpacked from its official packages instead, still inside .\toolchain (see Rust below).
Step '(a) tools'
$RustVersion = '1.99.0'
$RustHost = 'x86_64-pc-windows-gnu'
$RustupVersion = '1.29.1'
$RustupUrl = "https://static.rust-lang.org/rustup/archive/$RustupVersion/$RustHost/rustup-init.exe"
$RustupSha256 = '6D5B5709ADDC0122C916D8C810DA8D8A7B086A5D64FA805EF404D506392AADC8'
$PyVersion = '3.14.8'
$PyUrl = "https://www.python.org/ftp/python/$PyVersion/python-$PyVersion-embed-amd64.zip"
$PySha256 = 'A93ABE456AB01BD96D7A085B3CDB6566B3063F4241360D114142FBDB07F0A310'
$tc = Join-Path $root 'toolchain'
$tcDl = Join-Path $tc 'downloads'
$t = Get-Date

# Rust without rustup, for a drive that cannot make hard links. rustup makes cargo.exe, rustc.exe, ... as hard links to rustup.exe and stops
# there with "could not create link ... Incorrect function" (os error 1). So the four packages of rustup's "minimal" profile (rustc, cargo,
# rust-std, rust-mingw: the hashes are those of the release manifest https://static.rust-lang.org/dist/channel-rust-1.99.0.toml, whose own
# SHA-256 is CE6DDDC886364F8D786514771212CEBE9B731BA82D6B859951C6B0CCC516B6A2) are downloaded and unpacked by tools\rust_unpack.py into the
# folder rustup would have made, toolchain\rustup\toolchains\<version>-<host> (so the linker folder below is the same), and cargo.exe /
# rustc.exe are run from its bin\ directly. Chosen when -RustNoLinks is given, when toolchain\rust_direct.txt exists (an earlier setup of
# this folder did it) or when a test hard link in toolchain\ fails; any other drive gets rustup, exactly as before.
$RustDistUrl = 'https://static.rust-lang.org/dist/2026-10-01'
$RustPkgs = [ordered]@{
    "rustc-$RustVersion-$RustHost.tar.xz"      = '015F770C3CA0B3226A24FE159F30B03782A91679BA16159F7D83550F768FB795'
    "cargo-$RustVersion-$RustHost.tar.xz"      = '36DC7CBA4BE341C5A6CB3E10F660C4565B6F87F318D3CB87519B2CFE8C630BCA'
    "rust-std-$RustVersion-$RustHost.tar.xz"   = 'A2382BF189D5621E09DB647DA419A8E1D4C604D60941CB116DA00CE34042B1DD'
    "rust-mingw-$RustVersion-$RustHost.tar.xz" = 'AD2B4B41362A99A86E20A8DBCEC4BE81B99F2D507F6408A72C7A139BC1919080'
}
$RustDirectMark = Join-Path $tc 'rust_direct.txt'
function Test-HardLinks([string]$dir) {
    # $true when a hard link can be made in $dir (NTFS, ReFS); $false on exFAT, FAT32 and shares that cannot
    $a = $dir.TrimEnd('\') + '\linktest_a.tmp'
    $b = $dir.TrimEnd('\') + '\linktest_b.tmp'
    try {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        Remove-Item -LiteralPath $a, $b -Force -ErrorAction SilentlyContinue
        [System.IO.File]::WriteAllText($a, 'x')
        New-Item -ItemType HardLink -Path $b -Value $a -ErrorAction Stop | Out-Null
        return (Test-Path -LiteralPath $b -PathType Leaf)
    } catch { return $false } finally { Remove-Item -LiteralPath $a, $b -Force -ErrorAction SilentlyContinue }
}
$RustDirect = [bool]$RustNoLinks -or (Test-Path -LiteralPath $RustDirectMark -PathType Leaf)
if (-not $RustDirect -and -not (Test-HardLinks $tc)) {
    $RustDirect = $true
    Write-Host '  this drive cannot make hard links (exFAT / FAT32 / network share?): Rust is installed without rustup'
}
$rustHome = Join-Path $tc "rustup\toolchains\$RustVersion-$RustHost"      # where rustup puts the toolchain; with $RustDirect rust_unpack.py does

# Only this folder's toolchain is used: its own rustup/cargo homes, the pinned Rust version, and no build settings from the PC
# (they could change the build or its speed).
$env:RUSTUP_HOME = Join-Path $tc 'rustup'
$env:CARGO_HOME = Join-Path $tc 'cargo'
$env:RUSTUP_TOOLCHAIN = "$RustVersion-$RustHost"
$env:Path = (Join-Path $tc 'cargo\bin') + ';' + $env:Path
foreach ($v in @('RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET', 'CARGO_BUILD_RUSTFLAGS',
                  'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_TARGET_DIR', 'CARGO_PROFILE_RELEASE_OPT_LEVEL', 'CARGO_PROFILE_RELEASE_LTO', 'RUSTUP_DIST_SERVER')) {
    Remove-Item -LiteralPath "Env:$v" -ErrorAction SilentlyContinue
}
if ($RustDirect) {
    # no rustup proxies: cargo and rustc are the real files; cargo is told which rustc to run (no rustup to ask)
    $env:Path = (Join-Path $rustHome 'bin') + ';' + $env:Path
    $env:RUSTC = Join-Path $rustHome 'bin\rustc.exe'
}

function Get-Pinned($url, $file, $sha) {
    # Downloads $url to $file and checks its SHA-256; up to 3 tries.
    if ((Test-Path -LiteralPath $file) -and (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -eq $sha) { return }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $file) | Out-Null
    try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch { }
    $part = "$file.part"
    for ($try = 1; $try -le 3; $try++) {
        try {
            $old = $ProgressPreference; $ProgressPreference = 'SilentlyContinue'     # the progress bar makes Invoke-WebRequest very slow
            try { Invoke-WebRequest -Uri $url -OutFile $part -UseBasicParsing } finally { $ProgressPreference = $old }
            $h = (Get-FileHash -LiteralPath $part -Algorithm SHA256).Hash
            if ($h -eq $sha) { Move-Item -LiteralPath $part -Destination $file -Force; return }
            Write-Host "  download of $url has the wrong SHA-256 ($h), retrying" -ForegroundColor Yellow
        } catch {
            Write-Host "  download of $url failed ($($_.Exception.Message)), retrying" -ForegroundColor Yellow
        }
        Remove-Item -LiteralPath $part -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds (5 * $try)
    }
    Fail "could not download $url (check the internet connection, then run Setup.cmd again)"
}

# DirectX shader compiler (DXC) for the DirectX 12 renderer option (Options -> Graphics -> Screen -> Renderer). The game looks for
# dxcompiler.dll next to rac-rs.exe and falls back to the built-in FXC (about 7 s slower first start) without it, so a failed
# download here is only a warning, never a failed setup. Pinned release zip from Microsoft; only bin\x64\dxcompiler.dll is taken.
# Licence texts: NOTICE_dxc.md. Runs at the end of every setup/update/repair build and refetches the DLL when it is missing or differs.
$DxcZipUrl = 'https://github.com/microsoft/DirectXShaderCompiler/releases/download/v1.9.2609/dxc_2026_09_29.zip'
$DxcZipSha256 = 'AD31B1FC8443175D204F77A611FDB3EF2EC42759BDC2F1167368DE24A4A7E7F1'
$DxcDllSha256 = '0689E2FEE0A1C9DF8662A59680469F24E2A3AD7CE6BBBCC9284F7A83172BFC59'
function Install-Dxc([string]$exeDir) {
    if ($NoDxc) { Write-Host '  DirectX shader compiler: not fetched (-NoDxc)'; return }
    $dll = Join-Path $exeDir 'dxcompiler.dll'
    $tmp = Join-Path $exeDir 'dxc_download.tmp'
    try {
        if ((Test-Path -LiteralPath $dll -PathType Leaf) -and ((Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash -eq $DxcDllSha256)) {
            Write-Host '  DirectX shader compiler: dxcompiler.dll is already there'; return
        }
        Write-Host '  DirectX shader compiler: downloading dxcompiler.dll (Microsoft DXC v1.9.2609, about 32 MB) for the DirectX 12 option'
        try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch { }
        $zip = "$tmp.zip"
        $old = $ProgressPreference; $ProgressPreference = 'SilentlyContinue'
        try { Invoke-WebRequest -Uri $DxcZipUrl -OutFile $zip -UseBasicParsing -TimeoutSec 300 } finally { $ProgressPreference = $old }
        $h = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
        if ($h -ne $DxcZipSha256) { throw "wrong SHA-256 $h" }
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $za = [System.IO.Compression.ZipFile]::OpenRead($zip)
        try {
            # (this archive stores its names with backslashes; accept either slash)
            $entry = $za.Entries | Where-Object { $_.FullName.Replace('\', '/') -eq 'bin/x64/dxcompiler.dll' } | Select-Object -First 1
            if (-not $entry) { throw 'bin/x64/dxcompiler.dll is not in the archive' }
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $tmp, $true)
        } finally { $za.Dispose() }
        if ((Get-FileHash -LiteralPath $tmp -Algorithm SHA256).Hash -ne $DxcDllSha256) { throw 'dxcompiler.dll has an unexpected SHA-256' }
        Move-Item -LiteralPath $tmp -Destination $dll -Force
        Write-Host '  DirectX shader compiler: dxcompiler.dll installed next to rac-rs.exe'
    } catch {
        Write-Host "  DirectX shader compiler: could not get dxcompiler.dll ($($_.Exception.Message)); the DirectX 12 option still works, with a slower first start" -ForegroundColor Yellow
    } finally {
        Remove-Item -LiteralPath $tmp, "$tmp.zip" -Force -ErrorAction SilentlyContinue
    }
}

# Things that come with the game (since v0.4.0), in bundled\: omniloader.dll = the OmniLoader mod loader (our own MIT/Apache code, built
# with MSVC and the static CRT, no game data), copied next to rac-rs.exe (without it the game simply runs without script mods); and
# bundled\mods\<folder> = mods that come preinstalled (Arsenal Mastery), copied into mods\<folder> when the mod is missing or an older
# version is installed. A mod that is installed with the same or a higher version (the launcher's mod update, or the player's own copy) is
# never touched or downgraded; an older one is first moved whole to <root>\mod_backups\<folder>-<version> (outside mods\, where the game
# would load it as a second copy). Nothing is switched on here: a mod runs only when the player turns it on (launcher Mods page or
# title-screen Options > Mods, which write mods\active.txt); mods\active.txt, mods\settings.json and the mods' save data (player\omni) are
# never touched. Runs at the end of every setup/update/repair build. A problem is a warning, never a failed setup.

# Mod versions: MAJOR.MINOR.PATCH with an optional -suffix, a leading v is dropped (the rules of the mod loader and of the launcher's
# updater). Gives $null for anything else, else an object with Num (3 numbers) and Pre (the suffix or $null).
function ConvertTo-ModVersion([string]$text) {
    if (-not $text) { return $null }
    $m = [regex]::Match($text.Trim(), '^[vV]?(0|[1-9][0-9]{0,17})\.(0|[1-9][0-9]{0,17})\.(0|[1-9][0-9]{0,17})(?:-([A-Za-z0-9.-]+))?$')
    if (-not $m.Success) { return $null }
    $pre = $null
    if ($m.Groups[4].Success) { $pre = $m.Groups[4].Value }
    return [pscustomobject]@{ Num = @([long]$m.Groups[1].Value, [long]$m.Groups[2].Value, [long]$m.Groups[3].Value); Pre = $pre }
}

# -1 / 0 / 1 when version a is below / equal to / above version b; $null when either is not a version. A suffixed version is below the
# plain one; suffix parts split at dots compare as numbers when both are numbers, numbers sit below words.
function Compare-ModVersion([string]$a, [string]$b) {
    $x = ConvertTo-ModVersion $a
    $y = ConvertTo-ModVersion $b
    if (($null -eq $x) -or ($null -eq $y)) { return $null }
    for ($i = 0; $i -lt 3; $i++) {
        if ($x.Num[$i] -lt $y.Num[$i]) { return -1 }
        if ($x.Num[$i] -gt $y.Num[$i]) { return 1 }
    }
    if (($null -eq $x.Pre) -and ($null -eq $y.Pre)) { return 0 }
    if ($null -eq $x.Pre) { return 1 }
    if ($null -eq $y.Pre) { return -1 }
    $p = $x.Pre.Split('.')
    $q = $y.Pre.Split('.')
    for ($i = 0; $i -lt [Math]::Max($p.Length, $q.Length); $i++) {
        if ($i -ge $p.Length) { return -1 }
        if ($i -ge $q.Length) { return 1 }
        $pn = $p[$i] -match '^(0|[1-9][0-9]{0,17})$'
        $qn = $q[$i] -match '^(0|[1-9][0-9]{0,17})$'
        if ($pn -and $qn) {
            if ([long]$p[$i] -lt [long]$q[$i]) { return -1 }
            if ([long]$p[$i] -gt [long]$q[$i]) { return 1 }
        } elseif ($pn) { return -1 }
        elseif ($qn) { return 1 }
        else {
            $c = [string]::CompareOrdinal($p[$i], $q[$i])
            if ($c -lt 0) { return -1 }
            if ($c -gt 0) { return 1 }
        }
    }
    return 0
}

# id and version of the mod in folder $dir (omni-mod.json, else openrac-mod.json); $null when there is none or it cannot be read.
function Read-ModManifest([string]$dir) {
    foreach ($f in @('omni-mod.json', 'openrac-mod.json')) {
        $p = Join-Path $dir $f
        if (-not (Test-Path -LiteralPath $p -PathType Leaf)) { continue }
        try {
            $j = (Get-Content -LiteralPath $p -Raw -Encoding UTF8) | ConvertFrom-Json
            return [pscustomobject]@{ Id = [string]$j.id; Version = [string]$j.version }
        } catch { return $null }
    }
    return $null
}

# One folder of bundled\mods into mods\: fresh copy, replacement of an older installed version (with a backup), or nothing.
function Install-BundledMod([System.IO.DirectoryInfo]$m) {
    $modsDir = Join-Path $root 'mods'
    $new = Read-ModManifest $m.FullName
    $newId = ''
    $newVer = ''
    if ($new) { $newId = $new.Id; $newVer = $new.Version }
    # the installed copy: the folder of the same name, unless it holds another mod; else the folder whose manifest has the same id
    # (the launcher or the player may have put it under another name; a second copy would be a duplicate mod id in the game)
    $have = $null
    $haveMf = $null
    $same = Join-Path $modsDir $m.Name
    if (Test-Path -LiteralPath $same -PathType Container) {
        $have = Get-Item -LiteralPath $same
        $haveMf = Read-ModManifest $have.FullName
    }
    if ($newId -and (Test-Path -LiteralPath $modsDir -PathType Container) -and -not ($haveMf -and ($haveMf.Id -eq $newId))) {
        foreach ($d in @(Get-ChildItem -LiteralPath $modsDir -Directory)) {
            if ($have -and ($d.FullName -eq $have.FullName)) { continue }
            $mf = Read-ModManifest $d.FullName
            if ($mf -and ($mf.Id -eq $newId)) { $have = $d; $haveMf = $mf; break }
        }
    }
    if (-not $have) {
        & robocopy $m.FullName $same /E /COPY:DAT /DCOPY:T /R:1 /W:1 /NP /NFL /NDL /NJH /NJS | Out-Null
        if ($LASTEXITCODE -ge 8) { throw "could not copy the mod $($m.Name) (robocopy $LASTEXITCODE)" }
        Write-Host "  bundled mod: $($m.Name) in mods\$($m.Name) (off until you turn it on)"
        return
    }
    # a folder of that name that holds another mod is not ours to replace
    if ($newId -and $haveMf -and $haveMf.Id -and ($haveMf.Id -ne $newId)) {
        Write-Host "  bundled mod: $($m.Name) not copied (mods\$($have.Name) holds another mod, '$($haveMf.Id)')" -ForegroundColor Yellow
        return
    }
    $haveVer = ''
    if ($haveMf) { $haveVer = $haveMf.Version }
    # never replace what is not clearly older: an equal or higher version, or a bundled version that cannot be read
    $cmp = Compare-ModVersion $haveVer $newVer
    if ($null -ne $cmp) {
        if ($cmp -ge 0) {
            Write-Host "  bundled mod: $($m.Name) $newVer not copied (mods\$($have.Name) has $haveVer, which is the same or newer)"
            return
        }
    } elseif ($null -eq (ConvertTo-ModVersion $newVer)) {
        Write-Host "  bundled mod: $($m.Name) not copied (its version '$newVer' cannot be read, so mods\$($have.Name) is left as it is)" -ForegroundColor Yellow
        return
    }
    # older (or an installed version that cannot be read): move it whole to mod_backups, then copy the bundled one in its place
    $tag = 'unknown'
    if ($haveVer) { $tag = ($haveVer -replace '[^A-Za-z0-9._-]', '_') }
    if ($tag.Length -gt 40) { $tag = $tag.Substring(0, 40) }
    $backups = Join-Path $root 'mod_backups'
    New-Item -ItemType Directory -Force -Path $backups | Out-Null
    $bk = Join-Path $backups "$($have.Name)-$tag"
    $n = 2
    while (Test-Path -LiteralPath $bk) { $bk = Join-Path $backups "$($have.Name)-$tag ($n)"; $n++ }
    $target = $have.FullName
    Move-Item -LiteralPath $target -Destination $bk
    & robocopy $m.FullName $target /E /COPY:DAT /DCOPY:T /R:1 /W:1 /NP /NFL /NDL /NJH /NJS | Out-Null
    if ($LASTEXITCODE -ge 8) {
        $rc = $LASTEXITCODE
        Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction SilentlyContinue
        Move-Item -LiteralPath $bk -Destination $target -ErrorAction SilentlyContinue
        throw "could not copy the mod $($m.Name) (robocopy $rc); the installed version was put back"
    }
    Write-Host "  bundled mod: $($m.Name) $newVer replaces $haveVer in mods\$($have.Name); the old one is kept in mod_backups\$(Split-Path -Leaf $bk)"
}

function Install-Bundled([string]$exeDir) {
    $b = Join-Path $root 'bundled'
    try {
        $src = Join-Path $b 'omniloader.dll'
        if (Test-Path -LiteralPath $src -PathType Leaf) {
            $dst = Join-Path $exeDir 'omniloader.dll'
            if ((Test-Path -LiteralPath $dst -PathType Leaf) -and ((Get-FileHash -LiteralPath $dst -Algorithm SHA256).Hash -eq (Get-FileHash -LiteralPath $src -Algorithm SHA256).Hash)) {
                Write-Host '  mod loader: omniloader.dll is already there'
            } else {
                Copy-Item -LiteralPath $src -Destination $dst -Force
                Write-Host '  mod loader: omniloader.dll installed next to rac-rs.exe'
            }
        }
        $bm = Join-Path $b 'mods'
        if (Test-Path -LiteralPath $bm -PathType Container) {
            foreach ($m in @(Get-ChildItem -LiteralPath $bm -Directory)) {
                try { Install-BundledMod $m } catch { Write-Host "  bundled mod $($m.Name): $($_.Exception.Message); the game still runs, without that mod" -ForegroundColor Yellow }
            }
        }
    } catch {
        Write-Host "  bundled files: $($_.Exception.Message); the game still runs, without that mod support" -ForegroundColor Yellow
    }
}

# Rust. A first setup downloads and installs it (about 150 MB) in the background: Python and the assembler come meanwhile, then
# the disc check and the game code (Python only) run while it installs. Complete-Tools (below) finishes (a) once it is there.
$cargo = $(if ($RustDirect) { Join-Path $rustHome 'bin\cargo.exe' } else { Join-Path $tc 'cargo\bin\cargo.exe' })
$rustcExe = $(if ($RustDirect) { Join-Path $rustHome 'bin\rustc.exe' } else { Join-Path $tc 'cargo\bin\rustc.exe' })
# The GNU host's own linker tools (ld, dlltool for the Windows API import libraries) come with Rust; rustc finds dlltool on PATH
# (the GNU assembler that dlltool runs is added to the same folder by Complete-Tools).
$rsc = Join-Path $tc "rustup\toolchains\$RustVersion-$RustHost\lib\rustlib\$RustHost\bin\self-contained"
$env:Path = "$rsc;$env:Path"
# Another MinGW on the PC's PATH (MSYS2, Strawberry Perl, llvm-mingw, w64devkit, ...) broke every link ("cannot find crt2.o",
# "cannot find -lkernel32"): rustc turns its self-contained linking (the startup files and Windows import libraries in
# lib\self-contained) off when PATH has an x86_64-w64-mingw32-gcc.exe outside its own folder, but the gcc that runs is still
# Rust's linker-only one in $rsc (first on PATH), which has none of those files. So those folders are left off PATH for this
# setup only (the PC's PATH is not changed), and every PC links as one without another MinGW does.
$pathKeep = New-Object System.Collections.ArrayList
foreach ($pd in ($env:Path -split ';')) {
    if (-not $pd.Trim()) { continue }
    $otherGcc = $false
    try { $otherGcc = ($pd -ne $rsc) -and (Test-Path -LiteralPath (Join-Path $pd.Trim().Trim('"') 'x86_64-w64-mingw32-gcc.exe') -PathType Leaf) } catch { }
    if ($otherGcc) { Write-Host "  not using $pd for this setup (another x86_64-w64-mingw32-gcc.exe is there, it would break linking)"; continue }
    [void]$pathKeep.Add($pd)
}
$env:Path = $pathKeep -join ';'
function Get-RustVersion {
    $v = ''
    if (Test-Path -LiteralPath $cargo) { try { $v = (& $rustcExe --version 2>$null) } catch { $v = '' } }
    return "$v"
}
$rv = Get-RustVersion
$script:RustUnpack = @()
if ($rv -notmatch "^rustc $([regex]::Escape($RustVersion)) ") {
    $rl = Join-Path $tc 'rust_install.log'
    Remove-Item -LiteralPath $rl, "$rl.out" -Force -ErrorAction SilentlyContinue
    if ($RustDirect) {
        # the four packages come now (about 150 MB); tools\rust_unpack.py unpacks them in the background once Python is there (below)
        Write-Host "  downloading Rust $RustVersion ($RustHost, about 150 MB, without rustup) into toolchain\ ..."
        # remembered: this folder runs Rust without rustup from now on (also when the next setup or an update runs without -RustNoLinks)
        New-Item -ItemType Directory -Force -Path $tc | Out-Null
        [System.IO.File]::WriteAllText($RustDirectMark, "Rust $RustVersion is unpacked from its packages (tools\rust_unpack.py), without rustup: this drive cannot make hard links.`n", (New-Object System.Text.UTF8Encoding($false)))
        foreach ($e in $RustPkgs.GetEnumerator()) {
            $f = Join-Path $tcDl $e.Key
            Get-Pinned "$RustDistUrl/$($e.Key)" $f $e.Value
            $script:RustUnpack += $f
        }
    } else {
        Write-Host "  downloading Rust $RustVersion ($RustHost, about 150 MB) into toolchain\ (in the background) ..."
        $ri = Join-Path $tcDl "rustup-init-$RustupVersion.exe"
        Get-Pinned $RustupUrl $ri $RustupSha256
        # rustup checks every Rust package it downloads against the signed release manifest of $RustVersion.
        $p = Start-Process -FilePath $ri -ArgumentList @('-y', '--no-modify-path', '--profile', 'minimal', '--default-host', $RustHost, '--default-toolchain', $RustVersion) -NoNewWindow -PassThru -RedirectStandardOutput "$rl.out" -RedirectStandardError $rl
        $null = $p.Handle      # keeps the exit code readable
        $script:RustJob = [pscustomobject]@{ Proc = $p; Log = $rl; T0 = Get-Date }
    }
} else {
    Write-Host "  $rv ($RustHost, toolchain\)"
}

# Python (embeddable package: python.exe + the standard library, nothing else; the generators need nothing more)
$python = Join-Path $tc 'python\python.exe'
$pv = ''
if (Test-Path -LiteralPath $python) { try { $pv = (& $python -c "import sys; print('%d.%d.%d' % sys.version_info[:3])" 2>$null) } catch { $pv = '' } }
if ($pv -ne $PyVersion) {
    Write-Host "  downloading Python $PyVersion (embeddable, about 12 MB) into toolchain\ ..."
    $pz = Join-Path $tcDl "python-$PyVersion-embed-amd64.zip"
    Get-Pinned $PyUrl $pz $PySha256
    $pd = Join-Path $tc 'python'
    if (Test-Path -LiteralPath $pd) { Remove-Item -LiteralPath $pd -Recurse -Force }
    Expand-Archive -LiteralPath $pz -DestinationPath $pd -Force
    $pv = ''
    try { $pv = (& $python -c "import sys; print('%d.%d.%d' % sys.version_info[:3])" 2>$null) } catch { $pv = '' }
    if ($pv -ne $PyVersion) { Fail "Python in toolchain\ does not run (version: '$pv')" }
}
Write-Host "  python $pv (toolchain\python)"

# Rust without rustup (see above): unpack the downloaded packages in the background, like the rustup install in the normal case
if ($script:RustUnpack.Count -gt 0) {
    $ru = Join-Path $root 'tools\rust_unpack.py'
    if (-not (Test-Path -LiteralPath $ru -PathType Leaf)) { Fail 'tools\rust_unpack.py is missing (needed to install Rust on a drive without hard links); download the game files again' }
    $ua = @('-I', $ru, $rustHome) + @($script:RustUnpack) | ForEach-Object { if ($_ -match '[\s&]') { '"' + $_ + '"' } else { $_ } }
    $p = Start-Process -FilePath $python -ArgumentList $ua -NoNewWindow -PassThru -RedirectStandardOutput "$rl.out" -RedirectStandardError $rl
    $null = $p.Handle      # keeps the exit code readable
    $script:RustJob = [pscustomobject]@{ Proc = $p; Log = $rl; T0 = Get-Date }
}

# GNU assembler. rustc makes the Windows API import libraries of some crates (windows-link and its users) with dlltool.exe,
# which comes with Rust, and dlltool runs the GNU assembler (as.exe), which does not. as.exe and the four DLLs it needs are
# taken from pinned MSYS2 packages (about 9 MB, SHA-256 checked) by tools\mingw_as.py into Rust's linker folder next to dlltool.
$MsysRepo = 'https://repo.msys2.org/mingw/mingw64'
$AsPkgs = [ordered]@{
    'mingw-w64-x86_64-binutils-2.47-3-any.pkg.tar.zst'        = '827363748CE3320683319D860EE4FCFCDBB36BAF66AAC6DAB0EC3822BE2D4FF7'
    'mingw-w64-x86_64-gettext-runtime-1.0-1-any.pkg.tar.zst'  = 'BE68D7F260633284B910C588C6D82EE304A81C8817A686D2CD9DF83F872C27AF'
    'mingw-w64-x86_64-libiconv-1.19-1-any.pkg.tar.zst'        = '21E334D0911F25DE75D3E18E0697648BCECFA9658256D600CAD0827D719C2F35'
    'mingw-w64-x86_64-zlib-1.3.2-2-any.pkg.tar.zst'           = '9E75842A070BA648E986E12424E1C92C9D7D77200E85F6A34EEB600819F2E694'
    'mingw-w64-x86_64-zstd-1.5.7-2-any.pkg.tar.zst'           = '1ADD6705B344664F6ACA108C85F79AB5BDD9E1162662BB06A4CF40A34F6E0907'
}
$asFiles = @('as.exe', 'libintl-8.dll', 'libiconv-2.dll', 'zlib1.dll', 'libzstd.dll')
$script:AsPkgFiles = @()
if (@($asFiles | Where-Object { -not (Test-Path -LiteralPath (Join-Path $rsc $_)) }).Count -gt 0) {
    Write-Host '  downloading the GNU assembler (MSYS2 binutils 2.47, about 9 MB) into toolchain\ ...'
    foreach ($e in $AsPkgs.GetEnumerator()) {
        $f = Join-Path $tcDl $e.Key
        Get-Pinned "$MsysRepo/$($e.Key)" $f $e.Value
        $script:AsPkgFiles += $f
    }
}
$timings['tools'] = [math]::Round(((Get-Date) - $t).TotalSeconds, 1)

# Libraries download: the crates.io libraries of the game (wgpu, winit, cpal and what they use, about 150) need nothing from
# the disc, so cargo downloads and unpacks them in the background while (b), (c) and (c2) run, instead of at the start of (d).
# Download only (cargo fetch): nothing is compiled, so (d) builds exactly as without it. A failure here is not an error: (d)
# downloads whatever is missing and reports real errors. Messages: generated\fetch_libs.log.
function Read-SharedText([string]$path) {
    # a file another process is still writing
    if (-not (Test-Path -LiteralPath $path)) { return '' }
    $fs = $null
    try {
        $fs = New-Object System.IO.FileStream($path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
        $buf = New-Object byte[] ([int]$fs.Length)
        $r = $fs.Read($buf, 0, $buf.Length)
        return [System.Text.Encoding]::UTF8.GetString($buf, 0, $r)
    } catch { return '' } finally { if ($fs) { $fs.Dispose() } }
}

function Get-BgSeconds($job) {
    # seconds from the start of a background job to its end. Process.ExitTime reads as 0 for these processes, so the end
    # is the last write of its message files (or now).
    $end = Get-Date
    $last = $job.T0
    foreach ($f in @($job.Log, "$($job.Log).out")) {
        try { $w = (Get-Item -LiteralPath $f -ErrorAction Stop).LastWriteTime; if ($w -gt $last) { $last = $w } } catch { }
    }
    if ($last -gt $job.T0 -and $last -lt $end) { $end = $last }
    return [math]::Round(($end - $job.T0).TotalSeconds, 1)
}

function Start-LibsFetch {
    # cargo reads the whole workspace, the game crate oracle\game included, which (c) writes meanwhile: so it reads a copy of
    # the crates without the generated code (rac-rs and oracle without rec and game, about 7 MB) in generated\fetch_ws, with a
    # stub game crate. It downloads into the same cargo home as (d).
    $fws = Join-Path $root 'generated\fetch_ws'
    try {
        if (Test-Path -LiteralPath $fws) { Remove-Item -LiteralPath $fws -Recurse -Force }
        New-Item -ItemType Directory -Force -Path (Join-Path $fws 'oracle\game\src') | Out-Null
        Copy-Item -LiteralPath (Join-Path $root 'rac-rs') -Destination $fws -Recurse
        Get-ChildItem -LiteralPath (Join-Path $root 'oracle') | Where-Object { $_.Name -ne 'rec' -and $_.Name -ne 'game' } | Copy-Item -Destination (Join-Path $fws 'oracle') -Recurse
        # a cargo workspace root above rac-rs\ (Cargo.toml and its Cargo.lock at the top of the folder), when the release has one
        foreach ($n in @('Cargo.toml', 'Cargo.lock')) { $f = Join-Path $root $n; if (Test-Path -LiteralPath $f -PathType Leaf) { Copy-Item -LiteralPath $f -Destination $fws } }
        $u8 = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText((Join-Path $fws 'oracle\game\Cargo.toml'), "[package]`nname = `"ee-game`"`nversion = `"0.1.0`"`nedition = `"2021`"`n`n[dependencies]`nee-oracle = { path = `"..`" }`n", $u8)
        [System.IO.File]::WriteAllText((Join-Path $fws 'oracle\game\src\lib.rs'), "// stub for the libraries download (setup.ps1)`n", $u8)
    } catch {
        Write-Host "  (no background download of the libraries: $($_.Exception.Message))" -ForegroundColor Yellow
        return $null
    }
    $log = Join-Path $root 'generated\fetch_libs.log'
    Remove-Item -LiteralPath $log, "$log.out" -Force -ErrorAction SilentlyContinue
    $a = @('fetch', '--manifest-path', (Join-Path $fws 'rac-rs\Cargo.toml'), '--target', $RustHost) | ForEach-Object { if ($_ -match '[\s&]') { '"' + $_ + '"' } else { $_ } }
    $p = Start-Process -FilePath $cargo -ArgumentList $a -WorkingDirectory $root -NoNewWindow -PassThru -RedirectStandardOutput "$log.out" -RedirectStandardError $log
    $null = $p.Handle      # keeps the exit code readable
    Write-Host '  downloading the libraries of the game in the background (messages in generated\fetch_libs.log)'
    return [pscustomobject]@{ Proc = $p; Log = $log; T0 = Get-Date }
}

function Wait-LibsFetch {
    $lf = $script:LibsFetch
    if (-not $lf) { return }
    if (-not $lf.Proc.HasExited) { Write-Host '  waiting for the background download of the libraries ...' }
    if (-not $lf.Proc.WaitForExit(900000)) { Write-Host '  the background download takes too long; stopping it (step (d) downloads the rest)' -ForegroundColor Yellow; Stop-LibsFetch; return }
    $script:LibsFetch = $null
    Remove-Item -LiteralPath (Join-Path $root 'generated\fetch_ws') -Recurse -Force -ErrorAction SilentlyContinue
    $txt = Read-SharedText $lf.Log
    $n = ([regex]::Matches($txt, '(?m)^\s+Downloaded ')).Count
    $timings['fetch_bg'] = Get-BgSeconds $lf
    if ($lf.Proc.ExitCode -eq 0) { Write-Host "  libraries: $n downloaded in the background (in $($timings['fetch_bg']) s)" }
    else { Write-Host "  the background download of the libraries stopped (exit $($lf.Proc.ExitCode), see generated\fetch_libs.log); step (d) downloads the rest" -ForegroundColor Yellow }
}

$script:ToolsDone = $false
function Complete-Tools([bool]$wait) {
    # The rest of (a) once Rust is installed: check it, put the GNU assembler next to its dlltool, start the libraries download.
    # $wait: wait for a background Rust install; without it, return $false while the install still runs (called between the
    # steps of (b)..(c2), so that the libraries download starts as soon as it can).
    $rj = $script:RustJob
    if ($rj) {
        if (-not $rj.Proc.HasExited) {
            if (-not $wait) { return $false }
            Write-Host '  waiting for the Rust download (in the background since step (a)) ...'
            $rj.Proc.WaitForExit()
        }
        $script:RustJob = $null
        $timings['rust_bg'] = Get-BgSeconds $rj
        if ($rj.Proc.ExitCode -ne 0) {
            Get-Content -LiteralPath $rj.Log -Tail 15 -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "  $_" }
            Fail "installing Rust into toolchain\ failed (exit $($rj.Proc.ExitCode), messages in toolchain\rust_install.log); run Setup.cmd again"
        }
        $v = Get-RustVersion
        if ($v -notmatch "^rustc $([regex]::Escape($RustVersion)) ") { Fail "Rust in toolchain\ does not run (rustc --version: '$v')" }
        Write-Host "  $v ($RustHost, toolchain\; installed in the background in $($timings['rust_bg']) s)"
    }
    if ($script:ToolsDone) { return $true }
    $script:ToolsDone = $true
    if (-not (Test-Path -LiteralPath (Join-Path $rsc 'dlltool.exe'))) { Fail "Rust in toolchain\ is incomplete ($rsc\dlltool.exe missing); delete the toolchain folder and run Setup.cmd again" }
    if ($script:AsPkgFiles.Count -gt 0) {
        & $python -I (Join-Path $root 'tools\mingw_as.py') $rsc @($script:AsPkgFiles) | Out-Host
        if ($LASTEXITCODE -ne 0) { Fail 'unpacking the GNU assembler into toolchain\ failed (see the messages above); run Setup.cmd again' }
    }
    Write-Host '  GNU assembler (binutils 2.47, toolchain\)'
    if (Test-Path -LiteralPath $tcDl) { Remove-Item -LiteralPath $tcDl -Recurse -Force -ErrorAction SilentlyContinue }
    if (-not $CheckOnly -and -not $NoBuild -and -not $NoFetchAhead) { $script:LibsFetch = Start-LibsFetch }
    return $true
}
[void](Complete-Tools ([bool]$CheckOnly))      # -CheckOnly: (a) complete before (b), as before


# ---------------------------------------------------------------- (b) disc image
Step '(b) disc image'
if (-not $Iso) {
    # Setup.cmd (double-click) passes no -Iso: ask with a file picker, typed path as fallback.
    try {
        Add-Type -AssemblyName System.Windows.Forms
        $dlg = New-Object System.Windows.Forms.OpenFileDialog
        $dlg.Title = 'Pick your Ratchet & Clank (Europe or USA, v1.00) disc image'
        $dlg.Filter = 'Disc image (*.iso)|*.iso|All files (*.*)|*.*'
        if ($dlg.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { $Iso = $dlg.FileName }
    } catch { }
}
if (-not $Iso) { $Iso = Read-Host 'Path to your Ratchet & Clank (Europe or USA, v1.00) .iso file' }
$Iso = $Iso.Trim().Trim('"')
if (-not (Test-Path -LiteralPath $Iso -PathType Leaf)) { Fail "disc image not found: $Iso" }
$Iso = (Resolve-Path -LiteralPath $Iso).ProviderPath
$len = (Get-Item -LiteralPath $Iso).Length
Write-Host "  $Iso ($len bytes)"
$disc = $Discs | Where-Object { $_.Size -eq $len } | Select-Object -First 1
if (-not $disc) {
    Fail "wrong size: expected $($Discs[0].Size) bytes (Ratchet & Clank PAL/EU v1.00, serial SCES-50916) or $($Discs[1].Size) bytes (USA v1.00, serial SCUS-97199), both En/Fr/De/Es/It. Other regions and versions are not supported."
}
$ExpectedSize = $disc.Size
$ExpectedSha1 = $disc.Sha1
$DiscName = $disc.Name
Write-Host "  disc: Ratchet & Clank $DiscName (by size)"
if (-not $SkipHash) {
    Write-Host '  computing SHA-1 (about half a minute) ...'
    $t = Get-Date
    $h = (Get-FileHash -LiteralPath $Iso -Algorithm SHA1).Hash.ToUpper()
    $timings['sha1'] = [math]::Round(((Get-Date) - $t).TotalSeconds, 1)
    if ($h -ne $ExpectedSha1) { Fail "SHA-1 mismatch: $h (expected $ExpectedSha1). This is not the $DiscName disc image (re-dump, or another version)." }
    Write-Host "  SHA-1 ok: Ratchet & Clank $DiscName"
} else {
    Write-Host '  SHA-1 skipped (-SkipHash); size matches'
}
if ($CheckOnly) { Write-Host 'Checks passed.' -ForegroundColor Green; exit 0 }

# ---------------------------------------------------------------- (c) generate
Step '(c) generating game code from the disc image'
[void](Complete-Tools $false)
$gen = Join-Path $root 'generated'          # logs and stamps
$orc = Join-Path $root 'oracle'             # the generated crates go to oracle\rec and oracle\game (the layout of the dev tree)
$stamp = Join-Path $gen 'done.txt'
$wantBase = "$ExpectedSha1 $ExpectedSize flags=$($GenFlags -join ' ')"
$genFiles = @('tools/recgen_iso.py', 'tools/share_iso.py', 'tools/fn_bounds.tsv', 'oracle/stubs.txt')
if ($ExpectedSha1 -eq 'AEE9275384F739C9B24CD5DE6F7E200292DB7B01') { $genFiles += 'tools/fn_bounds_usa.tsv' }
$genHash = Get-FilesHash $genFiles
# Earlier generator revisions (the first is v0.1.0, whose stamp has no gen= part) that make byte-identical output: an install made by one of them
# is still current, so an update that only changed HOW the files are written does not regenerate. Add the old hash here when a generator
# change does not change its output.
$GenCompatible = @('9a84d825c333842cd58ed6d12d1ed54c7141ffab30276e6add765ef00c859c51', '1c5c1935a7b98a397719acf76287677cdf061816bbd749c6e07e42ce50afa050')
$want = "$wantBase gen=$genHash"
$have = ''
if (Test-Path -LiteralPath $stamp) { $have = (Get-Content -LiteralPath $stamp -Raw).Trim() }
if (-not (Test-Path -LiteralPath (Join-Path $orc 'game\src\lib.rs'))) { $have = '' }
if ($have -and $have -ne $want) {
    $oldGen = ''
    if ($have -eq $wantBase) { $oldGen = $GenCompatible[0] } elseif ($have.StartsWith($wantBase + ' gen=')) { $oldGen = $have.Substring($wantBase.Length + 5) }
    if ($oldGen -and ($GenCompatible -contains $oldGen)) {
        Set-Content -LiteralPath $stamp -Value $want -Encoding ASCII
        $have = $want
        Write-Host '  the generator changed but makes the same files: stamp updated, nothing to redo'
    }
}
if ($Regenerate -or $have -ne $want) {
    # The generators write in place and only touch files whose content differs (cargo rebuilds by file time): nothing is deleted first.
    Remove-Item -LiteralPath $stamp -Force -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath (Join-Path $gen 'logs')) { Remove-Item -LiteralPath (Join-Path $gen 'logs') -Recurse -Force }
    New-Item -ItemType Directory -Force -Path (Join-Path $gen 'logs') | Out-Null
    # content and time of every generated file, so that files which come out the same keep their time (below: only changed crates rebuild)
    $genSnap = Join-Path $gen 'logs\gen_files_before.txt'
    & $python (Join-Path $root 'tools\share_iso.py') --rec (Join-Path $orc 'rec') --snapshot $genSnap
    if ($LASTEXITCODE -ne 0) { Fail 'listing the generated files failed (tools\share_iso.py --snapshot)' }
    # shared templates of the previous generation (rebuilt below from the new level crates, which recgen_iso.py writes in full)
    Get-ChildItem -LiteralPath (Join-Path $orc 'rec') -Directory -Filter 'sh_*' -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force
    $t = Get-Date
    $queue = New-Object System.Collections.Queue
    foreach ($l in $Levels) { $queue.Enqueue($l) }
    $running = @()
    $failed = @()
    $gj = $(if ($Jobs -gt 0) { $Jobs } else { [math]::Min(12, [math]::Max(1, [Environment]::ProcessorCount)) })      # same files for any number
    Write-Host "  $gj generators at once"
    while ($queue.Count -gt 0 -or $running.Count -gt 0) {
        while ($queue.Count -gt 0 -and $running.Count -lt $gj) {
            $lv = $queue.Dequeue()
            $a = @(('"' + (Join-Path $root 'tools\recgen_iso.py') + '"'), '--iso', ('"' + $Iso + '"'), '--level', "$lv", '--out', ('"' + $orc + '"'), '--oracle', ('"' + $orc + '"')) + $GenFlags
            if ($lv -gt 0) { $a += '--keep-boot' }
            $log = Join-Path $gen "logs\level_$lv.log"
            $p = Start-Process -FilePath $python -ArgumentList $a -WorkingDirectory $root -NoNewWindow -PassThru -RedirectStandardOutput $log -RedirectStandardError "$log.err"
            $null = $p.Handle
            $running += [pscustomobject]@{ Level = $lv; Proc = $p; Log = $log }
            Write-Host "  level $lv started"
        }
        Start-Sleep -Milliseconds 250
        if ($script:RustJob -and $script:RustJob.Proc.HasExited) { [void](Complete-Tools $false) }
        $still = @()
        foreach ($r in $running) {
            if ($r.Proc.HasExited) {
                $r.Proc.WaitForExit()
                if ($r.Proc.ExitCode -ne 0) { $failed += $r.Level; Write-Host "  level $($r.Level) FAILED (see $($r.Log).err)" -ForegroundColor Red }
                else { Write-Host ("  level {0} done: {1}" -f $r.Level, (Get-Content -LiteralPath $r.Log -Tail 1)) }
            } else { $still += $r }
        }
        $running = $still
    }
    if ($failed.Count -gt 0) { Fail "generation failed for level(s) $($failed -join ', '); logs are in generated\logs" }
    # the parallel generators race on game\src\lib.rs: rewrite it once from what is on disk
    & $python (Join-Path $root 'tools\recgen_iso.py') --facade-only --out $orc --oracle $orc
    if ($LASTEXITCODE -ne 0) { Fail 'facade rewrite failed' }
    # functions that repeat across levels (same code, other addresses and constants) get one shared body each: rec\sh_NN
    # (bit-exact: the pass proves every copy rebuilds token for token from its shared body and its own values)
    & $python (Join-Path $root 'tools\share_iso.py') --rec (Join-Path $orc 'rec')
    if ($LASTEXITCODE -ne 0) { Fail 'sharing pass failed (tools\share_iso.py)' }
    [void](Complete-Tools $false)
    & $python (Join-Path $root 'tools\recgen_iso.py') --facade-only --out $orc --oracle $orc
    if ($LASTEXITCODE -ne 0) { Fail 'facade rewrite failed' }
    & $python (Join-Path $root 'tools\share_iso.py') --rec (Join-Path $orc 'rec') --keep-times $genSnap
    if ($LASTEXITCODE -ne 0) { Fail 'restoring file times failed (tools\share_iso.py --keep-times)' }
    # every crate must be in the facade (concurrent generators rewrite it)
    $nCrates = (Get-ChildItem -LiteralPath (Join-Path $orc 'rec') -Directory).Count
    $nReg = (Select-String -LiteralPath (Join-Path $orc 'game\src\lib.rs') -Pattern '::FNS\)' ).Count
    if ($nCrates -ne $nReg) { Fail "facade lists $nReg of $nCrates crates; run setup.ps1 -Regenerate -Jobs 1" }
    Set-Content -LiteralPath $stamp -Value $want -Encoding ASCII
    $timings['generate'] = [math]::Round(((Get-Date) - $t).TotalSeconds, 1)
    Write-Host "  generated $nCrates crates in $($timings['generate']) s"
} else {
    Write-Host '  already generated for this disc image (use -Regenerate to redo)'
}

# ---------------------------------------------------------------- (c2) VU microcode translations
# The VU0/VU1 microprograms (graphics/maths code of the game) are cut out of YOUR disc image and translated to Rust here;
# none of that is shipped. Output (written in place, the files the sources already include): oracle\src\vu0_prog0.rs,
# vu0_prog1.rs, vu0_prog9hi.rs, skinff_ee.rs, skinff_vu.rs (the skinning fast path: the loop's code words are read from the disc
# image, tools\skinff_ee.py and tools\skinff_gen.py run inside vugen_iso.py),
# rac-rs\src\gsfull\vu1_prog*.rs and vu1aot_gen.rs.
# An update from a release without these files (v0.2.2) regenerates: the files are missing and the tool hash ($vuWant) changed.
Step '(c2) generating the VU microcode translations from the disc image'
[void](Complete-Tools $false)
New-Item -ItemType Directory -Force -Path $gen | Out-Null
$vuStamp = Join-Path $gen 'vu_done.txt'
$vuBase = "$ExpectedSha1 vu1"
$VuCompatible = @('5b49455da24a986799cd727bb9a4563a70609ab5759df4c060f5df0b00bec669', '066ba6b16b9293793872b68e6a66c106bb2e4f4c8a2e184b1a83ce29eedf26d0')      # as $GenCompatible, for tools\vugen_iso.py, tools\skinff_ee.py, tools\skinff_gen.py and tools\vu\ (the first is v0.1.0)
$vuWant = "$vuBase gen=$(Get-FilesHash (Get-VuGenFiles))"
$vuHave = ''
if (Test-Path -LiteralPath $vuStamp) { $vuHave = (Get-Content -LiteralPath $vuStamp -Raw).Trim() }
if (-not (Test-Path -LiteralPath (Join-Path $orc 'src\vu0_prog0.rs')) -or -not (Test-Path -LiteralPath (Join-Path $orc 'src\vu0_prog1.rs')) -or -not (Test-Path -LiteralPath (Join-Path $orc 'src\vu0_prog9hi.rs')) -or -not (Test-Path -LiteralPath (Join-Path $orc 'src\skinff_ee.rs')) -or -not (Test-Path -LiteralPath (Join-Path $orc 'src\skinff_vu.rs')) -or -not (Test-Path -LiteralPath (Join-Path $root 'rac-rs\src\gsfull\vu1aot_gen.rs')) -or -not (Test-Path -LiteralPath (Join-Path $root 'rac-rs\src\particles\orb_tables.rs'))) { $vuHave = '' }
if ($vuHave -and $vuHave -ne $vuWant) {
    $oldGen = ''
    if ($vuHave -eq $vuBase) { $oldGen = $VuCompatible[0] } elseif ($vuHave.StartsWith($vuBase + ' gen=')) { $oldGen = $vuHave.Substring($vuBase.Length + 5) }
    if ($oldGen -and ($VuCompatible -contains $oldGen)) {
        Set-Content -LiteralPath $vuStamp -Value $vuWant -Encoding ASCII
        $vuHave = $vuWant
        Write-Host '  the VU generator changed but makes the same files: stamp updated, nothing to redo'
    }
}
if ($Regenerate -or $vuHave -ne $vuWant) {
    $t = Get-Date
    & $python (Join-Path $root 'tools\vugen_iso.py') --iso $Iso --tree $root
    if ($LASTEXITCODE -ne 0) { Fail 'VU microcode generation failed (tools\vugen_iso.py)' }
    Set-Content -LiteralPath $vuStamp -Value $vuWant -Encoding ASCII
    $timings['vu'] = [math]::Round(((Get-Date) - $t).TotalSeconds, 1)
    Write-Host "  generated in $($timings['vu']) s"
} else {
    Write-Host '  already generated (use -Regenerate to redo)'
}
if ($NoBuild) { [void](Complete-Tools $true); Write-Host 'Generation done (-NoBuild).' -ForegroundColor Green; exit 0 }

# ---------------------------------------------------------------- build meter (step d)
# While cargo runs, a background job samples memory, paging and CPU every 2 s into generated\logs\build_meter.csv; the summary
# is printed and written to generated\build_report.txt (stays on this PC, nothing is sent anywhere). It reads Windows' WMI
# performance classes, whose names are the same in every Windows language. A meter failure never fails the setup.
function Get-PcInfo {
    $i = [pscustomobject]@{ Logical = [Environment]::ProcessorCount; Cores = 0; Cpu = ''; RamGB = 0.0 }
    try {
        $p = @(Get-CimInstance Win32_Processor)
        $i.Logical = [int](($p | Measure-Object NumberOfLogicalProcessors -Sum).Sum)
        $i.Cores = [int](($p | Measure-Object NumberOfCores -Sum).Sum)
        $i.Cpu = ($p[0].Name -replace '\s+', ' ').Trim()
        $i.RamGB = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB, 1)
    } catch { }
    if ($i.Logical -lt 1) { $i.Logical = [Environment]::ProcessorCount }
    if ($i.Cores -lt 1) { $i.Cores = $i.Logical }
    return $i
}

function Get-CargoJobs($pc) {
    # One job per logical CPU (cargo's own default; on a 12-core / 24-thread PC 24 jobs built in 11m19s and 12 jobs in 16m49s),
    # but not more jobs than the RAM holds: a generated crate needs about 0.65 GB while it compiles (measured 15 GB for 24 jobs,
    # the largest single one 3.4 GB), and 4 GB stay for Windows and the open programs. Paging to disk slows the build much more
    # than fewer jobs do. 32 GB: no limit; 16 GB: 18 jobs; 8 GB: 6 jobs.
    $j = [math]::Max(1, $pc.Logical)
    if ($pc.RamGB -gt 0) { $j = [math]::Min($j, [math]::Max(2, [int][math]::Floor(($pc.RamGB - 4.0) / 0.65))) }
    return $j
}

function Start-BuildMeter($dir) {
    try {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        $csv = Join-Path $dir 'build_meter.csv'
        $stop = Join-Path $dir 'build_meter.stop'
        Remove-Item -LiteralPath $stop -Force -ErrorAction SilentlyContinue
        $job = Start-Job -ArgumentList $csv, $stop, $tc -ScriptBlock {
            param($csv, $stop, $tc)
            $w = New-Object System.IO.StreamWriter($csv, $false)
            $w.WriteLine('sec,commit_mb,commit_limit_mb,avail_mb,pages_in,pages_out,cpu_pct,rustc_n,rustc_mb,rustc_max_mb,other_rustc_n')
            $t0 = Get-Date
            while (-not (Test-Path -LiteralPath $stop)) {
                try {
                    $m = Get-CimInstance Win32_PerfRawData_PerfOS_Memory
                    $c = Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor -Filter "Name='_Total'"
                    $n = 0; $o = 0; $mb = 0.0; $mx = 0.0
                    foreach ($r in @(Get-Process -Name rustc -ErrorAction SilentlyContinue)) {
                        $path = ''
                        try { $path = $r.Path } catch { }
                        if ($path -and $path.StartsWith($tc, [StringComparison]::OrdinalIgnoreCase)) {
                            $n++; $v = $r.PrivateMemorySize64 / 1MB; $mb += $v; if ($v -gt $mx) { $mx = $v }
                        } else { $o++ }
                    }
                    $w.WriteLine(('{0:F0},{1:F0},{2:F0},{3},{4},{5},{6},{7},{8:F0},{9:F0},{10}' -f ((Get-Date) - $t0).TotalSeconds, ($m.CommittedBytes / 1MB),
                        ($m.CommitLimit / 1MB), $m.AvailableMBytes, $m.PagesInputPersec, $m.PagesOutputPersec, $c.PercentProcessorTime, $n, $mb, $mx, $o))
                    $w.Flush()
                } catch { }
                Start-Sleep -Milliseconds 2000
            }
            $w.Close()
        }
        return [pscustomobject]@{ Job = $job; Csv = $csv; Stop = $stop }
    } catch { return $null }
}

function Stop-BuildMeter($meter, $sec, $jobs, $pc, $report, $ok) {
    if (-not $meter) { return }
    try {
        Set-Content -LiteralPath $meter.Stop -Value 'stop' -Encoding ASCII
        $null = Wait-Job $meter.Job -Timeout 30
        Remove-Job $meter.Job -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $meter.Stop -Force -ErrorAction SilentlyContinue
        $rows = @(Import-Csv -LiteralPath $meter.Csv)
        if ($rows.Count -lt 2) { return }
        function Col($name) { return @($rows | ForEach-Object { [double]$_.$name }) }
        $commit = Col 'commit_mb'; $avail = Col 'avail_mb'; $pin = Col 'pages_in'; $pout = Col 'pages_out'
        $cpu = Col 'cpu_pct'; $rn = Col 'rustc_n'; $rmb = Col 'rustc_mb'; $rmx = Col 'rustc_max_mb'; $oth = Col 'other_rustc_n'
        $outMB = [math]::Round(($pout[-1] - $pout[0]) * 4096 / 1MB)      # 4 KB pages written to the page file
        $inMB = [math]::Round(($pin[-1] - $pin[0]) * 4096 / 1MB)        # 4 KB pages read back from disk (page file and mapped files)
        $minAvail = ($avail | Measure-Object -Minimum).Minimum
        $busy = @($rn | Where-Object { $_ -gt 0 })
        $paging = ($outMB -gt 512) -or ($minAvail -lt [math]::Max(512, $pc.RamGB * 1024 * 0.03))
        $l = @(
            ('build {0}: {1} s wall ({2}), cargo -j {3}; {4}, {5} cores / {6} logical CPUs, {7} GB RAM' -f $(if ($ok) { 'ok' } else { 'FAILED' }), $sec,
                [TimeSpan]::FromSeconds([math]::Round($sec)).ToString(), $jobs, $pc.Cpu, $pc.Cores, $pc.Logical, $pc.RamGB),
            ('  memory: peak commit {0:F1} GB of {1:F1} GB limit, lowest free RAM {2:F1} GB, compilers peak {3:F1} GB together, {4:F1} GB the largest one' -f
                (($commit | Measure-Object -Maximum).Maximum / 1024), ([double]$rows[-1].commit_limit_mb / 1024), ($minAvail / 1024),
                (($rmb | Measure-Object -Maximum).Maximum / 1024), (($rmx | Measure-Object -Maximum).Maximum / 1024)),
            ('  paging: {0} ({1} MB written to the page file, {2} MB read from disk)' -f $(if ($paging) { 'YES, Windows ran short of memory' } else { 'no' }), $outMB, $inMB),
            ('  compilers at once: max {0}, mean {1:F1}; CPU busy mean {2:F0}%; other compilers on this PC: max {3}' -f
                ($rn | Measure-Object -Maximum).Maximum, $(if ($busy.Count) { ($busy | Measure-Object -Average).Average } else { 0 }),
                ($cpu | Measure-Object -Average).Average, ($oth | Measure-Object -Maximum).Maximum)
        )
        $l | ForEach-Object { Write-Host "  $_" }
        Set-Content -LiteralPath $report -Value $l -Encoding UTF8
    } catch { Write-Host "  (build meter: $($_.Exception.Message))" -ForegroundColor Yellow }
}

# ---------------------------------------------------------------- (d) build
Step '(d) building the game (release; this takes a long time and a lot of memory on the first run)'
# The generated crates (oracle\rec\*, oracle\game) are 97% of the compile work: they build at opt-level 3 in one codegen unit
# (full optimisation, same behaviour; the game thread runs faster on the heavy planets, and with one codegen unit the build
# takes about as long as opt-level 1: 9m55s vs 9m21s on a 24-thread PC). Everything else (the
# emulator core, hands, renderer, wgpu) keeps the default release profile. Passed to cargo as a config file so that
# rac-rs\Cargo.toml stays unchanged.
$prof = Join-Path $root 'generated\build_profile.toml'
$pl = @('# written by setup.ps1 (step d): build profile of the generated crates', '[profile.release.package.ee-game]', 'opt-level = 3', 'codegen-units = 1')
foreach ($d in (Get-ChildItem -LiteralPath (Join-Path $orc 'rec') -Directory | Sort-Object Name)) {
    $pl += @('', "[profile.release.package.rec-$($d.Name.Replace('_', '-'))]", 'opt-level = 3', 'codegen-units = 1')
}
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $prof) | Out-Null
Set-Content -LiteralPath $prof -Value $pl -Encoding ASCII
# Profile-guided optimisation (PGO): pgo\<disc serial>.profdata.gz, recorded by the developers with a training build of this
# version playing through every planet, tells the compiler which code the game runs most, so it optimises and lays out that code
# for speed (same behaviour, faster game thread). The profile holds only function names, checksums and run counts, no game code
# or data. Where it does not fit a function (changed code) the compiler ignores it for that function: never wrong, only less
# fast. -NoPgo, no profile for this disc, or an unpack error: the normal build.
$cfgs = @('--config', $prof)
$pgoDir = Join-Path $gen 'pgo'
$pgoGz = Join-Path $root ('pgo\' + $disc.Serial + '.profdata.gz')
$pgoOn = $false
if (-not $NoPgo -and (Test-Path -LiteralPath $pgoGz -PathType Leaf)) {
    try {
        $pgoTag = (Get-Sha256Hex $pgoGz).Replace('-', '').Substring(0, 12).ToLower()
        $pgoFile = Join-Path $pgoDir ($disc.Serial + '-' + $pgoTag + '.profdata')      # checksum in the name: a new profile rebuilds
        if (-not (Test-Path -LiteralPath $pgoFile -PathType Leaf)) {
            if (Test-Path -LiteralPath $pgoDir) { Remove-Item -LiteralPath $pgoDir -Recurse -Force }    # an older profile
            New-Item -ItemType Directory -Force -Path $pgoDir | Out-Null
            $zin = [System.IO.File]::OpenRead($pgoGz)
            try {
                $gz = New-Object System.IO.Compression.GZipStream($zin, [System.IO.Compression.CompressionMode]::Decompress)
                $zout = [System.IO.File]::Create($pgoFile + '.part')
                try { $gz.CopyTo($zout) } finally { $zout.Dispose(); $gz.Dispose() }
            } finally { $zin.Dispose() }
            Move-Item -LiteralPath ($pgoFile + '.part') -Destination $pgoFile
        }
        # the rustflags go in a config file (no quoting trouble with spaces in the folder path); the LLVM switch keeps the log
        # quiet about functions the profile does not fit; -pgso=false keeps code that ran little optimised for speed, not size
        $pgoToml = Join-Path $pgoDir 'pgo.toml'
        $tl = "# written by setup.ps1 (step d): profile-guided optimisation`n[build]`nrustflags = [`"-Cprofile-use=" + $pgoFile.Replace('\', '\\') + "`", `"-Cllvm-args=-no-pgo-warn-mismatch`", `"-Cllvm-args=-pgso=false`"]`n"
        [System.IO.File]::WriteAllText($pgoToml, $tl, (New-Object System.Text.UTF8Encoding($false)))
        $cfgs += @('--config', $pgoToml)
        $pgoOn = $true
        Write-Host "  speed profile (PGO): on, pgo\$($disc.Serial).profdata.gz"
    } catch {
        Write-Host "  speed profile (PGO): off, could not unpack pgo\$($disc.Serial).profdata.gz ($($_.Exception.Message))" -ForegroundColor Yellow
    }
}
if (-not $pgoOn) {
    if (Test-Path -LiteralPath $pgoDir) { Remove-Item -LiteralPath $pgoDir -Recurse -Force -ErrorAction SilentlyContinue }
    if ($NoPgo) { Write-Host '  speed profile (PGO): off (-NoPgo)' }
    elseif (-not (Test-Path -LiteralPath $pgoGz -PathType Leaf)) { Write-Host "  speed profile (PGO): off (this version has none for $($disc.Serial))" }
}
[void](Complete-Tools $true)
Wait-LibsFetch
# How many compiler jobs run at once (cargo -j): -CargoJobs N, or 0 = automatic (see Get-CargoJobs).
$pcInfo = Get-PcInfo
$cj = $(if ($CargoJobs -gt 0) { $CargoJobs } else { Get-CargoJobs $pcInfo })
Write-Host ("  {0} compiler jobs at once ({1} logical CPUs, {2} cores, {3} GB RAM{4})" -f $cj, $pcInfo.Logical, $pcInfo.Cores, $pcInfo.RamGB, $(if ($CargoJobs -gt 0) { ', set by -CargoJobs' } else { '' }))
$meter = Start-BuildMeter (Join-Path $gen 'logs')
$t = Get-Date
& $cargo build --release --manifest-path (Join-Path $root 'rac-rs\Cargo.toml') --target-dir (Join-Path $root 'build') -p rac-rs-exe --bin rac-rs --features rac-rs-exe/windows_gui @cfgs -j $cj --timings
$cargoExit = $LASTEXITCODE
$timings['build'] = [math]::Round(((Get-Date) - $t).TotalSeconds, 1)
Stop-BuildMeter $meter $timings['build'] $cj $pcInfo (Join-Path $gen 'build_report.txt') ($cargoExit -eq 0)
if ($cargoExit -ne 0) { Fail 'cargo build failed' }
$exe = Join-Path $root 'build\release\rac-rs.exe'
if (-not (Test-Path -LiteralPath $exe)) { Fail "build finished but $exe is missing" }
Write-Host "  built $exe ($([math]::Round((Get-Item -LiteralPath $exe).Length / 1MB)) MB) in $($timings['build']) s"
Install-Dxc (Split-Path -Parent $exe)
Install-Bundled (Split-Path -Parent $exe)
[System.IO.File]::WriteAllText((Join-Path $root 'generated\built_version.txt'), ((Get-LocalVersion) + "`n"), (New-Object System.Text.UTF8Encoding($false)))      # the version this build was made from
Get-ChildItem -LiteralPath (Join-Path $root 'generated') -Filter 'synced_*' -File -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue      # (see Sync-Update / -ApplyUpdate)

# ---------------------------------------------------------------- (e) Play.cmd
Step '(e) play.cfg and Play.cmd'
if ($Iso -cmatch '[^\x00-\x7F]') { Write-Host '  note: the disc image path has non-ASCII characters; if Play.cmd cannot find it, move the .iso to a plain path and run setup again' -ForegroundColor Yellow }
$cmd = @"
@echo off
rem Written by setup.ps1. Starts Ratchet & Clank from your disc image: the game itself starts from the disc (logos, title screen, New Game),
rem there is no savestate. Press Cross (X) during the logo movies to skip them. Saves go to the player folder next to this file.
set "ORACLE_ISO=$Iso"
set "ORACLE_STUBS=%~dp0oracle\stubs.txt"
set /p ORACLE_HAND_SKIP=<"%~dp0oracle\hand\hand_skip.txt"
set "RAC_PLAYER_DIR=%~dp0player"
if not exist "%RAC_PLAYER_DIR%" mkdir "%RAC_PLAYER_DIR%"
rem Language: the game starts in the language picked in Options, Language (or in the launcher), English before that.
rem To force one for every start, add a line here like:  set COLD_LANG=de   (en fr de es it)
rem Safety copies of your save card before each start (the newest two starts)
if exist "%RAC_PLAYER_DIR%\card.start1.bin" copy /y "%RAC_PLAYER_DIR%\card.start1.bin" "%RAC_PLAYER_DIR%\card.start2.bin" >nul
if exist "%RAC_PLAYER_DIR%\card.bin" copy /y "%RAC_PLAYER_DIR%\card.bin" "%RAC_PLAYER_DIR%\card.start1.bin" >nul
"%~dp0build\release\rac-rs.exe" view "%ORACLE_ISO%" 0 --retail-cold --modern-cam %*
if errorlevel 1 pause
"@
Set-Content -LiteralPath (Join-Path $root 'Play.cmd') -Value $cmd -Encoding ASCII
Write-Host '  wrote Play.cmd'
Write-PlayCfg $Iso
Write-Host '  wrote generated\play.cfg'

Step 'done'
$timings.GetEnumerator() | ForEach-Object { Write-Host ("  {0,-9} {1} s" -f $_.Key, $_.Value) }
try {
    $dir = $ShortcutDir
    if (-not $dir) { $dir = [Environment]::GetFolderPath('Desktop') }
    if (-not $NoShortcut) {
        # always (re)written, so it points to THIS install even when an older shortcut of the same name exists
        $lnkPath = New-GameShortcut $dir
        Write-Host "  desktop shortcut created: $lnkPath"
    }
} catch { Write-Host "  could not create the desktop shortcut: $($_.Exception.Message)" -ForegroundColor Yellow }
# the checkboxes of the setup window as switches (never a prompt here): -NoLauncher remembers "no launcher", -AddToSteam adds the Steam entry (Steam open = a note, the set-up is still a success)
if ($NoLauncher) { try { Set-SetupChoice 'launcher' 'no'; Write-Host '  remembered: no launcher (generated\setup_choice.txt)' } catch { } }
if ($AddToSteam) {
    try { $steamRes = Invoke-SteamShortcut 'add'; Write-Host ('  Steam: ' + (Get-SteamResultText 'add' $steamRes) + $(if ($steamRes.Status -ne 'ok') { ' Run Setup.cmd -AddToSteam later to try again.' } else { '' })) }
    catch { Write-Host "  Steam: could not be changed ($($_.Exception.Message))" -ForegroundColor Yellow }
}
Write-Host ''
Write-Host ('All done. To play: double-click "' + $GameName + '" on your desktop, or open Setup.cmd and click Play (or Play.cmd in this folder).') -ForegroundColor Green
Write-Host 'Support the project: https://ko-fi.com/asobicodes'
