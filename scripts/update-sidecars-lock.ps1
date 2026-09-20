[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string]$YtDlpVersion,
    [Parameter(Mandatory = $true)] [string]$YtDlpSha256,
    [Parameter(Mandatory = $true)] [string]$FfmpegVersion,
    [Parameter(Mandatory = $true)] [string]$FfmpegSha256,
    [Parameter(Mandatory = $true)] [string]$FfprobeSha256,
    [string]$LockPath = (Join-Path (Split-Path -Parent $PSScriptRoot) "src-tauri\binaries\sidecars.lock.json")
)

$ErrorActionPreference = "Stop"
foreach ($Hash in @($YtDlpSha256, $FfmpegSha256, $FfprobeSha256)) {
    if ($Hash -notmatch "^[a-fA-F0-9]{64}$") { throw "Todos los hashes deben ser SHA-256 de 64 caracteres hexadecimales." }
}
if ($YtDlpVersion -notmatch "^\d{4}\.\d{2}\.\d{2}$") { throw "YtDlpVersion debe tener el formato YYYY.MM.DD." }
if ($FfmpegVersion -notmatch "^\d+\.\d+(?:\.\d+)?$") { throw "FfmpegVersion debe ser una versión numérica." }
if (-not (Test-Path $LockPath)) { throw "No se encontró el lock: $LockPath" }

$Lock = Get-Content $LockPath -Raw | ConvertFrom-Json
$YtDlp = @($Lock.tools | Where-Object name -eq "yt-dlp.exe")
$Ffmpeg = @($Lock.tools | Where-Object name -eq "ffmpeg")
if ($YtDlp.Count -ne 1 -or $Ffmpeg.Count -ne 1) { throw "El lock debe definir yt-dlp.exe y ffmpeg exactamente una vez." }

$YtDlp[0].version = $YtDlpVersion
$YtDlp[0].url = "https://github.com/yt-dlp/yt-dlp/releases/download/$YtDlpVersion/yt-dlp.exe"
$YtDlp[0].sha256 = $YtDlpSha256.ToLowerInvariant()
$Ffmpeg[0].version = $FfmpegVersion
$Ffmpeg[0].url = "https://github.com/GyanD/codexffmpeg/releases/download/$FfmpegVersion/ffmpeg-$FfmpegVersion-essentials_build.zip"
foreach ($File in @($Ffmpeg[0].files)) {
    if ($File.name -eq "ffmpeg.exe") { $File.sha256 = $FfmpegSha256.ToLowerInvariant() }
    elseif ($File.name -eq "ffprobe.exe") { $File.sha256 = $FfprobeSha256.ToLowerInvariant() }
}

$Lock | ConvertTo-Json -Depth 5 | Set-Content $LockPath -Encoding UTF8
Write-Host "Lock actualizado. Revisa el diff y ejecuta npm run sidecars:lock:check; npm run sidecars:download; npm run sidecars:verify."
