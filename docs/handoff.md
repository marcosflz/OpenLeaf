# Handoff — continuar LaTeX Studio (OpenLeaf) desde otro PC

> Este documento es para una sesión **nueva** de opencode que no conoce el historial de chat.
> La fuente de verdad del estado es `docs/board.md`; aquí está el contexto mínimo para
> retomar sin releer 70 commits.

## Qué es

**LaTeX Studio** es un IDE LaTeX nativo para Windows: Rust + GPUI (gpui-kit 0.7.3), visor PDF
propio (hayro), chat IA sobre **opencode portable** (`agent/opencode.exe`, 1.18.30) y
distribución portable con TeX Live reducido. El repositorio se llama **OpenLeaf**
(`https://github.com/marcosflz/OpenLeaf`), pero el producto y los binarios siguen llamándose
`latex-studio`.

## Estado al cerrar el relevo (2026-10-18)

| Hito | Estado |
|---|---|
| P0 (preflight), M0 (6 spikes), M0.5 (pulido XP), M1 (6 streams) | ✅ cerrados |
| **G1** (E2E + presupuestos) | ✅ PASS (`docs/qa/reports/G1-2026-10-17.md`) |
| Feedback del usuario sobre G1 | ✅ `docs/qa/reports/M1-C5-scroll-sync-2026-10-18.md` (foco de teclado, scroll sync `.tex`↔PDF) |
| Feedback del usuario (2.ª ronda) | ✅ `docs/qa/reports/M2-feedback-2026-10-18-chat-dock-skills.md` (Enter, permisos, razonamiento, skills, revert, dock con pestañas y paleta de símbolos) |
| **M2** (resto de TAREAS §5) | ⏳ pendiente: outline, miniaturas, tema claro, historial de chat, búsqueda en archivos, atajos configurables, métricas dev |
| **M3** (distribución) | 🟡 maquetado: `cargo xtask dist` (M3-1), `installer/` (M3-2) y `.github/workflows/installer.yml`. Falta producir y verificar el instalador |
| Gates | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` — verdes (lsx-ui 83, lsx-chat 42, resto en `docs/qa/reports/`) |

## Primero, en el PC nuevo

```powershell
git clone https://github.com/marcosflz/OpenLeaf.git
cd OpenLeaf
cargo build --release -p latex-studio      # primera vez: 15-25 min (GPUI)
cargo test --workspace                      # nextest no está instalado aquí
```

**Los activos pesados NO están en git** (a propósito):

| Activo | Tamaño | Cómo conseguirlo |
|---|---|---|
| `tex\` (TeX Live reducido) | 4,4 GB | copiar del otro PC, o `LATESTUDIO_ROOT` apuntando a un TeX Live existente |
| `agent\opencode.exe` | 171 MB | `curl -L -o opencode.zip https://github.com/sst/opencode/releases/download/v1.18.30/opencode-windows-x64.zip` y extraer como `agent\opencode.exe` (es lo que hace la Action) |
| `workspace\` (proyectos) | — | tus documentos; el repo no los lleva |
| `config\settings.json` | — | se genera en el primer arranque |

Sin `tex\` la app arranca (editor, PDF y chat funcionan) pero no compila; el instalador avisa
de ello al terminar.

## Objetivo inmediato (lo que pidió el usuario)

1. **Tener el instalador de Windows funcionando, construido en CI.**
   - Ejecutar el workflow: en GitHub, *Actions → installer → Run workflow*, o
     `git tag v0.1.0 && git push origin v0.1.0`.
   - El job descarga el agente de `OPENCODE_VERSION` (v1.18.30), compila en release, monta
     `dist\latex-ide` con `cargo xtask dist --no-tex --no-zip --clean`, instala Inno Setup 6
     por Chocolatey, compila `installer\latex-studio.iss` y publica el artifact
     `latex-studio-installer` (`Setup.exe` + `.sha256` + `SHA256SUMS.txt` + `VERSION`).
   - Si falla, arreglar el workflow (lo más probable: `-Version` o `choco install innosetup`).
   - Descargar el artifact, instalarlo en `%LOCALAPPDATA%\LaTeX Studio` y verificar el DoD.
2. **Validaciones manuales que el usuario tiene pendientes** (lista completa en
   `docs/qa/reports/M2-feedback-2026-10-18-chat-dock-skills.md` §3 y `docs/qa/e2e.md` pasos
   5b-5d y 7b-7f): `Enter`/`Shift+Enter`, preset de permisos sin modal, ↩ revert + deshacer,
   carga de una skill LaTeX, dock con pestañas y paleta de símbolos, scroll sync.
3. **Completar M3** (`docs/plan/TAREAS.md` §6): M3-3 chequeos de primer arranque + `--diagnose`,
   M3-4 `README.md` + `LICENSE` + `THIRD-PARTY-LICENSES.md` (con `cargo about` + `cargo deny`),
   M3-5 accesibilidad, M3-6 benches con criterion (`docs/perf.md`), M3-7 demo/vídeo (el borrado
   de `legacy\` **solo** con orden explícita del usuario), M3-8 bump de gpui,
   y M3-2 si se decide el sabor completo con `tex\`.

## DoD del instalador (M3-1/M3-2)

- [ ] El workflow `installer` termina en verde y publica el artifact.
- [ ] `Setup.exe` instala **por usuario** (sin UAC) en `%LOCALAPPDATA%\LaTeX Studio`.
- [ ] La app arranca desde ahí: encuentra `agent\opencode.exe` y `assets\skills` (el chat
      responde y `config\opencode.runtime.json` se genera con `skills.paths`).
- [ ] Con `tex\` copiado, compila un proyecto del usuario (botón **Compilar**) y el visor
      muestra el PDF.
- [ ] Reinstalar encima (actualización) no toca `tex\`, `workspace\` ni `config\`.
- [ ] Desinstalar deja intactos `workspace\` y `settings.json`, y se lleva `logs\`.
- [ ] Ruta con espacios y acentos funciona (`D:\prueba con espacios\`).

## Reglas duras (no negociables)

- Leer `AGENTS.md`, `docs/board.md` y las secciones que toquen de `docs/plan/INTERFACES.md`
  antes de tocar código. Cargar skills: `lsx-architecture`, `lsx-quality-gates` y la del
  stream que toque (`lsx-dist-portable` para M3, `lsx-gpui-patterns` + los oficiales
  `gpui-kit`/`gpui-kit-design-guides` para UI).
- **No tocar** los activos del usuario/distro: `tex\`, `agent\`, `workspace\`, `legacy\`.
- Gates obligatorios antes de cerrar cualquier tarea: `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- UI y mensajes en **español**; tema oscuro con acento `#4C8DFF`; look VS Code / Copilot Chat
  congelado en M0.5.
- Commits convencionales (`feat(ui): …`, `fix(chat): …`), sin `--no-verify`, sin secretos.
- Evidencia para los gates: captura DPI-aware con `scripts\capture-window.ps1` y informe en
  `docs/qa/reports/`.

## Trampas ya encontradas (no repetirlas)

- **Los módulos de test que hagan `use gpui_kit::*` no pueden usar `#[test]` del estándar**:
  con `test-support` el glob trae la macro `test` de GPUI y la sombrea. Importar explícito
  (ver `crates/ui/src/panels/*.rs`, sección `mod tests`).
- `Button` y otros componentes necesitan `use gpui_kit::component::button::*;` (no basta el glob).
- Los **listeners de teclado** (`capture_key_down`) no cortan las *acciones* del textarea: para
  "Enter envía" se usa `TextareaState::submit_on_enter(true)` + `InputEvent::PressEnter`.
- El chrome de una hoja (`PaneState::focus_if_outside`) no debe robar el foco al input.
- opencode 1.18.30 **no** descubre skills de proyecto: van por `skills.paths` en la config
  (`config\opencode.runtime.json`, generado por el supervisor).
- `tex\tlpkg` es necesario para `tlmgr`; `tex\texmf-var` es caché y se puede omitir.
- No filtrar procesos por nombre `opencode` en scripts: mata también el servidor del harness.

## Decisiones pendientes del usuario

1. **Visibilidad del repo**: hoy es **público**; se habló de privado por defecto.
2. Ejecutar la Action **ahora** (tag `v0.1.0`) o más tarde desde la web.
3. Sabor del instalador: ligero (sin TeX, recomendado) o completo con `tex\` (~2 GB).
