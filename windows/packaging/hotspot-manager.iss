; Hotspot Manager's setup program (Inno Setup 6.6+), as TurboDM's. Built by
; .github/workflows/windows.yml:
;   iscc /DVersion=1.0.0 windows\packaging\hotspot-manager.iss
; One admin prompt; installed for everyone in Program Files. After the files, install.ps1 (run
; -Quiet) sets up the helper service and downloads WinDivert, the packet driver behind speed
; limits, data use and blocking, as running it by hand does. Light or dark, following Windows.

#ifndef Version
  #define Version "0.0.0"
#endif
#define Art(Name) "art\" + Name + "-100.bmp,art\" + Name + "-125.bmp,art\" + Name + "-150.bmp,art\" + Name + "-175.bmp,art\" + Name + "-200.bmp,art\" + Name + "-250.bmp"

[Setup]
AppId={{94F06B60-F91B-4DA5-8C56-CF41C4F9BDC3}
AppName=Hotspot Manager
AppVersion={#Version}
AppVerName=Hotspot Manager {#Version}
AppPublisher=xsobhi
AppPublisherURL=https://github.com/xsobhi/hotspot-manager
AppSupportURL=https://github.com/xsobhi/hotspot-manager/issues
AppUpdatesURL=https://github.com/xsobhi/hotspot-manager/releases
VersionInfoVersion={#Version}
VersionInfoDescription=Hotspot Manager Setup
; (install.ps1 and the helper service expect it there)
DefaultDirName={autopf}\Hotspot Manager
DisableDirPage=yes
PrivilegesRequired=admin
DisableWelcomePage=no
DisableProgramGroupPage=yes
DisableReadyPage=yes
ShowLanguageDialog=no
WizardStyle=modern dynamic
WizardImageFile={#Art("wizard")}
WizardSmallImageFile={#Art("wizard-small")}
WizardImageAlphaFormat=defined
SetupIconFile=..\data\hotspot-manager.ico
UninstallDisplayIcon={app}\hotspot-manager.exe
UninstallDisplayName=Hotspot Manager
OutputDir=..\dist
OutputBaseFilename=HotspotManager-{#Version}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
CloseApplications=force
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0

[Messages]
WelcomeLabel1=Welcome to Hotspot Manager
WelcomeLabel2=Hotspot Manager {#Version} will be installed on this computer.%n%nShare this PC's internet over Wi-Fi (a USB Wi-Fi adapter works too), see who's connected, limit their speed, block them, and pick the hotspot's address and band.
FinishedHeadingLabel=Hotspot Manager is ready
FinishedLabel=Hotspot Manager is installed, with its helper service.
ClickFinish=

[Tasks]
Name: "defender"; Description: "Let Microsoft Defender leave Hotspot Manager's folder alone. Speed limits, data use and blocking need WinDivert, a packet driver Defender flags as potentially unwanted (as it does most of them, since malware uses them too). Without this, Defender may remove it, and everything else still works."; Flags: unchecked

[Files]
Source: "..\target\release\hotspot-manager.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\install.ps1"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Hotspot Manager"; Filename: "{app}\hotspot-manager.exe"
Name: "{autodesktop}\Hotspot Manager"; Filename: "{app}\hotspot-manager.exe"

[Run]
; the helper service and WinDivert
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\install.ps1"" -Elevated -Quiet -NoShortcuts"; StatusMsg: "Setting up the helper service and WinDivert..."; Flags: runhidden; Tasks: not defender
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\install.ps1"" -Elevated -Quiet -NoShortcuts -AllowWinDivert"; StatusMsg: "Setting up the helper service and WinDivert..."; Flags: runhidden; Tasks: defender
Filename: "{app}\hotspot-manager.exe"; Description: "Open Hotspot Manager"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
; the helper undoes its sharing as it stops; then the service, and Defender's exception if asked for
Filename: "powershell.exe"; Parameters: "-NoProfile -NonInteractive -Command ""Stop-Service HotspotManager -Force -ErrorAction SilentlyContinue; sc.exe delete HotspotManager | Out-Null; Get-Process hotspot-manager -ErrorAction SilentlyContinue | Stop-Process -Force; Remove-MpPreference -ExclusionPath '{app}' -ErrorAction SilentlyContinue"""; Flags: runhidden; RunOnceId: "Service"

[UninstallDelete]
; (WinDivert, downloaded by install.ps1)
Type: filesandordirs; Name: "{app}"

[Code]
// Before installing over a copy: stop the helper service and end Hotspot Manager (its window,
// and its own hotspot), so the files can be replaced; install.ps1 starts the service again.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Result := '';
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
       '-NoProfile -NonInteractive -Command "Stop-Service HotspotManager -Force -ErrorAction SilentlyContinue; ' +
       'Get-Process hotspot-manager -ErrorAction SilentlyContinue | Where-Object { $_.Path -like ''' +
       ExpandConstant('{app}') + '\*'' } | Stop-Process -Force"', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;
