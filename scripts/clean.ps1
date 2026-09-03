<#
.SYNOPSIS
    Limpia los artefactos de build de NoteTaker.

.DESCRIPTION
    Borra node_modules, dist y src-tauri\target. Con -Data borra ademas la
    carpeta de datos de la app (grabaciones, modelos whisper, base de datos),
    previa confirmacion.

.PARAMETER Data
    Borra tambien %LOCALAPPDATA%\com.efsteps.notetaker y %APPDATA%\com.efsteps.notetaker.

.EXAMPLE
    .\scripts\clean.ps1
    .\scripts\clean.ps1 -Data
#>
param(
    [switch]$Data
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot

foreach ($rel in @("node_modules", "dist", "src-tauri\target")) {
    $p = Join-Path $RepoRoot $rel
    if (Test-Path $p) {
        Write-Host "Borrando $p ..." -ForegroundColor Yellow
        Remove-Item -Recurse -Force $p
    }
}

if ($Data) {
    $dirs = @(
        (Join-Path $env:LOCALAPPDATA "com.efsteps.notetaker"),
        (Join-Path $env:APPDATA "com.efsteps.notetaker")
    )
    Write-Host ""
    Write-Host "Se borraran TODOS los datos de la app (grabaciones, modelos, base de datos):" -ForegroundColor Red
    $dirs | ForEach-Object { Write-Host "  $_" }
    $answer = Read-Host "Escribe SI para confirmar"
    if ($answer -eq "SI") {
        foreach ($d in $dirs) {
            if (Test-Path $d) { Remove-Item -Recurse -Force $d; Write-Host "Borrado $d" }
        }
    } else {
        Write-Host "Cancelado." -ForegroundColor Yellow
    }
}

Write-Host "[OK] Limpieza completa." -ForegroundColor Green
