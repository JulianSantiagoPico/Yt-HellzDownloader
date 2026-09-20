$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Root = Split-Path -Parent $PSScriptRoot
$Bin = Join-Path $Root "src-tauri\binaries"
$LockPath = Join-Path $Bin "sidecars.lock.json"
$Temp = Join-Path ([System.IO.Path]::GetTempPath()) ("ytpd-sidecars-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $Bin, $Temp | Out-Null

function Get-Sha256([string]$Path) {
    $Hasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        $Stream = [System.IO.File]::OpenRead($Path)
        try { return ([System.BitConverter]::ToString($Hasher.ComputeHash($Stream))).Replace("-", "").ToLowerInvariant() }
        finally { $Stream.Dispose() }
    } finally { $Hasher.Dispose() }
}

function Download-File([string]$Url, [string]$Path) {
    Invoke-WebRequest -UseBasicParsing $Url -OutFile $Path
}

function Assert-Hash([string]$Path, [string]$Expected, [string]$Name) {
    $Actual = Get-Sha256 $Path
    if ($Actual -ne $Expected.ToLowerInvariant()) { throw "Checksum inválido para $Name" }
}

if (-not (Test-Path $LockPath)) { throw "Falta sidecars.lock.json" }
$Lock = Get-Content $LockPath -Raw | ConvertFrom-Json
if ($Lock.schemaVersion -ne 1) { throw "schemaVersion de sidecars.lock.json no compatible" }
$YtDlp = @($Lock.tools | Where-Object name -eq "yt-dlp.exe")
$Ffmpeg = @($Lock.tools | Where-Object name -eq "ffmpeg")
if ($YtDlp.Count -ne 1 -or $Ffmpeg.Count -ne 1) { throw "El lock debe definir yt-dlp.exe y ffmpeg exactamente una vez" }

try {
    $YtPath = Join-Path $Temp "yt-dlp.exe"
    Download-File $YtDlp[0].url $YtPath
    Assert-Hash $YtPath $YtDlp[0].sha256 "yt-dlp.exe"
    Copy-Item $YtPath (Join-Path $Bin "yt-dlp.exe") -Force

    $Archive = Join-Path $Temp "ffmpeg.zip"
    $Extracted = Join-Path $Temp "ffmpeg"
    Download-File $Ffmpeg[0].url $Archive
    Expand-Archive $Archive $Extracted -Force
    foreach ($File in @($Ffmpeg[0].files)) {
        $RelativePath = $File.path.Replace("/", "\")
        $Source = Get-ChildItem $Extracted -Recurse -File | Where-Object { $_.FullName.EndsWith($RelativePath, [System.StringComparison]::OrdinalIgnoreCase) } | Select-Object -First 1
        if (-not $Source) { throw "El archivo FFmpeg no contiene $($File.name)" }
        Assert-Hash $Source.FullName $File.sha256 $File.name
        Copy-Item $Source.FullName (Join-Path $Bin $File.name) -Force
    }

    $Manifest = @{
        lockSchemaVersion = $Lock.schemaVersion
        tools = @(
            @{ name = $YtDlp[0].name; version = $YtDlp[0].version; sha256 = $YtDlp[0].sha256; source = $YtDlp[0].url }
        ) + @($Ffmpeg[0].files | ForEach-Object {
            @{ name = $_.name; version = $Ffmpeg[0].version; sha256 = $_.sha256; source = $Ffmpeg[0].url }
        })
    }
    $Manifest | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $Bin "sidecars.json") -Encoding UTF8
    Write-Host "Sidecars reproducibles descargados y verificados desde sidecars.lock.json"
} finally {
    Remove-Item $Temp -Recurse -Force -ErrorAction SilentlyContinue
}
