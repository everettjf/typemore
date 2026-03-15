#ifndef AppVersion
  #error AppVersion must be provided with /DAppVersion=...
#endif

#ifndef RepoRoot
  #error RepoRoot must be provided with /DRepoRoot=...
#endif

#ifndef SourceDir
  #error SourceDir must be provided with /DSourceDir=...
#endif

#ifndef OutputDir
  #define OutputDir "."
#endif

#define MyAppName "TypeMore"
#define MyAppExeName "TypeMore.exe"

[Setup]
AppId={{9A74C4B3-37D3-4701-8878-5F5602874F6B}
AppName={#MyAppName}
AppVersion={#AppVersion}
AppPublisher=everettjf
AppPublisherURL=https://typemore.app
AppSupportURL=https://typemore.app
AppUpdatesURL=https://github.com/everettjf/typemore/releases
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile={#RepoRoot}\LICENSE
OutputDir={#OutputDir}
OutputBaseFilename=TypeMore-Setup-{#AppVersion}
SetupIconFile={#RepoRoot}\src-tauri\icons\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
