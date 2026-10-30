# mp_build.ps1 - rebuilds the game after a code change (this fork's dev helper). Same toolchain, build profile and speed
# profile (PGO) as setup.ps1's step (d), so cargo rebuilds only what changed (a minute or two for rac-rs/src or mp/). Run
# setup.ps1 once first: it installs the toolchain and generates the game code from your disc.
#
#   powershell -ExecutionPolicy Bypass -File tools\mp_build.ps1 [-Test]
#   -Test: also run the rac-mp tests.
param([switch]$Test)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$tc = Join-Path $root 'toolchain'
$chains = Join-Path $tc 'rustup\toolchains'
if (-not (Test-Path -LiteralPath $chains)) { throw "no toolchain in ${tc}: run setup.ps1 first" }
$name = (Get-ChildItem -LiteralPath $chains -Directory | Select-Object -First 1).Name
$env:RUSTUP_HOME = Join-Path $tc 'rustup'
$env:CARGO_HOME = Join-Path $tc 'cargo'
$env:RUSTUP_TOOLCHAIN = $name
foreach ($v in @('RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET', 'CARGO_BUILD_RUSTFLAGS',
                  'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_TARGET_DIR', 'CARGO_PROFILE_RELEASE_OPT_LEVEL', 'CARGO_PROFILE_RELEASE_LTO')) {
    Remove-Item -LiteralPath "Env:$v" -ErrorAction SilentlyContinue
}
# The GNU host's own linker first; any other MinGW off PATH (it breaks linking, see setup.ps1).
$host_ = ($name -split '-', 2)[1]
$rsc = Join-Path $chains "$name\lib\rustlib\$host_\bin\self-contained"
$keep = @($rsc, (Join-Path $tc 'cargo\bin'))
foreach ($pd in ($env:Path -split ';')) {
    if (-not $pd.Trim()) { continue }
    if (Test-Path -LiteralPath (Join-Path $pd.Trim().Trim('"') 'x86_64-w64-mingw32-gcc.exe') -PathType Leaf) { continue }
    $keep += $pd
}
$env:Path = $keep -join ';'
$cargo = Join-Path $tc 'cargo\bin\cargo.exe'
$cfgs = @('--config', (Join-Path $root 'generated\build_profile.toml'))
$pgo = Join-Path $root 'generated\pgo\pgo.toml'
if (Test-Path -LiteralPath $pgo) { $cfgs += @('--config', $pgo) }
& $cargo build --release --manifest-path (Join-Path $root 'rac-rs\Cargo.toml') --target-dir (Join-Path $root 'build') -p rac-rs-exe --bin rac-rs --features rac-rs-exe/windows_gui @cfgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ($Test) {
    # A target folder of its own: the tests build in the dev profile and must not touch the game's build.
    & $cargo test --manifest-path (Join-Path $root 'mp\Cargo.toml') --target-dir (Join-Path $root 'build\mp-test') -p rac-mp
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
