# Живая проверка транслитерации (7.7): Ctrl+Alt+T на набираемом слове.
#
# Цель — собственное окно WinForms с многострочным полем. Слово набирается
# физическими клавишами (скан-коды, как в tools\stress-typing.ps1), поэтому
# события доходят и до поля, и до хука Klavium; затем нажимается Ctrl+Alt+T,
# движок транслитерирует слово и переключает раскладку.
#
# Направление проверки выбирается по пробной клавише: `p` даёт `p`
# в английской раскладке и `з` — в русской. Так проверка не зависит от того,
# какая раскладка была включена до запуска.
#
# Новый Блокнот Windows 11 не принимает синтетический ввод, поэтому цель —
# своё поле; путь «выделение» покрыт тестами движка и общим с 7.2 механизмом.
#
# Требуется запущенный Klavium (установленная копия) с хоткеем Ctrl+Alt+T.
# Код выхода: 0 — все проверки прошли, 1 — нет.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class TranslitKeys
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

    static void Send(INPUT i) { SendInput(1, new INPUT[] { i }, Marshal.SizeOf(typeof(INPUT))); }
    static INPUT Vk(ushort vk, bool down) { var i = new INPUT(); i.type = 1; i.u.ki.wVk = vk; i.u.ki.dwFlags = down ? 0u : 2u; return i; }
    static INPUT Scan(ushort scan, bool down) { var i = new INPUT(); i.type = 1; i.u.ki.wScan = scan; i.u.ki.dwFlags = 0x0008 | (down ? 0u : 2u); return i; }

    /// Нажать клавишу по скан-коду: символ зависит от текущей раскладки.
    public static void Key(ushort scan)
    {
        Send(Scan(scan, true)); System.Threading.Thread.Sleep(25);
        Send(Scan(scan, false)); System.Threading.Thread.Sleep(25);
    }

    /// Ctrl+Alt+T: хоткей транслитерации. Клавиша — по скан-коду (0x14),
    /// иначе Trigger::Char в хуке не сработает.
    public static void CtrlAltT()
    {
        Send(Vk(0x11, true)); System.Threading.Thread.Sleep(40);
        Send(Vk(0x12, true)); System.Threading.Thread.Sleep(40);
        Send(Scan(0x14, true)); System.Threading.Thread.Sleep(60);
        Send(Scan(0x14, false)); System.Threading.Thread.Sleep(40);
        Send(Vk(0x12, false)); System.Threading.Thread.Sleep(40);
        Send(Vk(0x11, false));
    }
}
'@

$scan = @{
    'p' = 0x19; 'r' = 0x13; 'i' = 0x17; 'v' = 0x2F; 'e' = 0x12; 't' = 0x14
    'g' = 0x22; 'h' = 0x23; 'b' = 0x30; 'd' = 0x20; 'n' = 0x31
    ' ' = 0x39
}

function Type-Scans {
    param([string]$Text)
    foreach ($ch in $Text.ToCharArray()) {
        $code = $scan[[string]$ch]
        if ($null -eq $code) { throw "нет скан-кода для '$ch'" }
        [TranslitKeys]::Key([ushort]$code)
    }
}

function Pump-Messages {
    param([int]$Milliseconds = 400)
    $deadline = [DateTime]::Now.AddMilliseconds($Milliseconds)
    while ([DateTime]::Now -lt $deadline) {
        [System.Windows.Forms.Application]::DoEvents()
        Start-Sleep -Milliseconds 20
    }
}

$form = New-Object System.Windows.Forms.Form
$form.Text = 'Проверка транслитерации'
$form.Width = 560
$form.Height = 220
$form.StartPosition = 'CenterScreen'
$box = New-Object System.Windows.Forms.TextBox
$box.Multiline = $true
$box.Dock = 'Fill'
$box.Font = New-Object System.Drawing.Font('Consolas', 14)
$form.Controls.Add($box)
$form.Show()
$form.Activate()
$box.Focus()
Pump-Messages -Milliseconds 600

$failed = 0

function Test-Translit {
    param([string]$Name)
    # 1. Пробная клавиша: в какой раскладке сейчас система.
    $box.Clear()
    $box.Focus()
    Type-Scans 'p'
    Pump-Messages -Milliseconds 350
    $probe = $box.Text
    # 2. Терминатор: движок забывает слово, поле очищается.
    Type-Scans ' '
    Pump-Messages -Milliseconds 200
    $box.Clear()

    # 3. Набор слова физическими клавишами и хоткей.
    if ($probe -eq 'p') {
        $keys = 'privet'
        $expected = 'привет'
    } elseif ($probe -eq 'з') {
        $keys = 'ghbdtn'
        $expected = 'privet'
    } else {
        Write-Host "$Name FAIL: не удалось определить раскладку (проба '$probe')"
        $script:failed++
        return
    }

    Type-Scans $keys
    Pump-Messages -Milliseconds 400
    $typed = $box.Text
    [TranslitKeys]::CtrlAltT()
    Pump-Messages -Milliseconds 1200

    if ($box.Text.Trim() -eq $expected) {
        Write-Host "$Name ok: $typed -> $expected"
    } else {
        Write-Host "$Name FAIL: $typed -> '$($box.Text)' (ожидали '$expected')"
        $script:failed++
    }
}

Test-Translit -Name 'слово по хоткею'
Test-Translit -Name 'слово по хоткею (повтор)'

$form.Close()

if ($failed -eq 0) {
    Write-Host 'Транслитерация: все проверки прошли'
    exit 0
}
Write-Host "Транслитерация: провалов — $failed"
exit 1
