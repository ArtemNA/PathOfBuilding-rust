; The installer of dist/pob-rust/ for the current user: no administrator rights, nothing outside the install folder but the shortcuts and the uninstall entry.
; User data stays in Documents\Path of Building (Rust) on update and on removal. The pob:// protocol is not registered: the original owns it.
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
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "uk"; MessagesFile: "compiler:Languages\Ukrainian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

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
