; The installer of dist/pob-rust/ for the current user: no administrator rights, nothing outside the install folder but the shortcuts and the uninstall entry.
; User data stays in Documents\Path of Building (Rust) on update and on removal. pob:// links are an option of the wizard: the handler they had (the original's, if it is installed) is kept and comes back on removal.
; Built by tools/installer.ps1, which passes MyVersion, RepoRoot and IconFile.

#define MyName "Path of Building (Rust)"
#define MyExe "Path of Building (Rust).exe"

[Setup]
AppId={{6F0C2E74-3B1D-4C58-9A27-5E1D8B7F4C20}
AppName={#MyName}
AppVersion={#MyVersion}
AppVerName={#MyName} {#MyVersion}
AppPublisher=PoB Rust
DefaultDirName={autopf}\{#MyName}
DefaultGroupName={#MyName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile={#RepoRoot}\dist\pob-rust\LICENSE
OutputDir={#RepoRoot}\dist
OutputBaseFilename=pob-rust-{#MyVersion}-windows-x64-setup
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\{#MyExe}
UninstallDisplayName={#MyName}
; The dictionary spans what is alike between tree versions. With the copies in the package it took the installer from 599 MB (the 32 MB of lzma2/max) to 292; without them (tools/package.ps1 keeps one of each) it is 290, this size not measured again
Compression=lzma2/ultra64
LZMADictionarySize=131072
LZMAUseSeparateProcess=yes
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "uk"; MessagesFile: "compiler:Languages\Ukrainian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "pobprotocol"; Description: "{cm:OpenPobLinks}"; GroupDescription: "{cm:LinksGroup}"

[CustomMessages]
en.OpenPobLinks=Open pob:// links in this application
en.LinksGroup=Links:
uk.OpenPobLinks=Відкривати посилання pob:// у цьому застосунку
uk.LinksGroup=Посилання:

[Registry]
Root: HKCU; Subkey: "Software\Classes\pob"; ValueType: string; ValueName: ""; ValueData: "URL:pob"; Tasks: pobprotocol
Root: HKCU; Subkey: "Software\Classes\pob"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: pobprotocol
Root: HKCU; Subkey: "Software\Classes\pob\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyExe},0"; Tasks: pobprotocol
Root: HKCU; Subkey: "Software\Classes\pob\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyExe}"" ""%1"""; Tasks: pobprotocol

; An update replaces the application's files whole: what an older version shipped and this one does not must not stay. These folders hold no user data.
[InstallDelete]
Type: filesandordirs; Name: "{app}\data"
Type: filesandordirs; Name: "{app}\data-patches"
Type: filesandordirs; Name: "{app}\runtime"
Type: filesandordirs; Name: "{app}\src"

[Files]
Source: "{#RepoRoot}\dist\pob-rust\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyName}"; Filename: "{app}\{#MyExe}"
Name: "{autodesktop}\{#MyName}"; Filename: "{app}\{#MyExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyExe}"; Description: "{cm:LaunchProgram,{#MyName}}"; Flags: nowait postinstall skipifsilent

[Code]
const
  CommandKey = 'Software\Classes\pob\shell\open\command';
  IconKey = 'Software\Classes\pob\DefaultIcon';
  BackupKey = 'Software\Path of Building (Rust)';

function IsOurs(Command: String): Boolean;
begin
  Result := Pos(Lowercase('{#MyExe}'), Lowercase(Command)) > 0;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Command, Icon: String;
begin
  // before the registry section: keep the handler the links had, once, and never our own
  if (CurStep = ssInstall) and WizardIsTaskSelected('pobprotocol')
    and RegQueryStringValue(HKCU, CommandKey, '', Command) and not IsOurs(Command) then
  begin
    RegWriteStringValue(HKCU, BackupKey, 'PreviousPobCommand', Command);
    if RegQueryStringValue(HKCU, IconKey, '', Icon) then
      RegWriteStringValue(HKCU, BackupKey, 'PreviousPobIcon', Icon);
  end;
end;

procedure CurUninstallStepChanged(CurStep: TUninstallStep);
var
  Command, Icon: String;
begin
  if CurStep <> usPostUninstall then Exit;
  if RegQueryStringValue(HKCU, CommandKey, '', Command) and IsOurs(Command) then
  begin
    if RegQueryStringValue(HKCU, BackupKey, 'PreviousPobCommand', Command) then
    begin
      RegWriteStringValue(HKCU, CommandKey, '', Command);
      if RegQueryStringValue(HKCU, BackupKey, 'PreviousPobIcon', Icon) then
        RegWriteStringValue(HKCU, IconKey, '', Icon)
      else
        RegDeleteKeyIncludingSubkeys(HKCU, IconKey);
    end
    else
      RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Classes\pob');
  end;
  RegDeleteKeyIncludingSubkeys(HKCU, BackupKey);
end;
