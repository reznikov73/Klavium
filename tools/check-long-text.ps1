# Временная проверка: набор длинного русского текста «в английской раскладке»
# и исправление выделенного через Ctrl+Alt+X.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Typist
{
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION u; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] p, int cb);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint thread);
    [DllImport("user32.dll")] public static extern IntPtr LoadKeyboardLayout(string id, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();

    static void Send(INPUT i) { SendInput(1, new INPUT[] { i }, Marshal.SizeOf(typeof(INPUT))); }
    static INPUT Vk(ushort vk, bool down) { var i = new INPUT(); i.type = 1; i.u.ki.wVk = vk; i.u.ki.dwFlags = down ? 0u : 2u; return i; }
    static INPUT Scan(ushort scan, bool down) { var i = new INPUT(); i.type = 1; i.u.ki.wScan = scan; i.u.ki.dwFlags = 0x0008 | (down ? 0u : 2u); return i; }

    public static void Force(IntPtr h) {
        uint pid; uint ft = GetWindowThreadProcessId(GetForegroundWindow(), out pid);
        uint ot = GetCurrentThreadId();
        AttachThreadInput(ot, ft, true); BringWindowToTop(h); SetForegroundWindow(h); AttachThreadInput(ot, ft, false);
    }
    public static string Layout(IntPtr h) {
        uint pid; uint t = GetWindowThreadProcessId(h, out pid);
        return ((uint)(GetKeyboardLayout(t).ToInt64() & 0xFFFF)).ToString("X4");
    }
    public static void SetEn(IntPtr h) {
        var hkl = LoadKeyboardLayout("00000409", 0x00000001);
        PostMessage(h, 0x0050, IntPtr.Zero, hkl);
    }
    public static void TypeAll(byte[] scans, byte[] shifts, int delayMs)
    {
        for (int i = 0; i < scans.Length; i++)
        {
            if (shifts[i] != 0) { Send(Scan(0x2A, true)); System.Threading.Thread.Sleep(40); }
            Send(Scan(scans[i], true));
            System.Threading.Thread.Sleep(delayMs);
            Send(Scan(scans[i], false));
            System.Threading.Thread.Sleep(delayMs);
            if (shifts[i] != 0) { Send(Scan(0x2A, false)); System.Threading.Thread.Sleep(12); }
        }
    }
    public static void Combo(ushort mod1, ushort mod2, ushort scan)
    {
        Send(Vk(mod1, true)); System.Threading.Thread.Sleep(40);
        if (mod2 != 0) { Send(Vk(mod2, true)); System.Threading.Thread.Sleep(40); }
        Send(Scan(scan, true)); System.Threading.Thread.Sleep(60);
        Send(Scan(scan, false)); System.Threading.Thread.Sleep(40);
        if (mod2 != 0) { Send(Vk(mod2, false)); System.Threading.Thread.Sleep(40); }
        Send(Vk(mod1, false));
    }
}
'@

function Pump { param([int]$Ms = 400)
    $deadline = [DateTime]::Now.AddMilliseconds($Ms)
    while ([DateTime]::Now -lt $deadline) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 20 }
}

$source = @'
– Ах, не говорите мне про Австрию! Я ничего не понимаю, может быть, но Австрия никогда не хотела и не хочет войны. Она предает нас. Россия одна должна быть спасительницей Европы. Наш благодетель знает свое высокое призвание и будет верен ему. Вот одно, во что я верю. Нашему доброму и чудному государю предстоит величайшая роль в мире, и он так добродетелен и хорош, что Бог не оставит его, и он исполнит свое призвание задавить гидру революции, которая теперь еще ужаснее в лице этого убийцы и злодея. Мы одни должны искупить кровь праведника. На кого нам надеяться, я вас спрашиваю?.. Англия с своим коммерческим духом не поймет и не может понять всю высоту души императора Александра. Она отказалась очистить Мальту. Она хочет видеть, ищет заднюю мысль наших действий. Что они сказали Новосильцеву? Ничего. Они не поняли, они не могут понять самоотвержения нашего императора, который ничего не хочет для себя и все хочет для блага мира. И что они обещали? Ничего. И что обещали, и того не будет! Пруссия уже объявила, что Бонапарте непобедим и что вся Европа ничего не может против него… И я не верю ни в одном слове ни Гарденбергу, ни Гаугвицу. Я верю в одного Бога и в высокую судьбу нашего милого императора. Он спасет Европу!.. – Она вдруг остановилась с улыбкой насмешки над своею горячностью.
'@

# Ожидаемый результат: буквы русские, кавычки — обычные, тире — дефис.
$expected = $source.
    Replace([string][char]0x00AB, '"').Replace([string][char]0x00BB, '"').
    Replace([string][char]0x2013, '-').Replace([string][char]0x2014, '-').
    Replace([string][char]0x2026, '...')

# Что набирает человек, печатающий этот текст в английской раскладке:
# кавычка — это Shift+2, то есть в EN получается '@'.
$normalized = $source.
    Replace([string][char]0x00AB, '@').Replace([string][char]0x00BB, '@').
    Replace([string][char]0x2013, '-').Replace([string][char]0x2014, '-').
    Replace([string][char]0x2026, '...')

$ruToKey = @{
    ([string][char]0x439) = 'q'; ([string][char]0x446) = 'w'; ([string][char]0x443) = 'e'; ([string][char]0x43A) = 'r'
    ([string][char]0x435) = 't'; ([string][char]0x43D) = 'y'; ([string][char]0x433) = 'u'; ([string][char]0x448) = 'i'
    ([string][char]0x449) = 'o'; ([string][char]0x437) = 'p'; ([string][char]0x445) = '['; ([string][char]0x44A) = ']'
    ([string][char]0x444) = 'a'; ([string][char]0x44B) = 's'; ([string][char]0x432) = 'd'; ([string][char]0x430) = 'f'
    ([string][char]0x43F) = 'g'; ([string][char]0x440) = 'h'; ([string][char]0x43E) = 'j'; ([string][char]0x43B) = 'k'
    ([string][char]0x434) = 'l'; ([string][char]0x436) = ';'; ([string][char]0x44D) = "'"
    ([string][char]0x44F) = 'z'; ([string][char]0x447) = 'x'; ([string][char]0x441) = 'c'; ([string][char]0x43C) = 'v'
    ([string][char]0x438) = 'b'; ([string][char]0x442) = 'n'; ([string][char]0x44C) = 'm'; ([string][char]0x431) = ','
    ([string][char]0x44E) = '.'; ([string][char]0x451) = '`'
}

$shifted = @{
    '~' = '`'; '!' = '1'; '@' = '2'; '#' = '3'; '$' = '4'; '%' = '5'; '^' = '6'; '&' = '7'; '*' = '8'
    '(' = '9'; ')' = '0'; '_' = '-'; '+' = '='; '{' = '['; '}' = ']'; ':' = ';'; '"' = "'"
    '<' = ','; '>' = '.'; '?' = '/'
}

$scan = @{
    '1' = 0x02; '2' = 0x03; '3' = 0x04; '4' = 0x05; '5' = 0x06; '6' = 0x07; '7' = 0x08; '8' = 0x09
    '9' = 0x0A; '0' = 0x0B; '-' = 0x0C; '=' = 0x0D
    'q' = 0x10; 'w' = 0x11; 'e' = 0x12; 'r' = 0x13; 't' = 0x14; 'y' = 0x15; 'u' = 0x16; 'i' = 0x17
    'o' = 0x18; 'p' = 0x19; '[' = 0x1A; ']' = 0x1B
    'a' = 0x1E; 's' = 0x1F; 'd' = 0x20; 'f' = 0x21; 'g' = 0x22; 'h' = 0x23; 'j' = 0x24; 'k' = 0x25
    'l' = 0x26; ';' = 0x27; "'" = 0x28; '`' = 0x29
    'z' = 0x2C; 'x' = 0x2D; 'c' = 0x2E; 'v' = 0x2F; 'b' = 0x30; 'n' = 0x31; 'm' = 0x32
    ',' = 0x33; '.' = 0x34; '/' = 0x35; ' ' = 0x39
}

$punct = @{ '.' = '/'; ',' = '?'; '?' = '&'; '!' = '!'; '-' = '-'; '"' = '"' }

$scans = New-Object System.Collections.Generic.List[byte]
$shifts = New-Object System.Collections.Generic.List[byte]
$intended = New-Object System.Text.StringBuilder
$unknown = @{}

foreach ($ch in $normalized.ToCharArray()) {
    $s = [string]$ch
    if ($ch -eq ' ') { $scans.Add([byte]$scan[' ']); $shifts.Add([byte]0); $null = $intended.Append(' '); continue }
    if ($ch -match '^[0-9]$') { $scans.Add([byte]$scan[$s]); $shifts.Add([byte]0); continue }

    if ($ruToKey.ContainsKey($s.ToLower())) {
        # В PowerShell хеш-таблицы регистронезависимы: регистр решаем сами.
        $base = $ruToKey[$s.ToLower()]
        if ([char]::IsUpper($ch)) {
            $candidate = ($shifted.GetEnumerator() | Where-Object { $_.Value -eq $base } | Select-Object -First 1).Key
            $mapped = if ($candidate) { $candidate } else { $base.ToUpperInvariant() }
        } else {
            $mapped = $base
        }
    } elseif ($punct.ContainsKey($s)) {
        $mapped = $punct[$s]
    } else {
        $mapped = $s
    }

    $isShift = 0
    if ($shifted.ContainsKey($mapped)) {
        $code = $scan[$shifted[$mapped]]
        $isShift = 1
    } elseif ($mapped.Length -eq 1 -and [char]::IsUpper([char]$mapped)) {
        $code = $scan[$mapped.ToLowerInvariant()]
        $isShift = 1
    } elseif ($scan.ContainsKey($mapped)) {
        $code = $scan[$mapped]
    } else {
        $unknown[$s] = $true
        continue
    }
    $scans.Add([byte]$code)
    $shifts.Add([byte]$isShift)
    $null = $intended.Append($mapped)
}

"символов к набору: $($scans.Count), неизвестных: $($unknown.Keys -join ',')"

$p = Start-Process notepad -PassThru
Start-Sleep -Milliseconds 2500
$p.Refresh()
$h = $p.MainWindowHandle
[Typist]::Force($h); Start-Sleep -Milliseconds 600
[Typist]::SetEn($h); Start-Sleep -Milliseconds 700
"раскладка окна: $([Typist]::Layout($h))"

$sw = [System.Diagnostics.Stopwatch]::StartNew()
[Typist]::TypeAll($scans.ToArray(), $shifts.ToArray(), 6)
$sw.Stop()
"набор: $([int]$sw.Elapsed.TotalSeconds) с"
Pump -Ms 1200

[Typist]::Combo(0x11, 0x00, 0x1E); Start-Sleep -Milliseconds 400
[Typist]::Combo(0x11, 0x00, 0x2E); Start-Sleep -Milliseconds 700
$typed = Get-Clipboard -Raw
Set-Content -Path "$env:TEMP\klavium-typed.txt" -Value $typed -Encoding utf8
Set-Content -Path "$env:TEMP\klavium-expected.txt" -Value $expected -Encoding utf8
"набрано символов: $($typed.Length), задумано: $($normalized.Length)"
$intendedText = $intended.ToString()
Set-Content -Path "$env:TEMP\klavium-intended.txt" -Value $intendedText -Encoding utf8
$diff = 0
$limit = [Math]::Min($typed.Length, $intendedText.Length)
for ($i = 0; $i -lt $limit; $i++) { if ($typed[$i] -cne $intendedText[$i]) { $diff++ } }
"расхождений с задуманным при наборе: $diff"

[Typist]::Combo(0x11, 0x00, 0x1E); Start-Sleep -Milliseconds 500
[Typist]::Combo(0x11, 0x12, 0x2D)
Start-Sleep -Milliseconds 3000
Pump -Ms 800

[Typist]::Combo(0x11, 0x00, 0x1E); Start-Sleep -Milliseconds 400
[Typist]::Combo(0x11, 0x00, 0x2E); Start-Sleep -Milliseconds 800
$result = Get-Clipboard -Raw
Set-Content -Path "$env:TEMP\klavium-result.txt" -Value $result -Encoding utf8

"результат сохранён: $env:TEMP\klavium-result.txt ($($result.Length) символов)"

Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue

# Сверка результата с ожидаемым (заглавные буквы и знаки важны).
$diffCount = 0
$firstDiff = -1
$limit2 = [Math]::Min($result.Length, $expected.Length)
for ($i = 0; $i -lt $limit2; $i++) {
    if ($result[$i] -cne $expected[$i]) {
        if ($firstDiff -lt 0) { $firstDiff = $i }
        $diffCount++
    }
}
$diffCount += [Math]::Abs($result.Length - $expected.Length)

if ($diffCount -eq 0) {
    "OK: текст восстановлен полностью ($($result.Length) символов, 0 расхождений)"
    exit 0
}

$from = [Math]::Max(0, $firstDiff - 30)
$len = [Math]::Min(80, $expected.Length - $from)
"FAIL: расхождений $diffCount, первое на позиции $firstDiff"
"ожидалось: '" + $expected.Substring($from, $len) + "'"
"получено:  '" + $result.Substring($from, [Math]::Min($len, $result.Length - $from)) + "'"
exit 1
