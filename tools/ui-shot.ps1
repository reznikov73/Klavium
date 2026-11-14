# Снимок окна Klavium в PNG (для проверки состояния интерфейса).
param(
    [string]$Out = 'E:\Work\GIGA\GitHub\Klavium\target\klavium-window.png',
    [int]$X = 78, [int]$Y = 78, [int]$W = 796, [int]$H = 599
)
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class Dpi { [DllImport("user32.dll")] public static extern uint GetDpiForSystem(); }
"@
Write-Host ("DPI: " + [Dpi]::GetDpiForSystem())

$bmp = New-Object System.Drawing.Bitmap($W, $H)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($X, $Y, 0, 0, (New-Object System.Drawing.Size($W, $H)))
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose()
$bmp.Dispose()
Write-Host ("saved: " + $Out)
