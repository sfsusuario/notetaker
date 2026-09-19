<#
.SYNOPSIS
    Atajo historico: reenvia a .\build.ps1 en la raiz del proyecto.

.DESCRIPTION
    La compilacion de produccion vive ahora en build.ps1 (raiz), para tenerla
    junto a run.ps1 y start.ps1. Este script se mantiene para no romper
    referencias antiguas.

.EXAMPLE
    .\scripts\build-installer.ps1 -Open
#>
param(
    [switch]$Open,
    [switch]$Clean,
    [switch]$Run,
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
& (Join-Path $RepoRoot "build.ps1") -Open:$Open -Clean:$Clean -Run:$Run -Force:$Force
exit $LASTEXITCODE
