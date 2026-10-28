; Windows installer, built with Inno Setup 6 (see packaging/README.md).
; Installs for the current user, so it needs no administrator rights.

#define AppVersion GetEnv("NEOPSYCH_VERSION")
#if AppVersion == ""
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6A1F2C3E-8B4D-4E5F-9A60-3D7C1B2E4F58}
AppName=neopsych
AppVersion={#AppVersion}
AppPublisher=sorenwacker
AppPublisherURL=https://github.com/sorenwacker/neopsych
DefaultDirName={autopf}\neopsych
DefaultGroupName=neopsych
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=neopsych-{#AppVersion}-windows-setup
SetupIconFile=..\..\assets\neopsych.ico
UninstallDisplayIcon={app}\neopsych.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked
Name: "screensaver"; Description: "Use neopsych as the Windows screensaver"; Flags: unchecked

[Files]
Source: "..\..\target\release\neopsych.exe"; DestDir: "{app}"; Flags: ignoreversion
; The same program under a screensaver's name, for Windows' screensaver settings.
Source: "..\..\target\release\neopsych.exe"; DestDir: "{app}"; DestName: "neopsych.scr"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\neopsych"; Filename: "{app}\neopsych.exe"; Parameters: "--play"
Name: "{group}\neopsych settings"; Filename: "{app}\neopsych.exe"; Parameters: "/c"
Name: "{group}\Uninstall neopsych"; Filename: "{uninstallexe}"
Name: "{autodesktop}\neopsych"; Filename: "{app}\neopsych.exe"; Parameters: "--play"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Control Panel\Desktop"; ValueType: string; ValueName: "SCRNSAVE.EXE"; ValueData: "{app}\neopsych.scr"; Flags: uninsdeletevalue; Tasks: screensaver
Root: HKCU; Subkey: "Control Panel\Desktop"; ValueType: string; ValueName: "ScreenSaveActive"; ValueData: "1"; Tasks: screensaver

[Run]
Filename: "{app}\neopsych.exe"; Parameters: "--play"; Description: "Start neopsych"; Flags: nowait postinstall skipifsilent
