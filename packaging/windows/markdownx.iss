#ifndef AppVersion
  #define AppVersion "0.1.16"
#endif
#ifndef AppSource
  #define AppSource "..\..\target\release"
#endif

[Setup]
AppId={{59DC98EE-A136-40C9-A7BD-EB0CC50E37C2}
AppName=MarkdownX
AppVersion={#AppVersion}
AppPublisher=MarkdownX
AppPublisherURL=https://github.com/neko233-com/MarkdownX
DefaultDirName={localappdata}\Programs\markdownx
DisableDirPage=no
DisableProgramGroupPage=yes
DisableWelcomePage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
OutputDir=..\..\dist\windows
OutputBaseFilename=markdownx-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\assets\branding\app-icon.ico
UninstallDisplayIcon={app}\markdownx.exe
WizardStyle=modern light
WizardSmallImageFile=wizard-logo.bmp
WizardSmallImageBackColor=$FFFFFF
WizardImageStretch=no
WizardSizePercent=105,105
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
SetupLogging=yes
#ifdef Signed
SignTool=markdownx
SignedUninstaller=yes
SignedUninstallerDir={#AppSource}
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
SelectDirLabel3=Choose an install folder. Your books are stored separately.
SelectTasksLabel2=Make MarkdownX easier to reach.
FinishedLabel=MarkdownX is ready. Open a book, or make one.

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#AppSource}\markdownx.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSource}\vcruntime140.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"
Source: "..\..\THIRD_PARTY.md"; DestDir: "{app}"

[Icons]
Name: "{userprograms}\MarkdownX"; Filename: "{app}\markdownx.exe"; WorkingDir: "{app}"
Name: "{userdesktop}\MarkdownX"; Filename: "{app}\markdownx.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
; Double-clicking a .mdxb opens it in MarkdownX. Per-user only, removed on uninstall.
Root: HKCU; Subkey: "Software\Classes\.mdxb"; ValueType: string; ValueData: "MarkdownX.Book"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\MarkdownX.Book"; ValueType: string; ValueData: "MarkdownX Book"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\MarkdownX.Book\DefaultIcon"; ValueType: string; ValueData: "{app}\markdownx.exe,0"
Root: HKCU; Subkey: "Software\Classes\MarkdownX.Book\shell\open\command"; ValueType: string; ValueData: """{app}\markdownx.exe"" ""%1"""

[Run]
Filename: "{app}\markdownx.exe"; Description: "Launch MarkdownX"; Flags: nowait postinstall skipifsilent
; The in-app updater installs silently with /UPDATE=1 and expects the app back afterwards.
Filename: "{app}\markdownx.exe"; Flags: nowait; Check: IsUpdate

[Code]
function IsUpdate: Boolean;
begin
  Result := ExpandConstant('{param:UPDATE|0}') = '1';
end;

procedure InitializeWizard;
begin
  WizardForm.Caption := 'MarkdownX';
end;
