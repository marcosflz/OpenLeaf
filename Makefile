# LaTeX Studio / OpenLeaf — atajos de desarrollo y release (Windows + PowerShell)
#
#   make help          lista los comandos
#   make agent         descarga agent/opencode.exe (v1.18.30) si falta
#   make build         compila la app en release
#   make dist          monta dist/latex-ide (sin tex; el instalador baja TinyTeX)
#   make installer     compila dist/LaTeX-Studio-Setup.exe
#   make release       installer + SHA-256
#   make gates         fmt + clippy + tests + OKF
#   make publish       crea la release en GitHub (requiere gh autenticado)
#
# Requisitos: Rust/MSVC, Inno Setup 6, PowerShell 7 y `make` (GnuWin32 o similar).

PWSH          ?= pwsh
CARGO         ?= cargo
AGENT_VERSION ?= v1.18.30
TAG           ?= v0.2.0
SETUP         := dist/LaTeX-Studio-Setup.exe

.PHONY: help agent build dist installer checksum release test fmt clippy kb gates clean run publish

help:
	@echo LaTeX Studio - comandos:
	@echo   make agent       descarga agent/opencode.exe $(AGENT_VERSION) si falta
	@echo   make build       compila latex-studio en release
	@echo   make dist        monta dist/latex-ide (sin tex/)
	@echo   make installer   compila $(SETUP)
	@echo   make release     installer + SHA-256
	@echo   make gates       fmt + clippy + tests + OKF
	@echo   make test        cargo test --workspace
	@echo   make run         abre la app en debug
	@echo   make clean       limpia target/ y dist/
	@echo   make publish     publica $(TAG) con gh (requiere auth)

agent:
	@$(PWSH) -NoProfile -File scripts/get-opencode.ps1 -Version $(AGENT_VERSION)

build:
	@$(CARGO) build --release -p latex-studio

dist: build agent
	@$(CARGO) run -q -p xtask -- dist --no-tex --no-zip --clean

installer: dist
	@$(PWSH) -NoProfile -File installer/build.ps1 -SkipBuild

checksum: installer
	@$(PWSH) -NoProfile -Command "$$h=(Get-FileHash '$(SETUP)' -Algorithm SHA256).Hash.ToLower(); \"$$h  LaTeX-Studio-Setup.exe\" | Set-Content '$(SETUP).sha256'; Write-Host \"SHA-256: $$h\""

release: checksum
	@$(PWSH) -NoProfile -Command "Write-Host 'Listo: $(SETUP)' -ForegroundColor Green; Get-Item '$(SETUP)' | Select-Object FullName,Length,LastWriteTime"

test:
	@$(CARGO) test --workspace

fmt:
	@$(CARGO) fmt --check

clippy:
	@$(CARGO) clippy --workspace --all-targets -- -D warnings

kb:
	@$(CARGO) run -q -p xtask -- kb check

gates: fmt clippy test kb
	@$(PWSH) -NoProfile -Command "Write-Host 'Gates verdes.' -ForegroundColor Green"

run:
	@$(CARGO) run -p latex-studio

clean:
	@$(CARGO) clean
	@$(PWSH) -NoProfile -Command "if (Test-Path dist) { Remove-Item -Recurse -Force dist }"

publish: release
	@gh release create $(TAG) "$(SETUP)" "$(SETUP).sha256" --title "LaTeX Studio $(TAG)" --notes-file docs/release-notes.md
