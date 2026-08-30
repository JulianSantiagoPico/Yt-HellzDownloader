$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Root = Split-Path -Parent $PSScriptRoot
$Bin = Join-Path $Root "src-tauri\binaries"
$Temp = Join-Path ([System.IO.Path]::GetTempPath()) ("ytpd-sidecars-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $Bin, $Temp | Out-Null

function Download-Verified([string]$Url, [string]$HashUrl, [string]$Name) {
    $Target = Join-Path $Temp $Name
    Invoke-WebRequest -UseBasicParsing $Url -OutFile $Target
    $ChecksumResponse = Invoke-WebRequest -UseBasicParsing $HashUrl
    $ChecksumText = if ($ChecksumResponse.Content -is [byte[]]) {
        [System.Text.Encoding]::UTF8.GetString($ChecksumResponse.Content).Trim()
    } else {
        ([string]$ChecksumResponse.Content).Trim()
    }
    $Published = ($ChecksumText -split "`n" | Where-Object { $_ -match ([regex]::Escape($Name) + "\s*$") } | Select-Object -First 1)
    if ($Published) {
        $Expected = ($Published.Trim() -split "\s+")[0].ToLowerInvariant()
    } elseif ($ChecksumText -match "^([a-fA-F0-9]{64})(\s|$)") {
        $Expected = $Matches[1].ToLowerInvariant()
    } else {
        throw "No se encontró checksum publicado para $Name"
    }
    $Actual = (Get-FileHash -Algorithm SHA256 $Target).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) { throw "Checksum inválido para $Name" }
    return @{ Path = $Target; Sha256 = $Actual }
}

try {
    $YtRelease = Invoke-RestMethod -Headers @{ "User-Agent" = "YT-Playlist-Downloader-Build" } "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest"
    $Yt = Download-Verified ($YtRelease.assets | Where-Object name -eq "yt-dlp.exe").browser_download_url ($YtRelease.assets | Where-Object name -eq "SHA2-256SUMS").browser_download_url "yt-dlp.exe"
    Copy-Item $Yt.Path (Join-Path $Bin "yt-dlp.exe") -Force

    $FfmpegUrl = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
    $FfmpegHashUrl = "$FfmpegUrl.sha256"
    $Archive = Download-Verified $FfmpegUrl $FfmpegHashUrl "ffmpeg-release-essentials.zip"
    Expand-Archive $Archive.Path (Join-Path $Temp "ffmpeg") -Force
    $Ffmpeg = Get-ChildItem (Join-Path $Temp "ffmpeg") -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
    $Ffprobe = Get-ChildItem (Join-Path $Temp "ffmpeg") -Recurse -Filter "ffprobe.exe" | Select-Object -First 1
    if (-not $Ffmpeg -or -not $Ffprobe) { throw "El paquete FFmpeg no contiene las herramientas esperadas" }
    Copy-Item $Ffmpeg.FullName (Join-Path $Bin "ffmpeg.exe") -Force
    Copy-Item $Ffprobe.FullName (Join-Path $Bin "ffprobe.exe") -Force

    $Manifest = @{
        generatedAt = (Get-Date).ToUniversalTime().ToString("o")
        tools = @(
            @{ name = "yt-dlp.exe"; version = $YtRelease.tag_name; sha256 = (Get-FileHash (Join-Path $Bin "yt-dlp.exe") -Algorithm SHA256).Hash.ToLowerInvariant(); source = "https://github.com/yt-dlp/yt-dlp/releases/tag/$($YtRelease.tag_name)" },
            @{ name = "ffmpeg.exe"; sha256 = (Get-FileHash (Join-Path $Bin "ffmpeg.exe") -Algorithm SHA256).Hash.ToLowerInvariant(); source = $FfmpegUrl },
            @{ name = "ffprobe.exe"; sha256 = (Get-FileHash (Join-Path $Bin "ffprobe.exe") -Algorithm SHA256).Hash.ToLowerInvariant(); source = $FfmpegUrl }
        )
    }
    $Manifest | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $Bin "sidecars.json") -Encoding UTF8
    Write-Host "Sidecars descargados y verificados en $Bin"
} finally {
    Remove-Item $Temp -Recurse -Force -ErrorAction SilentlyContinue
}
