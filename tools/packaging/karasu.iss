; karasu-slicer's Windows installer (TD-141, TD-150), for Inno Setup 6.
;
; Installs for the current user, without administrator rights: the application and the command
; line under %LOCALAPPDATA%\Programs\karasu-slicer, a Start menu entry (and a desktop one on
; request), the .karasu project association and an uninstaller.
;
; Built from the folder tools/packaging/package.py stages:
;   ISCC.exe /DVersion=0.1.0 /DSource=<staged folder> /DOutput=<output folder> karasu.iss

#ifndef Version
  #error Define Version, for example /DVersion=0.1.0
#endif
#ifndef Source
  #error Define Source, the folder package.py staged
#endif
#ifndef Output
  #define Output "."
#endif

#define Name "karasu-slicer"
#define ProgId "karasu-slicer.project"

[Setup]
; The application's identity for Windows: keep it for every version, so upgrades replace it.
AppId={{25F34AFC-06F6-4A2F-8E5A-A0494F480D2E}
AppName={#Name}
AppVersion={#Version}
AppVerName={#Name} {#Version}
AppPublisher=Ivan Maruca
AppCopyright=Copyright (C) 2026 Ivan Maruca
; The licence is shown before installing, as information (the AGPL needs no acceptance).
LicenseFile={#Source}\LICENSE.txt
DefaultDirName={autopf}\{#Name}
DefaultGroupName={#Name}
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#Output}
OutputBaseFilename={#Name}-{#Version}-windows-x64-setup
; The setup program's own icon; the application's is in karasu.exe.
SetupIconFile=..\..\assets\ui\icon\karasu.ico
UninstallDisplayIcon={app}\karasu.exe
UninstallDisplayName={#Name} {#Version}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Source}\karasu.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\karasu-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\README.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\TRADEMARKS.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; The shortcuts show the icon karasu.exe carries.
Name: "{autoprograms}\{#Name}"; Filename: "{app}\karasu.exe"
Name: "{autodesktop}\{#Name}"; Filename: "{app}\karasu.exe"; Tasks: desktopicon

[Registry]
; Per user (HKA is HKEY_CURRENT_USER without administrator rights). The uninstaller removes them.
Root: HKA; Subkey: "Software\Classes\.karasu"; ValueType: string; ValueName: ""; ValueData: "{#ProgId}"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.karasu\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: ""; ValueData: "{#Name} project"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\karasu.exe,0"
Root: HKA; Subkey: "Software\Classes\{#ProgId}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\karasu.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\karasu.exe\SupportedTypes"; ValueType: string; ValueName: ".karasu"; ValueData: ""; Flags: uninsdeletekey

[Run]
Filename: "{app}\karasu.exe"; Description: "{cm:LaunchProgram,{#Name}}"; Flags: nowait postinstall skipifsilent
