<#
.SYNOPSIS
    Compila el backend GPU (Vulkan) de whisper.cpp y lo instala para la app.

.DESCRIPTION
    whisper.cpp no publica binarios Vulkan para Windows, asi que este script
    compila SOLO ggml-vulkan.dll a partir del mismo tag que el servidor
    oficial instalado (whisper-bin-x64.zip) y lo deja en:

      %LOCALAPPDATA%\com.efsteps.notetaker\whisper\gpu\ggml-vulkan.dll
      %LOCALAPPDATA%\com.efsteps.notetaker\whisper\gpu\ggml-vulkan.tag

    La app lo carga con GGML_BACKEND_PATH cuando Ajustes -> Whisper local ->
    Aceleracion es "Automatico" o "GPU". No se toca la carpeta bin\ (el
    servidor oficial sigue funcionando igual en CPU).

    No necesita el Vulkan SDK ni permisos de administrador:
      - glslc (compilador de shaders): paquetes de MSYS2 (shaderc, glslang,
        spirv-tools + runtime de gcc), descomprimidos con el tar de Windows 11.
      - Cabeceras: Vulkan-Headers y SPIRV-Headers de KhronosGroup.
      - vulkan-1.lib: se genera a partir de C:\Windows\System32\vulkan-1.dll.
    Si requiere Visual Studio 2022+ con "Desarrollo para el escritorio con C++"
    (trae CMake y Ninja).

    Medido en un Core Ultra 7 258V + Arc 140V: large-v3-turbo pasa de 1.0x a
    4.3x tiempo real con el mismo texto.

.PARAMETER Tag
    Tag de whisper.cpp. Por defecto, RELEASE_TAG de src-tauri\src\stt\whisper\install.rs.

.PARAMETER VulkanHeaders
    Tag de Vulkan-Headers / SPIRV-Headers.

.PARAMETER WorkDir
    Carpeta de trabajo. Ruta corta a proposito: MSVC falla (C1041) con rutas
    de mas de 260 caracteres dentro del arbol de build.

.PARAMETER Clean
    Borrar la carpeta de trabajo antes de empezar.

.EXAMPLE
    .\scripts\build-whisper-vulkan.ps1
#>
param(
    [string]$Tag = "",
    [string]$VulkanHeaders = "vulkan-sdk-1.4.363.0",
    [string]$WorkDir = (Join-Path $env:TEMP "nt-vk"),
    [switch]$Clean
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Root   = Split-Path -Parent $PSScriptRoot
$AppDir = Join-Path $env:LOCALAPPDATA "com.efsteps.notetaker\whisper"
$GpuDir = Join-Path $AppDir "gpu"

if (-not $Tag) {
    $src = Get-Content (Join-Path $Root "src-tauri\src\stt\whisper\install.rs") -Raw
    if ($src -notmatch 'RELEASE_TAG: &str = "([^"]+)"') { throw "No se encontro RELEASE_TAG en install.rs" }
    $Tag = $Matches[1]
}
Write-Host "== NoteTaker: backend GPU (Vulkan) para whisper.cpp $Tag ==" -ForegroundColor Cyan

if (-not (Test-Path "C:\Windows\System32\vulkan-1.dll")) {
    throw "No hay driver Vulkan (falta vulkan-1.dll). Instala o actualiza el driver de la GPU."
}

# --- Visual Studio (compilador, CMake, Ninja) -------------------------------
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) { throw "No se encontro Visual Studio (vswhere.exe)." }
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
if (-not $vs) { throw "Visual Studio no tiene el componente C++ (Desarrollo para el escritorio con C++)." }
$vcvars = Join-Path $vs "VC\Auxiliary\Build\vcvars64.bat"
$cmakeDir = Join-Path $vs "Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
$ninjaDir = Join-Path $vs "Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja"
Write-Host "Visual Studio: $vs"

if ($Clean -and (Test-Path $WorkDir)) { Remove-Item -Recurse -Force $WorkDir }
New-Item -ItemType Directory -Force $WorkDir, "$WorkDir\dl", "$WorkDir\msys", "$WorkDir\lib" | Out-Null

function Get-File([string]$Url, [string]$Out) {
    if (Test-Path $Out) { return }
    Write-Host "Descargando $Url" -ForegroundColor Yellow
    Invoke-WebRequest -Uri $Url -OutFile "$Out.part" -UseBasicParsing
    Move-Item -Force "$Out.part" $Out
}

# --- glslc desde MSYS2 --------------------------------------------------------
$glslc = Join-Path $WorkDir "msys\mingw64\bin\glslc.exe"
if (-not (Test-Path $glslc)) {
    $repo = "https://repo.msys2.org/mingw/mingw64/"
    $index = (Invoke-WebRequest -Uri $repo -UseBasicParsing).Content
    $culture = [Globalization.CultureInfo]::InvariantCulture
    # libgcc + libstdc++ + libwinpthread = runtime de gcc que necesita glslc.exe
    foreach ($pkg in "shaderc", "glslang", "spirv-tools", "libgcc", "libstdc%2B%2B", "libwinpthread") {
        # Version mas reciente por fecha de publicacion (el orden por nombre
        # no sirve: "r9" quedaria detras de "r426").
        $pattern = 'href="(mingw-w64-x86_64-' + [regex]::Escape($pkg) + '-[0-9][^"]*?-any\.pkg\.tar\.zst)">[^<]*</a>\s+(\d{2}-\w{3}-\d{4} \d{2}:\d{2})'
        $latest = [regex]::Matches($index, $pattern) |
            Sort-Object { [datetime]::ParseExact($_.Groups[2].Value, "dd-MMM-yyyy HH:mm", $culture) } |
            Select-Object -Last 1
        if (-not $latest) { throw "No se encontro el paquete $pkg en MSYS2" }
        $file = $latest.Groups[1].Value
        $out = Join-Path $WorkDir ("dl\" + ($pkg -replace "%2B", "p") + ".pkg.tar.zst")
        Get-File ($repo + $file) $out
        & tar.exe -xf $out -C (Join-Path $WorkDir "msys")
        if ($LASTEXITCODE -ne 0) { throw "tar no pudo extraer $file (hace falta el tar de Windows 11, con soporte zstd)" }
    }
}
& $glslc --version | Select-Object -First 1

# --- Cabeceras y codigo fuente -------------------------------------------------
$vkh  = Join-Path $WorkDir "Vulkan-Headers-$VulkanHeaders"
$spvh = Join-Path $WorkDir "SPIRV-Headers-$VulkanHeaders"
$src  = Join-Path $WorkDir "whisper.cpp-$Tag"
foreach ($item in @(
        @{ Url = "https://github.com/KhronosGroup/Vulkan-Headers/archive/refs/tags/$VulkanHeaders.tar.gz"; Out = "vkh.tar.gz"; Dir = $vkh },
        @{ Url = "https://github.com/KhronosGroup/SPIRV-Headers/archive/refs/tags/$VulkanHeaders.tar.gz"; Out = "spvh.tar.gz"; Dir = $spvh },
        @{ Url = "https://github.com/ggml-org/whisper.cpp/archive/refs/tags/$Tag.tar.gz"; Out = "whisper-$Tag.tar.gz"; Dir = $src })) {
    $out = Join-Path $WorkDir "dl\$($item.Out)"
    Get-File $item.Url $out
    if (-not (Test-Path $item.Dir)) { & tar.exe -xzf $out -C $WorkDir }
}
# El Vulkan SDK trae las cabeceras SPIR-V junto a las de Vulkan; ggml-vulkan.cpp lo asume.
Copy-Item -Recurse -Force (Join-Path $spvh "include\spirv") (Join-Path $vkh "include\")

# --- Compilacion -------------------------------------------------------------------
$cmd = @"
@echo off
call "$vcvars" >nul || exit /b 1
set "PATH=$WorkDir\msys\mingw64\bin;$cmakeDir;$ninjaDir;%PATH%"
if not exist "$WorkDir\lib\vulkan-1.lib" (
  dumpbin /nologo /exports C:\Windows\System32\vulkan-1.dll > "$WorkDir\lib\exports.txt" || exit /b 1
)
if not exist "$WorkDir\spirv\share" (
  cmake -S "$spvh" -B "$WorkDir\spvh-build" -G Ninja -DSPIRV_HEADERS_ENABLE_TESTS=OFF -DCMAKE_INSTALL_PREFIX="$WorkDir\spirv" || exit /b 1
  cmake --install "$WorkDir\spvh-build" || exit /b 1
)
exit /b 0
"@
Set-Content -Encoding ascii (Join-Path $WorkDir "prep.cmd") $cmd
# Salida a archivo desde cmd: en PowerShell 5.1, "2>&1" sobre un ejecutable
# con ErrorActionPreference=Stop aborta al primer aviso que escriba en stderr.
cmd /c "`"$WorkDir\prep.cmd`" > `"$WorkDir\prep.log`" 2>&1"
if ($LASTEXITCODE -ne 0) { throw "Fallo la preparacion (vcvars / SPIRV-Headers); revisa $WorkDir\prep.log" }

if (-not (Test-Path "$WorkDir\lib\vulkan-1.lib")) {
    $names = Get-Content "$WorkDir\lib\exports.txt" | ForEach-Object {
        if ($_ -match '^\s+\d+\s+[0-9A-F]+\s+[0-9A-F]+\s+(vk\w+)') { $Matches[1] }
    }
    (@("LIBRARY vulkan-1", "EXPORTS") + $names) | Set-Content -Encoding ascii "$WorkDir\lib\vulkan-1.def"
    # vcvars64.bat de VS 2026 se queja de vswhere en stderr aunque funcione
    cmd /c "`"$vcvars`" >nul 2>&1 && lib /nologo /def:`"$WorkDir\lib\vulkan-1.def`" /machine:x64 /out:`"$WorkDir\lib\vulkan-1.lib`" >nul"
    if ($LASTEXITCODE -ne 0) { throw "No se pudo generar vulkan-1.lib" }
}

$build = Join-Path $WorkDir "build-$Tag"
$cmd = @"
@echo off
call "$vcvars" >nul || exit /b 1
set "PATH=$WorkDir\msys\mingw64\bin;$cmakeDir;$ninjaDir;%PATH%"
cmake -S "$src" -B "$build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_PREFIX_PATH="$WorkDir\spirv" ^
  -DBUILD_SHARED_LIBS=ON -DGGML_BACKEND_DL=ON -DGGML_NATIVE=OFF -DGGML_CPU_ALL_VARIANTS=OFF ^
  -DGGML_VULKAN=ON -DWHISPER_SDL2=OFF -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=OFF ^
  -DVulkan_INCLUDE_DIR="$vkh\include" -DVulkan_LIBRARY="$WorkDir\lib\vulkan-1.lib" ^
  -DVulkan_GLSLC_EXECUTABLE="$glslc" || exit /b 1
cmake --build "$build" --config Release -j $([Environment]::ProcessorCount) --target ggml-vulkan || exit /b 1
"@
Set-Content -Encoding ascii (Join-Path $WorkDir "build.cmd") $cmd
Write-Host "Compilando ggml-vulkan (los shaders tardan unos minutos)..." -ForegroundColor Yellow
cmd /c "`"$WorkDir\build.cmd`" > `"$WorkDir\build.log`" 2>&1"
$dll = Join-Path $build "bin\ggml-vulkan.dll"
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $dll)) {
    Select-String -Path "$WorkDir\build.log" -Pattern "error" | Select-Object -First 10 | ForEach-Object { $_.Line }
    throw "La compilacion fallo; revisa $WorkDir\build.log"
}

# --- Instalacion -------------------------------------------------------------------
New-Item -ItemType Directory -Force $GpuDir | Out-Null
try {
    Copy-Item -Force $dll (Join-Path $GpuDir "ggml-vulkan.dll")
} catch {
    throw "No se pudo copiar el DLL (esta en uso). Para la transcripcion en curso o cierra la app y repite."
}
# Con parametros con nombre: en PowerShell 5.1, "-NoNewline" con argumentos
# posicionales no escribe nada y tampoco falla.
Set-Content -Encoding ascii -NoNewline -Path (Join-Path $GpuDir "ggml-vulkan.tag") -Value $Tag
if (-not (Test-Path (Join-Path $GpuDir "ggml-vulkan.tag"))) { throw "No se pudo escribir ggml-vulkan.tag" }
Write-Host "Instalado en $GpuDir" -ForegroundColor Green

# --- Prueba rapida: cargar el backend con el servidor oficial ----------------------
$cli = Join-Path $AppDir "bin\whisper-cli.exe"
$model = Get-ChildItem (Join-Path $AppDir "models") -Filter "ggml-*.bin" -ErrorAction SilentlyContinue | Sort-Object Length | Select-Object -First 1
if ((Test-Path $cli) -and $model) {
    # 1 s de silencio a 16 kHz mono
    $wav = Join-Path $WorkDir "silence.wav"
    $data = New-Object byte[] 32000
    $hdr = New-Object IO.MemoryStream; $w = New-Object IO.BinaryWriter($hdr)
    $w.Write([Text.Encoding]::ASCII.GetBytes("RIFF")); $w.Write([int](36 + $data.Length)); $w.Write([Text.Encoding]::ASCII.GetBytes("WAVEfmt "))
    $w.Write([int]16); $w.Write([int16]1); $w.Write([int16]1); $w.Write([int]16000); $w.Write([int]32000); $w.Write([int16]2); $w.Write([int16]16)
    $w.Write([Text.Encoding]::ASCII.GetBytes("data")); $w.Write([int]$data.Length); $w.Write($data); $w.Flush()
    [IO.File]::WriteAllBytes($wav, $hdr.ToArray())
    $env:GGML_BACKEND_PATH = Join-Path $GpuDir "ggml-vulkan.dll"; $env:GGML_VK_DISABLE_COOPMAT = "1"
    $ErrorActionPreference = "Continue"  # whisper-cli escribe su log en stderr
    $out = & $cli -m $model.FullName -f $wav -l es 2>&1 | Out-String
    $ErrorActionPreference = "Stop"
    Remove-Item env:GGML_BACKEND_PATH, env:GGML_VK_DISABLE_COOPMAT
    if ($out -match "ggml_vulkan: 0 = ([^|]+)") { Write-Host "GPU detectada: $($Matches[1].Trim())" -ForegroundColor Green }
    if ($out -match "using Vulkan\d+ backend") {
        Write-Host "OK: whisper usa la GPU. En la app: Ajustes -> Whisper local -> Aceleracion = Automatico." -ForegroundColor Green
    } else {
        Write-Warning "El backend se instalo pero whisper no lo uso. Salida:`n$out"
    }
}
