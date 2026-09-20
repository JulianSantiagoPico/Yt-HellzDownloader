$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$LockPath = Join-Path $Root "src-tauri\binaries\sidecars.lock.json"

if (-not (Test-Path $LockPath)) { throw "Falta sidecars.lock.json" }
$Lock = Get-Content $LockPath -Raw | ConvertFrom-Json
if ($Lock.schemaVersion -ne 1) { throw "schemaVersion debe ser 1" }

foreach ($Tool in @($Lock.tools)) {
    if ([string]::IsNullOrWhiteSpace($Tool.name) -or [string]::IsNullOrWhiteSpace($Tool.version) -or [string]::IsNullOrWhiteSpace($Tool.url)) {
        throw "Cada sidecar debe declarar name, version y url"
    }
    if ($Tool.url -match "/latest(?:/|$)" -or $Tool.url -match "release-essentials") {
        throw "$($Tool.name) usa una URL mutable: $($Tool.url)"
    }
}

$YtDlp = @($Lock.tools | Where-Object name -eq "yt-dlp.exe")
if ($YtDlp.Count -ne 1 -or $YtDlp[0].sha256 -notmatch "^[a-f0-9]{64}$") { throw "yt-dlp.exe debe tener un SHA-256 fijo" }

$Ffmpeg = @($Lock.tools | Where-Object name -eq "ffmpeg")
if ($Ffmpeg.Count -ne 1 -or @($Ffmpeg[0].files).Count -ne 2) { throw "FFmpeg debe declarar los dos ejecutables extraídos" }
foreach ($File in @($Ffmpeg[0].files)) {
    if ($File.name -notin @("ffmpeg.exe", "ffprobe.exe") -or [string]::IsNullOrWhiteSpace($File.path) -or $File.sha256 -notmatch "^[a-f0-9]{64}$") {
        throw "La definición de FFmpeg no fija hashes válidos"
    }
}

Write-Host "Lock de sidecars válido: $LockPath"
