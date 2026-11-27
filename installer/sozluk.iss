; Inno Setup script for Sözlük. Per user, no elevation, like Hub's other apps.
; Build: ISCC.exe /DAppVersion=0.3.0 installer\sozluk.iss  (tools\build-installer.ps1 builds the exe first)
; The AppId below is the uninstall key {4FB1F2BC-2BEF-4862-B431-0EE595F96973}_is1 that Hub's catalog names. Never change it.

#ifndef AppVersion
  #define AppVersion "0.3.0"
#endif
#define Dash Pos("-", AppVersion)
#if Dash > 0
  #define NumVer Copy(AppVersion, 1, Dash - 1)
#else
  #define NumVer AppVersion
#endif
#define AppName "Sözlük"
#define AppExe "sozluk.exe"

[Setup]
AppId={{4FB1F2BC-2BEF-4862-B431-0EE595F96973}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Kaan88
AppPublisherURL=https://github.com/Kaan88/tdk-sozluk-rs
VersionInfoVersion={#NumVer}.0
DefaultDirName={localappdata}\Programs\Sozluk
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\icon.ico
UninstallDisplayName={#AppName}
CloseApplications=yes
RestartApplications=no
OutputDir=..\target\installer
OutputBaseFilename=Sozluk-Setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Turkish, like the app; English when the setup is started with /LANG=english.
LanguageDetectionMethod=none
ShowLanguageDialog=no

[Languages]
Name: "turkish"; MessagesFile: "compiler:Languages\Turkish.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
turkish.DesktopIcon=Masaüstü kısayolu oluştur
turkish.Shortcuts=Kısayollar:
turkish.StartApp={#AppName} uygulamasını başlat
english.DesktopIcon=Create a desktop shortcut
english.Shortcuts=Shortcuts:
english.StartApp=Start {#AppName}

[Tasks]
Name: "desktopicon"; Description: "{cm:DesktopIcon}"; GroupDescription: "{cm:Shortcuts}"; Flags: unchecked

[Files]
Source: "..\target\release\sozluk.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\icon.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\icon.ico"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\icon.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:StartApp}"; Flags: nowait postinstall skipifsilent
