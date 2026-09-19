<#
.SYNOPSIS
  Compila NoteTaker para produccion: genera el .exe final y los instaladores.

.DESCRIPTION
  Encadena todo el build de Tauri:
    1. npm run build   -> tsc + vite (frontend a dist\)
    2. cargo build --release  -> binario Rust optimizado
    3. Empaquetado NSIS + MSI (segun "bundle.targets" en tauri.conf.json)

  Resultado:
    src-tauri\target\release\notetaker.exe          <- ejecutable suelto
    src-tauri\target\release\bundle\nsis\*.exe      <- instalador
    src-tauri\target\release\bundle\msi\*.msi       <- instalador MSI

  La primera vez tarda varios minutos (compila todas las dependencias de Rust
  en modo release y descarga NSIS/WiX). Las siguientes son mucho mas rapidas.

.PARAMETER NoBundle
  Solo el .exe, sin generar instaladores. Bastante mas rapido.

.PARAMETER Clean
  Borra el target de release antes de compilar (build desde cero).

.PARAMETER Run
  Lanza el .exe al terminar.

.PARAMETER Open
  Abre el explorador en la carpeta del resultado.

.PARAMETER Force
  Cierra sin preguntar cualquier NoteTaker en ejecucion (bloquea el .exe).

.EXAMPLE
  .\build.ps1
.EXAMPLE
  .\build.ps1 -NoBundle -Run
.EXAMPLE
  .\build.ps1 -Clean -Open
#>
[CmdletBinding()]
param(
    [switch]$NoBundle,
    [switch]$Clean,
    [switch]$Run,
    [switch]$Open,
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
Set-Location $root

$ReleaseDir = Join-Path $root "src-tauri\target\release"
$Exe        = Join-Path $ReleaseDir "notetaker.exe"
$BundleDir  = Join-Path $ReleaseDir "bundle"

function Assert-Tool {
    param([string]$Name, [string]$Hint)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        Write-Host "[X] Falta '$Name'. $Hint" -ForegroundColor Red
        exit 1
    }
}

Write-Host "== NoteTaker: compilacion de produccion ==" -ForegroundColor Cyan

# --- Requisitos ---
Assert-Tool node  "Instala Node 22+: https://nodejs.org"
Assert-Tool npm   "Se instala junto con Node."
Assert-Tool cargo "Instala Rust: https://rustup.rs"
Write-Host "[OK] $(node --version) - $(cargo --version)" -ForegroundColor Green

# --- La app en marcha bloquea el .exe y el build falla a mitad ---
$running = Get-Process notetaker -ErrorAction SilentlyContinue
if ($running) {
    Write-Host ""
    Write-Host "[!] NoteTaker esta en ejecucion y bloquea el ejecutable:" -ForegroundColor Yellow
    $running | ForEach-Object { Write-Host "    PID $($_.Id)  $($_.Path)" }
    $answer = if ($Force) { "s" } else { Read-Host "Cerrarlo para continuar? (s/N)" }
    if ($answer -eq "s" -or $answer -eq "S") {
        $running | Stop-Process -Force
        Start-Sleep -Milliseconds 800
        Write-Host "[OK] Cerrado." -ForegroundColor Green
    } else {
        Write-Host "[X] Cierra NoteTaker (incluido el icono de la bandeja) y vuelve a intentarlo." -ForegroundColor Red
        exit 1
    }
}

# --- Limpieza opcional ---
if ($Clean) {
    Write-Host "Limpiando build de release anterior..." -ForegroundColor Yellow
    if (Test-Path $ReleaseDir) { Remove-Item -Recurse -Force $ReleaseDir }
}

# --- Dependencias npm ---
if (-not (Test-Path (Join-Path $root "node_modules"))) {
    Write-Host "Instalando dependencias npm..." -ForegroundColor Yellow
    npm install
    if ($LASTEXITCODE -ne 0) { Write-Host "[X] npm install fallo." -ForegroundColor Red; exit 1 }
}

# --- Build ---
Write-Host ""
if ($NoBundle) {
    Write-Host "Compilando solo el ejecutable (sin instaladores)..." -ForegroundColor Yellow
    npm run tauri build -- --no-bundle
} else {
    Write-Host "Compilando frontend + Rust release + instaladores..." -ForegroundColor Yellow
    Write-Host "(La primera vez tarda varios minutos.)" -ForegroundColor DarkGray
    npm run tauri build
}
if ($LASTEXITCODE -ne 0) {
    Write-Host "[X] El build fallo (revisa la salida de arriba)." -ForegroundColor Red
    exit 1
}

# --- Resultado ---
Write-Host ""
Write-Host "== Build completo ==" -ForegroundColor Green

if (Test-Path $Exe) {
    $mb = [math]::Round((Get-Item $Exe).Length / 1MB, 1)
    Write-Host "Ejecutable ($mb MB):" -ForegroundColor Cyan
    Write-Host "  $Exe"
} else {
    Write-Host "[X] No se genero el ejecutable en $Exe" -ForegroundColor Red
    exit 1
}

if (-not $NoBundle) {
    $installers = @()
    if (Test-Path $BundleDir) {
        $installers = Get-ChildItem -Path $BundleDir -Recurse -Include "*.exe", "*.msi" -ErrorAction SilentlyContinue
    }
    if ($installers.Count -gt 0) {
        Write-Host "Instalador(es):" -ForegroundColor Cyan
        foreach ($f in $installers) {
            $fmb = [math]::Round($f.Length / 1MB, 1)
            Write-Host "  $($f.FullName)  ($fmb MB)"
        }
    } else {
        Write-Host "No se encontraron instaladores en $BundleDir." -ForegroundColor Yellow
        Write-Host "Revisa 'bundle.targets' en src-tauri\tauri.conf.json."
    }
}

Write-Host ""
Write-Host "El motor local (whisper.cpp) no va dentro del instalador: se descarga" -ForegroundColor DarkGray
Write-Host "desde Ajustes -> Whisper local, o con .\scripts\setup-whisper.ps1." -ForegroundColor DarkGray

if ($Open) {
    $target = if ((-not $NoBundle) -and (Test-Path $BundleDir)) { $BundleDir } else { $ReleaseDir }
    Invoke-Item $target
}

if ($Run) {
    Write-Host ""
    Write-Host "[OK] Lanzando $Exe" -ForegroundColor Green
    Start-Process $Exe
}
