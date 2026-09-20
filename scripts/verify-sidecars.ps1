$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Bin = Join-Path $Root "src-tauri\binaries"
$LockPath = Join-Path $Bin "sidecars.lock.json"
if (-not (Test-Path $LockPath)) { throw "Falta sidecars.lock.json" }

function Get-Sha256([string]$Path) {
    $Hasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        $Stream = [System.IO.File]::OpenRead($Path)
        try { return ([System.BitConverter]::ToString($Hasher.ComputeHash($Stream))).Replace("-", "").ToLowerInvariant() }
        finally { $Stream.Dispose() }
    } finally { $Hasher.Dispose() }
}

$Lock = Get-Content $LockPath -Raw | ConvertFrom-Json
if ($Lock.schemaVersion -ne 1) { throw "schemaVersion de sidecars.lock.json no compatible" }
$ExpectedTools = @($Lock.tools | Where-Object name -eq "yt-dlp.exe" | ForEach-Object { $_ }) + @($Lock.tools | Where-Object name -eq "ffmpeg" | ForEach-Object { $_.files })
foreach ($Tool in $ExpectedTools) {
    $Path = Join-Path $Bin $Tool.name
    if (-not (Test-Path $Path)) { throw "Falta $($Tool.name)" }
    $Hash = Get-Sha256 $Path
    if ($Hash -ne $Tool.sha256) { throw "Checksum local inválido: $($Tool.name)" }
    $VersionArgument = if ($Tool.name -eq "yt-dlp.exe") { "--version" } else { "-version" }
    $Output = & $Path $VersionArgument 2>&1
    if ($LASTEXITCODE -ne 0) { throw "$($Tool.name) no se puede ejecutar: $Output" }
    Write-Host "OK $($Tool.name) $Hash"
}
