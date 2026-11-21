"""Proveedores LLM, su configuración por persona y el catálogo del selector de modelos.

Hay dos familias, como en Ajustes:

- ``api``: servicios que se pagan por uso con una clave (OpenRouter, OpenAI, Anthropic,
  Gemini, DeepSeek) o un servidor local OpenAI-compatible (LM Studio).
- ``subscription``: planes personales (OpenCode Go con su clave, Claude a través del CLI
  ``claude`` autenticado por la persona y ChatGPT con «Sign in with ChatGPT», ver
  ``chatgpt.py``).

Varios pueden estar activos a la vez. Cada uno guarda en ``config/secrets/ai.json``::

    {"<id>": {"apiKey": "...", "enabled": true, "baseUrl": "...",
              "models": [{"id": "...", "name": "..."}]}}

- ``apiKey``: la credencial (también vale la variable de entorno de ``env_key``).
- ``enabled``: interruptor; ``false`` lo saca del catálogo sin borrar la clave.
- ``baseUrl``: solo para proveedores con URL configurable (LM Studio).
- ``models``: los modelos **activos**, los únicos que ve el chat. Sin la clave se usan los
  sugeridos de la tabla. Una lista de cadenas es el formato antiguo («modelos extra»): se
  suman a los sugeridos.

No hay proveedor por defecto: el catálogo solo incluye los configurados. La forma del
catálogo es la de ``GET /config/providers`` de opencode (``ProviderCatalog`` en
``protocol.rs``), con ``kind`` añadido. La tabla ``PROVIDERS`` se replica en
``crates/ui/src/ai_setup.rs`` y un test de Rust comprueba que no divergen.
"""

from __future__ import annotations

import json
import os
import shutil
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from nas.adk.models import ModelRouter
from nas.adk.models.base_llm import BaseLlm

#: Presupuesto de la consulta del catálogo de un proveedor (s).
LIST_TIMEOUT = 12.0


@dataclass(frozen=True)
class ProviderSpec:
    id: str
    name: str
    #: ``api`` (pago por uso) o ``subscription`` (plan personal).
    kind: str
    #: Proveedor de nas-adk que lo sirve.
    backend: str
    #: Variable de entorno con la clave (vacía: sin clave).
    env_key: str
    #: Modelos sugeridos ``(id, nombre)``: los activos hasta que la persona elija.
    models: tuple[tuple[str, str], ...]
    help: str
    #: ``key`` (clave de API), ``cli`` (sesión del CLI ``claude``) o ``chatgpt`` (OAuth).
    link: str = "key"
    #: La clave es obligatoria.
    needs_key: bool = True
    #: URL base fija (``openai-compatible``) o la propuesta si es configurable.
    base_url: str = ""
    #: La persona puede cambiar la URL base (servidores locales).
    custom_base_url: bool = False
    #: Endpoint de listado de modelos (``GET``). Constante nuestra, nunca de la persona,
    #: salvo en los proveedores con URL configurable, donde se deriva de su URL base.
    models_url: str = ""
    #: Cómo se envía la clave al listar: ``bearer`` o ``anthropic`` (``x-api-key``).
    models_auth: str = "bearer"


PROVIDERS: tuple[ProviderSpec, ...] = (
    # --- Claves de API (pago por uso) ------------------------------------------------
    ProviderSpec(
        id="openrouter",
        name="OpenRouter",
        kind="api",
        backend="openrouter",
        env_key="OPENROUTER_API_KEY",
        models=(("openrouter/auto", "Auto (OpenRouter elige)"),),
        help="Clave de openrouter.ai; da acceso a modelos de muchos proveedores.",
        models_url="https://openrouter.ai/api/v1/models",
    ),
    ProviderSpec(
        id="openai",
        name="OpenAI",
        kind="api",
        backend="openai-compatible",
        env_key="OPENAI_API_KEY",
        models=(("gpt-5.5", "GPT-5.5"),),
        help="Clave de platform.openai.com (facturación por uso).",
        base_url="https://api.openai.com/v1",
        models_url="https://api.openai.com/v1/models",
    ),
    ProviderSpec(
        id="anthropic",
        name="Anthropic (API)",
        kind="api",
        backend="anthropic",
        env_key="ANTHROPIC_API_KEY",
        models=(
            ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
            ("claude-opus-5-5", "Claude Opus 5.5"),
            ("claude-haiku-5-5", "Claude Haiku 5.5"),
        ),
        help="Clave de la API de Anthropic (facturación por uso).",
        models_url="https://api.anthropic.com/v1/models?limit=100",
        models_auth="anthropic",
    ),
    ProviderSpec(
        id="gemini",
        name="Google Gemini",
        kind="api",
        backend="openai-compatible",
        env_key="GEMINI_API_KEY",
        models=(("gemini-3-flash", "Gemini 3 Flash"),),
        help="Clave de Google AI Studio.",
        base_url="https://generativelanguage.googleapis.com/v1beta/openai",
        models_url="https://generativelanguage.googleapis.com/v1beta/openai/models",
    ),
    ProviderSpec(
        id="deepseek",
        name="DeepSeek",
        kind="api",
        backend="openai-compatible",
        env_key="DEEPSEEK_API_KEY",
        models=(("deepseek-chat", "DeepSeek Chat"), ("deepseek-reasoner", "DeepSeek Reasoner")),
        help="Clave de platform.deepseek.com.",
        base_url="https://api.deepseek.com/v1",
        models_url="https://api.deepseek.com/models",
    ),
    ProviderSpec(
        id="lmstudio",
        name="LM Studio / OpenAI-compatible local",
        kind="api",
        backend="openai-compatible",
        env_key="LMSTUDIO_API_KEY",
        models=(("local-model", "Modelo cargado en el servidor"),),
        help="Servidor local compatible con OpenAI (LM Studio, Ollama…). La clave es opcional.",
        needs_key=False,
        base_url="http://localhost:1234/v1",
        custom_base_url=True,
    ),
    # --- Suscripciones (planes personales) ---------------------------------------------
    ProviderSpec(
        id="opencode",
        name="OpenCode Go",
        kind="subscription",
        backend="opencode",
        env_key="OPENCODE_API_KEY",
        models=(
            ("deepseek-v4.1-flash", "DeepSeek V4.1 Flash"),
            ("glm-5.3-flash", "GLM 5.3 Flash"),
            ("qwen3.8-flash", "Qwen 3.8 Flash"),
        ),
        help="Tu suscripción de OpenCode Go, con la clave de opencode.ai/console/go.",
        models_url="https://opencode.ai/zen/go/v1/models",
    ),
    ProviderSpec(
        id="claude-code",
        name="Claude (suscripción)",
        kind="subscription",
        backend="claude-code",
        env_key="",
        models=(
            ("default", "Modelo por defecto del CLI"),
            ("sonnet", "Sonnet"),
            ("opus", "Opus"),
            ("fable", "Fable"),
            ("haiku", "Haiku"),
        ),
        help=(
            "Tu plan de Claude (Pro o Max) a través del CLI `claude` ya autenticado "
            "(`claude auth login`). Uso personal y local según las condiciones de tu plan."
        ),
        link="cli",
        needs_key=False,
    ),
    ProviderSpec(
        id="chatgpt",
        name="ChatGPT",
        kind="subscription",
        backend="chatgpt",
        env_key="",
        # Sin sugeridos: al vincular se activan los modelos que ofrece el plan.
        models=(),
        help="Tu plan de ChatGPT (Plus, Pro…), con «Sign in with ChatGPT». Uso personal.",
        link="chatgpt",
        needs_key=False,
    ),
)
BY_ID = {spec.id: spec for spec in PROVIDERS}

#: Presupuesto para que el CLI `claude` anuncie sus modelos (no hace inferencia).
CLAUDE_LIST_TIMEOUT = 60.0


class Secrets:
    """Lee ``config/secrets/ai.json`` (ver el formato en el docstring del módulo)."""

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

    def entry(self, provider: str) -> dict[str, Any] | None:
        entry = self.load().get(provider)
        return entry if isinstance(entry, dict) else None

    def save_entry(self, provider: str, entry: dict[str, Any]) -> None:
        """Sustituye la entrada de un proveedor con escritura atómica (temporal + rename)."""
        if self.path is None:
            raise ValueError("no hay archivo de credenciales (--secrets)")
        data = self.load()
        data[provider] = entry
        self.path.parent.mkdir(parents=True, exist_ok=True)
        temporary = self.path.with_suffix(".json.tmp")
        temporary.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")
        os.replace(temporary, self.path)

    def api_key(self, provider: str) -> str:
        entry = self.entry(provider) or {}
        if isinstance(entry.get("apiKey"), str) and entry["apiKey"].strip():
            return entry["apiKey"].strip()
        spec = BY_ID.get(provider)
        return os.getenv(spec.env_key, "").strip() if spec and spec.env_key else ""

    def base_url(self, spec: ProviderSpec) -> str:
        entry = self.entry(spec.id) or {}
        custom = entry.get("baseUrl")
        if spec.custom_base_url and isinstance(custom, str) and custom.strip():
            return custom.strip().rstrip("/")
        return spec.base_url

    def enabled(self, provider: str) -> bool:
        entry = self.entry(provider) or {}
        return entry.get("enabled") is not False

    def active_models(self, spec: ProviderSpec) -> list[tuple[str, str]]:
        """Modelos activos ``(id, nombre)``, sin duplicados y en el orden guardado."""
        entry = self.entry(spec.id) or {}
        raw = entry.get("models")
        if not isinstance(raw, list):
            return list(spec.models)
        legacy = all(isinstance(item, str) for item in raw)
        out: list[tuple[str, str]] = list(spec.models) if legacy else []
        seen = {model_id for model_id, _ in out}
        for item in raw:
            if isinstance(item, str):
                model_id, name = item.strip(), item.strip()
            elif isinstance(item, dict) and isinstance(item.get("id"), str):
                model_id = item["id"].strip()
                label = item.get("name")
                name = label.strip() if isinstance(label, str) and label.strip() else model_id
            else:
                continue
            if model_id and model_id not in seen:
                seen.add(model_id)
                out.append((model_id, name))
        return out


def claude_cli() -> str | None:
    return shutil.which(os.getenv("CLAUDE_CODE_BIN", "claude"))


def has_credential(spec: ProviderSpec, secrets: Secrets) -> bool:
    """Hay con qué conectarse: clave, CLI vinculado o (sin clave) el proveedor añadido."""
    if spec.link == "cli":
        return claude_cli() is not None
    if spec.link == "chatgpt":
        from . import chatgpt

        return chatgpt.has_account(secrets)
    if not spec.needs_key:
        return secrets.entry(spec.id) is not None or bool(secrets.api_key(spec.id))
    return bool(secrets.api_key(spec.id))


def configured(spec: ProviderSpec, secrets: Secrets) -> bool:
    """Listo para el chat: con credencial y sin desactivar."""
    return has_credential(spec, secrets) and secrets.enabled(spec.id)


def catalog(secrets: Secrets) -> dict[str, Any]:
    providers = []
    for spec in PROVIDERS:
        if not configured(spec, secrets):
            continue
        models = {model_id: {"name": name} for model_id, name in secrets.active_models(spec)}
        if not models:
            continue
        providers.append({"id": spec.id, "name": spec.name, "kind": spec.kind, "models": models})
    # Sin `default`: el predeterminado es `ai.model` de settings.json y lo resuelve la app.
    return {"providers": providers, "default": {}}


def status(secrets: Secrets) -> list[dict[str, Any]]:
    """Estado de todos los proveedores, configurados o no (pantalla de ajustes)."""
    return [
        {
            "id": spec.id,
            "name": spec.name,
            "kind": spec.kind,
            "link": spec.link,
            "configured": configured(spec, secrets),
            "hasCredential": has_credential(spec, secrets),
            "enabled": secrets.enabled(spec.id),
            "needsKey": spec.needs_key,
            "canList": can_list(spec),
            "models": [{"id": m, "name": n} for m, n in secrets.active_models(spec)],
            "help": spec.help,
        }
        for spec in PROVIDERS
    ]


def can_list(spec: ProviderSpec) -> bool:
    return spec.link in ("cli", "chatgpt") or bool(spec.models_url) or spec.custom_base_url


def claude_models() -> list[dict[str, str]]:
    """Modelos que anuncia el CLI `claude` con la sesión de la persona.

    Usa ``ClaudeCodeLlm.list_models`` de nas-adk (protocolo de control del CLI, sin
    inferencia ni cuota): alias (``default``, ``opus``, ``sonnet``…) y versiones concretas
    de la cuenta. Con un token de ``setup-token`` añade las versiones conocidas del CLI.
    """
    import asyncio

    from nas.adk.models.claude_code.llm import ClaudeCodeLlm

    async def fetch() -> list[dict[str, str]]:
        llm = ClaudeCodeLlm(env_policy="scrub")
        models = await llm.list_models(timeout=CLAUDE_LIST_TIMEOUT)
        return [{"id": m.id, "name": m.label or m.id} for m in models if m.id]

    models = asyncio.run(fetch())
    # `default` (lo que elija el CLI) siempre disponible y primero.
    if not any(m["id"] == "default" for m in models):
        models.insert(0, {"id": "default", "name": "Modelo por defecto del CLI"})
    return models


def extract_models(payload: Any) -> list[dict[str, str]]:
    """``{data:[{id, name?}]}``, ``{models:[…]}`` o lista pelada → ``[{id, name}]`` ordenados."""
    entries: dict[str, str] = {}

    def text(record: dict[str, Any], key: str) -> str | None:
        value = record.get(key)
        return value.strip() if isinstance(value, str) and value.strip() else None

    def push(item: Any) -> None:
        if isinstance(item, str):
            if item.strip():
                entries.setdefault(item.strip(), item.strip())
            return
        if not isinstance(item, dict):
            return
        model_id = text(item, "id") or text(item, "slug") or text(item, "name")
        # Gemini devuelve `models/<id>`; el endpoint de chat espera el id sin prefijo.
        model_id = model_id.removeprefix("models/") if model_id else None
        if not model_id or model_id in entries:
            return
        name = (
            text(item, "display_name")
            or text(item, "displayName")
            or (text(item, "name") if text(item, "id") else None)
            or model_id
        )
        entries.setdefault(model_id, name.removeprefix("models/"))

    items: Any = payload
    if isinstance(payload, dict):
        items = (
            payload.get("data") if isinstance(payload.get("data"), list) else payload.get("models")
        )
    if isinstance(items, list):
        for item in items:
            push(item)
    return [{"id": k, "name": v} for k, v in sorted(entries.items())]


def list_models(provider: str, secrets: Secrets, api_key: str = "") -> dict[str, Any]:
    """Catálogo completo del proveedor para elegir los modelos activos (Ajustes).

    Devuelve ``{"ok": true, "models": [...]}`` o ``{"ok": false, "error": "..."}``. La clave
    sale de ``api_key`` (aún sin guardar) o de la guardada.
    """
    spec = BY_ID.get(provider)
    if spec is None:
        return {"ok": False, "error": f"proveedor no soportado: {provider}"}
    if spec.link == "cli":
        if claude_cli() is None:
            return {"ok": False, "error": "No se encontró el CLI `claude`: instálalo y vuelve"}
        try:
            return {"ok": True, "models": claude_models()}
        except Exception as error:  # noqa: BLE001 — se informa a la UI y se ofrecen los alias
            return {
                "ok": True,
                "models": [{"id": m, "name": n} for m, n in spec.models],
                "warning": f"El CLI no listó sus modelos ({type(error).__name__}); "
                "se muestran los alias.",
            }
    if spec.link == "chatgpt":
        import asyncio

        from . import chatgpt

        try:
            return {"ok": True, "models": asyncio.run(chatgpt.list_models(secrets))}
        except chatgpt.ChatGptError as error:
            return {"ok": False, "error": str(error)}
    url = spec.models_url
    if spec.custom_base_url:
        url = f"{secrets.base_url(spec)}/models"
    if not url:
        return {"ok": False, "error": "Este proveedor no permite listar modelos: escribe su nombre"}
    key = api_key.strip() or secrets.api_key(spec.id)
    if spec.needs_key and not key:
        return {"ok": False, "error": "Guarda primero la clave para listar los modelos"}
    headers = {"Accept": "application/json", "User-Agent": "LaTeX-Studio"}
    if key:
        if spec.models_auth == "anthropic":
            headers |= {"x-api-key": key, "anthropic-version": "2023-06-01"}
        else:
            headers["Authorization"] = f"Bearer {key}"
    request = urllib.request.Request(url, headers=headers, method="GET")
    try:
        with urllib.request.urlopen(request, timeout=LIST_TIMEOUT) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        if error.code in (401, 403):
            return {"ok": False, "error": "La clave no es válida o ha caducado"}
        return {"ok": False, "error": f"El proveedor respondió {error.code}"}
    except TimeoutError:
        return {"ok": False, "error": "El proveedor tardó demasiado en responder"}
    except (urllib.error.URLError, OSError) as error:
        return {"ok": False, "error": f"No se pudo contactar con el proveedor: {error}"[:300]}
    except (json.JSONDecodeError, UnicodeDecodeError):
        return {"ok": False, "error": "El proveedor devolvió una respuesta no válida"}
    models = extract_models(payload)
    if not models:
        return {"ok": False, "error": "El proveedor no devolvió ningún modelo"}
    return {"ok": True, "models": models}


def build_model(provider: str, model: str, secrets: Secrets) -> BaseLlm:
    spec = BY_ID.get(provider)
    if spec is None:
        raise ValueError(f"proveedor no soportado: {provider}")
    if not has_credential(spec, secrets):
        place = "Suscripciones" if spec.kind == "subscription" else "Claves de API"
        raise ValueError(f"{spec.name} no está configurado; añádelo en Ajustes › {place}")
    if not secrets.enabled(spec.id):
        raise ValueError(f"{spec.name} está desactivado en Ajustes")
    if spec.backend == "claude-code":
        # Las variables que facturarían por API se quitan solo del proceso hijo.
        os.environ.setdefault("CLAUDE_CODE_ENV_POLICY", "scrub")
        return ModelRouter(provider="claude-code", model="" if model == "default" else model)
    if spec.backend == "chatgpt":
        from . import chatgpt

        return chatgpt.build(model, secrets)
    kwargs: dict[str, Any] = {
        "provider": spec.backend,
        "model": model,
        "api_key": secrets.api_key(provider),
    }
    if spec.backend == "openai-compatible":
        kwargs["base_url"] = secrets.base_url(spec)
    if spec.backend == "openrouter":
        kwargs["app_title"] = "LaTeX Studio"
    return ModelRouter(**kwargs)
