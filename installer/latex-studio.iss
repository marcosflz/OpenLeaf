; Instalador de LaTeX Studio (Inno Setup 6.3+).
;
; Empaqueta la distribución ya montada por `cargo xtask dist` (carpeta
; `dist\latex-ide`): aplicación + agente opencode + skills. NO incluye tus
; documentos (workspace\) ni la configuración del usuario (config\).
;
; Dos sabores:
;   iscc latex-studio.iss                      → instalador ligero (~85 MB):
;                                                app + agente + skills.
;   iscc /DBundledTex latex-studio.iss         → instalador completo (además
;                                                TeX Live reducido, ~4,4 GB).
;   iscc /DOutputName=… /DMyAppVersion=…       → personalizar salida/versión.
;
; Con `-BundledTex` el desinstalador se lleva también `tex\`; sin él, conserva
; el TeX Live que el usuario tenga en la carpeta (por eso no lo toca).

#define MyAppName "LaTeX Studio"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#define MyAppPublisher "LaTeX Studio"
#define MyAppExeName "latex-studio.exe"

#ifndef OutputName
  #define OutputName "LaTeX-Studio-Setup"
#endif

[Setup]
AppId={{8C1D9D2E-5A2B-4C3D-9E4F-6A7B8C9D0E1F}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
; Instalación por usuario (sin administrador), igual que la distro anterior.
DefaultDirName={localappdata}\{#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
OutputDir=..\dist
OutputBaseFilename={#OutputName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
SetupLogging=yes
SetupIconFile=latex-studio.ico
UninstallDisplayName={#MyAppName}
UninstallDisplayIcon={app}\latex-studio.ico
; La app escribe en su propia carpeta: no cerrar nada del usuario al desinstalar.
CloseApplications=no

[Languages]
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[Tasks]
Name: "desktopicon"; Description: "Crear un acceso directo en el escritorio"; GroupDescription: "Accesos directos:"; Flags: unchecked

[Files]
; --- Aplicación + agente + skills (payload que monta `xtask dist`) ---
Source: "..\dist\latex-ide\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\latex-ide\VERSION";         DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\latex-ide\README.md";       DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\dist\latex-ide\SHA256SUMS.txt";  DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\dist\latex-ide\agent\*";         DestDir: "{app}\agent";  Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\dist\latex-ide\assets\*";        DestDir: "{app}\assets"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "latex-studio.ico";                  DestDir: "{app}"; Flags: ignoreversion

; --- TeX Live reducido: solo con /DBundledTex ---
#ifdef BundledTex
Source: "..\dist\latex-ide\tex\*";           DestDir: "{app}\tex"; Flags: ignoreversion recursesubdirs createallsubdirs
#endif

[Icons]
Name: "{group}\{#MyAppName}";              Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\latex-studio.ico"
Name: "{group}\Desinstalar {#MyAppName}";  Filename: "{uninstallexe}"; IconFilename: "{app}\latex-studio.ico"
Name: "{autodesktop}\{#MyAppName}";        Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon; IconFilename: "{app}\latex-studio.ico"

[Run]
Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Description: "Iniciar {#MyAppName} ahora"; Flags: postinstall nowait skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}\logs"
Type: filesandordirs; Name: "{app}\config\opencode.runtime.json"
; Nunca se borran los proyectos del usuario ({app}\workspace) ni sus ajustes.
#ifdef BundledTex
Type: filesandordirs; Name: "{app}\tex"
#endif

[Code]
// Tras copiar los ficheros: comprobar que hay un TeX Live utilizable. Si no lo
// hay (instalador ligero sin tex\), se avisa de cómo añadirlo — es el chequeo de
// primer arranque de M3-3, aquí en el propio instalador.
procedure CurStepChanged(CurStep: TSetupStep);
var
  Marker: String;
begin
  if CurStep = ssPostInstall then
  begin
    Marker := ExpandConstant('{app}\tex\bin\windows\latexmk.exe');
    if not FileExists(Marker) then
      MsgBox('La instalación se ha completado, pero no hay un TeX Live en' + #13#10 +
             ExpandConstant('{app}\tex') + #13#10#13#10 +
             'Puedes copiar ahí tu carpeta "tex" (la del proyecto o de tu TeX Live' +
             ' reducido) y LaTeX Studio la usará al arrancar. Sin ella, el editor y' +
             ' el chat funcionan, pero no se puede compilar.',
             mbInformation, MB_OK);
  end;
end;
