# Install Hotspot Manager for Windows (it asks for administrator rights once):
#   powershell -ExecutionPolicy Bypass -File windows\install.ps1             install (builds it first, with Rust)
#   powershell -ExecutionPolicy Bypass -File windows\install.ps1 -Uninstall  remove it
#   ... -AllowWinDivert   also tell Microsoft Defender to leave the install folder alone (it flags
#                         WinDivert as potentially unwanted, as it does most packet drivers)
# It goes to C:\Program Files\Hotspot Manager, with its helper service "HotspotManager" (as the Linux
# app's hotspotctl: it counts, limits and blocks the devices' traffic and gives out their addresses)
# and WinDivert (https://reqrypt.org/windivert.html, LGPL), the driver the helper works through; and
# in the Start menu and on the desktop. Your settings stay in %APPDATA%\Hotspot Manager.
# The setup program (packaging\hotspot-manager.iss) runs it too, with -Quiet -NoShortcuts: no
# "Press Enter" at the end, and the shortcuts are its own.
param([switch]$Uninstall, [switch]$Elevated, [switch]$AllowWinDivert, [switch]$Quiet, [switch]$NoShortcuts)
$ErrorActionPreference = "Stop"
$dir = Join-Path $env:ProgramFiles "Hotspot Manager"
$exe = Join-Path $dir "hotspot-manager.exe"
$service = "HotspotManager"
$windivert = "https://github.com/basil00/WinDivert/releases/download/v2.2.2/WinDivert-2.2.2-A.zip"
$windivertSize = 405137
$source = @((Join-Path $PSScriptRoot "hotspot-manager.exe"), (Join-Path $PSScriptRoot "target\release\hotspot-manager.exe")) |
    Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $Elevated) {
    if (-not $Uninstall -and -not $source) {
        Push-Location $PSScriptRoot
        cargo build --release
        Pop-Location
        if ($LASTEXITCODE) { throw "The build failed" }
    }
    $mode = @(if ($Uninstall) { "-Uninstall" }) + @(if ($AllowWinDivert) { "-AllowWinDivert" }) -join " "
    Start-Process powershell -Verb RunAs -Wait -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Elevated $mode"
    return
}

function Step($text) { Write-Host "- $text" }
$links = @((Join-Path ([Environment]::GetFolderPath("CommonPrograms")) "Hotspot Manager.lnk"),
           (Join-Path ([Environment]::GetFolderPath("CommonDesktopDirectory")) "Hotspot Manager.lnk"))
# (the first version installed for one user only: tidied away)
$old = Join-Path $env:LOCALAPPDATA "Programs\Hotspot Manager"
$oldLinks = @((Join-Path ([Environment]::GetFolderPath("Programs")) "Hotspot Manager.lnk"),
              (Join-Path ([Environment]::GetFolderPath("Desktop")) "Hotspot Manager.lnk"))

try {
    if (Get-Service $service -ErrorAction SilentlyContinue) {
        Step "Stopping the helper service"
        Stop-Service $service -Force -ErrorAction SilentlyContinue
    }
    Get-Process hotspot-manager -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $exe -or $_.Path -like "$old\*" } | Stop-Process -Force
    if ($Uninstall) {
        sc.exe delete $service | Out-Null
        $links + $oldLinks | ForEach-Object { Remove-Item $_ -ErrorAction SilentlyContinue }
        Remove-Item $dir, $old -Recurse -Force -ErrorAction SilentlyContinue
        Remove-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "Hotspot Manager" -ErrorAction SilentlyContinue
        Remove-MpPreference -ExclusionPath $dir -ErrorAction SilentlyContinue
        Write-Host "Hotspot Manager is removed. (Your settings stay in %APPDATA%\Hotspot Manager; the helper's in %ProgramData%\Hotspot Manager.)"
        return
    }

    Step "Copying the app to $dir"
    New-Item -ItemType Directory -Force $dir | Out-Null
    # (run from the installed copy, they're there already)
    if ($source -ne $exe) { Copy-Item $source $exe -Force }
    if ($PSCommandPath -ne (Join-Path $dir "install.ps1")) { Copy-Item $PSCommandPath (Join-Path $dir "install.ps1") -Force }

    if ($AllowWinDivert) {
        Step "Asking Microsoft Defender to leave $dir alone (only that folder)"
        Add-MpPreference -ExclusionPath $dir
    }
    if (-not (Test-Path (Join-Path $dir "WinDivert.dll"))) {
        Step "Downloading WinDivert 2.2.2"
        # (into the install folder: with -AllowWinDivert, Defender leaves it there)
        $work = Join-Path $dir "windivert-download"
        $zip = Join-Path $work "WinDivert.zip"
        try {
            if (-not ((Test-Path $zip) -and (Get-Item $zip).Length -eq $windivertSize)) {
                Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
                New-Item -ItemType Directory -Force $work | Out-Null
                [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
                Invoke-WebRequest $windivert -OutFile $zip -UseBasicParsing
            }
            if ((Get-Item $zip).Length -ne $windivertSize) { throw "the download isn't the expected file" }
            # (Defender may still be looking at it: a few tries)
            foreach ($try in 1..6) {
                try { Expand-Archive $zip -DestinationPath $work -Force; break }
                catch { if ($try -eq 6) { throw } else { Start-Sleep 3 } }
            }
            $x64 = Get-ChildItem $work -Recurse -Directory -Filter x64 | Select-Object -First 1
            $driver = Join-Path $x64.FullName "WinDivert64.sys"
            if ((Get-AuthenticodeSignature $driver).Status -ne "Valid") { throw "its driver isn't properly signed" }
            Copy-Item (Join-Path $x64.FullName "WinDivert.dll"), $driver $dir -Force
            Get-ChildItem $work -Recurse -Filter "LICENSE*" | Select-Object -First 1 | Copy-Item -Destination (Join-Path $dir "WinDivert-LICENSE.txt")
            Step "WinDivert is in place"
            Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
        } catch {
            Write-Host ""
            Write-Host "WinDivert couldn't be set up: $($_.Exception.Message)" -ForegroundColor Yellow
            Write-Host "Microsoft Defender often stops it (it flags packet drivers as potentially unwanted, because malware"
            Write-Host "uses them too). To use speed limits, data use and blocking, run this installer again with -AllowWinDivert,"
            Write-Host "which asks Defender to leave only this program's folder alone."
            Write-Host "Everything else works without it." -ForegroundColor Yellow
            Write-Host ""
        }
    }

    if (-not (Get-Service $service -ErrorAction SilentlyContinue)) {
        Step "Adding the helper service"
        New-Service -Name $service -BinaryPathName "`"$exe`" --service" -DisplayName "Hotspot Manager" -StartupType Automatic `
            -Description "Counts, limits and blocks the devices on the Wi-Fi hotspot, and gives out their addresses (Hotspot Manager)" | Out-Null
        sc.exe failure $service reset= 86400 actions= restart/5000/restart/5000/restart/60000 | Out-Null
    }
    Step "Starting the helper service"
    Start-Service $service

    if (-not $NoShortcuts) { Step "Adding Hotspot Manager to the Start menu and the desktop" }
    $shell = New-Object -ComObject WScript.Shell
    foreach ($path in @($links | Where-Object { -not $NoShortcuts })) {
        $link = $shell.CreateShortcut($path)
        $link.TargetPath = $exe
        $link.WorkingDirectory = $dir
        $link.Description = "Turn the Wi-Fi hotspot on or off, change its password and manage its devices"
        $link.Save()
    }
    $oldLinks | ForEach-Object { Remove-Item $_ -ErrorAction SilentlyContinue }
    Remove-Item $old -Recurse -Force -ErrorAction SilentlyContinue
    $run = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
    if (Get-ItemProperty $run -Name "Hotspot Manager" -ErrorAction SilentlyContinue) {
        Set-ItemProperty $run -Name "Hotspot Manager" -Value "`"$exe`" --turn-on" # (it moved)
    }
    Write-Host ""
    Write-Host "Hotspot Manager is installed." -ForegroundColor Green
} catch {
    Write-Host "Installing failed: $($_.Exception.Message)" -ForegroundColor Red
    if ($Quiet) { exit 1 }
}
if (-not $Quiet) { Read-Host "Press Enter to close" }
