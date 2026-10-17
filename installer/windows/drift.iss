; Drift's Windows installer (Inno Setup 6). Build it with scripts\package-windows.ps1, which
; passes the version:  ISCC /DAppVersion=0.1.0-alpha.1 installer\windows\drift.iss
;
; It installs for the current user by default (no administrator prompt) and offers "for all
; users" in a dialog; adds a Start menu entry, an optional desktop icon, and opens .drift
; sketches with Drift.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Identifies Drift to Windows across versions (upgrades replace the old one). Never change it.
AppId={{8E5B2C5E-6B0A-4E0E-9B5B-0D5A7C1F4D21}
AppName=Drift
AppVersion={#AppVersion}
AppVerName=Drift {#AppVersion}
AppPublisher=Drift
AppPublisherURL=https://github.com/chrisplange/Drift
DefaultDirName={autopf}\Drift
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=..\..\dist
OutputBaseFilename=Drift-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\apps\native\assets\icon.ico
UninstallDisplayIcon={app}\drift.exe
UninstallDisplayName=Drift
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
; Ask a running Drift to close (it prompts about unsaved work) rather than fail on a locked file.
CloseApplications=yes
RestartApplications=no
MinVersion=10.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "associate"; Description: "Open .drift sketches with Drift"; GroupDescription: "File types:"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\drift.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Drift"; Filename: "{app}\drift.exe"
Name: "{autodesktop}\Drift"; Filename: "{app}\drift.exe"; Tasks: desktopicon

[Registry]
; HKA is the current user's classes, or everyone's when installed for all users.
Root: HKA; Subkey: "Software\Classes\.drift"; ValueType: string; ValueName: ""; ValueData: "Drift.Sketch"; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Drift.Sketch"; ValueType: string; ValueName: ""; ValueData: "Drift sketch"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Drift.Sketch\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\drift.exe,0"; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Drift.Sketch\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\drift.exe"" ""%1"""; Tasks: associate

[Run]
Filename: "{app}\drift.exe"; Description: "{cm:LaunchProgram,Drift}"; Flags: nowait postinstall skipifsilent
; Drift's in-app updater runs this installer with /SILENT /RELAUNCH=1: open Drift again after.
Filename: "{app}\drift.exe"; Flags: nowait runasoriginaluser; Check: WantsRelaunch

[Code]
function WantsRelaunch: Boolean;
begin
  Result := ExpandConstant('{param:relaunch|0}') = '1';
end;
