# Удержание сочетания: key-down, пауза, key-up (для push-to-talk).
param(
    [string[]]$Mods = @('Ctrl', 'Shift'),
    [int]$Vk = 0x4D, # M
    [int]$Scan = 0x32, # скан-код M
    [int]$HoldMs = 1500
)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class Hold
{
    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    public struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public MOUSEINPUT mi; }
    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT { public uint type; public INPUTUNION u; }

    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] p, int cb);

    const uint INPUT_KEYBOARD = 1;
    const uint KEYEVENTF_KEYUP = 0x0002;

    static INPUT Key(ushort vk, ushort scan, bool up)
    {
        var i = new INPUT();
        i.type = INPUT_KEYBOARD;
        i.u.ki.wVk = vk;
        i.u.ki.wScan = scan;
        i.u.ki.dwFlags = up ? KEYEVENTF_KEYUP : 0;
        return i;
    }

    public static void Down(ushort vk, ushort scan) { SendInput(1, new[] { Key(vk, scan, false) }, Marshal.SizeOf(typeof(INPUT))); }
    public static void Up(ushort vk, ushort scan) { SendInput(1, new[] { Key(vk, scan, true) }, Marshal.SizeOf(typeof(INPUT))); }
}
"@

$codes = @{ 'Ctrl' = 0x11; 'Shift' = 0x10; 'Alt' = 0x12; 'Win' = 0x5B }
foreach ($m in $Mods) { [Hold]::Down([uint16]$codes[$m], [uint16]$codes[$m]); Start-Sleep -Milliseconds 40 }
[Hold]::Down([uint16]$Vk, [uint16]$Scan)
Start-Sleep -Milliseconds $HoldMs
[Hold]::Up([uint16]$Vk, [uint16]$Scan)
Start-Sleep -Milliseconds 60
foreach ($m in $Mods) { [Hold]::Up([uint16]$codes[$m], [uint16]$codes[$m]); Start-Sleep -Milliseconds 30 }
Write-Host "hold ok: $($Mods -join '+') + vk=$('{0:X2}' -f $Vk) ${HoldMs}ms"
