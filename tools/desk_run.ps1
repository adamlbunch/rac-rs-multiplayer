# desk_run.ps1 - starts a program on a desktop of its own (this fork's test helper): its windows never show on the user's desktop
# and never take the focus, so test games can run while the user works. The desktop (default "racmp-test") is made on first use and
# lasts while a program runs on it. Prints the process id.
#
#   powershell -ExecutionPolicy Bypass -File tools\desk_run.ps1 -Exe C:\path\rac-rs.exe -Args 'view ...' -Dir C:\path
param([Parameter(Mandatory)][string]$Exe, [string]$Args = '', [string]$Dir = '', [string]$Desktop = 'racmp-test')
$ErrorActionPreference = 'Stop'
if (-not ('DeskRun' -as [type])) {
	Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DeskRun {
	[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
	static extern IntPtr CreateDesktop(string name, IntPtr device, IntPtr devmode, int flags, uint access, IntPtr sa);
	[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
	struct StartupInfo {
		public int cb; public string reserved; public string desktop; public string title;
		public int x, y, xSize, ySize, xChars, yChars, fill, flags; public short show, reserved2b;
		public IntPtr reserved2, stdIn, stdOut, stdErr;
	}
	[StructLayout(LayoutKind.Sequential)]
	struct ProcessInfo { public IntPtr process, thread; public int pid, tid; }
	[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
	static extern bool CreateProcess(string app, string cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string dir, ref StartupInfo si, out ProcessInfo pi);
	[DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
	// GENERIC_ALL on the desktop; the handle stays open in this process, and the desktop lasts while anything runs on it.
	public static int Run(string desktop, string exe, string args, string dir) {
		if (CreateDesktop(desktop, IntPtr.Zero, IntPtr.Zero, 0, 0x10000000, IntPtr.Zero) == IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
		var si = new StartupInfo { cb = Marshal.SizeOf(typeof(StartupInfo)), desktop = "WinSta0\\" + desktop };
		ProcessInfo pi;
		if (!CreateProcess(exe, "\"" + exe + "\" " + args, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, string.IsNullOrEmpty(dir) ? null : dir, ref si, out pi)) throw new System.ComponentModel.Win32Exception();
		CloseHandle(pi.thread); CloseHandle(pi.process);
		return pi.pid;
	}
}
'@
}
[DeskRun]::Run($Desktop, $Exe, $Args, $Dir)
