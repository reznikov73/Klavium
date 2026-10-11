# Сборка дистрибутива Klavium: NSIS-установщик + portable zip.
#
# Использование:
#   pwsh -File tools\build_dist.ps1                # собрать всё (release + установщик + zip)
#   pwsh -File tools\build_dist.ps1 -SkipBuild     # только упаковать уже собранное
#   pwsh -File tools\build_dist.ps1 -Version 0.2.0 # переопределить версию
#
# Требуется: Rust (MSVC), NSIS 3 (makensis в PATH, либо $env:MAKENSIS, либо
# стандартные места установки). LIBCLANG_PATH — как для обычной сборки.

param(
    [string]$Version = "",
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# Версия: из Cargo.toml (workspace.package), если не задана явно.
if (-not $Version) {
    $cargo = Get-Content (Join-Path $root 'Cargo.toml') -Raw
    if ($cargo -match '(?ms)\[workspace\.package\].*?version\s*=\s*"([^"]+)"') {
        $Version = $Matches[1]
    } else {
        $Version = '0.1.0'
    }
}
Write-Host "Версия: $Version"

# 1) Релизная сборка.
if (-not $SkipBuild) {
    if (-not $env:LIBCLANG_PATH) {
        $env:LIBCLANG_PATH = "$env:USERPROFILE\.libclang\clang\native"
    }
    cargo build --release -p klavium-app
    if ($LASTEXITCODE -ne 0) { throw 'cargo build --release завершился с ошибкой' }
}

$exe = Join-Path $root 'target\release\klavium.exe'
if (-not (Test-Path $exe)) { throw "нет релизного бинарника: $exe" }

# 2) NSIS-установщик.
$makensis = $env:MAKENSIS
if (-not $makensis) {
    $cmd = Get-Command makensis -ErrorAction SilentlyContinue
    if ($cmd) { $makensis = $cmd.Source }
}
if (-not $makensis) {
    foreach ($cand in @(
        "$env:USERPROFILE\Tools\nsis-3.10\nsis-3.10\makensis.exe",
        'C:\Program Files (x86)\NSIS\makensis.exe',
        'C:\Program Files\NSIS\makensis.exe'
    )) {
        if (Test-Path $cand) { $makensis = $cand; break }
    }
}
if (-not $makensis) {
    throw "makensis не найден: добавьте NSIS в PATH или задайте `$env:MAKENSIS"
}
Write-Host "NSIS: $makensis"

# NSIS без BOM читает .nsi как ANSI — кириллица превращается в мусор.
# Перекодируем скрипт в UTF-8 с BOM перед сборкой (идемпотентно).
$nsiPath = Join-Path $root 'installer\klavium.nsi'
$nsiText = [System.IO.File]::ReadAllText($nsiPath, [System.Text.Encoding]::UTF8)
[System.IO.File]::WriteAllText($nsiPath, $nsiText, [System.Text.UTF8Encoding]::new($true))

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force -Path $dist | Out-Null

& $makensis "/DVERSION=$Version" "/DSRCDIR=$root\target\release" `
    "/DOUTFILE=$dist\KlaviumSetup-$Version.exe" `
    "/DICONFILE=$root\crates\klavium-app\resources\klavium.ico" `
    (Join-Path $root 'installer\klavium.nsi')
if ($LASTEXITCODE -ne 0) { throw 'makensis завершился с ошибкой' }

# 3) Портативная версия (zip).
$stage = Join-Path $env:TEMP "klavium-portable-$Version"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item $exe $stage

$readme = @"
Klavium $Version — портативная версия (Windows 10/11 x64)

Запуск: klavium.exe. Настройки открываются сразу при ручном запуске;
приложение живёт в трее. Выход: трей -> «Выход» или Ctrl+Shift+Q.

Данные (настройки, статистика, модели, журнал) хранятся в подпапке data\
рядом с exe — папку можно переносить целиком.

Первый запуск: приложение само создаст data\ и config.json.
Модели распознавания скачиваются в Настройках (раздел «Голос»):
Whisper (офлайн), Google (онлайн), Vosk (офлайн).

Горячие клавиши по умолчанию: F8 — исправление раскладки, F9 — диктовка.
"@
Set-Content -Path (Join-Path $stage 'ПРОЧТИ-МЕНЯ.txt') -Value $readme -Encoding utf8

Compress-Archive -Path (Join-Path $stage '*') `
    -DestinationPath (Join-Path $dist "Klavium-$Version-portable.zip") -Force

Write-Host ''
Write-Host 'Готово, содержимое dist:'
Get-ChildItem $dist | ForEach-Object {
    "  {0}  {1} МБ" -f $_.Name, [math]::Round($_.Length / 1MB, 1)
}
