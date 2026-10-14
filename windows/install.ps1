# Install Hotspot Manager for Windows (it asks for administrator rights once):
#   powershell -ExecutionPolicy Bypass -File windows\install.ps1             install (builds it first, with Rust)
#   powershell -ExecutionPolicy Bypass -File windows\install.ps1 -Uninstall  remove it
# It goes to C:\Program Files\Hotspot Manager, with its helper service "HotspotManager" (as the Linux
# app's hotspotctl: it counts, limits and blocks the devices' traffic and gives out their addresses)
# and WinDivert (https://reqrypt.org/windivert.html, LGPL), the driver the helper works through; and
# in the Start menu and on the desktop. Your settings stay in %APPDATA%\Hotspot Manager.
param([switch]$Uninstall, [switch]$Elevated)
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
    $mode = if ($Uninstall) { "-Uninstall" } else { "" }
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
        Write-Host "Hotspot Manager is removed. (Your settings stay in %APPDATA%\Hotspot Manager; the helper's in %ProgramData%\Hotspot Manager.)"
        return
    }

    Step "Copying the app to $dir"
    New-Item -ItemType Directory -Force $dir | Out-Null
    Copy-Item $source $exe -Force
    Copy-Item $PSCommandPath (Join-Path $dir "install.ps1") -Force

    if (-not (Test-Path (Join-Path $dir "WinDivert.dll"))) {
        Step "Downloading WinDivert 2.2.2"
        $work = Join-Path $env:TEMP "hotspot-manager-windivert"
        Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Force $work | Out-Null
        $zip = Join-Path $work "WinDivert.zip"
        try {
            [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
            Invoke-WebRequest $windivert -OutFile $zip -UseBasicParsing
            if ((Get-Item $zip).Length -ne $windivertSize) { throw "the download isn't the expected file" }
            Expand-Archive $zip -DestinationPath $work -Force
            $x64 = Get-ChildItem $work -Recurse -Directory -Filter x64 | Select-Object -First 1
            $driver = Join-Path $x64.FullName "WinDivert64.sys"
            if ((Get-AuthenticodeSignature $driver).Status -ne "Valid") { throw "its driver isn't properly signed" }
            Copy-Item (Join-Path $x64.FullName "WinDivert.dll"), $driver $dir -Force
            Get-ChildItem $work -Recurse -Filter "LICENSE*" | Select-Object -First 1 | Copy-Item -Destination (Join-Path $dir "WinDivert-LICENSE.txt")
            Step "WinDivert is in place"
        } catch {
            Write-Host ""
            Write-Host "WinDivert couldn't be set up: $($_.Exception.Message)" -ForegroundColor Yellow
            Write-Host "Microsoft Defender often stops it (it flags packet drivers as potentially unwanted, because malware"
            Write-Host "uses them too). To use speed limits, data use and blocking: Windows Security > Virus & threat protection >"
            Write-Host "Protection history > the WinDivert item > Actions > Allow on device, then run this installer again."
            Write-Host "Everything else works without it." -ForegroundColor Yellow
            Write-Host ""
        }
    }

    if (-not (Get-Service $service -ErrorAction SilentlyContinue)) {
        Step "Adding the helper service"
        sc.exe create $service binPath= "`"$exe`" --service" start= auto DisplayName= "Hotspot Manager" | Out-Null
        sc.exe description $service "Counts, limits and blocks the devices on the Wi-Fi hotspot, and gives out their addresses (Hotspot Manager)" | Out-Null
        sc.exe failure $service reset= 86400 actions= restart/5000/restart/5000/restart/60000 | Out-Null
    }
    Step "Starting the helper service"
    Start-Service $service

    Step "Adding Hotspot Manager to the Start menu and the desktop"
    $shell = New-Object -ComObject WScript.Shell
    foreach ($path in $links) {
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
}
Read-Host "Press Enter to close"
