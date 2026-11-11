; Inno Setup script for the Windows installer (https://jrsoftware.org/isinfo.php).
; Build: cargo build --release -p lychee, then
;   iscc /DAppVersion=0.1.0 packaging\windows\lychee.iss
; The installer lands in dist\. It installs per user by default (no admin prompt) and offers
; an all-users install. Lychee has no runtime dependencies on Windows: the C runtime is linked
; statically (.cargo/config.toml).

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{6F2E7C1A-93B4-4C8E-A5D2-1E7B9C3F4A60}
AppName=Lychee
AppVersion={#AppVersion}
AppVerName=Lychee {#AppVersion}
AppPublisher=Lychee
AppPublisherURL=https://github.com/Y2Kwastaken/Lychee
AppSupportURL=https://github.com/Y2Kwastaken/Lychee/issues
AppUpdatesURL=https://github.com/Y2Kwastaken/Lychee/releases
SourceDir=..\..
OutputDir=dist
OutputBaseFilename=Lychee-{#AppVersion}-windows-x64-setup
DefaultDirName={autopf}\Lychee
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
SetupIconFile=assets\windows\lychee.ico
UninstallDisplayIcon={app}\lychee.ico
UninstallDisplayName=Lychee
LicenseFile=LICENSE
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
ChangesAssociations=yes
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "target\release\lychee.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "assets\windows\lychee.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Lychee"; Filename: "{app}\lychee.exe"; IconFilename: "{app}\lychee.ico"; Comment: "Play your music library"
Name: "{autodesktop}\Lychee"; Filename: "{app}\lychee.exe"; IconFilename: "{app}\lychee.ico"; Tasks: desktopicon

[Registry]
; Offer Lychee under "Open with" for audio files, without taking over the default player.
Root: HKA; Subkey: "Software\Classes\Lychee.AudioFile"; ValueType: string; ValueName: ""; ValueData: "Audio file"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Lychee.AudioFile\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\lychee.ico"
Root: HKA; Subkey: "Software\Classes\Lychee.AudioFile\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\lychee.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "Lychee"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\lychee.ico"
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\lychee.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\.mp3\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".mp3"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.mp2\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".mp2"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.mp1\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".mp1"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.flac\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".flac"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.wav\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".wav"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.wave\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".wave"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.aif\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".aif"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.aiff\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".aiff"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.aifc\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".aifc"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.ogg\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".ogg"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.oga\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".oga"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.opus\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".opus"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.m4a\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".m4a"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.m4b\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".m4b"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.aac\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".aac"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.caf\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".caf"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.mka\OpenWithProgids"; ValueType: string; ValueName: "Lychee.AudioFile"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\lychee.exe\SupportedTypes"; ValueType: string; ValueName: ".mka"; ValueData: ""

[Run]
Filename: "{app}\lychee.exe"; Description: "{cm:LaunchProgram,Lychee}"; Flags: nowait postinstall skipifsilent
