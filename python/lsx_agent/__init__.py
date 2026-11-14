"""Agente LaTeX de LaTeX Studio.

Sidecar que la app lanza como ``python -I -m lsx_agent``. Habla NDJSON por
stdio (ver ``protocol.py``) y ejecuta un agente ``nas-adk`` especializado en
escribir, compilar y reparar proyectos LaTeX.

Los eventos que emite conservan la forma ``{"type", "properties"}`` de la API
v1 de opencode. Así el reductor de ``lsx-chat`` (tabla I-4) los consume sin
cambios. Ver ``docs/decisions/ADR-001-agente-nas-adk.md``.
"""

__version__ = "0.3.0"
