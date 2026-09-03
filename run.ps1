<#
.SYNOPSIS
  Arranca NoteTaker (transcripcion de reuniones en vivo o desde grabacion).

.DESCRIPTION
  Por defecto ejecuta la app en modo desarrollo (npm run tauri dev, con hot
  reload). Con -Release compila el binario de produccion y lo ejecuta.

.PARAMETER Release
  Compila en modo produccion (npm run tauri build) y lanza el .exe resultante.

.PARAMETER Clean
  Borra node_modules y el target de Rust antes de arrancar (build desde cero).

.EXAMPLE
  .\run.ps1              # desarrollo con hot reload
  .\run.ps1 -Release     # binario de produccion
#>
[CmdletBinding()]
param(
    [switch]$Release,
    [switch]$Clean
)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
Set-Location $root

function Assert-Tool {
    param([string]$Name, [string]$Hint)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        Write-Host "[X] Falta '$Name'. $Hint" -ForegroundColor Red
        exit 1
    }
}

Write-Host "== NoteTaker ==" -ForegroundColor Cyan

# --- Requisitos ---
Assert-Tool node  "Instala Node 22+: https://nodejs.org"
Assert-Tool npm   "Se instala junto con Node."
Assert-Tool cargo "Instala Rust: https://rustup.rs"

$nodeMajor = [int]((node --version).TrimStart("v").Split(".")[0])
if ($nodeMajor -lt 20) {
    Write-Host "[X] Node $(node --version) es demasiado antiguo (se necesita 20+)." -ForegroundColor Red
    exit 1
}
Write-Host "[OK] node $(node --version) - $(cargo --version)" -ForegroundColor Green

# --- Limpieza opcional ---
if ($Clean) {
    Write-Host "Limpiando node_modules y target de Rust..." -ForegroundColor Yellow
    if (Test-Path "$root\node_modules")     { Remove-Item -Recurse -Force "$root\node_modules" }
    if (Test-Path "$root\src-tauri\target") { Remove-Item -Recurse -Force "$root\src-tauri\target" }
}

# --- Dependencias npm ---
if (-not (Test-Path "$root\node_modules")) {
    Write-Host "Instalando dependencias npm..." -ForegroundColor Yellow
    npm install
    if ($LASTEXITCODE -ne 0) { Write-Host "[X] npm install fallo." -ForegroundColor Red; exit 1 }
}

# --- Ejecucion ---
if ($Release) {
    Write-Host "Compilando binario de produccion (esto tarda varios minutos)..." -ForegroundColor Yellow
    npm run tauri build
    if ($LASTEXITCODE -ne 0) { Write-Host "[X] El build de produccion fallo." -ForegroundColor Red; exit 1 }

    $exe = Join-Path $root "src-tauri\target\release\notetaker.exe"
    if (-not (Test-Path $exe)) {
        Write-Host "[X] No se encontro el ejecutable en $exe" -ForegroundColor Red
        exit 1
    }
    Write-Host "[OK] Lanzando $exe" -ForegroundColor Green
    Start-Process $exe
}
else {
    Write-Host "Arrancando en modo desarrollo (Ctrl+C para salir)..." -ForegroundColor Yellow
    Write-Host "Primer uso: abre Ajustes y guarda la API key de Deepgram y la del proveedor de IA," -ForegroundColor DarkGray
    Write-Host "o instala el motor local con .\scripts\setup-whisper.ps1 (tambien desde Ajustes)." -ForegroundColor DarkGray
    npm run tauri dev
    exit $LASTEXITCODE
}
