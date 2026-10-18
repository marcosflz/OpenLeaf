//! Cliente REST + stream SSE contra `opencode serve` (API v1).
//!
//! Sin password (spike) o con `OPENCODE_SERVER_PASSWORD` → basic auth
//! `opencode:{password}` (INTERFACES §5.4).

use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{ChatError, Result};
use crate::protocol::{
    Health, ModelRef, PermissionReply, ProviderCatalog, SessionInfo, prompt_body_with,
};
use crate::sse::{SseEvent, SseParser};

/// Cliente HTTP del server opencode.
#[derive(Debug, Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    base_url: String,
    password: Option<String>,
}

impl ApiClient {
    pub fn new(base_url: impl Into<String>, password: Option<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("lsx-chat-spike/0.1")
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            password,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let request = self
            .http
            .request(method, format!("{}{}", self.base_url, path));
        match &self.password {
            Some(password) => request.basic_auth("opencode", Some(password)),
            None => request,
        }
    }

    /// `GET /global/health`.
    pub async fn health(&self) -> Result<Health> {
        let response = self
            .request(reqwest::Method::GET, "/global/health")
            .timeout(Duration::from_secs(3))
            .send()
            .await?;
        ensure_success(response, "/global/health")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `GET /config/providers` — catálogo de proveedores/modelos.
    pub async fn providers(&self) -> Result<ProviderCatalog> {
        let response = self
            .request(reqwest::Method::GET, "/config/providers")
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        ensure_success(response, "/config/providers")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `GET /agent` — agentes disponibles (crudo; se parsea con
    /// [`crate::protocol::parse_agents`]).
    pub async fn agents(&self) -> Result<Value> {
        let response = self
            .request(reqwest::Method::GET, "/agent")
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        ensure_success(response, "/agent")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `GET /session` — todas las sesiones del server (se filtran por
    /// `directory` en el servicio).
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let response = self
            .request(reqwest::Method::GET, "/session")
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        ensure_success(response, "/session")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `GET /session/:id`.
    pub async fn session(&self, session_id: &str) -> Result<SessionInfo> {
        let response = self
            .request(reqwest::Method::GET, &format!("/session/{session_id}"))
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        ensure_success(response, "GET /session/:id")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `DELETE /session/:id`.
    pub async fn delete_session(&self, session_id: &str) -> Result<bool> {
        let response = self
            .request(reqwest::Method::DELETE, &format!("/session/{session_id}"))
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        ensure_success(response, "DELETE /session")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `POST /session` — crea una sesión (opcionalmente scoped a `directory`,
    /// verificado en M0-S4: un mismo server puede servir varios proyectos).
    pub async fn create_session(
        &self,
        title: &str,
        directory: Option<&Path>,
    ) -> Result<SessionInfo> {
        let mut request = self
            .request(reqwest::Method::POST, "/session")
            .timeout(Duration::from_secs(30));
        if let Some(directory) = directory {
            request = request.query(&[("directory", directory.to_string_lossy())]);
        }
        let response = request.json(&json!({ "title": title })).send().await?;
        ensure_success(response, "POST /session")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `POST /session/:id/prompt_async` — 204; el resultado llega por SSE.
    pub async fn prompt_async(
        &self,
        session_id: &str,
        model: Option<&ModelRef>,
        text: &str,
    ) -> Result<()> {
        self.prompt_async_with(session_id, model, None, text).await
    }

    /// Igual que [`Self::prompt_async`] pero con agente (`build`, `latex-fix`…).
    pub async fn prompt_async_with(
        &self,
        session_id: &str,
        model: Option<&ModelRef>,
        agent: Option<&str>,
        text: &str,
    ) -> Result<()> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/session/{session_id}/prompt_async"),
            )
            .timeout(Duration::from_secs(30))
            .json(&prompt_body_with(model, agent, text))
            .send()
            .await?;
        let response = ensure_success(response, "POST prompt_async").await?;
        if response.status() != reqwest::StatusCode::NO_CONTENT && !response.status().is_success() {
            return Err(ChatError::UnexpectedStatus {
                route: "POST prompt_async".to_owned(),
                status: response.status().as_u16(),
            });
        }
        Ok(())
    }

    /// `POST /session/:id/abort`.
    pub async fn abort(&self, session_id: &str) -> Result<()> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/session/{session_id}/abort"),
            )
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        ensure_success(response, "POST abort").await?;
        Ok(())
    }

    /// `POST /session/:id/permissions/:permissionID` (v1) body `{response}`.
    pub async fn respond_permission(
        &self,
        session_id: &str,
        permission_id: &str,
        reply: PermissionReply,
    ) -> Result<bool> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/session/{session_id}/permissions/{permission_id}"),
            )
            .timeout(Duration::from_secs(15))
            .json(&json!({ "response": reply.as_str() }))
            .send()
            .await?;
        ensure_success(response, "POST permissions")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `POST /session/:id/revert` body `{messageID}` — deshace ese mensaje y
    /// sus efectos sobre los archivos (revert nativo de opencode).
    pub async fn revert(&self, session_id: &str, message_id: &str) -> Result<SessionInfo> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/session/{session_id}/revert"),
            )
            .timeout(Duration::from_secs(30))
            .json(&json!({ "messageID": message_id }))
            .send()
            .await?;
        ensure_success(response, "POST session/revert")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `POST /session/:id/unrevert` — restaura los mensajes revertidos.
    pub async fn unrevert(&self, session_id: &str) -> Result<SessionInfo> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/session/{session_id}/unrevert"),
            )
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        ensure_success(response, "POST session/unrevert")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `GET /session/status` — mapa `{ses_id: {type: idle|busy|retry}}`.
    pub async fn session_statuses(&self) -> Result<Value> {
        let response = self
            .request(reqwest::Method::GET, "/session/status")
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        ensure_success(response, "/session/status")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `GET /session/:id/message?limit=` — historial (refetch tras gaps SSE).
    pub async fn session_messages(&self, session_id: &str, limit: usize) -> Result<Value> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("/session/{session_id}/message"),
            )
            .query(&[("limit", limit.to_string())])
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        ensure_success(response, "GET message")
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// `GET /event` — abre el stream SSE (sin timeout; vive hasta abortar).
    pub async fn open_event_stream(&self) -> Result<SseStream> {
        let response = self
            .request(reqwest::Method::GET, "/event")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .send()
            .await?;
        let response = ensure_success(response, "GET /event").await?;
        Ok(SseStream {
            response,
            parser: SseParser::new(),
            queue: VecDeque::new(),
            done: false,
        })
    }
}

/// Stream SSE incremental (pull-based, sin timeout global).
pub struct SseStream {
    response: reqwest::Response,
    parser: SseParser,
    queue: VecDeque<SseEvent>,
    done: bool,
}

impl SseStream {
    /// Siguiente evento; `Ok(None)` = el server cerró el stream.
    pub async fn next(&mut self) -> Result<Option<SseEvent>> {
        loop {
            if let Some(event) = self.queue.pop_front() {
                return Ok(Some(event));
            }
            if self.done {
                return Ok(None);
            }
            match self.response.chunk().await? {
                Some(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    self.queue.extend(self.parser.push(&text));
                }
                None => {
                    self.done = true;
                    self.queue.extend(self.parser.finish());
                }
            }
        }
    }
}

async fn ensure_success(response: reqwest::Response, route: &str) -> Result<reqwest::Response> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(ChatError::UnexpectedStatus {
            route: route.to_owned(),
            status: response.status().as_u16(),
        })
    }
}
