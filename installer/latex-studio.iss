; Instalador de LaTeX Studio (Inno Setup 6.3+).
;
; Empaqueta la distribución ya montada por `cargo xtask dist` (carpeta
; `dist\latex-ide`): aplicación + agente opencode + skills. NO incluye tus
; documentos (workspace\) ni la configuración del usuario (config\).
;
; Dos sabores:
;   iscc latex-studio.iss              → instalador ligero:
;                                        app + agente + skills + página "Motor LaTeX".
;   iscc /DBundledTex latex-studio.iss → instalador completo (además TeX Live
;                                        reducido, ~4,4 GB). No descarga nada.
;   iscc /DOutputName=… /DMyAppVersion=… → personalizar salida/versión.
;
; Perfil ligero, página "Motor LaTeX" (después de elegir carpeta):
;   1. si ya existe {app}\tex\bin\windows\latexmk.exe, se usa tal cual;
;   2. si no existe {app}\tex, descarga TinyTeX-1 v2026.10 (~74 MB), verifica
;      SHA-256, extrae con -y -o{app}, renombra TinyTeX → tex y ejecuta
;      `tlmgr update --self` + los paquetes base;
;   3. "No instalar ahora" deja la app sin motor y muestra un aviso accionable.
;
; En /VERYSILENT o /SILENT no hay página: si no hay tex, descarga TinyTeX;
; si ya hay tex, lo usa sin modificarlo. /DBundledTex es la vía offline.
; Un fallo de descarga/extracción/tlmgr NO aborta la instalación: se escribe
; {app}\logs\tex-bootstrap.log y se muestra un aviso (o se registra en silencio).
; El motor descargado deja un marcador en {app}\tex\.lsx-bootstrap.json.
;
; Con `-BundledTex` el desinstalador se lleva también `tex\`; sin él, conserva
; el TeX que el usuario tenga o el TinyTeX descargado (puede tener paquetes suyos).

#define MyAppName "LaTeX Studio"
#ifndef MyAppVersion
  #define MyAppVersion "0.2.0"
#endif
#define MyAppPublisher "LaTeX Studio"
#define MyAppExeName "latex-studio.exe"

#ifndef OutputName
  #define OutputName "LaTeX-Studio-Setup"
#endif

; --- TinyTeX fijado y verificado ------------------------------------------------
; Se pueden sobrescribir con /DTinyTexUrl=… /DTinyTexSha256=… para pruebas.
#ifndef TinyTexVersion
  #define TinyTexVersion "2026.10"
#endif
#ifndef TinyTexFile
  #define TinyTexFile "TinyTeX-1-windows-v2026.10.exe"
#endif
#ifndef TinyTexUrl
  #define TinyTexUrl "https://github.com/rstudio/tinytex-releases/releases/download/v2026.10/TinyTeX-1-windows-v2026.10.exe"
#endif
#ifndef TinyTexSha256
  #define TinyTexSha256 "36c329c644683d981a668419d13c8804696e2816ae4d287ad755788fee231a31"
#endif
#ifndef TinyTexSize
  #define TinyTexSize 73701889
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
; El TinyTeX descargado en el perfil ligero tampoco se borra: puede contener
; paquetes que el usuario haya añadido. Solo el perfil /DBundledTex borra tex\.
#ifdef BundledTex
Type: filesandordirs; Name: "{app}\tex"
#endif

[Code]
const
  TexModeExisting = 0;
  TexModeDownload = 1;
  TexModeSkip = 2;
  TinyTexExpectedSize = {#TinyTexSize};

var
  TexPage: TInputOptionWizardPage;
  TexDownloadPage: TDownloadWizardPage;
  TexProgressPage: TOutputProgressWizardPage;
  TexMode: Integer;
  TexExisting: Boolean;
  TexDirPresent: Boolean;
  TexDownloadFailed: Boolean;
  TexFailureReason: String;
  TexPathProblem: String;

function TinyTexSource: String;
begin
  Result := '{#TinyTexUrl}';
end;

function TinyTexHash: String;
begin
  Result := '{#TinyTexSha256}';
end;

function TinyTexBaseName: String;
begin
  Result := '{#TinyTexFile}';
end;

function TinyTexRelease: String;
begin
  Result := '{#TinyTexVersion}';
end;

function TrailingSlash(const Dir: String): String;
begin
  if (Length(Dir) > 0) and (Dir[Length(Dir)] <> '\') then
    Result := Dir + '\'
  else
    Result := Dir;
end;

function EngineExistsIn(const Dir: String): Boolean;
begin
  Result := FileExists(TrailingSlash(Dir) + 'tex\bin\windows\latexmk.exe');
end;

function TexDirExistsIn(const Dir: String): Boolean;
begin
  Result := DirExists(TrailingSlash(Dir) + 'tex');
end;

function GetTexLogPath: String;
begin
  Result := ExpandConstant('{app}\logs\tex-bootstrap.log');
end;

function SiNo(const Value: Boolean): String;
begin
  if Value then
    Result := 'si'
  else
    Result := 'no';
end;

function JsonBool(const Value: Boolean): String;
begin
  if Value then
    Result := 'true'
  else
    Result := 'false';
end;

function IsAsciiPath(const Dir: String): Boolean;
var
  I: Integer;
begin
  Result := True;
  for I := 1 to Length(Dir) do
    if Ord(Dir[I]) > 127 then
    begin
      Result := False;
      Exit;
    end;
end;

function FriendlyDownloadError(const Raw: String): String;
begin
  if (Pos('12029', Raw) > 0) or (Pos('12007', Raw) > 0) or (Pos('12002', Raw) > 0) then
    Result := 'No se pudo conectar con el servidor (sin red, proxy o URL no disponible).'
  else if Pos('12028', Raw) > 0 then
    Result := 'El proxy o la conexion rechazaron la descarga.'
  else if (Pos('hash', Raw) > 0) or (Pos('SHA', Raw) > 0) or (Pos('verificaci', Raw) > 0) then
    Result := 'El archivo descargado no coincide con el SHA-256 esperado.'
  else
    Result := Raw;
end;

procedure TexLog(const Line: String);
var
  Path: String;
begin
  Log('[tex] ' + Line);
  Path := GetTexLogPath;
  ForceDirectories(ExtractFileDir(Path));
  SaveStringToFile(Path, GetDateTimeString('yyyy-mm-dd hh:nn:ss', '-', ':') + ' ' + Line + #13#10, True);
end;

procedure TexLogHeader;
var
  S: String;
begin
  ForceDirectories(ExpandConstant('{app}\logs'));
  S := #13#10 + '=== LaTeX Studio {#MyAppVersion} - motor LaTeX - ' +
       GetDateTimeString('yyyy-mm-dd hh:nn:ss', '-', ':') + ' ===' + #13#10 +
       'app=' + ExpandConstant('{app}') + #13#10 +
       'tinytex=' + TinyTexSource + #13#10 +
       'sha256=' + TinyTexHash + #13#10 +
       'size=' + IntToStr(TinyTexExpectedSize) + #13#10;
  SaveStringToFile(GetTexLogPath, S, True);
  Log('[tex] inicio de bootstrap; log=' + GetTexLogPath);
  if not IsAsciiPath(ExpandConstant('{app}')) then
    TexLog('Aviso: la ruta de instalacion contiene caracteres no ASCII; TinyTeX/latexmk puede fallar en ella.');
end;

function MissingBinaries(const BinDir: String): String;
begin
  Result := '';
  if not FileExists(BinDir + '\latexmk.exe') then
    Result := Result + 'latexmk ';
  if not FileExists(BinDir + '\synctex.exe') then
    Result := Result + 'synctex ';
  if not FileExists(BinDir + '\biber.exe') then
    Result := Result + 'biber ';
  Result := Trim(Result);
end;

function PsQuote(const S: String): String;
var
  I: Integer;
begin
  Result := '''';
  for I := 1 to Length(S) do
  begin
    if S[I] = '''' then
      Result := Result + ''''''
    else
      Result := Result + S[I];
  end;
  Result := Result + '''';
end;

function RunnerScript: String;
begin
  Result :=
    'param(' + #13#10 +
    '  [Parameter(Mandatory=$true)][string]$Exe,' + #13#10 +
    '  [string]$Arguments = "",' + #13#10 +
    '  [string]$WorkingDirectory = "",' + #13#10 +
    '  [string]$OutputDir = "",' + #13#10 +
    '  [Parameter(Mandatory=$true)][string]$LogPath,' + #13#10 +
    '  [int]$TimeoutSeconds = 1800' + #13#10 +
    ')' + #13#10 +
    '$ErrorActionPreference = "Stop"' + #13#10 +
    'function Write-Log([string]$Text) {' + #13#10 +
    '  if ($Text -ne $null -and $Text -ne "") {' + #13#10 +
    '    Add-Content -LiteralPath $LogPath -Value $Text -Encoding UTF8' + #13#10 +
    '  }' + #13#10 +
    '}' + #13#10 +
    'try {' + #13#10 +
    '  $psi = New-Object System.Diagnostics.ProcessStartInfo' + #13#10 +
    '  $psi.FileName = $Exe' + #13#10 +
    '  $psi.Arguments = $Arguments' + #13#10 +
    '  if ($OutputDir -ne "") {' + #13#10 +
    '    $psi.Arguments = $Arguments + " -o" + [char]34 + $OutputDir + [char]34' + #13#10 +
    '  }' + #13#10 +
    '  $psi.UseShellExecute = $false' + #13#10 +
    '  $psi.CreateNoWindow = $true' + #13#10 +
    '  $psi.RedirectStandardOutput = $true' + #13#10 +
    '  $psi.RedirectStandardError = $true' + #13#10 +
    '  $psi.RedirectStandardInput = $true' + #13#10 +
    '  if ($WorkingDirectory -ne "") { $psi.WorkingDirectory = $WorkingDirectory }' + #13#10 +
    '  $p = New-Object System.Diagnostics.Process' + #13#10 +
    '  $p.StartInfo = $psi' + #13#10 +
    '  $null = $p.Start()' + #13#10 +
    '  $p.StandardInput.Close()' + #13#10 +
    '  $outTask = $p.StandardOutput.ReadToEndAsync()' + #13#10 +
    '  $errTask = $p.StandardError.ReadToEndAsync()' + #13#10 +
    '  if (-not $p.WaitForExit($TimeoutSeconds * 1000)) {' + #13#10 +
    '    $old = $ErrorActionPreference' + #13#10 +
    '    $ErrorActionPreference = "SilentlyContinue"' + #13#10 +
    '    & taskkill.exe /PID $p.Id /T /F 2>$null | Out-Null' + #13#10 +
    '    $ErrorActionPreference = $old' + #13#10 +
    '    if (-not $p.HasExited) { try { $p.Kill() } catch { } }' + #13#10 +
    '    Write-Log ("TIMEOUT after " + $TimeoutSeconds + "s: " + $Exe + " " + $Arguments)' + #13#10 +
    '    exit 124' + #13#10 +
    '  }' + #13#10 +
    '  $p.WaitForExit()' + #13#10 +
    '  Write-Log $outTask.Result' + #13#10 +
    '  Write-Log $errTask.Result' + #13#10 +
    '  Write-Log ("EXIT " + $p.ExitCode + ": " + $Exe + " " + $Arguments)' + #13#10 +
    '  exit $p.ExitCode' + #13#10 +
    '} catch {' + #13#10 +
    '  Write-Log ("ERROR: " + $_.Exception.Message)' + #13#10 +
    '  exit 1' + #13#10 +
    '}' + #13#10;
end;

function PrepareRunner: Boolean;
var
  Path: String;
begin
  Path := ExpandConstant('{tmp}\lsx-run.ps1');
  Result := SaveStringToFile(Path, RunnerScript, False);
  if Result then
    TexLog('Ejecutor PowerShell preparado: ' + Path)
  else
    TexLog('No se pudo escribir ' + Path);
end;

function RunStep(const StepName, Exe, Args, WorkDir, OutputDir: String;
  TimeoutSeconds: Integer; var ExitCode: Integer): Boolean;
var
  Runner, Cmd, Params, Ps: String;
begin
  Runner := ExpandConstant('{tmp}\lsx-run.ps1');
  Ps := ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe');
  Cmd := '& ' + PsQuote(Runner) +
         ' -Exe ' + PsQuote(Exe) +
         ' -Arguments ' + PsQuote(Args) +
         ' -LogPath ' + PsQuote(GetTexLogPath) +
         ' -TimeoutSeconds ' + IntToStr(TimeoutSeconds);
  if WorkDir <> '' then
    Cmd := Cmd + ' -WorkingDirectory ' + PsQuote(WorkDir);
  if OutputDir <> '' then
    Cmd := Cmd + ' -OutputDir ' + PsQuote(OutputDir);
  Params := '-NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "' +
            Cmd + '; if ($null -eq $LASTEXITCODE) { exit 1 } else { exit $LASTEXITCODE }"';
  TexLog('Ejecutando ' + StepName + '...');
  Result := Exec(Ps, Params, '', SW_HIDE, ewWaitUntilTerminated, ExitCode);
  if Result then
    TexLog(StepName + ': exit=' + IntToStr(ExitCode))
  else
  begin
    TexLog('No se pudo lanzar ' + StepName + ': ' + SysErrorMessage(ExitCode));
    ExitCode := -1;
  end;
end;

procedure CleanupStage(const Stage: String);
begin
  if DirExists(Stage) then
  begin
    if DelTree(Stage, True, True, True) then
      TexLog('Restos eliminados: ' + Stage)
    else
      TexLog('Aviso: no se pudieron eliminar los restos de ' + Stage);
  end;
end;

function TinyTexFileReady: Boolean;
begin
  Result := FileExists(ExpandConstant('{tmp}\') + TinyTexBaseName);
end;

procedure LogDownloadedFile;
var
  P: String;
  Size: Int64;
begin
  P := ExpandConstant('{tmp}\') + TinyTexBaseName;
  if FileSize64(P, Size) then
  begin
    TexLog('TinyTeX descargado: ' + P + ' (' + IntToStr(Size) + ' bytes; esperado ' +
      IntToStr(TinyTexExpectedSize) + '); SHA-256 verificado.');
    if Size <> TinyTexExpectedSize then
      TexLog('Aviso: el tamano del archivo no coincide con el esperado.');
  end
  else
    TexLog('Aviso: no se pudo leer el tamano de ' + P);
end;

function DownloadTinyTexInteractive: Boolean;
var
  ErrorText, Raw: String;
  Retry: Boolean;
begin
  Result := False;
  repeat
    Retry := False;
    TexDownloadPage.Clear;
    TexDownloadPage.Add(TinyTexSource, TinyTexBaseName, TinyTexHash);
    TexDownloadPage.ShowBaseNameInsteadOfUrl := True;
    TexDownloadPage.Show;
    try
      try
        TexDownloadPage.Download;
        Result := True;
      except
        if TexDownloadPage.AbortedByUser then
        begin
          TexFailureReason := 'Descarga cancelada por el usuario.';
          TexLog(TexFailureReason);
        end
        else
        begin
          Raw := GetExceptionMessage;
          TexLog('Error de descarga: ' + Raw);
          ErrorText := Format('%s: %s', [TexDownloadPage.LastBaseNameOrUrl, FriendlyDownloadError(Raw)]);
          TexFailureReason := 'No se pudo descargar TinyTeX: ' + ErrorText;
          TexLog(TexFailureReason);
          if MsgBox('No se pudo descargar TinyTeX-1.' + #13#10#13#10 + ErrorText + #13#10#13#10 +
                    'La aplicacion se instalara igualmente, pero sin motor LaTeX.' + #13#10 +
                    'Comprueba la conexion y el proxy, o usa el instalador /DBundledTex.' + #13#10#13#10 +
                    '¿Reintentar la descarga?', mbError, MB_RETRYCANCEL) = IDRETRY then
            Retry := True;
        end;
      end;
    finally
      TexDownloadPage.Hide;
    end;
  until not Retry;
  if Result then
  begin
    TexFailureReason := '';
    LogDownloadedFile;
  end;
end;

function DownloadTinyTexSilent: Boolean;
var
  P, Raw: String;
  Bytes: Int64;
begin
  Result := False;
  P := ExpandConstant('{tmp}\') + TinyTexBaseName;
  try
    TexLog('Descargando TinyTeX en modo silencioso...');
    Bytes := DownloadTemporaryFile(TinyTexSource, TinyTexBaseName, TinyTexHash, nil);
    if FileExists(P) then
    begin
      Result := True;
      TexLog('Descarga completada (' + IntToStr(Bytes) + ' bytes nuevos); SHA-256 verificado.');
      LogDownloadedFile;
    end
    else
      TexLog('La descarga termino pero no se encontro ' + P);
  except
    Raw := GetExceptionMessage;
    TexLog('Error de descarga: ' + Raw);
    TexFailureReason := 'No se pudo descargar TinyTeX: ' + FriendlyDownloadError(Raw);
    TexLog(TexFailureReason);
    Result := False;
  end;
  TexDownloadFailed := not Result;
end;

procedure WriteBootstrapMarker(const Status: String;
  const HasLatexmk, HasSynctex, HasBiber: Boolean);
var
  S: String;
begin
  S := '{' + #13#10 +
       '  "schema": 1,' + #13#10 +
       '  "mode": "tinytex",' + #13#10 +
       '  "source": "' + TinyTexSource + '",' + #13#10 +
       '  "version": "' + TinyTexRelease + '",' + #13#10 +
       '  "sha256": "' + TinyTexHash + '",' + #13#10 +
       '  "installedAt": "' + GetDateTimeString('yyyy-mm-dd hh:nn:ss', '-', ':') + '",' + #13#10 +
       '  "status": "' + Status + '",' + #13#10 +
       '  "binaries": {' + #13#10 +
       '    "latexmk": ' + JsonBool(HasLatexmk) + ',' + #13#10 +
       '    "synctex": ' + JsonBool(HasSynctex) + ',' + #13#10 +
       '    "biber": ' + JsonBool(HasBiber) + #13#10 +
       '  }' + #13#10 +
       '}' + #13#10;
  if not SaveStringToFile(ExpandConstant('{app}\tex\.lsx-bootstrap.json'), S, False) then
    TexLog('Aviso: no se pudo escribir {app}\tex\.lsx-bootstrap.json');
end;

procedure WarnTexIncomplete(const Reason: String);
var
  Msg: String;
begin
  Msg := 'El motor LaTeX no quedo completo.' + #13#10#13#10 +
    Reason + #13#10#13#10 +
    'LaTeX Studio se ha instalado igualmente: el editor y el chat funcionan, ' +
    'pero la compilacion necesita un motor.' + #13#10#13#10 +
    'Detalle en:' + #13#10 + GetTexLogPath + #13#10#13#10 +
    'Para reintentar, vuelve a ejecutar este instalador y elige "Descargar TinyTeX-1".';
  if WizardSilent then
    TexLog(Msg)
  else
    MsgBox(Msg, mbError, MB_OK);
end;

procedure WarnTexSkipped;
var
  Msg: String;
begin
  Msg := 'No se ha instalado un motor LaTeX.' + #13#10#13#10;
  if TexFailureReason <> '' then
    Msg := Msg + TexFailureReason + #13#10#13#10
  else if TexPathProblem <> '' then
    Msg := Msg + TexPathProblem + #13#10#13#10;
  Msg := Msg +
    'LaTeX Studio se ha instalado y arrancara, pero no podra compilar hasta que haya un motor en:' + #13#10 +
    ExpandConstant('{app}\tex') + #13#10#13#10 +
    'Para anadirlo, vuelve a ejecutar este instalador y elige "Descargar TinyTeX-1" ' +
    '(necesita conexion) o copia ahi tu carpeta tex con latexmk.exe.';
  TexLog('Bootstrap omitido: ' + Msg);
  if not WizardSilent then
    MsgBox(Msg, mbInformation, MB_OK);
end;

procedure VerifyExistingTex;
var
  BinDir, Missing: String;
begin
  BinDir := ExpandConstant('{app}\tex\bin\windows');
  Missing := MissingBinaries(BinDir);
  if Missing = '' then
    TexLog('Motor existente verificado: latexmk, synctex y biber presentes.')
  else
  begin
    TexFailureReason := 'El motor existente no tiene: ' + Missing + '.';
    TexLog(TexFailureReason + ' No se modifica la instalacion del usuario.');
    if not WizardSilent then
      MsgBox('Se usara el motor LaTeX existente en:' + #13#10 + BinDir + #13#10#13#10 +
             'Pero faltan: ' + Missing + '.' + #13#10#13#10 +
             'No se ha modificado tu TeX. Si es TeX Live, puedes anadir esos paquetes con tlmgr; ' +
             'si no, vuelve a ejecutar el instalador en una carpeta sin tex para descargar TinyTeX-1.',
             mbInformation, MB_OK);
  end;
end;

procedure VerifyBundledTex;
var
  BinDir, Missing: String;
begin
  BinDir := ExpandConstant('{app}\tex\bin\windows');
  Missing := MissingBinaries(BinDir);
  if Missing = '' then
    TexLog('TeX incluido (/DBundledTex) verificado: latexmk, synctex y biber.')
  else
  begin
    TexFailureReason := 'El TeX incluido no tiene: ' + Missing + '.';
    TexLog(TexFailureReason);
    if not WizardSilent then
      MsgBox('El instalador completo no ha dejado estos binarios: ' + Missing + '.' + #13#10#13#10 +
             'Revisa el log: ' + GetTexLogPath, mbError, MB_OK);
  end;
end;

procedure TexProgress(const Step, Total: Integer; const Text, Detail: String);
begin
  if Assigned(TexProgressPage) and (not WizardSilent) then
  begin
    TexProgressPage.SetText(Text, Detail);
    TexProgressPage.SetProgress(Step, Total);
  end;
end;

procedure ShowTexProgress(const Text: String);
begin
  if Assigned(TexProgressPage) and (not WizardSilent) then
  begin
    TexProgressPage.Show;
    TexProgressPage.SetText(Text, '');
    TexProgressPage.SetProgress(0, 4);
  end;
end;

procedure HideTexProgress;
begin
  if Assigned(TexProgressPage) and (not WizardSilent) then
    TexProgressPage.Hide;
end;

procedure InstallTinyTex;
var
  Apps, Sfx, TexDir, Stage, BinDir, CmdExe, Missing: String;
  Code: Integer;
  Ok: Boolean;
  HasLatexmk, HasSynctex, HasBiber: Boolean;
begin
  Apps := ExpandConstant('{app}');
  Sfx := ExpandConstant('{tmp}\') + TinyTexBaseName;
  TexDir := TrailingSlash(Apps) + 'tex';
  Stage := TrailingSlash(Apps) + 'TinyTeX';
  BinDir := TexDir + '\bin\windows';
  CmdExe := ExpandConstant('{sys}\cmd.exe');

  TexLog('Modo descarga TinyTeX. Instalador=' + Sfx);

  if not IsAsciiPath(Apps) then
  begin
    if TexPathProblem <> '' then
      TexFailureReason := TexPathProblem
    else
      TexFailureReason := 'La ruta de instalacion contiene caracteres no ASCII; TinyTeX no funciona en ella.';
    TexLog(TexFailureReason);
    WarnTexIncomplete(TexFailureReason);
    Exit;
  end;

  if TexDirExistsIn(Apps) then
  begin
    TexFailureReason := 'Ya existe una carpeta tex en ' + TexDir +
      ', pero no contiene latexmk.exe. No se ha modificado.';
    TexLog(TexFailureReason);
    WarnTexIncomplete(TexFailureReason);
    Exit;
  end;

  if not TinyTexFileReady then
  begin
    TexFailureReason := 'No hay instalador de TinyTeX descargado en ' + Sfx + '.';
    TexLog(TexFailureReason);
    WarnTexIncomplete(TexFailureReason);
    Exit;
  end;

  if not PrepareRunner then
  begin
    TexFailureReason := 'No se pudo preparar el ejecutor de PowerShell.';
    TexLog(TexFailureReason);
    WarnTexIncomplete(TexFailureReason);
    Exit;
  end;

  ShowTexProgress('Extrayendo TinyTeX-1...');
  try
    try
      if DirExists(Stage) then
      begin
        TexLog('Limpiando restos de una extraccion anterior: ' + Stage);
        if not DelTree(Stage, True, True, True) then
          TexLog('Aviso: no se pudo limpiar por completo ' + Stage);
      end;

      TexProgress(1, 4, 'Extrayendo TinyTeX-1...', 'unos 74 MB');
      Ok := RunStep('extraer TinyTeX', Sfx, '-y', Apps, Apps, 900, Code);
      if (not Ok) or (Code <> 0) or (not FileExists(Stage + '\bin\windows\latexmk.exe')) then
      begin
        TexFailureReason := Format('La extraccion de TinyTeX fallo (exit=%d).', [Code]);
        TexLog(TexFailureReason);
        CleanupStage(Stage);
        WarnTexIncomplete(TexFailureReason);
        Exit;
      end;
      TexLog('TinyTeX extraido en ' + Stage);

      TexProgress(2, 4, 'Preparando el motor LaTeX...', '');
      if not RenameFile(Stage, TexDir) then
      begin
        TexFailureReason := 'No se pudo mover ' + Stage + ' a ' + TexDir + '.';
        TexLog(TexFailureReason);
        CleanupStage(Stage);
        WarnTexIncomplete(TexFailureReason);
        Exit;
      end;
      TexLog('Carpeta tex creada en ' + TexDir);

      TexProgress(2, 4, 'Actualizando tlmgr...', 'puede tardar unos minutos');
      Ok := RunStep('tlmgr update --self', CmdExe,
        '/d /c .\tlmgr.bat update --self', BinDir, '', 900, Code);
      if (not Ok) or (Code <> 0) then
        TexLog('Aviso: tlmgr update --self no termino bien (exit=' + IntToStr(Code) + ').');

      TexProgress(3, 4, 'Instalando synctex y biber...', '');
      Ok := RunStep('tlmgr install synctex biber', CmdExe,
        '/d /c .\tlmgr.bat install synctex biber', BinDir, '', 1200, Code);
      if (not Ok) or (Code <> 0) then
        TexLog('Aviso: tlmgr install synctex biber fallo (exit=' + IntToStr(Code) + ').');

      TexProgress(3, 4, 'Instalando paquetes base...', 'beamer, pgf, babel...');
      Ok := RunStep('tlmgr install paquetes base', CmdExe,
        '/d /c .\tlmgr.bat install beamer pgf grfext titlesec enumitem preprint marvosym roboto fancyhdr tools babel-spanish babel-english',
        BinDir, '', 1800, Code);
      if (not Ok) or (Code <> 0) then
        TexLog('Aviso: tlmgr install de paquetes base fallo (exit=' + IntToStr(Code) + ').');

      TexProgress(4, 4, 'Verificando el motor LaTeX...', '');
      HasLatexmk := FileExists(BinDir + '\latexmk.exe');
      HasSynctex := FileExists(BinDir + '\synctex.exe');
      HasBiber := FileExists(BinDir + '\biber.exe');
      Missing := MissingBinaries(BinDir);

      if HasLatexmk then
      begin
        if Missing = '' then
          WriteBootstrapMarker('ok', True, True, True)
        else
          WriteBootstrapMarker('partial', HasLatexmk, HasSynctex, HasBiber);
      end;

      if Missing = '' then
      begin
        TexFailureReason := '';
        TexLog('Motor LaTeX listo: latexmk, synctex y biber presentes.');
        if not WizardSilent then
          MsgBox('Motor LaTeX instalado.' + #13#10#13#10 +
                 'TinyTeX ' + TinyTexRelease + ' con latexmk, synctex y biber esta listo en:' + #13#10 +
                 TexDir, mbInformation, MB_OK);
      end
      else
      begin
        TexFailureReason := 'Faltan binarios: ' + Missing + '.';
        TexLog('Aviso: ' + TexFailureReason);
        if not HasLatexmk then
        begin
          TexLog('El motor no es utilizable; se elimina la carpeta tex creada por el instalador.');
          DelTree(TexDir, True, True, True);
        end;
        WarnTexIncomplete(TexFailureReason);
      end;
    except
      TexFailureReason := 'Error inesperado: ' + GetExceptionMessage;
      TexLog(TexFailureReason);
      WarnTexIncomplete(TexFailureReason);
    end;
  finally
    HideTexProgress;
  end;
end;

procedure RefreshTexPage(const Dir: String);
begin
  TexExisting := EngineExistsIn(Dir);
  TexDirPresent := TexDirExistsIn(Dir);
  TexPathProblem := '';
  if not IsAsciiPath(Dir) then
    TexPathProblem := 'La ruta de instalacion contiene caracteres no ASCII (acentos, ñ, etc.). ' +
      'TinyTeX no funciona en esas rutas; usa una carpeta ASCII, por ejemplo C:\LaTeXStudio.';
  if TexExisting then
    TexFailureReason := ''
  else if TexDirPresent then
    TexFailureReason := 'Ya existe una carpeta tex en ' + Dir +
      ', pero no contiene latexmk.exe. No se ha modificado.'
  else if TexPathProblem <> '' then
    TexFailureReason := TexPathProblem
  else
    TexFailureReason := '';
  if not Assigned(TexPage) then
  begin
    if TexExisting then
      TexMode := TexModeExisting
    else if (TexDirPresent) or (TexPathProblem <> '') then
      TexMode := TexModeSkip
    else
      TexMode := TexModeDownload;
    Exit;
  end;
  TexPage.CheckListBox.ItemEnabled[0] := TexExisting;
  TexPage.CheckListBox.ItemEnabled[1] := (not TexDirPresent) and (TexPathProblem = '');
  if TexExisting then
    TexPage.SelectedValueIndex := 0
  else if (not TexDirPresent) and (TexPathProblem = '') then
    TexPage.SelectedValueIndex := 1
  else
    TexPage.SelectedValueIndex := 2;
  Log(Format('[tex] motor: existente=%s, carpeta=%s, opcion=%d, ruta_ascii=%s', [SiNo(TexExisting), SiNo(TexDirPresent), TexPage.SelectedValueIndex, SiNo(TexPathProblem = '')]));
  if TexPathProblem <> '' then
    Log('[tex] ' + TexPathProblem);
end;

procedure InitializeWizard;
begin
  #ifndef BundledTex
  TexMode := TexModeDownload;
  if not WizardSilent then
  begin
    TexPage := CreateInputOptionPage(wpSelectDir,
      'Motor LaTeX',
      'Elige como preparar el motor de compilacion.',
      'LaTeX Studio necesita latexmk, synctex y biber. Si ya existe una carpeta tex, ' +
      'el instalador no la modificara ni la borrara. La descarga recomendada solo esta ' +
      'disponible cuando no hay una carpeta tex. Si la ruta contiene acentos o caracteres ' +
      'no ASCII, TinyTeX no funciona: elige una carpeta ASCII (por ejemplo C:\LaTeXStudio).',
      True, False);
    TexPage.Add('Usar el motor existente (detectado en la carpeta de instalacion)');
    TexPage.Add('Descargar TinyTeX-1 (recomendado, ~74 MB)');
    TexPage.Add('No instalar ahora (la app arrancara sin compilar)');
    TexPage.SelectedValueIndex := 1;
    TexDownloadPage := CreateDownloadPage('Motor LaTeX',
      'Descargando TinyTeX-1 y verificando su SHA-256...', nil);
    TexDownloadPage.ShowBaseNameInsteadOfUrl := True;
    TexProgressPage := CreateOutputProgressPage('Motor LaTeX',
      'Preparando el motor de compilacion...');
  end;
  #endif
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := False;
  #ifndef BundledTex
  if Assigned(TexPage) then
    if (PageID = TexPage.ID) and WizardSilent then
      Result := True;
  #endif
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  #ifndef BundledTex
  if Assigned(TexPage) then
  begin
    if CurPageID = wpSelectDir then
      RefreshTexPage(WizardForm.DirEdit.Text)
    else if CurPageID = TexPage.ID then
    begin
      case TexPage.SelectedValueIndex of
        0: TexMode := TexModeExisting;
        1: TexMode := TexModeDownload;
      else
        TexMode := TexModeSkip;
      end;
      if (TexMode = TexModeExisting) and (not EngineExistsIn(WizardForm.DirEdit.Text)) then
      begin
        MsgBox('No se encontro latexmk.exe en la carpeta seleccionada. Elige otra opcion.',
          mbError, MB_OK);
        Result := False;
      end
      else if (TexMode = TexModeDownload) and (TexDirExistsIn(WizardForm.DirEdit.Text) or (TexPathProblem <> '')) then
      begin
        MsgBox('No se puede descargar TinyTeX en esta carpeta: ya existe una carpeta tex o la ' +
               'ruta contiene caracteres no ASCII. El instalador no modificara nada. Elige una ' +
               'carpeta ASCII sin tex o usa el instalador /DBundledTex.',
          mbError, MB_OK);
        Result := False;
      end;
    end;
  end;
  if (CurPageID = wpReady) and (TexMode = TexModeDownload) and (not WizardSilent) then
  begin
    if TexPathProblem <> '' then
    begin
      TexFailureReason := TexPathProblem;
      TexMode := TexModeSkip;
    end
    else if not DownloadTinyTexInteractive then
    begin
      TexDownloadFailed := True;
      TexMode := TexModeSkip;
    end;
  end;
  #endif
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep <> ssPostInstall then
    Exit;

  TexLogHeader;

  #ifdef BundledTex
  VerifyBundledTex;
  #else
  if WizardSilent then
    RefreshTexPage(ExpandConstant('{app}'));

  // Red de seguridad: si aparecio un latexmk valido, no lo pisamos.
  if (TexMode = TexModeDownload) and EngineExistsIn(ExpandConstant('{app}')) then
    TexMode := TexModeExisting;

  case TexMode of
    TexModeExisting:
      VerifyExistingTex;
    TexModeDownload:
      begin
        if WizardSilent then
        begin
          if not DownloadTinyTexSilent then
          begin
            TexMode := TexModeSkip;
            WarnTexSkipped;
          end;
        end;
        if TexMode = TexModeDownload then
          InstallTinyTex;
      end;
  else
    WarnTexSkipped;
  end;
  #endif
end;
