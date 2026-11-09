# hv_game.ps1 - the game in the Hyper-V test VM (hv_create.ps1), driven from the host over PowerShell Direct.
#   hv_game.ps1 -Copy -Iso DISC.iso       the built game (build\release), its oracle files, mp_play.ps1 and the disc into C:\rac in the VM
#   hv_game.ps1 -Fields 1700 [-Mp join:ADDR:2410] [-Pad ...] [-Travel L:F] [-Log FILE]
#       runs tools\mp_play.ps1 in the VM's logged-on desktop session (a scheduled task: PowerShell Direct's own session has no
#       desktop for a window), waits for it and saves its run log to FILE (default: DIR\vm-log.txt).
# The VM has no GPU, and Windows' own software one (WARP) lost its device and ran the game out of memory; so the game draws
# through Vulkan on the CPU, with the SwiftShader driver and Vulkan loader that Microsoft Edge ships in every Windows 11.
param([switch]$Copy, [string]$Iso = '', [int]$Fields = 0, [string]$Mp = '', [string]$Pad = '', [string]$Travel = '', [string]$Log = '',
	[string]$Name = 'RacTest', [string]$Dir = "$env:USERPROFILE\vm\hyperv", [int]$TimeoutMin = 90)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
if (-not $Log) { $Log = "$Dir\vm-log.txt" }
$kv = @{}; Get-Content "$Dir\vm-account.txt" | ForEach-Object { $k, $v = $_ -split '=', 2; $kv[$k] = $v }
$cred = New-Object System.Management.Automation.PSCredential("RACVM\$($kv.user)", (ConvertTo-SecureString $kv.password -AsPlainText -Force))
if ($Copy) {
	if (-not $Iso) { throw '-Copy needs -Iso, the game disc' }
	$files = [ordered]@{ 'build\release\rac-rs.exe' = "$root\build\release\rac-rs.exe"; 'build\release\dxcompiler.dll' = "$root\build\release\dxcompiler.dll";
		'build\release\omniloader.dll' = "$root\build\release\omniloader.dll"; 'oracle\stubs.txt' = "$root\oracle\stubs.txt";
		'oracle\hand\hand_skip.txt' = "$root\oracle\hand\hand_skip.txt"; 'tools\mp_play.ps1' = "$root\tools\mp_play.ps1"; 'tools\desk_run.ps1' = "$root\tools\desk_run.ps1"; 'disc.iso' = $Iso }
	# Over a PowerShell Direct session (Hyper-V's own Copy-VMFile refused the second file: "the request is not supported").
	$s = New-PSSession -VMName $Name -Credential $cred
	foreach ($n in $files.Keys) {
		$from = $files[$n]; $to = "C:\rac\$n"
		$have = Invoke-Command -Session $s -ArgumentList $to -ScriptBlock { param($t) if (Test-Path $t) { (Get-Item $t).Length } else { -1 } }
		if ($have -eq (Get-Item -LiteralPath $from).Length) { "${n}: there"; continue }
		Invoke-Command -Session $s -ArgumentList $to -ScriptBlock { param($t) New-Item -ItemType Directory -Force (Split-Path $t) | Out-Null }
		$t = Get-Date
		Copy-Item -LiteralPath $from -Destination $to -ToSession $s -Force
		"{0}: {1:n0} s" -f $n, ((Get-Date) - $t).TotalSeconds
	}
	Invoke-Command -Session $s -ScriptBlock { New-Item -ItemType Directory -Force C:\rac\generated | Out-Null; Set-Content C:\rac\generated\play.cfg 'iso=C:\rac\disc.iso' -Encoding ascii }
	Remove-PSSession $s
	exit 0
}
$a = "-Fields $Fields -OnDesktop -Iso C:\rac\disc.iso -Player C:\rac\run -Wait"
if ($Mp) { $a += " -Mp $Mp -Name WinJoiner" }
if ($Pad) { $a += " -Pad '$Pad'" }
if ($Travel) { $a += " -Travel $Travel" }
$job = @"
Get-Process rac-rs -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item -Recurse -Force C:\rac\run, C:\rac\job.done -ErrorAction SilentlyContinue
# The capture layer on the CPU, the picture at half scale in a small window: everything here is drawn on the CPU.
`$env:RAC_GSCAP_GPU = '0'; `$env:RAC_RENDER_SCALE = '50'; `$env:RAC_TEST_SIZE = '640x360'
`$icd = Get-ChildItem 'C:\Program Files (x86)\Microsoft\Edge\Application\*\vk_swiftshader_icd.json' | Select-Object -First 1
Copy-Item (Join-Path `$icd.DirectoryName 'vulkan-1.dll') C:\rac\build\release\vulkan-1.dll -Force
`$env:VK_ICD_FILENAMES = `$icd.FullName; `$env:VK_DRIVER_FILES = `$icd.FullName
& C:\rac\tools\mp_play.ps1 $a *> C:\rac\job.out
'done' | Set-Content C:\rac\job.done
"@
Invoke-Command -VMName $Name -Credential $cred -ArgumentList $job, $kv.user -ScriptBlock {
	param($job, $user)
	Set-Content C:\rac\job.ps1 $job -Encoding utf8
	$act = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File C:\rac\job.ps1'
	$pr = New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\$user" -LogonType Interactive
	Register-ScheduledTask -TaskName RacRun -Action $act -Principal $pr -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit 0) -Force | Out-Null
	Remove-Item C:\rac\job.done -ErrorAction SilentlyContinue
	Start-ScheduledTask -TaskName RacRun
}
"started in the VM: mp_play.ps1 $a"
$end = (Get-Date).AddMinutes($TimeoutMin)
do { Start-Sleep -Seconds 20; $done = Invoke-Command -VMName $Name -Credential $cred -ScriptBlock { Test-Path C:\rac\job.done } } while (-not $done -and (Get-Date) -lt $end)
$text = Invoke-Command -VMName $Name -Credential $cred -ScriptBlock { if (Test-Path C:\rac\run\log.txt) { Get-Content C:\rac\run\log.txt -Raw } else { 'no log; ' + (Get-Content C:\rac\job.out -Raw -ErrorAction SilentlyContinue) } }
Set-Content -Path $Log -Value $text -Encoding utf8
"log saved: $Log$(if (-not $done) { ' (the run did not end in time)' })"
