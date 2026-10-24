; The Windows installer (Inno Setup 6). Built by the release workflow after `cargo build --release`:
;   ISCC.exe /DAppVersion=0.9.0 installer\grapht.iss
; It writes dist\grapht-setup-x64.exe. Installs for the current user by default (no admin needed); the first page
; offers "all users" instead.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Never change the AppId: Windows uses it to find the installed copy for updates and uninstalling.
AppId={{80B2B55B-B506-4D39-AEE8-5BE5DFF825D9}
AppName=Grapht
AppVersion={#AppVersion}
AppVerName=Grapht {#AppVersion}
AppPublisher=Gowtham Jeyabalan
AppPublisherURL=https://github.com/gowthamjeyabalan/Grapht
AppSupportURL=https://github.com/gowthamjeyabalan/Grapht/issues
AppUpdatesURL=https://github.com/gowthamjeyabalan/Grapht/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\Grapht
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=grapht-setup-x64
SetupIconFile=..\assets\icon\grapht.ico
UninstallDisplayIcon={app}\grapht.exe
UninstallDisplayName=Grapht
LicenseFile=..\LICENSE
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
ChangesAssociations=yes
; a running Grapht is closed before its files are replaced
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "association"; Description: "Open .grapht session files with Grapht"; GroupDescription: "File types:"

[Files]
Source: "..\target\release\grapht.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\sample\*.csv"; DestDir: "{app}\sample"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Grapht"; Filename: "{app}\grapht.exe"
Name: "{autodesktop}\Grapht"; Filename: "{app}\grapht.exe"; Tasks: desktopicon

[Registry]
; .grapht sessions open in Grapht (in HKCU for a per-user install, HKLM for all users)
Root: HKA; Subkey: "Software\Classes\.grapht"; ValueType: string; ValueName: ""; ValueData: "Grapht.Session"; Flags: uninsdeletevalue; Tasks: association
Root: HKA; Subkey: "Software\Classes\.grapht\OpenWithProgids"; ValueType: string; ValueName: "Grapht.Session"; ValueData: ""; Flags: uninsdeletevalue; Tasks: association
Root: HKA; Subkey: "Software\Classes\Grapht.Session"; ValueType: string; ValueName: ""; ValueData: "Grapht session"; Flags: uninsdeletekey; Tasks: association
Root: HKA; Subkey: "Software\Classes\Grapht.Session\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\grapht.exe,0"; Tasks: association
Root: HKA; Subkey: "Software\Classes\Grapht.Session\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\grapht.exe"" ""%1"""; Tasks: association
; "Open with" lists Grapht for its data files too
Root: HKA; Subkey: "Software\Classes\Applications\grapht.exe\SupportedTypes"; ValueType: string; ValueName: ".grapht"; ValueData: ""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\grapht.exe\SupportedTypes"; ValueType: string; ValueName: ".csv"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\grapht.exe\SupportedTypes"; ValueType: string; ValueName: ".json"; ValueData: ""

[Run]
Filename: "{app}\grapht.exe"; Description: "{cm:LaunchProgram,Grapht}"; Flags: nowait postinstall skipifsilent
