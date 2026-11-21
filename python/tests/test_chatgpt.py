"""Suscripción de ChatGPT: credenciales, refresco con rotación, cierre de sesión y catálogo."""

from __future__ import annotations

import asyncio
import json
import time
from pathlib import Path

import pytest
from nas.adk.models.chatgpt import ChatGptCredentials
from nas.adk.models.errors import LlmAuthError

from lsx_agent import chatgpt, providers
from lsx_agent.providers import Secrets, build_model, catalog, list_models


def secrets_with(tmp_path: Path, entry: dict) -> Secrets:
    path = tmp_path / "ai.json"
    path.write_text(json.dumps({"chatgpt": entry}), encoding="utf-8")
    return Secrets(path)


def creds_json(expires_in: float = 3600, refresh: str = "rt-1") -> dict:
    return {
        "clientId": "client-1",
        "accessToken": "at-1",
        "refreshToken": refresh,
        "expiresAt": time.time() + expires_in,
        "account": "persona@example.com",
    }


def test_chatgpt_is_a_subscription_without_suggested_models() -> None:
    spec = providers.BY_ID["chatgpt"]
    assert spec.kind == "subscription" and spec.link == "chatgpt" and spec.models == ()


def test_without_account_it_is_not_offered(tmp_path: Path) -> None:
    secrets = secrets_with(tmp_path, {"models": [{"id": "gpt-5.5", "name": "GPT-5.5"}]})
    assert not chatgpt.has_account(secrets)
    assert all(p["id"] != "chatgpt" for p in catalog(secrets)["providers"])
    with pytest.raises(ValueError, match="Suscripciones"):
        build_model("chatgpt", "gpt-5.5", secrets)


def test_linked_account_with_models_appears_in_the_catalog(tmp_path: Path) -> None:
    secrets = secrets_with(
        tmp_path,
        {"credentials": creds_json(), "models": [{"id": "gpt-5.5", "name": "GPT-5.5"}]},
    )
    assert chatgpt.account(secrets) == "persona@example.com"
    (entry,) = [p for p in catalog(secrets)["providers"] if p["id"] == "chatgpt"]
    assert entry["kind"] == "subscription"
    assert entry["models"] == {"gpt-5.5": {"name": "GPT-5.5"}}
    model = build_model("chatgpt", "gpt-5.5", secrets)
    assert isinstance(model, chatgpt.RefreshingChatGptLlm)


def test_an_expiring_token_is_refreshed_and_the_rotated_one_saved(
    tmp_path: Path, monkeypatch
) -> None:
    secrets = secrets_with(tmp_path, {"credentials": creds_json(expires_in=10), "hostId": "h"})

    async def fake_refresh(creds: ChatGptCredentials, **_: object) -> ChatGptCredentials:
        assert creds.refresh_token == "rt-1"
        return ChatGptCredentials(
            client_id=creds.client_id,
            access_token="at-2",
            refresh_token="rt-2",
            expires_at=time.time() + 3600,
            account=creds.account,
        )

    monkeypatch.setattr(chatgpt.oauth, "refresh", fake_refresh)
    renewed = asyncio.run(chatgpt.fresh_credentials(secrets))
    assert renewed.access_token == "at-2"
    saved = secrets.entry("chatgpt")
    assert saved["credentials"]["refreshToken"] == "rt-2"
    assert saved["hostId"] == "h", "refrescar no debe perder el resto de la entrada"


def test_a_rejected_refresh_asks_to_link_again(tmp_path: Path, monkeypatch) -> None:
    secrets = secrets_with(tmp_path, {"credentials": creds_json(expires_in=10)})

    async def rejected(*_: object, **__: object) -> ChatGptCredentials:
        raise LlmAuthError("invalid_grant")

    monkeypatch.setattr(chatgpt.oauth, "refresh", rejected)
    with pytest.raises(chatgpt.ChatGptError, match="vuelve a vincular"):
        asyncio.run(chatgpt.fresh_credentials(secrets))


def test_logout_removes_credentials_but_keeps_the_client(tmp_path: Path, monkeypatch) -> None:
    secrets = secrets_with(
        tmp_path, {"credentials": creds_json(), "clientId": "client-1", "hostId": "h"}
    )

    async def no_revoke(*_: object, **__: object) -> None:
        return None

    monkeypatch.setattr(chatgpt.oauth, "revoke", no_revoke)
    assert chatgpt.logout(secrets) == {"ok": True}
    entry = secrets.entry("chatgpt")
    assert "credentials" not in entry
    assert entry["clientId"] == "client-1" and entry["hostId"] == "h"


def test_list_models_uses_the_plan_catalog(tmp_path: Path, monkeypatch) -> None:
    secrets = secrets_with(tmp_path, {"credentials": creds_json()})

    async def fake_list(token: str, **_: object) -> list[dict[str, str]]:
        assert token == "at-1"
        return [{"id": "gpt-5.5", "label": "GPT-5.5"}, {"id": "gpt-5.5-mini", "label": ""}]

    monkeypatch.setattr(chatgpt.oauth, "list_models", fake_list)
    result = list_models("chatgpt", secrets)
    assert result == {
        "ok": True,
        "models": [
            {"id": "gpt-5.5", "name": "GPT-5.5"},
            {"id": "gpt-5.5-mini", "name": "gpt-5.5-mini"},
        ],
    }


def test_login_captures_the_loopback_redirect(tmp_path: Path, monkeypatch) -> None:
    secrets = secrets_with(tmp_path, {})
    seen: dict = {}

    def fake_wait(timeout: float) -> str:
        return "http://127.0.0.1:1455/auth/callback?code=c&state=s"

    async def fake_exchange(redirect: str, pending, **_: object) -> ChatGptCredentials:
        seen["redirect"] = redirect
        seen["client"] = pending.client_id
        return ChatGptCredentials(
            client_id="issued-client",
            access_token="at",
            refresh_token="rt",
            expires_at=time.time() + 3600,
            account="persona@example.com",
        )

    async def fake_list(token: str, **_: object) -> list[dict[str, str]]:
        return [{"id": "gpt-5.5", "label": "GPT-5.5"}]

    monkeypatch.setattr(chatgpt, "_wait_for_callback", fake_wait)
    monkeypatch.setattr(chatgpt.oauth, "exchange_code", fake_exchange)
    monkeypatch.setattr(chatgpt.oauth, "list_models", fake_list)
    result = chatgpt.login(secrets, open_browser=False)
    assert result == {"ok": True, "account": "persona@example.com"}
    assert seen["redirect"].startswith("http://127.0.0.1:1455/auth/callback")
    entry = secrets.entry("chatgpt")
    assert entry["clientId"] == "issued-client"
    assert entry["hostId"].startswith("urn:uuid:")
    # Primera vinculación: quedan activos los modelos del plan.
    assert entry["models"] == [{"id": "gpt-5.5", "name": "GPT-5.5"}]


def test_claude_listing_falls_back_to_aliases_if_the_cli_fails(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setattr(providers, "claude_cli", lambda: "C:/bin/claude.exe")

    def broken() -> list[dict[str, str]]:
        raise RuntimeError("sin red")

    monkeypatch.setattr(providers, "claude_models", broken)
    result = list_models("claude-code", Secrets(tmp_path / "ai.json"))
    assert result["ok"] is True and "warning" in result
    assert [m["id"] for m in result["models"]][:2] == ["default", "sonnet"]
