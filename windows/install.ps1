# Install Hotspot Manager for Windows for the current user (no administrator needed):
#   powershell -ExecutionPolicy Bypass -File windows\install.ps1
# Builds it if needed (Rust), copies it to %LOCALAPPDATA%\Programs\Hotspot Manager and adds it to
# the Start menu and the desktop. -Uninstall removes it (and its start at sign-in).
param([switch]$Uninstall)
$ErrorActionPreference = "Stop"
$dir = Join-Path $env:LOCALAPPDATA "Programs\Hotspot Manager"
$exe = Join-Path $dir "hotspot-manager.exe"
$links = @((Join-Path ([Environment]::GetFolderPath("Programs")) "Hotspot Manager.lnk"),
           (Join-Path ([Environment]::GetFolderPath("Desktop")) "Hotspot Manager.lnk"))

if ($Uninstall) {
    Get-Process hotspot-manager -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe } | Stop-Process
    Remove-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "Hotspot Manager" -ErrorAction SilentlyContinue
    $links | ForEach-Object { Remove-Item $_ -ErrorAction SilentlyContinue }
    Remove-Item $dir -Recurse -ErrorAction SilentlyContinue
    Write-Host "Removed. (Your settings stay in $env:APPDATA\Hotspot Manager.)"
    return
}

$built = Join-Path $PSScriptRoot "target\release\hotspot-manager.exe"
if (-not (Test-Path $built)) {
    Push-Location $PSScriptRoot
    cargo build --release
    Pop-Location
    if ($LASTEXITCODE) { throw "build failed" }
}
Get-Process hotspot-manager -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe } | Stop-Process
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item $built $exe -Force
$shell = New-Object -ComObject WScript.Shell
foreach ($path in $links) {
    $link = $shell.CreateShortcut($path)
    $link.TargetPath = $exe
    $link.WorkingDirectory = $dir
    $link.Description = "Turn the Wi-Fi hotspot on or off, change its password and see the devices on it"
    $link.Save()
}
Write-Host "Installed: open Hotspot Manager from the Start menu or the desktop."
