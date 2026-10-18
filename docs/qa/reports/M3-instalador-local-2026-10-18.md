# M3 — Instalador compilado y verificado **en local** (2026-10-18)

- **Origen**: petición del usuario — «de momento olvidémonos de GitHub Actions: generamos el `.exe` en local».
- **Entorno**: Windows · Inno Setup 6 (`%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe`, ya instalado) · PowerShell 7 · `tex\` portátil del repo.
- **Cadena de compilación** (sin CI):
  ```
  cargo build --release -p latex-studio
  cargo run -p xtask -- dist --no-tex --no-zip --clean
  pwsh -File installer\build.ps1 -SkipBuild
  ```
- **Artefacto**: `dist\LaTeX-Studio-Setup.exe`
  - 53,8 MB (56 417 328 bytes) · versión `0.1.0-20261018` · sin firma de código (esperado, no hay certificado)
  - SHA-256 `FE0684343DB87B73B7CDDFD5B0DC7034A8C3E6A23DF5A8531C257B727535288C`
  - Comprime con `lzma2/max` un payload de ~254 MB: app 82,9 MB + agente `opencode.exe` 171,5 MB + `assets\` (4 skills, plantillas, tree-sitter)
  - **No incluye** `tex\` (perfil ligero) ni `workspace\` (documentos del usuario)
- **Gates**: `cargo fmt --check` ✔ · `cargo clippy --workspace --all-targets -- -D warnings` ✔ · `cargo test --workspace` ✔ (**328 tests, 0 fallos**)

## 1. Instalación verificada (perfil ligero, por usuario)

| Comprobación | Resultado |
|---|---|
| Instala sin UAC en `%LOCALAPPDATA%\LaTeX Studio` | ✅ `/VERYSILENT`, exit 0, con `/LOG` |
| Entrada de desinstalación en HKCU | ✅ `LaTeX Studio · 0.1.0-20261018` + `unins000.exe` |
| Accesos directos (menú Inicio + desinstalador) | ✅ |
| No toca datos del usuario | ✅ el payload no trae `workspace\` ni `config\settings.json`; reinstalar encima conserva `workspace\` |
| Aviso de TeX ausente | ✅ va al **log** (`Aviso: no hay TeX Live en …`) en instalación silenciosa, sin diálogo |

## 2. Primer arranque verificado

| Comprobación | Evidencia |
|---|---|
| La app arranca desde la carpeta instalada | ventana «LaTeX Studio» maximizada, `demo-articulo` en el título |
| Crea el layout que falta (`workspace\`, `config\`) | ✅ tras el arreglo nº 1 |
| **Arranca el agente** desde `agent\opencode.exe` | `serve --hostname 127.0.0.1 --port 52282` (PID propio), barra de estado **IA lista** |
| Config efectiva con las skills portables | `config\opencode.runtime.json` con `skills.paths = ["<app>\assets\skills"]` |
| **El chat responde y conoce las skills** | pregunta real al agente instalado → responde «latex-beamer, latex-compilar-depurar, latex-matematicas y latex-tablas-figuras» |
| Las skills **no** se instalan globales | `~/.agents/skills` conserva solo las del usuario (`officecli`, `microsoft-foundry`); las 4 portables se leen desde `<app>\assets\skills` |
| TeX funciona desde la carpeta instalada | con `tex\` enlazado (*junction*, sin copiar 4,4 GB): toolbar en verde «TeX Live» y `latexmk` genera `main.pdf` (111,8 KB) |
| Modelo del chat | chip del header con `opencode-go/deepseek-v4.1-flash` (tras el arreglo nº 4) |

**Visual**: `shots/m3-instalado.png`, `shots/m3-instalado-proyecto.png`, `shots/m3-instalado-dock.png`, `shots/m3-instalado-dock2.png`, `shots/m3-dock-estrecho.png`.

## 3. Fallos de primer arranque encontrados y arreglados

| # | Fallo | Arreglo |
|---|---|---|
| 1 | **Instalación limpia sin `workspace\` → el agente no arrancaba.** El chat usa `workspace\` como `cwd` de `opencode serve`; al no existir, `Command::current_dir` fallaba y el supervisor se quedaba sin agente **sin error visible** (la app parecía correcta). | `PortableRoot::ensure_layout()` — crea `workspace/` y `config/` (idempotente) — y el shell lo llama al montar (`AppRoot::new`), con aviso `root.layout` si no puede. Test nuevo en `lsx-core`. |
| 2 | El `.iss` mostraba un `MsgBox` final **también en `/VERYSILENT`**: bloquea instalaciones desatendidas y CI. | Con `WizardSilent` se escribe en el log en lugar de abrir el diálogo. |
| 3 | **La tira de pestañas del dock inferior se cortaba** en la columna del Explorador (18 % del ancho por defecto, ~280 px lógicos a 125 % de escala): `SÍMBOLOS` y los botones de la derecha quedaban fuera de la vista, **sin forma de alcanzarlos** (el usuario lo detectó como «no veo la paleta»). | Las pestañas **envuelven en una segunda fila** cuando no caben (`flex_wrap` + `min_h` en la fila): icono + nombre completo siempre visibles. Antes: `shots/m3-instalado-dock.png`; después: `shots/m3-dock-estrecho.png`. |
| 4 | El **modelo por defecto no estaba fijado** en los ajustes (`ai.model: None`): opencode resolvía el modelo de su **config global** del usuario (en la prueba, `gpt-5.6-luna`) en vez del documentado, aunque `agent/opencode.json` sí lo declare. | `DEFAULT_AI_MODEL = "opencode-go/deepseek-v4.1-flash"` como defecto de `AiSettings.model`, con `#[serde(default = "ai_model")]` para que los `settings.json` antiguos también lo tomen; test nuevo. |

## 4. Auditoría de secretos (petición del usuario)

| Comprobación | Resultado |
|---|---|
| Ficheros versionados con nombre de credenciales (`auth`, `credential`, `secret`, `.env`, `.pem`, `.key`, `.pfx`, `token`, `settings.json`, `.db`) | ✅ ninguno |
| Patrones de clave en el contenido versionado (`sk-…`, `ghp_…`, `github_pat_…`, `AKIA…`, `Bearer …`, claves PEM) | ✅ ninguna coincidencia |
| Historial completo (`git log --all -p`) | ✅ ninguna clave. Único «match»: **falso positivo** en `fixtures/opencode/events/spike-*.jsonl` — un `reasoningEncryptedContent` (blob cifrado de OpenAI en base64) contiene la secuencia `sk-` por azar; no es credencial ni es descifrable |
| Ignorados por git (verificado con `git check-ignore`) | ✅ `config/settings.json`, `agent/auth.json`, `agent/opencode.exe`, `config/opencode.runtime.json`, `tex/`, `workspace/`, `dist/`, `shots/` |

## 5. Hallazgo no bloqueante

- El agente del bundle es **opencode 1.18.30** y esta máquina tiene además **opencode v2.0.32** (el del harness), que ha migrado la base de datos compartida (`%USERPROFILE%\.local\share\opencode\opencode.db`, 1,09 GB). El server de la app arranca, **crea sesiones y chatea con normalidad** (verificado), pero el subcomando `opencode debug skill` del 1.18.30 contra esa BD responde `no such column: project_id`. Solo afecta a ese comando de diagnóstico; a un usuario sin otro opencode instalado no le ocurre. Si en M3 se quiere aislamiento total del agente (datos propios dentro de la carpeta portable) hay que decidir qué hacer con la `auth.json` del usuario.

## 6. Pendiente

1. **Validación manual del usuario**: pulsar **Compilar** en la app instalada (recompilación + recarga del visor) y las validaciones de chat de M2 (Enter, permisos, revert, skills, paleta de símbolos).
2. **Decisión**: ¿el instalador se sigue construyendo en CI (`.github/workflows/installer.yml` con `choco install innosetup`) o nos quedamos con este camino local? El workflow sigue en el repo, sin usar.
3. Limpieza opcional en este PC: `%LOCALAPPDATA%\LaTeX Studio` (instalación de prueba, se quita con `unins000.exe`) y el *junction* `tex` → `..\..\.vsc\latex-ide\tex` que creé para la prueba.
