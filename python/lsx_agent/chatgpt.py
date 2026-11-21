"""Plan de ChatGPT de la persona («Sign in with ChatGPT»), sobre nas-adk.

Igual que en nexus, se usa el OAuth oficial con PKCE de nas-adk
(``nas.adk.models.chatgpt.oauth``). La diferencia es que aquí la app y el navegador están
en el mismo equipo: en vez de pedir que se pegue la dirección de vuelta, el agente escucha
en ``127.0.0.1:1455`` (la redirección fija de OpenAI), recoge el código y lo canjea.

Las credenciales viven en ``config/secrets/ai.json``, en la entrada ``chatgpt``::

    {"chatgpt": {"credentials": {"clientId", "accessToken", "refreshToken",
                                 "expiresAt", "account"},
                 "clientId": "...", "hostId": "urn:uuid:...", "enabled": true,
                 "models": [{"id", "name"}]}}

El token de refresco **rota** en cada uso: cada refresco guarda el nuevo enseguida.
"""

from __future__ import annotations

import asyncio
import logging
import threading
import webbrowser
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import TYPE_CHECKING, Any
from urllib.parse import urlsplit

from nas.adk.models.base_llm import LlmRequest, LlmResponse
from nas.adk.models.chatgpt import ChatGptCredentials, ChatGptPlanLlm, oauth
from nas.adk.models.errors import LlmAuthError, LlmError, LlmPermissionError
from pydantic import PrivateAttr

if TYPE_CHECKING:
    from collections.abc import AsyncGenerator

    from .providers import Secrets

log = logging.getLogger("lsx_agent")

PROVIDER = "chatgpt"
APP_NAME = "LaTeX Studio"
#: Margen antes de la caducidad con el que se refresca el token (s).
REFRESH_MARGIN = 300.0
#: Tiempo máximo para completar el inicio de sesión en el navegador (s).
LOGIN_TIMEOUT = 300.0
CALLBACK_HOST = "127.0.0.1"
CALLBACK_PORT = 1455

_DONE_PAGE = (
    "<!doctype html><meta charset=utf-8><title>LaTeX Studio</title>"
    "<body style='font-family:system-ui;padding:3rem'>"
    "<h2>{title}</h2><p>{body}</p></body>"
)


class ChatGptError(Exception):
    """Error mostrable en Ajustes (ya traducido)."""


# --- credenciales ------------------------------------------------------------------------


def credentials(secrets: Secrets) -> ChatGptCredentials | None:
    entry = secrets.entry(PROVIDER) or {}
    raw = entry.get("credentials")
    if not isinstance(raw, dict):
        return None
    try:
        return ChatGptCredentials(
            client_id=str(raw["clientId"]),
            access_token=str(raw["accessToken"]),
            refresh_token=str(raw["refreshToken"]),
            expires_at=float(raw["expiresAt"]),
            account=raw.get("account"),
        )
    except (KeyError, TypeError, ValueError):
        return None


def has_account(secrets: Secrets) -> bool:
    return credentials(secrets) is not None


def account(secrets: Secrets) -> str | None:
    creds = credentials(secrets)
    return creds.account if creds else None


def _store(secrets: Secrets, creds: ChatGptCredentials) -> None:
    entry = dict(secrets.entry(PROVIDER) or {})
    entry["clientId"] = creds.client_id
    entry["credentials"] = {
        "clientId": creds.client_id,
        "accessToken": creds.access_token,
        "refreshToken": creds.refresh_token,
        "expiresAt": creds.expires_at,
        "account": creds.account,
    }
    secrets.save_entry(PROVIDER, entry)


async def fresh_credentials(secrets: Secrets) -> ChatGptCredentials:
    """Credenciales con un token de acceso vigente (refresca y guarda si hace falta)."""
    creds = credentials(secrets)
    if creds is None:
        raise ChatGptError("Vincula tu cuenta de ChatGPT en Ajustes › Suscripciones.")
    if not creds.expires_within(REFRESH_MARGIN):
        return creds
    try:
        renewed = await oauth.refresh(creds)
    except LlmAuthError as error:
        raise ChatGptError(
            "La sesión de ChatGPT ha caducado: vuelve a vincular la cuenta en Ajustes."
        ) from error
    except LlmError as error:
        raise ChatGptError("No se pudo contactar con OpenAI para renovar la sesión.") from error
    _store(secrets, renewed)
    return renewed


# --- modelo ------------------------------------------------------------------------------


class RefreshingChatGptLlm(ChatGptPlanLlm):
    """``ChatGptPlanLlm`` que renueva el token antes de cada petición y guarda el rotado."""

    _secrets: Any = PrivateAttr(default=None)

    def bind(self, secrets: Secrets) -> RefreshingChatGptLlm:
        self._secrets = secrets
        return self

    async def _ensure_token(self) -> None:
        if self._secrets is None:
            return
        try:
            creds = await fresh_credentials(self._secrets)
        except ChatGptError as error:
            raise LlmAuthError(str(error)) from error
        self.api_key = creds.access_token

    async def generate_content_async(self, request: LlmRequest) -> LlmResponse:
        await self._ensure_token()
        return await super().generate_content_async(request)

    async def generate_stream_async(self, request: LlmRequest) -> AsyncGenerator[LlmResponse, None]:
        await self._ensure_token()
        async for chunk in super().generate_stream_async(request):
            yield chunk


def build(model: str, secrets: Secrets) -> RefreshingChatGptLlm:
    creds = credentials(secrets)
    if creds is None:
        raise ValueError("Vincula tu cuenta de ChatGPT en Ajustes › Suscripciones.")
    return RefreshingChatGptLlm(model=model, access_token=creds.access_token).bind(secrets)


# --- catálogo ----------------------------------------------------------------------------


async def list_models(secrets: Secrets) -> list[dict[str, str]]:
    creds = await fresh_credentials(secrets)
    try:
        models = await oauth.list_models(creds.access_token)
    except LlmAuthError as error:
        raise ChatGptError("OpenAI rechazó la sesión: vuelve a vincular la cuenta.") from error
    except LlmError as error:
        raise ChatGptError("No se pudieron leer los modelos de tu plan de ChatGPT.") from error
    return [
        {"id": m["id"], "name": m.get("label") or m["id"]}
        for m in models
        if isinstance(m, dict) and m.get("id")
    ]


# --- inicio y cierre de sesión -----------------------------------------------------------


def _wait_for_callback(timeout: float) -> str:
    """Sirve ``/auth/callback`` en el loopback y devuelve la dirección completa recibida."""
    received: dict[str, str] = {}
    done = threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # nombre fijado por http.server
            path = urlsplit(self.path).path
            if path != "/auth/callback":
                self.send_response(404)
                self.end_headers()
                return
            received["url"] = f"http://{CALLBACK_HOST}:{CALLBACK_PORT}{self.path}"
            page = _DONE_PAGE.format(
                title="Listo",
                body="Ya puedes cerrar esta pestaña y volver a LaTeX Studio.",
            )
            body = page.encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            done.set()

        def log_message(self, *_: object) -> None:  # silencio: stdout es el canal JSON
            return

    try:
        server = HTTPServer((CALLBACK_HOST, CALLBACK_PORT), Handler)
    except OSError as error:
        raise ChatGptError(
            f"El puerto {CALLBACK_PORT} está ocupado (¿otra app iniciando sesión con OpenAI?). "
            "Ciérrala y vuelve a intentarlo."
        ) from error
    server.timeout = 0.5
    thread = threading.Thread(target=lambda: _serve_until(server, done, timeout), daemon=True)
    thread.start()
    finished = done.wait(timeout)
    thread.join(2)
    server.server_close()
    if not finished or "url" not in received:
        raise ChatGptError("No se completó el inicio de sesión a tiempo. Vuelve a intentarlo.")
    return received["url"]


def _serve_until(server: HTTPServer, done: threading.Event, timeout: float) -> None:
    import time

    deadline = time.monotonic() + timeout
    while not done.is_set() and time.monotonic() < deadline:
        server.handle_request()


def login(secrets: Secrets, *, open_browser: bool = True) -> dict[str, Any]:
    """Inicio de sesión completo. Bloquea hasta que la persona autoriza (o caduca)."""
    entry = dict(secrets.entry(PROVIDER) or {})
    host_id = entry.get("hostId") if isinstance(entry.get("hostId"), str) else None
    if not host_id:
        host_id = oauth.new_host_id()
        entry["hostId"] = host_id
        secrets.save_entry(PROVIDER, entry)
    client_id = entry.get("clientId") if isinstance(entry.get("clientId"), str) else None
    pending = oauth.start_login(host_id=host_id, app_name=APP_NAME, client_id=client_id)
    if open_browser:
        webbrowser.open(pending.url)
    redirect = _wait_for_callback(LOGIN_TIMEOUT)
    try:
        creds = asyncio.run(oauth.exchange_code(redirect, pending))
    except LlmPermissionError as error:
        raise ChatGptError(
            "No se activó el uso del plan de ChatGPT para LaTeX Studio, o tu cuenta no puede usarlo."
        ) from error
    except LlmAuthError as error:
        raise ChatGptError(
            "OpenAI no validó el inicio de sesión. Vuelve a intentarlo desde Ajustes."
        ) from error
    except LlmError as error:
        raise ChatGptError("No se pudo contactar con OpenAI. Inténtalo de nuevo.") from error
    _store(secrets, creds)
    # Primera vinculación: se activan los modelos del plan (luego se eligen en Ajustes).
    entry = dict(secrets.entry(PROVIDER) or {})
    if not entry.get("models"):
        try:
            models = asyncio.run(list_models(secrets))
        except ChatGptError:
            models = []
        if models:
            entry["models"] = models
            secrets.save_entry(PROVIDER, entry)
    return {"ok": True, "account": creds.account}


def logout(secrets: Secrets) -> dict[str, Any]:
    """Revoca (mejor esfuerzo) y borra las credenciales; conserva ``clientId`` y ``hostId``."""
    creds = credentials(secrets)
    if creds is not None:
        try:
            asyncio.run(oauth.revoke(creds))
        except Exception as error:  # noqa: BLE001 — revocar es opcional
            log.info("no se pudo revocar la sesión de ChatGPT (%s)", type(error).__name__)
    entry = dict(secrets.entry(PROVIDER) or {})
    entry.pop("credentials", None)
    secrets.save_entry(PROVIDER, entry)
    return {"ok": True}
