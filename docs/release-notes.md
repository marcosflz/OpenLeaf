# LaTeX Studio 0.2.0

Instalador para Windows (por usuario, sin UAC) del IDE LaTeX nativo y portable.
Incluye `opencode` 1.18.30, las 4 skills LaTeX del producto y el modelo por defecto
`opencode-go/deepseek-v4.1-flash`.

## Lo nuevo

- **Motor LaTeX desde el primer arranque**: el instalador ofrece descargar TinyTeX-1
  (~74 MB) y deja `latexmk`, `synctex`, `biber` y los paquetes base listos. Ya no hace
  falta copiar una carpeta `tex\` a mano.
- **Perfil offline**: `/DBundledTex` sigue disponible para instalar con un TeX Live
  completo incluido.
- **Ajustes 2.0**: Apariencia, Editor, Compilación, PDF, IA y Avanzado; tema
  claro/oscuro/sistema, acento configurable, validación y restablecimiento.
- **Conexión de IA**: estado de opencode, `auth login` en consola, modelos disponibles,
  selección de modelo/agente y permisos.
- **Ventana de escritorio**: recuerda tamaño/posición/maximizado, barra de menús,
  título con proyecto y cierre seguro con documentos sin guardar.
- **Probe real del motor** y diálogo de diagnóstico desde la toolbar/status bar.
- **Tema claro coherente** en editor, PDF, chat, problemas y ajustes.

## Requisitos

- Windows 10/11 x64.
- Conexión a internet en la instalación si no tienes ya un TeX Live en `tex\`.
- Para el chat: una cuenta/proveedor configurado en opencode (OpenCode Zen/Go,
  API key, etc.). El instalador incluye `agent\opencode.exe`; las credenciales se
  piden desde Ajustes → IA.

## Verificación

- `LaTeX-Studio-Setup.exe` instala por usuario en `%LOCALAPPDATA%\LaTeX Studio`.
- Primer arranque: crea `workspace\` y `config\`; el motor queda listo si se descargó
  TinyTeX.
- Desinstalar no borra `workspace\` ni `config\settings.json`.
- SHA-256 del asset en `LaTeX-Studio-Setup.exe.sha256`.

## Limitaciones conocidas

- TinyTeX/`latexmk` no funciona en rutas con caracteres no ASCII; usa una carpeta ASCII
  o el perfil `/DBundledTex`.
- La primera compilación puede instalar paquetes adicionales con `tlmgr` si el documento
  usa algo fuera de la base incluida.
- La selección/búsqueda de texto dentro del PDF queda para una versión posterior.
