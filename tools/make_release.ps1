# make_release.ps1 - makes this fork's release zip, rac-rs-multiplayer-VERSION.zip, from the committed tree as GitHub's archive of
# the tag has it (the files as stored, one top folder), and with -Publish tags it and publishes it on GitHub (gh). The game's update
# check offers the newest published release (setup.ps1 Read-Remote), so publishing is what reaches the players.
#
#   powershell -ExecutionPolicy Bypass -File tools\make_release.ps1             # build\release-zip\rac-rs-multiplayer-VERSION.zip
#   powershell -ExecutionPolicy Bypass -File tools\make_release.ps1 -Publish    # and tag vVERSION, push the tag, publish the release
#
# Before: set VERSION (a fourth number on upstream's: 0.4.1.1), write its "## vVERSION" section in CHANGELOG.md (its notes), commit.
param([switch]$Publish)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location -LiteralPath $root
$repo = 'adamlbunch/rac-rs-multiplayer'
$ver = ([System.IO.File]::ReadAllText((Join-Path $root 'VERSION'))).Trim()
if ($ver -notmatch '^\d+\.\d+\.\d+(\.\d+)?$') { throw "VERSION is not a version: $ver" }

# release_files.txt: every file of the release; an update deletes what the old list has and the new one does not.
$files = @(git ls-files | Where-Object { $_ -ne 'release_files.txt' })
$want = ($files -join "`n") + "`n"
$listFile = Join-Path $root 'release_files.txt'
$have = $(if (Test-Path -LiteralPath $listFile) { [System.IO.File]::ReadAllText($listFile).Replace("`r", '') } else { '' })
if ($have -ne $want) {
    [System.IO.File]::WriteAllText($listFile, $want.Replace("`n", "`r`n"), (New-Object System.Text.UTF8Encoding($false)))
    throw 'release_files.txt was out of date and is updated now: commit it, then run this again'
}
if (git status --porcelain --untracked-files=no) { throw 'commit your changes first: the release is made from HEAD' }

# The notes: VERSION's section of CHANGELOG.md.
$notes = New-Object System.Collections.ArrayList
$in = $false
foreach ($l in (Get-Content -LiteralPath (Join-Path $root 'CHANGELOG.md'))) {
    if ($l -match '^##\s+v?(\S+)') { $in = ($Matches[1] -eq $ver); continue }
    if ($in) { [void]$notes.Add($l) }
}
$notesText = (($notes -join "`n").Trim())
if (-not $notesText) { throw "CHANGELOG.md has no section ## v$ver" }

$out = Join-Path $root 'build\release-zip'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$zip = Join-Path $out "rac-rs-multiplayer-$ver.zip"
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
# The files as stored (no line-end conversion: setup.sh must keep LF ends, Setup.cmd its CRLF ones), as GitHub's archive has them.
git -c core.autocrlf=false archive --format=zip --prefix="rac-rs-multiplayer-$ver/" -o $zip HEAD
if ($LASTEXITCODE -ne 0) { throw 'git archive failed' }
"made $zip ({0:N1} MB, {1} files, from {2})" -f ((Get-Item -LiteralPath $zip).Length / 1MB), $files.Count, (git rev-parse --short HEAD)

if ($Publish) {
    $tag = "v$ver"
    if (git tag --list $tag) { throw "tag $tag exists already: raise VERSION for a new release" }
    $notesFile = Join-Path $out "notes-$ver.md"
    $intro = "Download **rac-rs-multiplayer-$ver.zip** below, unzip it, and double-click **Setup.cmd** (see the README's Play together section). Already installed? The game offers this update when it starts."
    [System.IO.File]::WriteAllText($notesFile, ($intro + "`n`n" + $notesText + "`n"), (New-Object System.Text.UTF8Encoding($false)))
    git tag -a $tag -m "rac-rs-multiplayer $ver"
    if ($LASTEXITCODE -ne 0) { throw 'git tag failed' }
    git push origin $tag
    if ($LASTEXITCODE -ne 0) { throw 'git push of the tag failed' }
    gh release create $tag $zip --repo $repo --title "rac-rs-multiplayer $ver" --notes-file $notesFile --verify-tag
    if ($LASTEXITCODE -ne 0) { throw 'gh release create failed' }
    "published ${tag}: https://github.com/$repo/releases/tag/$tag"
}
