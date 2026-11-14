# Стресс: набор N слов «ghbdtn » скан-кодами в Блокнот (без нажатия хоткея).
param([int]$Words = 20)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class Stress
{
    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT { public uint type; public INPUTUNION u; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] p, int cb);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr LoadKeyboardLayout(string id, uint flags);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr w, IntPtr l);

    static INPUT Scan(ushort scan, bool down)
    {
        var i = new INPUT();
        i.type = 1;
        i.u.ki.wScan = scan;
        i.u.ki.dwFlags = 0x0008 | (down ? 0u : 2u);
        return i;
    }

    public static void TypeScans(ushort[] scans)
    {
        foreach (var s in scans)
        {
            SendInput(1, new INPUT[] { Scan(s, true) }, Marshal.SizeOf(typeof(INPUT)));
            System.Threading.Thread.Sleep(25);
            SendInput(1, new INPUT[] { Scan(s, false) }, Marshal.SizeOf(typeof(INPUT)));
            System.Threading.Thread.Sleep(25);
        }
    }

    public static void SwitchToEnglish()
    {
        var hkl = LoadKeyboardLayout("00000409", 0);
        PostMessage(GetForegroundWindow(), 0x0050, IntPtr.Zero, hkl);
    }

    public static void Focus(IntPtr h) { ShowWindow(h, 9); SetForegroundWindow(h); }
}
"@

$np = Get-Process notepad -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $np) { Start-Process notepad.exe; Start-Sleep -Milliseconds 1500; $np = Get-Process notepad | Select-Object -First 1 }
[Stress]::Focus($np.MainWindowHandle)
Start-Sleep -Milliseconds 400
[Stress]::SwitchToEnglish()
Start-Sleep -Milliseconds 700

$word = [uint16[]]@(0x22, 0x23, 0x30, 0x20, 0x14, 0x31, 0x39)
for ($i = 0; $i -lt $Words; $i++) {
    [Stress]::TypeScans($word)
    Start-Sleep -Milliseconds 120
}
Write-Host ("набрано слов: " + $Words)
