<#
.SYNOPSIS
    Compila NoteTaker en modo release y genera el instalador de Windows.

.DESCRIPTION
    Ejecuta `npm run tauri build`, que encadena:
      1. beforeBuildCommand (tauri.conf.json) -> "npm run build" (tsc + vite build del frontend)
      2. Compilacion del binario Rust en modo release
      3. Empaquetado segun "bundle.targets" en tauri.conf.json ("all" -> NSIS + MSI en Windows)

    La primera ejecucion puede tardar varios minutos: Tauri descarga las
    herramientas de empaquetado (NSIS/WiX) si no las tiene en cache.

.PARAMETER Open
    Al terminar, abre el explorador de Windows en la carpeta de los instaladores.

.EXAMPLE
    .\scripts\build-installer.ps1
.EXAMPLE
    .\scripts\build-installer.ps1 -Open
#>
param(
    [switch]$Open
)

$ErrorActionPreference = "Stop"

# Raiz del repo = carpeta padre de esta carpeta scripts\, sea cual sea el
# directorio desde el que se invoque el script.
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

Write-Host "== NoteTaker: build de release ==" -ForegroundColor Cyan
Write-Host "Repo: $RepoRoot"

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    throw "No se encontro 'npm' en el PATH. Instala Node.js antes de continuar."
}

if (-not (Test-Path (Join-Path $RepoRoot "node_modules"))) {
    Write-Host "Instalando dependencias (npm install)..." -ForegroundColor Yellow
    npm install
    if ($LASTEXITCODE -ne 0) { throw "npm install fallo." }
}

Write-Host ""
Write-Host "Compilando (frontend + Rust release + empaquetado)..." -ForegroundColor Yellow
Write-Host "(La primera vez puede tardar varios minutos: descarga NSIS/WiX.)" -ForegroundColor DarkGray
npm run tauri build
if ($LASTEXITCODE -ne 0) {
    throw "El build de Tauri fallo (revisa la salida de arriba)."
}

$BundleDir = Join-Path $RepoRoot "src-tauri\target\release\bundle"
$RawExe    = Join-Path $RepoRoot "src-tauri\target\release\notetaker.exe"

Write-Host ""
Write-Host "== Build completo ==" -ForegroundColor Green

if (Test-Path $RawExe) {
    Write-Host "Ejecutable suelto (sin instalar): $RawExe"
}

$Installers = @()
if (Test-Path $BundleDir) {
    $Installers = Get-ChildItem -Path $BundleDir -Recurse -Include "*.exe", "*.msi" -ErrorAction SilentlyContinue
}

if ($Installers.Count -gt 0) {
    Write-Host "Instalador(es) generado(s):"
    foreach ($f in $Installers) {
        Write-Host "  - $($f.FullName)"
    }
} else {
    Write-Host "No se encontraron instaladores en $BundleDir." -ForegroundColor Yellow
    Write-Host "Revisa 'bundle.targets' en src-tauri\tauri.conf.json y la salida del build."
}

Write-Host ""
Write-Host "Nota: el motor local (whisper.cpp) NO va dentro del instalador; se descarga" -ForegroundColor DarkGray
Write-Host "desde Ajustes -> Whisper local o con scripts\setup-whisper.ps1." -ForegroundColor DarkGray

if ($Open) {
    $OpenTarget = if (Test-Path $BundleDir) { $BundleDir } else { Split-Path $RawExe -Parent }
    if (Test-Path $OpenTarget) {
        Invoke-Item $OpenTarget
    }
}
