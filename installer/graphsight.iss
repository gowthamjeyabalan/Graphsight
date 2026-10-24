; The Windows installer (Inno Setup 6). Built by the release workflow after `cargo build --release`:
;   ISCC.exe /DAppVersion=1.0.0 installer\graphsight.iss
; It writes dist\graphsight-setup-x64.exe. Installs for the current user by default (no admin needed); the first page
; offers "all users" instead.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Never change the AppId: Windows uses it to find the installed copy for updates and uninstalling.
AppId={{80B2B55B-B506-4D39-AEE8-5BE5DFF825D9}
AppName=Graphsight
AppVersion={#AppVersion}
AppVerName=Graphsight {#AppVersion}
AppPublisher=Gowtham Jeyabalan
AppPublisherURL=https://github.com/gowthamjeyabalan/Graphsight
AppSupportURL=https://github.com/gowthamjeyabalan/Graphsight/issues
AppUpdatesURL=https://github.com/gowthamjeyabalan/Graphsight/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\Graphsight
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=graphsight-setup-x64
SetupIconFile=..\assets\icon\graphsight.ico
UninstallDisplayIcon={app}\graphsight.exe
UninstallDisplayName=Graphsight
LicenseFile=..\LICENSE
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
ChangesAssociations=yes
; a running Graphsight is closed before its files are replaced
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "association"; Description: "Open .graphsight (and older .grapht) session files with Graphsight"; GroupDescription: "File types:"

[Files]
Source: "..\target\release\graphsight.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\sample\*.csv"; DestDir: "{app}\sample"; Flags: ignoreversion

; Upgrading an install made while the app was called Grapht: take away its program and shortcuts.
[InstallDelete]
Type: files; Name: "{app}\grapht.exe"
Type: files; Name: "{autoprograms}\Grapht.lnk"
Type: files; Name: "{autodesktop}\Grapht.lnk"

[Icons]
Name: "{autoprograms}\Graphsight"; Filename: "{app}\graphsight.exe"
Name: "{autodesktop}\Graphsight"; Filename: "{app}\graphsight.exe"; Tasks: desktopicon

[Registry]
; .graphsight sessions open in Graphsight (in HKCU for a per-user install, HKLM for all users)
Root: HKA; Subkey: "Software\Classes\.graphsight"; ValueType: string; ValueName: ""; ValueData: "Graphsight.Session"; Flags: uninsdeletevalue; Tasks: association
Root: HKA; Subkey: "Software\Classes\.graphsight\OpenWithProgids"; ValueType: string; ValueName: "Graphsight.Session"; ValueData: ""; Flags: uninsdeletevalue; Tasks: association
Root: HKA; Subkey: "Software\Classes\Graphsight.Session"; ValueType: string; ValueName: ""; ValueData: "Graphsight session"; Flags: uninsdeletekey; Tasks: association
Root: HKA; Subkey: "Software\Classes\Graphsight.Session\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\graphsight.exe,0"; Tasks: association
Root: HKA; Subkey: "Software\Classes\Graphsight.Session\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\graphsight.exe"" ""%1"""; Tasks: association
; sessions saved while the app was called Grapht open in Graphsight too; its old file type goes
Root: HKA; Subkey: "Software\Classes\.grapht"; ValueType: string; ValueName: ""; ValueData: "Graphsight.Session"; Flags: uninsdeletevalue; Tasks: association
Root: HKA; Subkey: "Software\Classes\.grapht\OpenWithProgids"; ValueType: string; ValueName: "Graphsight.Session"; ValueData: ""; Flags: uninsdeletevalue; Tasks: association
Root: HKA; Subkey: "Software\Classes\.grapht\OpenWithProgids"; ValueType: none; ValueName: "Grapht.Session"; Flags: deletevalue
Root: HKA; Subkey: "Software\Classes\Grapht.Session"; Flags: deletekey
Root: HKA; Subkey: "Software\Classes\Applications\grapht.exe"; Flags: deletekey
; "Open with" lists Graphsight for its data files too
Root: HKA; Subkey: "Software\Classes\Applications\graphsight.exe"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\graphsight.exe\SupportedTypes"; ValueType: string; ValueName: ".graphsight"; ValueData: ""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\graphsight.exe\SupportedTypes"; ValueType: string; ValueName: ".grapht"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\graphsight.exe\SupportedTypes"; ValueType: string; ValueName: ".csv"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\graphsight.exe\SupportedTypes"; ValueType: string; ValueName: ".json"; ValueData: ""

[Run]
Filename: "{app}\graphsight.exe"; Description: "{cm:LaunchProgram,Graphsight}"; Flags: nowait postinstall skipifsilent
