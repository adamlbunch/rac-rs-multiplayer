# hv_create.ps1 - a Windows 11 test VM in Hyper-V for multiplayer runs (this fork's test helper). Your account must be in
# Hyper-V Administrators. Creates and starts one new VM and touches nothing else in Hyper-V: no other VM, no switch (it joins the
# built-in Default Switch), no GPU partitioning. The install is unattended (autounattend.xml next to this script); a password is
# generated for its test account and kept in DIR\vm-account.txt, outside the repo.
#
#   powershell -ExecutionPolicy Bypass -File tools\hyperv\hv_create.ps1 -WinIso C:\path\Win11.iso
#   then: hv.ps1 wait; hv_game.ps1 -Copy -Iso <the game's disc>; hv_pair.ps1
#
# The VM gets half the default CPU weight, so another VM on this PC wins when both need the CPU. It has no GPU: the game draws
# through the CPU Vulkan driver Microsoft Edge ships (SwiftShader, hv_game.ps1), at about 1 field a second.
param([Parameter(Mandatory)][string]$WinIso, [string]$Name = 'RacTest', [string]$Dir = "$env:USERPROFILE\vm\hyperv", [int]$MemGB = 12, [int]$Cpus = 6)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (Get-VM -Name $Name -ErrorAction SilentlyContinue) { throw "a VM named $Name exists already" }
New-Item -ItemType Directory -Force "$Dir\answer" | Out-Null
$acct = "$Dir\vm-account.txt"
if (-not (Test-Path $acct)) {
	$chars = [char[]]'abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789'
	$bytes = New-Object byte[] 18
	[Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
	$pw = 'Rt-' + (-join ($bytes | ForEach-Object { $chars[$_ % $chars.Length] }))
	Set-Content $acct "user=racvm`npassword=$pw" -Encoding ascii
}
$pw = ((Get-Content $acct) | Where-Object { $_ -like 'password=*' }).Substring(9)
(Get-Content "$here\autounattend.xml" -Raw).Replace('@PASSWORD@', $pw) | Set-Content "$Dir\answer\autounattend.xml" -Encoding utf8 -NoNewline
& python -I "$here\mkiso.py" "$Dir\answer" "$Dir\answer.iso" RACANSWER
Remove-Item "$Dir\answer\autounattend.xml"
New-VM -Name $Name -Generation 1 -MemoryStartupBytes ($MemGB * 1GB) -NewVHDPath "$Dir\$Name.vhdx" -NewVHDSizeBytes 80GB -SwitchName 'Default Switch' -Path $Dir | Out-Null
Set-VMMemory -VMName $Name -DynamicMemoryEnabled $false
Set-VMProcessor -VMName $Name -Count $Cpus -RelativeWeight 50
Set-VMDvdDrive -VMName $Name -ControllerNumber 1 -ControllerLocation 0 -Path $WinIso
Add-VMDvdDrive -VMName $Name -ControllerNumber 0 -ControllerLocation 1 -Path "$Dir\answer.iso"
Set-VM -Name $Name -AutomaticCheckpointsEnabled $false -AutomaticStartAction Nothing -AutomaticStopAction ShutDown
Start-VM -Name $Name
"$Name is installing Windows (20 to 30 minutes); hv.ps1 wait tells when it is up"
