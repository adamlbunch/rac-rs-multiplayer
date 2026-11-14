# build_apworld.ps1 - packs archipelago/rac1 into rac1.apworld (a zip whose top folder is rac1/), the file Archipelago installs
# (double-click it, or put it in Archipelago's custom_worlds folder). The tests and caches stay out.
#
#   powershell -ExecutionPolicy Bypass -File tools\build_apworld.ps1 [-Out path\rac1.apworld]
param([string]$Out = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$src = Join-Path $root 'archipelago\rac1'
if (-not $Out) { $Out = Join-Path $root 'build\rac1.apworld' }
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Out) | Out-Null
if (Test-Path -LiteralPath $Out) { Remove-Item -LiteralPath $Out -Force }
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::Open($Out, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    Get-ChildItem -LiteralPath $src -Recurse -File | Where-Object { $_.FullName -notmatch '\\(__pycache__|test)\\' -and $_.Extension -ne '.pyc' } | ForEach-Object {
        $rel = 'rac1/' + $_.FullName.Substring($src.Length + 1).Replace('\', '/')
        [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $_.FullName, $rel, [System.IO.Compression.CompressionLevel]::Optimal) | Out-Null
    }
} finally {
    $zip.Dispose()
}
Write-Output "made $Out"
