$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
& (Join-Path $PSScriptRoot "verify-sidecars.ps1")
Push-Location $Root
try { npm run tauri build -- --no-bundle } finally { Pop-Location }
$Out = Join-Path $Root "artifacts\portable"
Remove-Item $Out -Recurse -Force -ErrorAction SilentlyContinue
$PortableBin = Join-Path $Out "binaries"
New-Item -ItemType Directory -Force $Out, $PortableBin | Out-Null
Copy-Item (Join-Path $Root "src-tauri\target\release\yt-playlist-downloader.exe") $Out
Copy-Item (Join-Path $Root "src-tauri\binaries\*.exe") $PortableBin
Copy-Item (Join-Path $Root "src-tauri\binaries\sidecars.json") $PortableBin
Copy-Item (Join-Path $Root "docs\THIRD_PARTY_NOTICES.md") $Out
$Archive = Join-Path $Root "artifacts\YT-Playlist-Downloader-portable.zip"
for ($Attempt = 1; $Attempt -le 5; $Attempt++) {
    try {
        Compress-Archive (Join-Path $Out "*") $Archive -Force
        break
    } catch {
        if ($Attempt -eq 5) { throw }
        Remove-Item $Archive -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
    }
}
