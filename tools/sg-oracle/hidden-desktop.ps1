# A hidden Win32 desktop (CreateDesktop, same window station WinSta0) for the sandbox exe and our own app under test: windows started
# there never show on the user's monitors and can never take the focus. Dot-sourced by run.ps1 and the *-input.ps1 drivers.
#   . (Join-Path $PSScriptRoot 'hidden-desktop.ps1')
#   $p = Start-SgHidden -Exe $exe -Arguments $args -WorkingDirectory $rt     # -> System.Diagnostics.Process
#   $h = Get-SgWindow -Process $p                                          # the process's top-level window on that desktop (0 if none yet)
# Env SG_VISIBLE=1 (or run.ps1 -Foreground) keeps the old visible launch.
$script:SgDesktopName = 'pob-sg-oracle'
if (-not ('SgHiddenDesktop' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class SgHiddenDesktop {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct STARTUPINFO { public int cb; public string lpReserved; public string lpDesktop; public string lpTitle; public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags; public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError; }
    [StructLayout(LayoutKind.Sequential)]
    struct PROCESS_INFORMATION { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateDesktop(string name, IntPtr device, IntPtr devmode, int flags, uint access, IntPtr sa);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr OpenDesktop(string name, int flags, bool inherit, uint access);
    [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr h);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CreateProcess(string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string cwd, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumDesktopWindows(IntPtr desk, EnumProc fn, IntPtr lParam);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out int pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint cmd);
    const uint GENERIC_ALL = 0x10000000;
    static IntPtr desk = IntPtr.Zero;
    public static IntPtr Ensure(string name) {
        if (desk != IntPtr.Zero) return desk;
        desk = OpenDesktop(name, 0, false, GENERIC_ALL);
        if (desk == IntPtr.Zero) desk = CreateDesktop(name, IntPtr.Zero, IntPtr.Zero, 0, GENERIC_ALL, IntPtr.Zero);
        if (desk == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        return desk;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct SECURITY_ATTRIBUTES { public int nLength; public IntPtr lpSecurityDescriptor; public bool bInheritHandle; }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateFile(string name, uint access, uint share, ref SECURITY_ATTRIBUTES sa, uint disp, uint flags, IntPtr tmpl);
    public static int Start(string name, string exe, string args, string cwd) { return Start(name, exe, args, cwd, null); }
    // errPath: stderr (and stdout) of the child into that file (Start-Process -RedirectStandardError), via an inherited handle
    public static int Start(string name, string exe, string args, string cwd, string errPath) {
        Ensure(name);
        var si = new STARTUPINFO();
        si.cb = Marshal.SizeOf(typeof(STARTUPINFO));
        si.lpDesktop = "WinSta0\\" + name;
        IntPtr err = IntPtr.Zero;
        bool inherit = false;
        if (!string.IsNullOrEmpty(errPath)) {
            var sa = new SECURITY_ATTRIBUTES { nLength = Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES)), bInheritHandle = true };
            // GENERIC_WRITE, FILE_SHARE_READ, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL
            err = CreateFile(errPath, 0x40000000, 1, ref sa, 2, 0x80, IntPtr.Zero);
            if (err == new IntPtr(-1)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            si.dwFlags = 0x100; // STARTF_USESTDHANDLES
            si.hStdError = err; si.hStdOutput = err; si.hStdInput = IntPtr.Zero;
            inherit = true;
        }
        PROCESS_INFORMATION pi;
        var cmd = new StringBuilder("\"" + exe + "\" " + args);
        bool ok = CreateProcess(exe, cmd, IntPtr.Zero, IntPtr.Zero, inherit, 0, IntPtr.Zero, cwd, ref si, out pi);
        int le = Marshal.GetLastWin32Error();
        if (err != IntPtr.Zero) CloseHandle(err);
        if (!ok) throw new System.ComponentModel.Win32Exception(le);
        CloseHandle(pi.hThread); CloseHandle(pi.hProcess);
        return pi.dwProcessId;
    }
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    // the client area w x h at (0,0): a hidden desktop gives a new window a small default size, the scenes expect the visible one
    public static string SizeClient(IntPtr h, int w, int hgt) {
        RECT wr, cr;
        GetWindowRect(h, out wr); GetClientRect(h, out cr);
        int fw = (wr.R - wr.L) - (cr.R - cr.L), fh = (wr.B - wr.T) - (cr.B - cr.T);
        SetWindowPos(h, IntPtr.Zero, 0, 0, w + fw, hgt + fh, 0x14); // SWP_NOZORDER | SWP_NOACTIVATE
        GetClientRect(h, out cr);
        return (cr.R - cr.L) + "x" + (cr.B - cr.T);
    }
    public static IntPtr Window(string name, int pid) {
        IntPtr d = Ensure(name), found = IntPtr.Zero;
        EnumDesktopWindows(d, (h, l) => {
            int p; GetWindowThreadProcessId(h, out p);
            // the top-level, unowned, visible window of that process (what Process.MainWindowHandle gives on the input desktop)
            if (p == pid && IsWindowVisible(h) && GetWindow(h, 4) == IntPtr.Zero) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
}
function Test-SgVisible { $env:SG_VISIBLE -eq '1' }
function Start-SgHidden([string]$Exe, [string]$Arguments, [string]$WorkingDirectory, [string]$RedirectStandardError) {
    if (-not $WorkingDirectory) { $WorkingDirectory = (Get-Location).Path }
    $errPath = if ($RedirectStandardError) { [System.IO.Path]::GetFullPath($RedirectStandardError) } else { $null }
    $id = [SgHiddenDesktop]::Start($script:SgDesktopName, $Exe, $Arguments, $WorkingDirectory, $errPath)
    $p = Get-Process -Id $id
    $null = $p.Handle # keep a handle, so ExitCode stays readable after the exit
    $p
}
# Drop-in for `Start-Process -FilePath .. -ArgumentList @(..) [-WorkingDirectory ..] -PassThru [-RedirectStandardError ..]`:
# the hidden desktop unless SG_VISIBLE=1.
function Start-SgApp {
    param([string]$FilePath, [string[]]$ArgumentList, [string]$WorkingDirectory, [switch]$PassThru, [string]$RedirectStandardError)
    if (Test-SgVisible) {
        $a = @{ FilePath = $FilePath; ArgumentList = $ArgumentList; PassThru = $true }
        if ($WorkingDirectory) { $a.WorkingDirectory = $WorkingDirectory }
        if ($RedirectStandardError) { $a.RedirectStandardError = $RedirectStandardError }
        return Start-Process @a
    }
    Start-SgHidden -Exe $FilePath -Arguments ($ArgumentList -join ' ') -WorkingDirectory $WorkingDirectory -RedirectStandardError $RedirectStandardError
}
# Drop-in for `$Process.Refresh(); $Process.MainWindowHandle`: also finds a window on the hidden desktop.
function Get-SgWindow([System.Diagnostics.Process]$Process) {
    $Process.Refresh()
    if ($Process.MainWindowHandle -ne [IntPtr]::Zero) { return $Process.MainWindowHandle }
    [SgHiddenDesktop]::Window($script:SgDesktopName, $Process.Id)
}
