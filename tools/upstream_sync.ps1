# upstream_sync.ps1 - brings upstream rac-rs (AsobiCodes/rac-rs) into this fork (docs/UPSTREAM_SYNC.md says why and how).
# Never touches the checkout it is started from: the merge, the generated game code and the build are in a worktree beside it
# (<repo>-sync, branch upstream-sync), so a session working in the main folder is not disturbed.
#
#   powershell -ExecutionPolicy Bypass -File tools\upstream_sync.ps1 -Check      # new upstream commits? which files would conflict? (no changes)
#   powershell -ExecutionPolicy Bypass -File tools\upstream_sync.ps1             # merge in the worktree, auto-resolve the bookkeeping files
#   powershell -ExecutionPolicy Bypass -File tools\upstream_sync.ps1 -Continue   # after resolving the remaining conflicts by hand: go on
#   powershell -ExecutionPolicy Bypass -File tools\upstream_sync.ps1 -Push       # merge (or continue), verify, commit, push to origin main
#
# Verify = generate the game code from your disc when upstream changed the generators (else copy it from this install), cargo check
# of the game crates, the rac-mp tests, upstream's engine/rac1 boundary tests, and the release build of the exe. -NoBuild skips the
# release build (about 10 minutes). Run setup.ps1 once in the main folder first: the toolchain, the disc path and the generated code
# come from there.
#
# Exit codes: 0 up to date / done, 10 (-Check) upstream has commits this fork lacks, 20 conflicts left for a person or an agent
# (listed in generated\upstream_sync.md), 30 verification failed, 1 any other failure.
param([switch]$Check, [switch]$Continue, [switch]$Push, [switch]$NoBuild, [int]$Jobs = 8)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$wt = "$root-sync"
$branch = 'upstream-sync'
$upUrl = 'https://github.com/AsobiCodes/rac-rs.git'
$report = Join-Path $root 'generated\upstream_sync.md'
# Files that conflict on every upstream release and are resolved here by rule (docs/UPSTREAM_SYNC.md "Bookkeeping files").
$bookkeeping = @('VERSION', 'CHANGELOG.md', 'Cargo.lock', 'release_files.txt', 'README.md')
# The game-code generators (setup.ps1 $genFiles): when upstream changed one, the game code is generated again from the disc.
$genFiles = @('tools/recgen_iso.py', 'tools/share_iso.py', 'tools/fn_bounds.tsv', 'tools/fn_bounds_usa.tsv', 'oracle/stubs.txt')

$lines = New-Object System.Collections.ArrayList
function Say([string]$t) { Write-Host $t; [void]$lines.Add($t) }
function Save-Report { New-Item -ItemType Directory -Force -Path (Split-Path -Parent $report) | Out-Null; [System.IO.File]::WriteAllText($report, (($lines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false))) }
function Done([int]$code) { Save-Report; Write-Host "report: $report"; exit $code }

# git without PowerShell 5.1 turning its stderr into errors. Returns stdout lines; $LASTEXITCODE is git's.
function G {
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try { $out = & git @args 2>$null } finally { $ErrorActionPreference = $old }
    return $out
}
function Gw { G -C $wt @args }

# ---------------------------------------------------------------- fetch and compare
Set-Location -LiteralPath $root
if (-not ((G remote) -contains 'upstream')) { G remote add upstream $upUrl | Out-Null; G remote set-url --push upstream DISABLE | Out-Null }
G fetch --quiet origin | Out-Null; if ($LASTEXITCODE) { throw 'git fetch origin failed' }
G fetch --quiet --tags upstream | Out-Null; if ($LASTEXITCODE) { throw 'git fetch upstream failed' }
$upVer = ((G show upstream/main:VERSION) -join '').Trim()
$ourVer = ((G show origin/main:VERSION) -join '').Trim()
$new = @(G log --format='%h %ad %s' --date=short origin/main..upstream/main)
Say "# Upstream sync $(Get-Date -Format 'yyyy-MM-dd HH:mm')"
Say ''
Say "- upstream rac-rs: v$upVer ($((G rev-parse --short upstream/main) -join ''))"
Say "- this fork (origin/main): v$ourVer ($((G rev-parse --short origin/main) -join ''))"
Say "- upstream commits this fork lacks: $($new.Count)"
foreach ($c in $new) { Say "  - $c" }

if ($Check) {
    if (-not $new.Count) { Say ''; Say 'Up to date.'; Done 0 }
    $mt = @(G merge-tree --write-tree --name-only --no-messages origin/main upstream/main)
    $conf = @($mt | Select-Object -Skip 1 | Where-Object { $_ })
    $real = @($conf | Where-Object { $bookkeeping -notcontains $_ })
    Say ''
    Say "Predicted conflicts: $($conf.Count) ($($conf.Count - $real.Count) bookkeeping, resolved by rule; $($real.Count) to resolve by hand)"
    foreach ($f in $real) { Say "  - $f" }
    $changedGen = @(G diff --name-only origin/main upstream/main -- @genFiles)
    if ($changedGen.Count) { Say "Generators changed upstream ($($changedGen -join ', ')): the game code is generated again (a few minutes) and players' updates rebuild it." }
    Done 10
}

# ---------------------------------------------------------------- the worktree and the merge
function Resolve-Bookkeeping {
    $u = @(Gw diff --name-only --diff-filter=U)
    if ($u -contains 'VERSION') {
        [System.IO.File]::WriteAllText((Join-Path $wt 'VERSION'), "$upVer.1`n", (New-Object System.Text.UTF8Encoding($false)))
        Gw add VERSION | Out-Null; Say "- VERSION: $upVer.1 (upstream's, this fork's first release on it)"
    }
    if ($u -contains 'Cargo.lock') {
        Gw checkout --theirs -- Cargo.lock | Out-Null; Gw add Cargo.lock | Out-Null; Say '- Cargo.lock: upstream''s (cargo adds rac-mp at the first build)'
    }
    if ($u -contains 'CHANGELOG.md') {
        # One conflict at the top: ours = this fork's sections since the last sync, theirs = upstream's new ones (newer versions).
        $p = Join-Path $wt 'CHANGELOG.md'
        $t = [System.IO.File]::ReadAllText($p).Replace("`r`n", "`n")
        $m = [regex]::Match($t, '(?s)<<<<<<< [^\n]*\n(.*?)=======\n(.*?)>>>>>>> [^\n]*\n')
        if ($m.Success -and -not ([regex]::Matches($t, '<<<<<<< ').Count -gt 1)) {
            $head = "## v$upVer.1`n`n- New: everything of upstream rac-rs v$upVer (its notes follow below).`n`n"
            $t = $t.Substring(0, $m.Index) + $head + $m.Groups[2].Value + $m.Groups[1].Value + $t.Substring($m.Index + $m.Length)
            [System.IO.File]::WriteAllText($p, $t, (New-Object System.Text.UTF8Encoding($false)))
            Gw add CHANGELOG.md | Out-Null; Say "- CHANGELOG.md: a v$upVer.1 section, then upstream's new sections, then this fork's (write the v$upVer.1 notes before releasing)"
        }
    }
    if ($u -contains 'README.md') {
        # Take this fork's side of a hunk that names this fork's repository (the download line upstream changes every release).
        $p = Join-Path $wt 'README.md'
        $t = [System.IO.File]::ReadAllText($p).Replace("`r`n", "`n")
        $t2 = [regex]::Replace($t, '(?s)<<<<<<< [^\n]*\n(.*?)=======\n.*?>>>>>>> [^\n]*\n', { param($m) if ($m.Groups[1].Value -match 'adamlbunch/rac-rs-multiplayer') { $m.Groups[1].Value } else { $m.Value } })
        [System.IO.File]::WriteAllText($p, $t2, (New-Object System.Text.UTF8Encoding($false)))
        if ($t2 -notmatch '<<<<<<< ') { Gw add README.md | Out-Null; Say '- README.md: this fork''s download line kept' }
    }
    if ($u -contains 'release_files.txt') {
        Gw checkout --theirs -- release_files.txt | Out-Null; Gw add release_files.txt | Out-Null; Say '- release_files.txt: written again from the tree before the commit'
    }
}

if (-not $Continue) {
    if (-not $new.Count) { Say ''; Say 'Up to date.'; Done 0 }
    if (Test-Path -LiteralPath $wt) {
        if (@(Gw status --porcelain --untracked-files=no).Count) { Say ''; Say "The worktree $wt has work in it: finish it with -Continue, or remove it (git worktree remove --force $wt)."; Done 1 }
        Gw checkout --quiet -B $branch origin/main | Out-Null
    } else {
        G worktree add -B $branch $wt origin/main | Out-Null; if ($LASTEXITCODE) { throw "git worktree add $wt failed" }
    }
    Gw config rerere.enabled true | Out-Null
    Gw -c merge.renameLimit=100000 -c diff.renameLimit=100000 merge --no-ff --no-commit upstream/main | Out-Null
    Say ''
    Say "## Merge in $wt (branch $branch)"
    Resolve-Bookkeeping
}
if (-not (Test-Path -LiteralPath $wt)) { Say ''; Say "No worktree $wt to continue: run without -Continue first."; Done 1 }
$left = @(Gw diff --name-only --diff-filter=U)
if ($left.Count) {
    Say ''
    Say "Conflicts to resolve by hand ($($left.Count)). Keep upstream's code and put this fork's lines back (docs/UPSTREAM_SYNC.md 'Resolving'), then run -Continue:"
    foreach ($f in $left) { Say "  - $f" }
    Done 20
}

# ---------------------------------------------------------------- verify
$tc = Join-Path $root 'toolchain'
$chains = Join-Path $tc 'rustup\toolchains'
if (-not (Test-Path -LiteralPath $chains)) { Say 'No toolchain: run setup.ps1 in the main folder first.'; Done 1 }
$name = (Get-ChildItem -LiteralPath $chains -Directory | Select-Object -First 1).Name
$env:RUSTUP_HOME = Join-Path $tc 'rustup'; $env:CARGO_HOME = Join-Path $tc 'cargo'; $env:RUSTUP_TOOLCHAIN = $name
foreach ($v in @('RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET', 'CARGO_BUILD_RUSTFLAGS',
                  'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_TARGET_DIR', 'CARGO_PROFILE_RELEASE_OPT_LEVEL', 'CARGO_PROFILE_RELEASE_LTO')) { Remove-Item -LiteralPath "Env:$v" -ErrorAction SilentlyContinue }
# The toolchain is used from the main folder's real path (a junction to it in the worktree makes rustc drop its own linker files).
$rsc = Join-Path $chains "$name\lib\rustlib\$(($name -split '-', 2)[1])\bin\self-contained"
$keep = @($rsc, (Join-Path $tc 'cargo\bin'))
foreach ($pd in ($env:Path -split ';')) { if ($pd.Trim() -and -not (Test-Path -LiteralPath (Join-Path $pd.Trim().Trim('"') 'x86_64-w64-mingw32-gcc.exe') -PathType Leaf)) { $keep += $pd } }
$env:Path = $keep -join ';'
$cargo = Join-Path $tc 'cargo\bin\cargo.exe'
$python = Join-Path $tc 'python\python.exe'
$iso = ''
$cfg = Join-Path $root 'generated\play.cfg'
if (Test-Path -LiteralPath $cfg) { foreach ($l in [System.IO.File]::ReadAllLines($cfg)) { if ($l -like 'iso=*') { $iso = $l.Substring(4) } } }
New-Item -ItemType Directory -Force -Path (Join-Path $wt 'build'), (Join-Path $wt 'generated') | Out-Null
foreach ($f in 'play.cfg', 'multiplayer.cfg') { $s = Join-Path $root "generated\$f"; if (Test-Path -LiteralPath $s) { Copy-Item -LiteralPath $s -Destination (Join-Path $wt 'generated') -Force } }

# A step: runs a native command with all its output in build\<name>.log; returns its exit code.
function Step([string]$label, [string]$log, [string]$exe, [string[]]$argv) {
    $logPath = Join-Path $wt "build\$log"
    $q = ($argv | ForEach-Object { if ($_ -match '[\s&]') { '"' + $_ + '"' } else { $_ } }) -join ' '
    Push-Location -LiteralPath $wt
    try { cmd /c "`"$exe`" $q > `"$logPath`" 2>&1" } finally { Pop-Location }
    $c = $LASTEXITCODE
    Say ("- {0}: {1} (log build\{2})" -f $label, $(if ($c -eq 0) { 'ok' } else { "FAILED, exit $c" }), $log)
    return $c
}

Say ''
Say '## Verify'
$genChanged = @(G diff --name-only origin/main upstream/main -- @genFiles).Count -gt 0
$haveGen = Test-Path -LiteralPath (Join-Path $wt 'oracle\game\src\lib.rs')
if ($genChanged -or -not $haveGen) {
    if ($genChanged -and -not (Test-Path -LiteralPath $iso)) { Say "The generators changed but the disc image is not found (generated\play.cfg iso=$iso)."; Done 30 }
    if (-not $genChanged) {
        foreach ($d in 'oracle\rec', 'oracle\game') { & robocopy (Join-Path $root $d) (Join-Path $wt $d) /MIR /NFL /NDL /NJH /NJS /NP /MT:16 | Out-Null }
        Say '- game code: the generators are unchanged, copied from this install'
    } else {
        $orc = Join-Path $wt 'oracle'
        foreach ($d in 'rec', 'game') { $p = Join-Path $orc $d; if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Recurse -Force } }
        $flags = @('--no-comments', '--compact', '--shared-slow', '--outline-mem', '--direct-mem', '--skin-ff', '--vu-guard')
        $rg = Join-Path $wt 'tools\recgen_iso.py'
        if (Step 'generate level 0' 'gen_0.log' $python (@($rg, '--iso', $iso, '--level', '0', '--out', $orc, '--oracle', $orc) + $flags)) { Done 30 }
        $procs = @()
        foreach ($lv in 1..20) {
            $a = @(('"' + $rg + '"'), '--iso', ('"' + $iso + '"'), '--level', "$lv", '--out', ('"' + $orc + '"'), '--oracle', ('"' + $orc + '"'), '--keep-boot') + $flags
            $procs += Start-Process -FilePath $python -ArgumentList $a -WorkingDirectory $wt -NoNewWindow -PassThru -RedirectStandardOutput (Join-Path $wt "build\gen_$lv.log") -RedirectStandardError (Join-Path $wt "build\gen_$lv.err")
            while (@($procs | Where-Object { -not $_.HasExited }).Count -ge $Jobs) { Start-Sleep -Milliseconds 300 }
        }
        $procs | ForEach-Object { $_.WaitForExit() }
        $bad = @($procs | Where-Object { $_.ExitCode -ne 0 })
        Say "- generate levels 1-20: $(if ($bad.Count) { "FAILED ($($bad.Count))" } else { 'ok' })"
        if ($bad.Count) { Done 30 }
        if (Step 'facade' 'gen_facade.log' $python @($rg, '--facade-only', '--out', $orc, '--oracle', $orc)) { Done 30 }
        if (Step 'shared bodies' 'gen_share.log' $python @((Join-Path $wt 'tools\share_iso.py'), '--rec', (Join-Path $orc 'rec'))) { Done 30 }
        if (Step 'facade again' 'gen_facade2.log' $python @($rg, '--facade-only', '--out', $orc, '--oracle', $orc)) { Done 30 }
    }
}
if (Test-Path -LiteralPath $iso) {
    if (Step 'VU code' 'vugen.log' $python @((Join-Path $wt 'tools\vugen_iso.py'), '--iso', $iso, '--tree', $wt)) { Done 30 }
} else {
    Say '- VU code: no disc image found, copied from this install'
    Copy-Item (Join-Path $root 'oracle\src\vu0_prog*.rs'), (Join-Path $root 'oracle\src\skinff_*.rs') (Join-Path $wt 'oracle\src') -Force
    Copy-Item (Join-Path $root 'rac-rs\src\engine\gsfull\vu1*_*.rs') (Join-Path $wt 'rac-rs\src\engine\gsfull') -Force -ErrorAction SilentlyContinue
    Copy-Item (Join-Path $root 'rac-rs\src\rac1\particles\orb_tables.rs') (Join-Path $wt 'rac-rs\src\rac1\particles') -Force -ErrorAction SilentlyContinue
}
$pl = @('# written by setup.ps1 (step d): build profile of the generated crates', '[profile.release.package.ee-game]', 'opt-level = 3', 'codegen-units = 1')
foreach ($d in (Get-ChildItem -LiteralPath (Join-Path $wt 'oracle\rec') -Directory | Sort-Object Name)) { $pl += @('', "[profile.release.package.rec-$($d.Name.Replace('_', '-'))]", 'opt-level = 3', 'codegen-units = 1') }
Set-Content -LiteralPath (Join-Path $wt 'generated\build_profile.toml') -Value $pl -Encoding ASCII

$fail = 0
$fail += [int](0 -ne (Step 'cargo check (rac1, rac-rs, rac-mp)' 'check.log' $cargo @('check', '--target-dir', 'build/check', '-p', 'rac1', '-p', 'rac-rs', '-p', 'rac-mp')))
$fail += [int](0 -ne (Step 'rac-mp tests' 'test_mp.log' $cargo @('test', '--target-dir', 'build/check', '-p', 'rac-mp')))
$fail += [int](0 -ne (Step 'boundary tests (engine / rac1)' 'test_boundary.log' $cargo @('test', '--target-dir', 'build/check', '-p', 'rac-rs', '--lib', 'boundary')))
# Size caps: this fork's own files are over them since before this script (docs/UPSTREAM_SYNC.md "Known debt"); reported, not a gate yet.
[void](Step 'size caps (report only)' 'test_size.log' $cargo @('test', '--target-dir', 'build/check', '-p', 'rac-rs', '--lib', 'size_limits'))
if (-not $NoBuild -and -not $fail) {
    $fail += [int](0 -ne (Step 'release build of the exe' 'release.log' $cargo @('build', '--release', '-j', "$Jobs", '--manifest-path', 'rac-rs\Cargo.toml', '--target-dir', 'build',
        '-p', 'rac-rs-exe', '--bin', 'rac-rs', '--features', 'rac-rs-exe/windows_gui', '--config', 'generated\build_profile.toml')))
}
if ($fail) { Say ''; Say "Verification failed: fix it in $wt, then run -Continue."; Done 30 }

# ---------------------------------------------------------------- commit and push
$files = @(Gw ls-files | Where-Object { $_ -ne 'release_files.txt' })
[System.IO.File]::WriteAllText((Join-Path $wt 'release_files.txt'), (($files -join "`r`n") + "`r`n"), (New-Object System.Text.UTF8Encoding($false)))
Gw add -A | Out-Null
Gw rev-parse -q --verify MERGE_HEAD | Out-Null
if ($LASTEXITCODE -eq 0) {
    Gw commit --quiet -m "Merge upstream rac-rs v$upVer" -m "Brings AsobiCodes/rac-rs v$upVer into this fork (tools/upstream_sync.ps1; docs/UPSTREAM_SYNC.md)." | Out-Null
    if ($LASTEXITCODE) { Say 'git commit failed'; Done 1 }
} elseif (@(Gw status --porcelain --untracked-files=no).Count) {
    Gw commit --quiet -m 'Upstream sync: release file list' | Out-Null
}
Say ''
Say "Merged and verified in $wt ($((Gw rev-parse --short HEAD) -join ''))."
if (-not $Push) { Say 'Not pushed (-Push pushes it to origin main).'; Done 0 }

G fetch --quiet origin | Out-Null
if (((Gw rev-list --count HEAD..origin/main) -join '').Trim() -ne '0') {
    # main moved while this ran: take it in (conflicts here are this fork's own work meeting the merge).
    Gw merge --no-edit origin/main | Out-Null
    if ($LASTEXITCODE) { Say 'origin/main moved and does not merge cleanly: resolve in the worktree, then -Continue -Push.'; Done 20 }
    Say '- origin/main moved meanwhile: merged in'
}
Gw push origin "HEAD:main" | Out-Null
if ($LASTEXITCODE) { Say 'git push failed (credentials?)'; Done 1 }
Say "Pushed to origin main. The main folder pulls it with: git pull --rebase (a session working there does that before its next push)."
Say "Next: write the v$upVer.1 notes in CHANGELOG.md if they are only the stub, then tools\make_release.ps1 -Publish to reach the players."
G worktree remove --force $wt | Out-Null
G branch -D $branch | Out-Null
Say "Worktree and branch $branch removed."
Done 0
