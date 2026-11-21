"""Agente LaTeX de LaTeX Studio.

Sidecar que la app lanza como ``python -I -m lsx_agent``. Habla NDJSON por
stdio (ver ``protocol.py``) y ejecuta un agente ``nas-adk`` especializado en
escribir, compilar y reparar proyectos LaTeX.

Los eventos que emite conservan la forma ``{"type", "properties"}`` de la API
v1 de opencode. Así el reductor de ``lsx-chat`` (tabla I-4) los consume sin
cambios. Ver ``docs/decisions/ADR-001-agente-nas-adk.md``.
"""

import sys
import types

__version__ = "0.3.1"


def _posix_only(name: str) -> types.ModuleType:
    """Módulo vacío: cualquier atributo falla con un mensaje claro."""

    class _Missing(types.ModuleType):
        def __getattr__(self, attr: str) -> object:
            # AttributeError (no ImportError): `hasattr` debe seguir devolviendo False.
            raise AttributeError(f"{name}.{attr} no existe en Windows (solo POSIX)")

    return _Missing(name)


def _import_nas_on_windows() -> None:
    """Importa nas-adk en Windows pese a su login solo POSIX.

    nas-adk 0.5.0 importa `fcntl` y `termios` al cargar `claude_code/login.py`
    (`SetupTokenSession`, login con pseudo-terminal) y `nas.adk.models` lo carga en
    cadena: sin esto el paquete entero no importa en Windows. El agente no usa ese
    login (en Windows vale la sesión del CLI `claude`). Los módulos falsos solo existen
    durante ese import, para no engañar a la librería estándar. Quitar cuando nas-adk
    importe esos módulos de forma perezosa.
    """
    import pathlib  # noqa: F401 — stdlib que mira `fcntl` al cargar: antes del shim
    import shutil  # noqa: F401

    stubs = {name: _posix_only(name) for name in ("fcntl", "termios") if name not in sys.modules}
    sys.modules.update(stubs)
    try:
        import nas.adk.models  # noqa: F401
    finally:
        for name, stub in stubs.items():
            if sys.modules.get(name) is stub:
                del sys.modules[name]


if sys.platform == "win32":
    _import_nas_on_windows()
