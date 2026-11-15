"""Configuración de proveedores: familias, activación, modelos activos y catálogo."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from lsx_agent import providers
from lsx_agent.providers import (
    PROVIDERS,
    Secrets,
    build_model,
    catalog,
    extract_models,
    list_models,
    status,
)


@pytest.fixture(autouse=True)
def clean_env(monkeypatch) -> None:
    for spec in PROVIDERS:
        if spec.env_key:
            monkeypatch.delenv(spec.env_key, raising=False)
    monkeypatch.setenv("CLAUDE_CODE_BIN", "claude-que-no-existe")


def write(tmp_path: Path, data: dict) -> Secrets:
    path = tmp_path / "ai.json"
    path.write_text(json.dumps(data), encoding="utf-8")
    return Secrets(path)


def test_every_provider_belongs_to_one_family() -> None:
    kinds = {spec.id: spec.kind for spec in PROVIDERS}
    assert set(kinds.values()) == {"api", "subscription"}
    assert kinds["opencode"] == kinds["claude-code"] == "subscription"
    assert kinds["openrouter"] == kinds["anthropic"] == "api"
    assert len(kinds) == len(PROVIDERS), "ids repetidos"


def test_several_providers_are_active_at_once(tmp_path: Path) -> None:
    secrets = write(
        tmp_path,
        {"openrouter": {"apiKey": "sk-or-1"}, "opencode": {"apiKey": "oc-1"}},
    )
    data = catalog(secrets)
    assert [p["id"] for p in data["providers"]] == ["openrouter", "opencode"]
    assert {p["kind"] for p in data["providers"]} == {"api", "subscription"}


def test_disabled_provider_keeps_its_key_but_leaves_the_catalog(tmp_path: Path) -> None:
    secrets = write(tmp_path, {"openrouter": {"apiKey": "sk-or-1", "enabled": False}})
    assert catalog(secrets)["providers"] == []
    entry = next(s for s in status(secrets) if s["id"] == "openrouter")
    assert entry["hasCredential"] is True and entry["enabled"] is False
    with pytest.raises(ValueError, match="desactivado"):
        build_model("openrouter", "openrouter/auto", secrets)


def test_active_models_replace_the_suggested_ones(tmp_path: Path) -> None:
    secrets = write(
        tmp_path,
        {
            "anthropic": {
                "apiKey": "sk-ant-1",
                "models": [
                    {"id": "claude-opus-5-5", "name": "Opus"},
                    {"id": "claude-opus-5-5", "name": "duplicado"},
                    {"id": "  "},
                ],
            }
        },
    )
    (provider,) = catalog(secrets)["providers"]
    assert provider["models"] == {"claude-opus-5-5": {"name": "Opus"}}


def test_legacy_string_models_are_added_to_the_suggested(tmp_path: Path) -> None:
    secrets = write(tmp_path, {"openrouter": {"apiKey": "k", "models": ["x/y"]}})
    (provider,) = catalog(secrets)["providers"]
    assert list(provider["models"]) == ["openrouter/auto", "x/y"]


def test_a_provider_without_active_models_is_not_offered(tmp_path: Path) -> None:
    secrets = write(tmp_path, {"openrouter": {"apiKey": "k", "models": [{"id": ""}]}})
    assert catalog(secrets)["providers"] == []


def test_keyless_local_provider_must_be_added_and_uses_its_base_url(tmp_path: Path) -> None:
    assert catalog(write(tmp_path, {}))["providers"] == []
    secrets = write(tmp_path, {"lmstudio": {"baseUrl": "http://127.0.0.1:9999/v1/"}})
    assert [p["id"] for p in catalog(secrets)["providers"]] == ["lmstudio"]
    model = build_model("lmstudio", "qwen", secrets)
    assert model.provider == "openai-compatible"
    assert model.base_url == "http://127.0.0.1:9999/v1"


def test_openai_compatible_services_use_their_fixed_endpoint(tmp_path: Path) -> None:
    # Una baseUrl guardada no redirige la clave de un servicio con URL fija.
    secrets = write(tmp_path, {"deepseek": {"apiKey": "sk-1", "baseUrl": "http://evil.test"}})
    model = build_model("deepseek", "deepseek-chat", secrets)
    assert model.base_url == "https://api.deepseek.com/v1"


def test_claude_code_depends_on_the_cli(tmp_path: Path, monkeypatch) -> None:
    secrets = write(tmp_path, {})
    assert catalog(secrets)["providers"] == []
    monkeypatch.setattr(providers, "claude_cli", lambda: "C:/bin/claude.exe")
    assert [p["id"] for p in catalog(secrets)["providers"]] == ["claude-code"]
    result = list_models("claude-code", secrets)
    assert result["ok"] and result["models"][0]["id"] == "default"


def test_missing_provider_explains_where_to_configure_it(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="Suscripciones"):
        build_model("opencode", "glm-5.3-flash", write(tmp_path, {}))
    with pytest.raises(ValueError, match="Claves de API"):
        build_model("openai", "gpt-5.5", write(tmp_path, {}))


def test_extract_models_accepts_the_usual_shapes() -> None:
    assert extract_models({"data": [{"id": "b"}, {"id": "a", "name": "A bonito"}]}) == [
        {"id": "a", "name": "A bonito"},
        {"id": "b", "name": "b"},
    ]
    assert extract_models({"models": [{"name": "models/gemini-3-flash", "displayName": "G3"}]}) == [
        {"id": "gemini-3-flash", "name": "G3"}
    ]
    assert extract_models(["x", "x", " "]) == [{"id": "x", "name": "x"}]
    assert extract_models({"nada": 1}) == []


def test_list_models_needs_a_key_and_sends_it(tmp_path: Path, monkeypatch) -> None:
    secrets = write(tmp_path, {})
    assert list_models("openrouter", secrets)["ok"] is False
    assert list_models("nope", secrets)["ok"] is False

    seen: dict = {}

    class Reply:
        def __enter__(self):
            return self

        def __exit__(self, *_):
            return False

        def read(self) -> bytes:
            return json.dumps({"data": [{"id": "claude-opus-5-5"}]}).encode()

    def fake_urlopen(request, timeout):
        seen["url"] = request.full_url
        seen["headers"] = dict(request.header_items())
        return Reply()

    monkeypatch.setattr(providers.urllib.request, "urlopen", fake_urlopen)
    result = list_models("anthropic", write(tmp_path, {"anthropic": {"apiKey": "sk-ant-9"}}))
    assert result == {"ok": True, "models": [{"id": "claude-opus-5-5", "name": "claude-opus-5-5"}]}
    assert seen["url"].startswith("https://api.anthropic.com/")
    assert seen["headers"]["X-api-key"] == "sk-ant-9"
