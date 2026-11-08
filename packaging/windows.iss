; Install Fast for the current user and leave their chess data on uninstall.
[Setup]
AppId=dev.fast.app
AppName=Fast
AppVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\Fast
DefaultGroupName=Fast
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=Fast-{#AppVersion}-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\assets\app\fast.ico
UninstallDisplayIcon={app}\fast.exe
LicenseFile=..\LICENSE

[Files]
Source: "..\target\release\fast.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"
Source: "..\assets\LICENSES.md"; DestDir: "{app}\licenses"
Source: "..\crates\app\third_party.json"; DestDir: "{app}\licenses"

[Icons]
Name: "{userprograms}\Fast"; Filename: "{app}\fast.exe"

[Run]
Filename: "{app}\fast.exe"; Description: "Launch Fast"; Flags: nowait postinstall skipifsilent
