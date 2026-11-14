# Снимок окна Klavium с прокруткой содержимого.
# Находит окно настроек по PID, ставит курсор в центр контента, крутит колесо
# и делает снимок ровно по прямоугольнику окна.
param(
    [int]$Notches = 0,
    [int]$Width = 0,
    [int]$Height = 0,
    [Parameter(Mandatory = $true)][string]$Out
)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class Win
{
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT { public uint type; public INPUTUNION u; }

    delegate bool EnumProc(IntPtr h, IntPtr p);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr p);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] p, int cb);

    const uint INPUT_MOUSE = 0;
    const uint MOUSEEVENTF_WHEEL = 0x0800;

    public static IntPtr FirstWindow(uint target)
    {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, p) =>
        {
            uint pid;
            GetWindowThreadProcessId(h, out pid);
            if (pid != target || !IsWindowVisible(h)) return true;
            var sb = new StringBuilder(256);
            GetWindowTextW(h, sb, 256);
            if (sb.Length == 0) return true;
            found = h;
            return false;
        }, IntPtr.Zero);
        return found;
    }

    public static void Scroll(int notches)
    {
        var i = new INPUT();
        i.type = INPUT_MOUSE;
        i.u.mi.dwFlags = MOUSEEVENTF_WHEEL;
        i.u.mi.mouseData = unchecked((uint)(notches * 120));
        SendInput(1, new[] { i }, Marshal.SizeOf(typeof(INPUT)));
    }
}
'@

$proc = Get-Process -Name klavium -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) { throw 'Klavium не запущен' }

$hwnd = [Win]::FirstWindow([uint32]$proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { throw 'Окно настроек Klavium не найдено' }

[Win]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 400

if ($Width -gt 0 -and $Height -gt 0) {
    # SWP_NOMOVE = 0x0002, SWP_NOZORDER = 0x0004
    [Win]::SetWindowPos($hwnd, [IntPtr]::Zero, 0, 0, $Width, $Height, 0x0006) | Out-Null
    Start-Sleep -Milliseconds 600
}

$rect = New-Object Win+RECT
[Win]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top

# Курсор — в область контента (правее сайдбара, ниже шапки).
[Win]::SetCursorPos(($rect.Left + [int]($w * 0.6)), ($rect.Top + [int]($h * 0.5))) | Out-Null
Start-Sleep -Milliseconds 250

if ($Notches -ne 0) {
    [Win]::Scroll($Notches)
    Start-Sleep -Milliseconds 500
}

& "$PSScriptRoot\ui-shot.ps1" -X $rect.Left -Y $rect.Top -W $w -H $h -Out $Out | Out-Null
Write-Host "window=$($rect.Left),$($rect.Top) ${w}x${h}"
