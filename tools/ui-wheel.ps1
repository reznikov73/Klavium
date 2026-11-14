# Прокрутка колесом мыши в текущей позиции курсора (для снятия длинных секций).
param([int]$Notches = 3)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class Wheel
{
    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT { public uint type; public INPUTUNION u; }

    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] p, int cb);

    const uint INPUT_MOUSE = 0;
    const uint MOUSEEVENTF_WHEEL = 0x0800;

    public static void Scroll(int notches)
    {
        var i = new INPUT();
        i.type = INPUT_MOUSE;
        i.u.mi.dwFlags = MOUSEEVENTF_WHEEL;
        i.u.mi.mouseData = unchecked((uint)(notches * 120));
        SendInput(1, new[] { i }, Marshal.SizeOf(typeof(INPUT)));
    }
}
"@

[Wheel]::Scroll($Notches)
