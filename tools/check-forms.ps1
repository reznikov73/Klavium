# Живая проверка морфологии по основе (7.16): словоформа вне словаря исправляется.
#
# Цель — собственное окно WinForms: новый Блокнот Windows 11 не принимает
# синтетический ввод, а чужие окна трогать нельзя (в них бывают живые данные).
# Слово набирается физическими клавишами (скан-коды, как в
# tools\stress-typing.ps1), поэтому события доходят и до поля, и до хука Klavium.
#
# Проверяемый случай: «раскладке» — в словаре только начальная форма
# («раскладка»), формы «раскладке» нет. До 0.3.3 движок оценивал её
# эвристикой (0.45 < порог 0.7) и не исправлял; после — по основе (0.95).
#
# Требуется запущенный Klavium (установленная копия). Код выхода: 0 — прошло.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class FormKeys
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
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr LoadKeyboardLayout(string id, uint flags);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();

    /// Взять фокус силой: без AttachThreadInput чужое активное окно не отдаёт его.
    public static void ForceForeground(IntPtr h)
    {
        var fg = GetForegroundWindow();
        var tid = GetCurrentThreadId();
        var fgTid = GetWindowThreadProcessId(fg, IntPtr.Zero);
        AttachThreadInput(tid, fgTid, true);
        SetForegroundWindow(h);
        AttachThreadInput(tid, fgTid, false);
    }

    static void Send(INPUT i) { SendInput(1, new INPUT[] { i }, Marshal.SizeOf(typeof(INPUT))); }
    static INPUT Scan(ushort scan, bool down) { var i = new INPUT(); i.type = 1; i.u.ki.wScan = scan; i.u.ki.dwFlags = 0x0008 | (down ? 0u : 2u); return i; }

    /// Нажать клавишу по скан-коду: символ зависит от текущей раскладки.
    public static void Key(ushort scan)
    {
        Send(Scan(scan, true)); System.Threading.Thread.Sleep(25);
        Send(Scan(scan, false)); System.Threading.Thread.Sleep(25);
    }

    /// Переключить раскладку окна на английскую (WM_INPUTLANGCHANGEREQUEST).
    public static void SwitchToEnglish(IntPtr h)
    {
        var hkl = LoadKeyboardLayout("00000409", 0);
        PostMessage(h, 0x0050, IntPtr.Zero, hkl);
    }

    static INPUT Vk(ushort vk, bool down) { var i = new INPUT(); i.type = 1; i.u.ki.wVk = vk; i.u.ki.dwFlags = down ? 0u : 2u; return i; }

    /// Ctrl+Shift+Z — хоткей исправления. Z — по скан-коду (0x2C), иначе
    /// `Trigger::Char` в хуке не сработает.
    public static void CtrlShiftZ()
    {
        Send(Vk(0x11, true)); System.Threading.Thread.Sleep(40);
        Send(Vk(0x10, true)); System.Threading.Thread.Sleep(40);
        Send(Scan(0x2C, true)); System.Threading.Thread.Sleep(60);
        Send(Scan(0x2C, false)); System.Threading.Thread.Sleep(40);
        Send(Vk(0x10, false)); System.Threading.Thread.Sleep(40);
        Send(Vk(0x11, false));
    }
}
'@

# Скан-коды для «hfcrkflrt» — это «раскладке», набранное в английской раскладке.
$scan = @{
    'h' = 0x23; 'f' = 0x21; 'c' = 0x2E; 'r' = 0x13; 'k' = 0x25
    'l' = 0x26; 't' = 0x14; ' ' = 0x39
}

function Type-Scans {
    param([string]$Text)
    foreach ($ch in $Text.ToCharArray()) {
        $code = $scan[[string]$ch]
        if ($null -eq $code) { throw "нет скан-кода для '$ch'" }
        [FormKeys]::Key([ushort]$code)
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
$form.Text = 'Проверка словоформ'
$form.Width = 640
$form.Height = 180
$form.StartPosition = 'CenterScreen'
$form.TopMost = $true
$box = New-Object System.Windows.Forms.TextBox
$box.Multiline = $true
$box.Dock = 'Fill'
$box.Font = New-Object System.Drawing.Font('Consolas', 14)
$form.Controls.Add($box)
$form.Show()
$form.Activate()
$box.Focus()
Pump-Messages -Milliseconds 500

# Пробная клавиша: 'h' в английской раскладке даёт 'h', в русской — 'р'.
# Фокус берётся силой и с повторами: сразу после старта приложения
# активным может остаться чужое окно.
$probe = ''
for ($attempt = 1; $attempt -le 4; $attempt++) {
    [FormKeys]::ForceForeground($form.Handle)
    $box.Focus()
    Pump-Messages -Milliseconds 300
    [FormKeys]::SwitchToEnglish($form.Handle)
    Pump-Messages -Milliseconds 400

    $box.Clear()
    Type-Scans 'h'
    Pump-Messages -Milliseconds 300
    $probe = $box.Text
    # Терминатор: иначе проба приклеится к следующему слову («hhfcrkflrt»).
    Type-Scans ' '
    Pump-Messages -Milliseconds 300
    $box.Clear()
    Pump-Messages -Milliseconds 200
    if ($probe -eq 'h') { break }
    Start-Sleep -Milliseconds 700
}

if ($probe -ne 'h') {
    $form.Close()
    Write-Host "Словоформы FAIL: не удалось включить английскую раскладку (проба '$probe')"
    exit 1
}

Type-Scans 'hfcrkflrt '
Pump-Messages -Milliseconds 800
Write-Host ("после набора: «" + $box.Text + "»")

# Автокоррекция у пользователя может быть выключена — тогда исправляет хоткей.
if ($box.Text.Trim() -eq 'hfcrkflrt') {
    [FormKeys]::CtrlShiftZ()
    Pump-Messages -Milliseconds 1500
    Write-Host ("после хоткея: «" + $box.Text + "»")
}

$actual = $box.Text
$form.Close()

if ($actual.Trim() -eq 'раскладке') {
    Write-Host "Словоформы ok: 'hfcrkflrt' -> '$($actual.Trim())'"
    exit 0
}
Write-Host "Словоформы FAIL: в поле '$actual' (ожидали 'раскладке')"
exit 1
