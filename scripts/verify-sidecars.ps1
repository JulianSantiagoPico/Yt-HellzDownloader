$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Bin = Join-Path $Root "src-tauri\binaries"
$ManifestPath = Join-Path $Bin "sidecars.json"
if (-not (Test-Path $ManifestPath)) { throw "Falta sidecars.json; ejecuta npm run sidecars:download" }

function Get-Sha256([string]$Path) {
    $Hasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        $Stream = [System.IO.File]::OpenRead($Path)
        try {
            return ([System.BitConverter]::ToString($Hasher.ComputeHash($Stream))).Replace("-", "").ToLowerInvariant()
        } finally {
            $Stream.Dispose()
        }
    } finally {
        $Hasher.Dispose()
    }
}

$Manifest = Get-Content $ManifestPath -Raw | ConvertFrom-Json
foreach ($Tool in $Manifest.tools) {
    $Path = Join-Path $Bin $Tool.name
    if (-not (Test-Path $Path)) { throw "Falta $($Tool.name)" }
    $Hash = Get-Sha256 $Path
    if ($Hash -ne $Tool.sha256) { throw "Checksum local inválido: $($Tool.name)" }
    $VersionArgument = if ($Tool.name -eq "yt-dlp.exe") { "--version" } else { "-version" }
    $Output = & $Path $VersionArgument 2>&1
    if ($LASTEXITCODE -ne 0) { throw "$($Tool.name) no se puede ejecutar: $Output" }
    Write-Host "OK $($Tool.name) $Hash"
}
