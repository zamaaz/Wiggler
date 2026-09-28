[Setup]
AppId={{7D1C49BC-6E8E-4A4F-A3EA-8D2A3A49F100}
AppName=Wiggler
AppVersion=1.0.0
DefaultDirName={localappdata}\Programs\Wiggler
DefaultGroupName=Wiggler
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=Wiggler-Setup-v1.0.0
Compression=lzma
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName=Wiggler

[Files]
Source: "..\target\release\wiggler.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Wiggler"; Filename: "{app}\wiggler.exe"
Name: "{autodesktop}\Wiggler"; Filename: "{app}\wiggler.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Run]
Filename: "{app}\wiggler.exe"; Description: "Launch Wiggler"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  StartupCommand: String;
begin
  if CurUninstallStep = usUninstall then
    if RegQueryStringValue(HKEY_CURRENT_USER,
      'Software\Microsoft\Windows\CurrentVersion\Run', 'Wiggler', StartupCommand) then
      if Lowercase(StartupCommand) = Lowercase(ExpandConstant('"{app}\wiggler.exe"')) then
        RegDeleteValue(HKEY_CURRENT_USER,
          'Software\Microsoft\Windows\CurrentVersion\Run', 'Wiggler');
end;
