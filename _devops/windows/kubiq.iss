; Kubiq per-user installer. Build: iscc /DMyAppVersion=1.2.3 _devops/windows/kubiq.iss
; Silent install (what the in-app updater runs): /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER
#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif

[Setup]
AppId={{6F1C2B7A-3D4E-4B8F-9A51-7C0E2D9B4A13}
AppName=Kubiq
AppVersion={#MyAppVersion}
AppPublisher=Kubiq
DefaultDirName={localappdata}\Programs\Kubiq
DefaultGroupName=Kubiq
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline
CloseApplications=yes
RestartApplications=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\kubiq.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SourceDir=..\..
OutputDir=.
OutputBaseFilename=Kubiq-Setup-x86_64

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop icon"; GroupDescription: "Additional icons:"
Name: "foldermenu"; Description: "Add ""Open with Kubiq"" to the folder context menu"; GroupDescription: "Shell integration:"; Flags: unchecked

[Files]
Source: "target\kubiq-windows-x86_64\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Kubiq"; Filename: "{app}\kubiq.exe"
Name: "{autodesktop}\Kubiq"; Filename: "{app}\kubiq.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Classes\Directory\shell\Kubiq"; ValueType: string; ValueName: ""; ValueData: "Open with Kubiq"; Flags: uninsdeletekey; Tasks: foldermenu
Root: HKCU; Subkey: "Software\Classes\Directory\shell\Kubiq"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\kubiq.exe"; Tasks: foldermenu
Root: HKCU; Subkey: "Software\Classes\Directory\shell\Kubiq\command"; ValueType: string; ValueName: ""; ValueData: """{app}\kubiq.exe"" ""%1"""; Tasks: foldermenu

[Run]
Filename: "{app}\kubiq.exe"; Description: "Launch Kubiq"; Flags: nowait postinstall skipifsilent
