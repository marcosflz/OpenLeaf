# LaTeX Studio 0.3.0

Instalador para Windows (por usuario, sin UAC) del IDE LaTeX nativo y portable.
Perfil ligero: el motor LaTeX (TinyTeX-1) se descarga al instalar.

## Lo nuevo

- **Asistente LaTeX propio** sobre nas-adk 0.3.0 (sustituye a opencode): modelo ↔
  herramientas, permisos con diff antes de escribir, revert por mensaje y modos
  Escribir, Reparar y Preguntar.
- **Proveedores de IA separados en dos secciones** de Ajustes:
  - **Claves de API**: OpenRouter, OpenAI, Anthropic, Gemini, DeepSeek y LM Studio (local).
  - **Suscripciones**: OpenCode Go y Claude (vía el CLI `claude`, con botón de inicio de sesión).
- **Varios proveedores activos a la vez**, con interruptor por proveedor.
- **Modelos activos por proveedor**: «Elegir modelos» consulta el catálogo real del
  proveedor, con búsqueda y alta manual por nombre. Solo los activos aparecen en el chat.
- **Modelo predeterminado** (★) que usa el chat; el chat recarga el catálogo al cambiar
  Ajustes, sin reiniciar el asistente.

## Requisitos

- Windows 10/11 x64.
- Conexión a internet en la instalación si no tienes un TeX Live en `tex\`.
- Para el chat: al menos un proveedor con clave, o la suscripción de Claude con el CLI
  `claude` autenticado. Se configura en Ajustes → Claves de API / Suscripciones.

## Verificación

- `LaTeX-Studio-Setup.exe` instala por usuario en `%LOCALAPPDATA%\LaTeX Studio`.
- Desinstalar no borra `workspace\` ni `config\settings.json`.
- SHA-256 del asset en `LaTeX-Studio-Setup.exe.sha256`.

## Limitaciones conocidas

- ChatGPT («Sign in with ChatGPT») no está incluido: nas-adk 0.3.0 no lo soporta.
- El selector de modelos y el inicio de sesión de Claude se probaron con capturas, no con
  un proveedor real en esta versión.
