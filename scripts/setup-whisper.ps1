<#
.SYNOPSIS
    Instala el motor local de transcripcion (whisper.cpp) y un modelo GGML.

.DESCRIPTION
    Descarga el binario oficial precompilado de whisper.cpp para Windows x64
    (whisper-bin-x64.zip, CPU) y el modelo indicado desde Hugging Face, y los
    deja exactamente donde la app los espera:

      %LOCALAPPDATA%\com.efsteps.notetaker\whisper\bin\whisper-server.exe (+ DLLs)
      %LOCALAPPDATA%\com.efsteps.notetaker\whisper\models\ggml-<Model>.bin

    Es lo mismo que hace la app desde Ajustes -> Whisper local; este script
    sirve para preinstalar sin abrir la app o para automatizar despliegues.

.PARAMETER Model
    Modelo a descargar: tiny (75 MB), base (142 MB, recomendado en vivo),
    small (466 MB), medium (1.5 GB) o large-v3-turbo (1.6 GB).

.PARAMETER Tag
    Tag de release de whisper.cpp (por defecto el que usa la app).

.PARAMETER SkipServer
    No descargar el servidor (solo el modelo).

.PARAMETER Force
    Volver a descargar aunque ya exista.

.EXAMPLE
    .\scripts\setup-whisper.ps1
    .\scripts\setup-whisper.ps1 -Model small
    .\scripts\setup-whisper.ps1 -Model tiny -SkipServer
#>
param(
    [ValidateSet("tiny", "base", "small", "medium", "large-v3-turbo")]
    [string]$Model = "base",
    [string]$Tag = "b4938",
    [switch]$SkipServer,
    [switch]$Force
)

$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Dest      = Join-Path $env:LOCALAPPDATA "com.efsteps.notetaker\whisper"
$BinDir    = Join-Path $Dest "bin"
$ModelsDir = Join-Path $Dest "models"
$ZipUrl    = "https://github.com/ggml-org/whisper.cpp/releases/download/$Tag/whisper-bin-x64.zip"
$ModelUrl  = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-$Model.bin"

New-Item -ItemType Directory -Force $BinDir    | Out-Null
New-Item -ItemType Directory -Force $ModelsDir | Out-Null

Write-Host "== NoteTaker: motor local whisper.cpp ==" -ForegroundColor Cyan
Write-Host "Destino: $Dest"

function Download-File {
    param([string]$Url, [string]$Out)
    Write-Host "Descargando $Url" -ForegroundColor Yellow
    $tmp = "$Out.part"
    if (Test-Path $tmp) { Remove-Item -Force $tmp }
    # Invoke-WebRequest muestra progreso; -UseBasicParsing por compatibilidad con PS 5.1
    Invoke-WebRequest -Uri $Url -OutFile $tmp -UseBasicParsing
    Move-Item -Force $tmp $Out
}

# --- Servidor ---
$ServerExe = Join-Path $BinDir "whisper-server.exe"
if (-not $SkipServer) {
    if ((Test-Path $ServerExe) -and -not $Force) {
        Write-Host "[OK] Servidor ya instalado: $ServerExe" -ForegroundColor Green
    } else {
        $zip = Join-Path $Dest "whisper-bin-x64.zip"
        Download-File -Url $ZipUrl -Out $zip
        $tmpDir = Join-Path $Dest "_extract"
        if (Test-Path $tmpDir) { Remove-Item -Recurse -Force $tmpDir }
        Expand-Archive -Path $zip -DestinationPath $tmpDir -Force
        # El zip trae todo bajo Release\; se aplanan exe + DLLs a bin\
        $files = Get-ChildItem -Path $tmpDir -Recurse -File | Where-Object {
            $_.Extension -eq ".dll" -or $_.Name -in @("whisper-server.exe", "whisper-cli.exe", "whisper-stream.exe")
        }
        foreach ($f in $files) {
            Copy-Item -Force $f.FullName (Join-Path $BinDir $f.Name)
        }
        Remove-Item -Recurse -Force $tmpDir
        Remove-Item -Force $zip
        if (-not (Test-Path $ServerExe)) { throw "No se encontro whisper-server.exe en el zip." }
        Write-Host "[OK] Servidor instalado: $ServerExe ($($files.Count) archivos)" -ForegroundColor Green
    }
}

# --- Modelo ---
$ModelPath = Join-Path $ModelsDir "ggml-$Model.bin"
if ((Test-Path $ModelPath) -and ((Get-Item $ModelPath).Length -gt 1MB) -and -not $Force) {
    Write-Host "[OK] Modelo ya descargado: $ModelPath" -ForegroundColor Green
} else {
    Download-File -Url $ModelUrl -Out $ModelPath
    $mb = [math]::Round((Get-Item $ModelPath).Length / 1MB)
    Write-Host "[OK] Modelo descargado: $ModelPath ($mb MB)" -ForegroundColor Green
}

Write-Host ""
Write-Host "Listo. En la app, elige 'Whisper local' como motor y el modelo '$Model'." -ForegroundColor Cyan
