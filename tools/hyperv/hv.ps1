# hv.ps1 - the Hyper-V test VM from the host (hv_create.ps1 made it): `shot OUT.png` saves its screen, `run SCRIPT.ps1` runs a
# script in it through PowerShell Direct (no network needed), `wait` waits until that works (the install is done).
param([Parameter(Position = 0)][string]$Cmd, [Parameter(Position = 1)][string]$Arg, [string]$Name = 'RacTest', [string]$Dir = "$env:USERPROFILE\vm\hyperv", [int]$Minutes = 60)
$ErrorActionPreference = 'Stop'
function Cred {
	$kv = @{}; Get-Content "$Dir\vm-account.txt" | ForEach-Object { $k, $v = $_ -split '=', 2; $kv[$k] = $v }
	New-Object System.Management.Automation.PSCredential("RACVM\$($kv.user)", (ConvertTo-SecureString $kv.password -AsPlainText -Force))
}
switch ($Cmd) {
	'shot' {
		$svc = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_VirtualSystemManagementService
		$cs = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_ComputerSystem -Filter "ElementName='$Name'"
		$set = Get-CimAssociatedInstance -InputObject $cs -ResultClassName Msvm_VirtualSystemSettingData | Where-Object VirtualSystemType -eq 'Microsoft:Hyper-V:System:Realized'
		$r = Invoke-CimMethod -InputObject $svc -MethodName GetVirtualSystemThumbnailImage -Arguments @{ TargetSystem = $set; WidthPixels = [uint16]1024; HeightPixels = [uint16]768 }
		Add-Type -AssemblyName System.Drawing
		$bmp = New-Object System.Drawing.Bitmap 1024, 768, ([System.Drawing.Imaging.PixelFormat]::Format16bppRgb565)
		$data = $bmp.LockBits((New-Object System.Drawing.Rectangle 0, 0, 1024, 768), 'WriteOnly', $bmp.PixelFormat)
		$bytes = [byte[]]$r.ImageData
		[System.Runtime.InteropServices.Marshal]::Copy($bytes, 0, $data.Scan0, [Math]::Min($bytes.Length, $data.Stride * 768))
		$bmp.UnlockBits($data); $bmp.Save($Arg, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
		"saved $Arg"
	}
	'run' { Invoke-Command -VMName $Name -Credential (Cred) -FilePath $Arg }
	'wait' {
		$end = (Get-Date).AddMinutes($Minutes)
		while ((Get-Date) -lt $end) {
			if ((Get-VM $Name).State -ne 'Running') { "the VM is $((Get-VM $Name).State)"; exit 1 }
			try { Invoke-Command -VMName $Name -Credential (Cred) -ScriptBlock { "$env:COMPUTERNAME up, $((Get-CimInstance Win32_OperatingSystem).Caption)" } -ErrorAction Stop; exit 0 } catch {}
			Start-Sleep -Seconds 30
		}
		"not reachable in $Minutes min"; exit 1
	}
	default { "usage: hv.ps1 shot OUT.png | run SCRIPT.ps1 | wait" }
}
