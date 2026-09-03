<#
.SYNOPSIS
  Lanza el binario ya compilado de NoteTaker (sin recompilar).

.DESCRIPTION
  Busca el ejecutable de produccion (src-tauri\target\release\notetaker.exe)
  y, si no existe, el de debug. No compila nada: para compilar usa
  .\run.ps1 -Release.

.EXAMPLE
  .\start.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot

$candidates = @(
    (Join-Path $root "src-tauri\target\release\notetaker.exe"),
    (Join-Path $root "src-tauri\target\debug\notetaker.exe")
)

$exe = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $exe) {
    Write-Host "[X] No hay ningun binario compilado." -ForegroundColor Red
    Write-Host "    Compila primero con:  .\run.ps1 -Release   (produccion)" -ForegroundColor Yellow
    Write-Host "    o ejecuta en dev con: .\run.ps1" -ForegroundColor Yellow
    exit 1
}

if ($exe -like "*\debug\*") {
    Write-Host "[!] Solo hay binario de DEBUG (mas lento). Para produccion: .\run.ps1 -Release" -ForegroundColor Yellow
}

Write-Host "[OK] Lanzando $exe" -ForegroundColor Green
Start-Process $exe
