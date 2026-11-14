# Нажатие Ctrl+Alt+Z по скан-коду (отмена исправления).
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class UndoCombo
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

    static void Send(INPUT i) { SendInput(1, new INPUT[] { i }, Marshal.SizeOf(typeof(INPUT))); }
    static INPUT Vk(ushort vk, bool down) { var i = new INPUT(); i.type = 1; i.u.ki.wVk = vk; i.u.ki.dwFlags = down ? 0u : 2u; return i; }
    static INPUT Scan(ushort scan, bool down) { var i = new INPUT(); i.type = 1; i.u.ki.wScan = scan; i.u.ki.dwFlags = 0x0008 | (down ? 0u : 2u); return i; }

    public static void CtrlAltZ()
    {
        Send(Vk(0x11, true)); System.Threading.Thread.Sleep(40);
        Send(Vk(0x12, true)); System.Threading.Thread.Sleep(40);
        Send(Scan(0x2C, true)); System.Threading.Thread.Sleep(60);
        Send(Scan(0x2C, false)); System.Threading.Thread.Sleep(40);
        Send(Vk(0x12, false)); System.Threading.Thread.Sleep(40);
        Send(Vk(0x11, false));
    }
}
"@
[UndoCombo]::CtrlAltZ()
Write-Host 'Ctrl+Alt+Z отправлен'
