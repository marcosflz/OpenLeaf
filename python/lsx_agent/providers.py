"""Proveedores LLM disponibles y su catálogo para el selector de modelos.

No hay proveedor por defecto: el catálogo solo incluye los que están
configurados. Uno con API se considera configurado si tiene clave en
``config/secrets/ai.json`` o en el entorno; ``claude-code``, si el CLI
``claude`` está en el PATH. La forma del catálogo es la de
``GET /config/providers`` de opencode (``ProviderCatalog`` en ``protocol.rs``).
"""

from __future__ import annotations

import json
import os
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from nas.adk.models import ModelRouter


@dataclass(frozen=True)
class ProviderSpec:
    id: str
    name: str
    env_key: str
    models: tuple[tuple[str, str], ...]
    help: str


PROVIDERS: tuple[ProviderSpec, ...] = (
    ProviderSpec(
        id="opencode",
        name="OpenCode Go",
        env_key="OPENCODE_API_KEY",
        models=(
            ("deepseek-v4.1-flash", "DeepSeek V4.1 Flash"),
            ("glm-5.3-flash", "GLM 5.3 Flash"),
            ("qwen3.8-flash", "Qwen 3.8 Flash"),
        ),
        help="Clave de opencode.ai (suscripción Go).",
    ),
    ProviderSpec(
        id="openrouter",
        name="OpenRouter",
        env_key="OPENROUTER_API_KEY",
        models=(("openrouter/auto", "Auto (OpenRouter elige)"),),
        help="Clave de openrouter.ai; admite cualquier modelo con `proveedor/modelo`.",
    ),
    ProviderSpec(
        id="anthropic",
        name="Anthropic (API)",
        env_key="ANTHROPIC_API_KEY",
        models=(
            ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
            ("claude-opus-5-5", "Claude Opus 5.5"),
            ("claude-haiku-5-5", "Claude Haiku 5.5"),
        ),
        help="Clave de la API de Anthropic (facturación por uso).",
    ),
    ProviderSpec(
        id="claude-code",
        name="Claude (suscripción)",
        env_key="",
        models=(
            ("default", "Modelo por defecto del CLI"),
            ("sonnet", "Sonnet"),
            ("opus", "Opus"),
            ("haiku", "Haiku"),
        ),
        help=(
            "Usa tu CLI `claude` ya autenticado (`claude auth login`). Uso personal y local "
            "según las condiciones de tu plan."
        ),
    ),
)
BY_ID = {spec.id: spec for spec in PROVIDERS}


class Secrets:
    """Lee ``config/secrets/ai.json``: ``{"opencode": {"apiKey": "..."}, ...}``."""

    def __init__(self, path: Path | None) -> None:
        self.path = path

    def load(self) -> dict[str, Any]:
        if self.path is None or not self.path.is_file():
            return {}
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return {}
        return data if isinstance(data, dict) else {}

    def api_key(self, provider: str) -> str:
        entry = self.load().get(provider)
        if isinstance(entry, dict) and isinstance(entry.get("apiKey"), str):
            return entry["apiKey"].strip()
        spec = BY_ID.get(provider)
        return os.getenv(spec.env_key, "").strip() if spec and spec.env_key else ""

    def extra_models(self, provider: str) -> list[str]:
        entry = self.load().get(provider)
        models = entry.get("models") if isinstance(entry, dict) else None
        return [m for m in models if isinstance(m, str) and m] if isinstance(models, list) else []


def claude_cli() -> str | None:
    return shutil.which(os.getenv("CLAUDE_CODE_BIN", "claude"))


def configured(spec: ProviderSpec, secrets: Secrets) -> bool:
    if spec.id == "claude-code":
        return claude_cli() is not None
    return bool(secrets.api_key(spec.id))


def catalog(secrets: Secrets) -> dict[str, Any]:
    providers = []
    for spec in PROVIDERS:
        if not configured(spec, secrets):
            continue
        models = {model_id: {"name": name} for model_id, name in spec.models}
        for extra in secrets.extra_models(spec.id):
            models.setdefault(extra, {"name": extra})
        providers.append({"id": spec.id, "name": spec.name, "models": models})
    # Sin `default`: si hay varios proveedores, elige la persona.
    return {"providers": providers, "default": {}}


def status(secrets: Secrets) -> list[dict[str, Any]]:
    """Estado de todos los proveedores, configurados o no (pantalla de ajustes)."""
    return [
        {
            "id": spec.id,
            "name": spec.name,
            "configured": configured(spec, secrets),
            "needsKey": bool(spec.env_key),
            "help": spec.help,
        }
        for spec in PROVIDERS
    ]


def build_model(provider: str, model: str, secrets: Secrets) -> ModelRouter:
    spec = BY_ID.get(provider)
    if spec is None:
        raise ValueError(f"proveedor no soportado: {provider}")
    if not configured(spec, secrets):
        raise ValueError(f"{spec.name} no está configurado; añade la clave en Ajustes › IA")
    if provider == "claude-code":
        # Las variables que facturarían por API se quitan solo del proceso hijo.
        os.environ.setdefault("CLAUDE_CODE_ENV_POLICY", "scrub")
        return ModelRouter(provider="claude-code", model="" if model == "default" else model)
    kwargs: dict[str, Any] = {
        "provider": provider,
        "model": model,
        "api_key": secrets.api_key(provider),
    }
    if provider == "openrouter":
        kwargs["app_title"] = "LaTeX Studio"
    return ModelRouter(**kwargs)
