---
name: lsx-dist-portable
description: Usar cuando se trabaje en distribución portable (M3, docs agent) — tarea cargo xtask dist, layout de dist/latex-ide y de los zips full/app, checksums, checks de primer arranque (tex/, agent/opencode.exe), adaptación del instalador Inno desde legacy/installer y pruebas en rutas con espacios y acentos. Solo aplica a partir de M3; hasta entonces, referencia de lectura.
---

# Distribución portable (M3)

Fuente: `docs/plan/PLAN-MAESTRO.md` §9; `docs/plan/TAREAS.md` M3-1..M3-7. Aplica a partir de M3; antes, solo consulta.

## `xtask dist` (M3-1)

- Hoy: stub (`cargo run -p xtask -- dist` imprime `TODO(M3)`). La implementación la hace el orchestrator (dueño de `xtask/`); el docs agent valida el resultado.
- Salida `dist\latex-ide\`:

```
latex-studio.exe
assets\            config\ (defaults)     README.txt     LICENSES\
tex\               ← copiada o enlazada desde la raíz (variante full)
agent\             ← opencode.exe + opencode.json
workspace\         ← vacío con una plantilla de ejemplo
```

- Zips: `latex-studio-<ver>-portable-full.zip` (~1.5–2 GB con `tex\`) y `...-app.zip` (~25 MB, exe + assets).
- `TODO(M3): verificar` el formato del checksum (p. ej. SHA-256 con `Get-FileHash`) y dónde se publica; no inventar nombres de archivo más allá del patrón del plan.

## Primer arranque (M3-3)

- Chequeos: ¿existe `tex\`?, ¿`agent\opencode.exe`?, ¿`config\` escribible? Si falta `tex\` → aviso claro con enlace a la distro; si falta `opencode.exe` → chat deshabilitado con explicación, el resto funciona (PLAN §9).
- `--diagnose` CLI: vuelca informe (versiones, rutas, estado del server, últimas 200 líneas de log) sin secretos — requiere que `app/` lo exponga; `TODO(M3): verificar` el contrato con el orchestrator.
- Sin registro, sin admin, sin DLLs externas; SmartScreen puede avisar la primera vez (sin firma; queda en backlog).

## Instalador Inno (M3-2)

- Adoptar COPIANDO `legacy\installer\latex-ide.iss` y adaptarlo (paths y exe nuevos); nunca editar `legacy\` in situ (activo de solo lectura).
- Instalación por usuario, sin admin; no romper si `tex\` ya existe en la máquina; desinstalación limpia.

## Validación (gate G3)

- Instalación limpia desde zip en carpeta nueva; E2E completo (`docs/qa/e2e.md`) incluido el paso 12 (ruta con espacios/acentos tipo `D:\prueba con espacios\`).
- Sin red: arranque, compilación (paquetes ya presentes) y chat local funcionan.
- Licencias (M3-4): `README.md` + `LICENSE` + `THIRD-PARTY-LICENSES` generado con `cargo about`; `cargo deny` verde. Recordatorios: texlab (M4) solo como binario separado GPL; `tex\` se redistribuye tal cual.
- Demo/video (M3-7) y limpieza de `legacy\`: SOLO con decisión explícita del usuario.

## Enlaces

- E2E: `docs/qa/e2e.md` · presupuestos: `docs/plan/PLAN-MAESTRO.md` §6.3.
- Criterios de aceptación exactos: `docs/plan/TAREAS.md` §6.
