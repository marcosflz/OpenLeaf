# LaTeX Studio 0.3.1

Instalador para Windows (por usuario, sin UAC) del IDE LaTeX nativo y portable.
Perfil ligero: el motor LaTeX (TinyTeX-1) se descarga al instalar.

## Correcciones

- **Botones de la barra superior**: el engranaje (Ajustes), los toggles de paneles y la
  barra de menús no respondían en Windows. El clic se convertía en «mover ventana».
- **Menús de la cabecera del chat** (modo, permisos, modelo, historial): se abrían hacia
  arriba, encima de la barra superior. Ahora se despliegan hacia abajo.

## Lo nuevo

- **Suscripción de ChatGPT** («Sign in with ChatGPT»): se vincula desde Ajustes ›
  Suscripciones. El navegador se abre, autorizas y la app recoge la sesión sola. Se activan
  los modelos de tu plan y la sesión se renueva automáticamente.
- **Todos los modelos de Claude**: «Elegir modelos» en la suscripción de Claude lista lo que
  anuncia tu CLI `claude` (alias como `opus`, `sonnet`, `fable`, `haiku` y versiones
  concretas), sin gastar cuota.
- Asistente sobre **nas-adk 0.5.0**.

## Actualizar

- Ejecuta el nuevo instalador encima de la versión que tengas: reconoce la instalación
  anterior y la actualiza en la misma carpeta. No hace falta desinstalar antes.
- Si LaTeX Studio está abierto, el instalador lo cierra para reemplazar el programa. Guarda
  tus cambios antes.
- Tus proyectos (`workspace\`) y ajustes (`config\`) no se tocan.

## Requisitos

- Windows 10/11 x64.
- Conexión a internet en la instalación si no tienes un TeX Live en `tex\`.
- Para el chat: al menos un proveedor con clave, la suscripción de Claude con el CLI `claude`
  autenticado, o tu cuenta de ChatGPT. Se configura en Ajustes → Claves de API /
  Suscripciones.

## Verificación

- `LaTeX-Studio-Setup.exe` instala por usuario en `%LOCALAPPDATA%\LaTeX Studio`.
- Desinstalar no borra `workspace\` ni `config\settings.json`.
- SHA-256 del asset en `LaTeX-Studio-Setup.exe.sha256`.

## Limitaciones conocidas

- El inicio de sesión de ChatGPT usa el puerto local 1455. Si otra app lo ocupa, ciérrala y
  vuelve a intentarlo.
- Los clics de la barra superior y el inicio de sesión real de ChatGPT están pendientes de
  revisión manual en esta versión.
