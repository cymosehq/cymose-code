//! Client for the Cymose API.
//!
//! Only the wire contract lives here — what to send and what comes back. How
//! the server answers (which provider serves a model, what a turn costs, what
//! it summarises with) is not this repository's business and must not be
//! reconstructed in it. See docs/api-contract.md.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::router::Decision;

#[derive(Debug, Clone, Serialize)]
pub struct InferenceRequest {
    /// Logging and billing only — the server keeps no context between calls.
    /// The transcript is local, and every turn sends the part of it that
    /// matters.
    pub session_id: String,
    /// One model per request. The chain and the failover live in
    /// [`crate::router`]: when this call comes back 429, the client picks the
    /// next model and sends a new request, so the server never has to know a
    /// chain exists.
    pub model: String,
    pub messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSchema>,
    pub max_tokens: u32,
    pub stream: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    /// Null on an assistant turn that only calls tools — the distinction
    /// between "said nothing" and "said an empty string" is one providers care
    /// about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Set on a `tool` message, matching the call it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ApiMessage {
    pub fn text(role: &str, content: impl Into<String>) -> Self {
        ApiMessage {
            role: role.to_string(),
            content: Some(content.into()),
            ..Default::default()
        }
    }

    /// The result of running a tool, on its way back to the model.
    pub fn tool_result(tool_call_id: &str, content: impl Into<String>) -> Self {
        ApiMessage {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.to_string()),
        }
    }
}

/// A tool the model asked for. The client runs it — see [`crate::agent`] — and
/// the server only relays the request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input: u32,
    #[serde(default)]
    pub output: u32,
}

/// One SSE event of a streamed turn.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    TextDelta {
        delta: String,
    },
    ToolCall(ToolCall),
    Done {
        stop_reason: String,
        tokens_used: Usage,
        model: String,
    },
    /// The provider failed after the stream had already started, so there is
    /// no HTTP status left to fail on — the status it would have been is in
    /// `provider_status`, and the router treats it the same way.
    Error {
        kind: String,
        provider_status: u16,
        message: String,
    },
    /// An event type this build doesn't know. Ignored on purpose: the server
    /// may add events without a protocol bump, and a hard failure here would
    /// make every such addition a breaking change.
    Unknown,
}

/// Parses one `event:`/`data:` pair from the stream.
///
/// Split out from the transport so the event shapes can be tested without a
/// server, which is the only part of streaming that can be pinned down before
/// the route exists.
pub fn parse_stream_event(event: &str, data: &str) -> StreamEvent {
    let value: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return StreamEvent::Unknown,
    };
    let text = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };

    match event {
        "text_delta" => StreamEvent::TextDelta {
            delta: text("delta"),
        },
        "tool_call" => match serde_json::from_value::<ToolCall>(value) {
            Ok(call) => StreamEvent::ToolCall(call),
            Err(_) => StreamEvent::Unknown,
        },
        "done" => StreamEvent::Done {
            stop_reason: text("stop_reason"),
            tokens_used: value
                .get("tokens_used")
                .and_then(|u| serde_json::from_value(u.clone()).ok())
                .unwrap_or_default(),
            model: text("model"),
        },
        "error" => StreamEvent::Error {
            kind: text("type"),
            provider_status: value
                .get("provider_status")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(500) as u16,
            message: text("message"),
        },
        _ => StreamEvent::Unknown,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SummarizeRequest {
    pub session_id: String,
    /// What the session set out to do — the summary is written against it.
    pub task: String,
    pub transcript: Vec<ApiMessage>,
    /// The client's own verdict, not the model's. Whether the work succeeded
    /// is a fact the client already knows, and letting a summariser infer it
    /// from a transcript is how a failed attempt gets inherited as a success.
    pub outcome: String,
    /// Optional: the server picks a cheap default. Summarising every session
    /// on an expensive model would undo the saving the graph exists for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// The structured summary a child session inherits. The server guarantees the
/// shape via the provider's structured-output mode, so this is parsed, not
/// scraped out of prose.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionSummary {
    #[serde(default)]
    pub task: String,
    #[serde(default)]
    pub files_touched: Vec<String>,
    #[serde(default)]
    pub approach: String,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub key_decisions: Vec<String>,
    #[serde(default)]
    pub errors_encountered: Vec<String>,
    #[serde(default)]
    pub tokens_used: u32,
}

impl SessionSummary {
    /// Flattens the structured summary into the text a child inherits.
    ///
    /// The store keeps summaries as text, so this is where structure is spent.
    /// Errors come before decisions: what went wrong is what a sibling session
    /// most needs not to repeat.
    pub fn render(&self) -> String {
        let mut out = self.approach.trim().to_string();
        for error in &self.errors_encountered {
            out.push_str(&format!("\nWent wrong: {}", error.trim()));
        }
        for decision in &self.key_decisions {
            out.push_str(&format!("\nDecided: {}", decision.trim()));
        }
        out
    }
}

/// Where a turn is sent.
///
/// 0.1 is BYOK only: the user's own OpenRouter key, straight to OpenRouter,
/// with nothing of ours in the path. That is the honest shape for a beta —
/// no account to make, no billing to trust, no service to be down, and the
/// user can see exactly what each turn costs on their own dashboard.
///
/// The Cymose backend is the later half: it exists (see the API's
/// `/v1/code/*`) and is what Cymose Web syncs through, but it is not wired up
/// here yet and nothing in this build depends on it.
#[derive(Debug, Clone)]
pub enum Backend {
    OpenRouter {
        api_key: String,
    },
    /// Not usable yet — see [`Client::inference`].
    Cymose {
        base_url: String,
        token: String,
        device_id: String,
    },
}

const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

pub struct Client {
    http: reqwest::Client,
    backend: Backend,
}

impl Client {
    pub fn new(backend: Backend) -> Self {
        Client {
            http: reqwest::Client::new(),
            backend,
        }
    }

    /// BYOK: the 0.1 path.
    pub fn openrouter(api_key: impl Into<String>) -> Self {
        Client::new(Backend::OpenRouter {
            api_key: api_key.into(),
        })
    }

    pub fn is_authenticated(&self) -> bool {
        match &self.backend {
            Backend::OpenRouter { api_key } => !api_key.trim().is_empty(),
            Backend::Cymose { token, .. } => !token.trim().is_empty(),
        }
    }

    /// One agent turn.
    ///
    /// Streaming is not wired up yet, so asking for a stream fails loudly
    /// rather than silently buffering — which would look like the model
    /// hanging. The event shapes it will produce are already pinned down in
    /// [`parse_stream_event`].
    pub async fn inference(&self, req: &InferenceRequest) -> Result<InferenceResult> {
        if req.stream {
            return Err(Error::NotImplemented("streaming inference"));
        }
        match &self.backend {
            Backend::OpenRouter { api_key } => self.openrouter_inference(req, api_key).await,
            Backend::Cymose { .. } => Err(Error::NotImplemented("the Cymose backend")),
        }
    }

    /// The BYOK turn: OpenAI-compatible chat completions, straight to
    /// OpenRouter.
    ///
    /// The translation lives here rather than in the caller because the wire
    /// format this crate speaks is deliberately not OpenAI's — `input` as a
    /// parsed object rather than a JSON string is one less place for a tool
    /// call to get double-encoded — and the agent loop should not have to know
    /// which backend it is talking to.
    async fn openrouter_inference(
        &self,
        req: &InferenceRequest,
        api_key: &str,
    ) -> Result<InferenceResult> {
        if api_key.trim().is_empty() {
            return Err(Error::NotAuthenticated);
        }

        let body = serde_json::json!({
            "model": req.model,
            "messages": req.messages.iter().map(openai_message).collect::<Vec<_>>(),
            "max_tokens": req.max_tokens,
            "stream": false,
            "tools": req.tools.iter().map(|t| serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                },
            })).collect::<Vec<_>>(),
        });

        let response = self
            .http
            .post(OPENROUTER_URL)
            .bearer_auth(api_key)
            // Attribution on OpenRouter's public leaderboard. Optional, free,
            // and the only thing we add to a BYOK request.
            .header("HTTP-Referer", "https://cymose.dev")
            .header("X-Title", "Cymose Code")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let (_, message) = parse_error_body(&text);
            return Err(match status.as_u16() {
                401 => Error::NotAuthenticated,
                402 | 403 => Error::Refused(message),
                other => Error::Upstream {
                    status: other,
                    message,
                },
            });
        }

        let value: serde_json::Value = response.json().await?;
        Ok(parse_openai_completion(&value))
    }

    /// Compresses a finished session into the summary its children inherit.
    ///
    /// Synchronous and unstreamed: it is short, and nothing is waiting on it —
    /// a failure here delays a summary, it doesn't block the agent.
    pub async fn summarize(&self, req: &SummarizeRequest) -> Result<SessionSummary> {
        match &self.backend {
            // BYOK summarising asks the model for the structured shape
            // directly. The prompt lives in [`crate::summarize`] in this case
            // rather than on a server we aren't talking to.
            Backend::OpenRouter { .. } => Err(Error::NotImplemented("BYOK summaries")),
            Backend::Cymose { .. } => self.post("/v1/code/summarize", req).await,
        }
    }

    /// Sends a session's outcome up to Cymose Web. Needs the Cymose backend by
    /// definition — there is nothing to promote to without it.
    pub async fn promote(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        self.post("/v1/promote", body).await
    }

    /// A call to the Cymose backend. Unreachable in 0.1 — every caller checks
    /// the backend first — and kept because the routes it targets exist and
    /// are what the web integration will use.
    async fn post<B: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<R> {
        let Backend::Cymose {
            base_url,
            token,
            device_id,
        } = &self.backend
        else {
            return Err(Error::NotImplemented("the Cymose backend"));
        };
        let response = self
            .http
            .post(format!("{}{path}", base_url.trim_end_matches('/')))
            .bearer_auth(token)
            .header("X-Cymose-Device", device_id)
            .json(body)
            .send()
            .await?;

        let status = response.status();
        if status.is_success() {
            return Ok(response.json::<R>().await?);
        }

        let text = response.text().await.unwrap_or_default();
        let (provider_status, message) = parse_error_body(&text);

        // The provider's status, when the body carries one: the server passes
        // provider failures through instead of normalising them, and the
        // router's whole failover policy is built on that status.
        Err(match provider_status.unwrap_or(status.as_u16()) {
            401 => Error::NotAuthenticated,
            402 | 403 | 451 => Error::Refused(message),
            other => Error::Upstream {
                status: other,
                message,
            },
        })
    }

    /// What the router should do about a failed call. Kept next to the
    /// transport so the mapping from failure to behaviour has one home.
    ///
    /// `None` means the failure is not the model's fault (a local IO or store
    /// error), so trying a different one would be pointless.
    pub fn decision_for(error: &Error, router: &crate::router::Router) -> Option<Decision> {
        match error {
            Error::Upstream { status, message } => Some(router.decide(*status, message)),
            Error::NotAuthenticated => Some(Decision::Reauthenticate),
            Error::Refused(message) => Some(Decision::Refuse(message.clone())),
            _ => None,
        }
    }
}

/// What one non-streamed turn produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InferenceResult {
    pub content: String,
    /// Tools the model wants run. The client runs them — see [`crate::agent`].
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: String,
}

/// One of our messages in OpenAI's shape.
fn openai_message(message: &ApiMessage) -> serde_json::Value {
    if message.role == "tool" {
        return serde_json::json!({
            "role": "tool",
            "tool_call_id": message.tool_call_id,
            "content": message.content.clone().unwrap_or_default(),
        });
    }
    if !message.tool_calls.is_empty() {
        return serde_json::json!({
            "role": message.role,
            "content": message.content,
            "tool_calls": message.tool_calls.iter().map(|c| serde_json::json!({
                "id": c.id,
                "type": "function",
                "function": { "name": c.name, "arguments": c.input.to_string() },
            })).collect::<Vec<_>>(),
        });
    }
    serde_json::json!({
        "role": message.role,
        "content": message.content.clone().unwrap_or_default(),
    })
}

/// Reads a completion back out of OpenAI's shape.
///
/// Tolerant on purpose: a missing field yields an empty result rather than an
/// error, because a turn that produced no text but did call a tool is normal,
/// and so is the reverse.
fn parse_openai_completion(value: &serde_json::Value) -> InferenceResult {
    let choice = value.get("choices").and_then(|c| c.get(0));
    let message = choice.and_then(|c| c.get("message"));

    let tool_calls = message
        .and_then(|m| m.get("tool_calls"))
        .and_then(|t| t.as_array())
        .map(|calls| {
            calls
                .iter()
                .map(|call| {
                    let function = call.get("function");
                    let arguments = function
                        .and_then(|f| f.get("arguments"))
                        .and_then(|a| a.as_str())
                        .unwrap_or("{}");
                    ToolCall {
                        id: call
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        name: function
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        // Arguments arrive as a JSON string. If the model
                        // produced something unparsable, keep it verbatim
                        // rather than dropping the call — the client can then
                        // report what it actually got.
                        input: serde_json::from_str(arguments).unwrap_or_else(
                            |_| serde_json::json!({ "_malformed_arguments": arguments }),
                        ),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    InferenceResult {
        content: message
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string(),
        tool_calls,
        stop_reason: choice
            .and_then(|c| c.get("finish_reason"))
            .and_then(|f| f.as_str())
            .unwrap_or("stop")
            .to_string(),
    }
}

/// Pulls the provider status and the message out of an error body.
///
/// The server's shape is `{"error": {"type", "provider_status", "message"}}`,
/// but an error can also come from something in front of it — a proxy's HTML,
/// an empty body — so anything unrecognised is passed through as the message
/// rather than replaced with a guess. A refusal explains itself in terms only
/// the server knows; paraphrasing it here would lose that.
fn parse_error_body(text: &str) -> (Option<u16>, String) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return (None, text.to_string());
    };
    let Some(error) = value.get("error") else {
        return (None, text.to_string());
    };
    if let Some(message) = error.as_str() {
        return (None, message.to_string());
    }
    let status = error
        .get("provider_status")
        .and_then(serde_json::Value::as_u64)
        .map(|s| s as u16);
    let message = error
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or(text)
        .to_string();
    (status, message)
}

/// Strips bearer tokens from anything about to be logged or attached to a bug
/// report. Applied at the edge, so a token that reaches a log line is a bug in
/// one place rather than everywhere.
pub fn redact(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut previous_was_bearer = false;
    for word in text.split_whitespace() {
        // A JWT is the shape that actually shows up in a pasted log, with or
        // without the "Bearer" in front of it.
        let looks_like_a_token = word.len() > 20 && word.matches('.').count() >= 2;
        if previous_was_bearer || looks_like_a_token {
            out.push("[redacted]");
        } else {
            out.push(word);
        }
        previous_was_bearer = word.eq_ignore_ascii_case("bearer");
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_without_a_key_refuses_before_it_dials() {
        assert!(!Client::openrouter("").is_authenticated());
        assert!(!Client::openrouter("   ").is_authenticated());
        assert!(Client::openrouter("sk-or-v1-whatever").is_authenticated());
    }

    #[tokio::test]
    async fn the_cymose_backend_is_reported_as_unimplemented() {
        // 0.1 is BYOK only. The routes exist server-side; nothing here talks
        // to them yet, and pretending otherwise would fail at the first turn.
        let client = Client::new(Backend::Cymose {
            base_url: "https://example.invalid".into(),
            token: "t".into(),
            device_id: "device".into(),
        });
        let req = InferenceRequest {
            session_id: "s".into(),
            model: "claude-sonnet".into(),
            messages: vec![ApiMessage::text("user", "hi")],
            tools: vec![],
            max_tokens: 128,
            stream: false,
        };
        assert!(matches!(
            client.inference(&req).await,
            Err(Error::NotImplemented(_))
        ));
    }

    #[tokio::test]
    async fn streaming_is_reported_as_unimplemented_not_silently_buffered() {
        let client = Client::openrouter("sk-or-v1-whatever");
        let req = InferenceRequest {
            session_id: "s".into(),
            model: "claude-sonnet".into(),
            messages: vec![],
            tools: vec![],
            max_tokens: 128,
            stream: true,
        };
        assert!(matches!(
            client.inference(&req).await,
            Err(Error::NotImplemented(_))
        ));
    }

    #[test]
    fn tokens_are_redacted_from_log_text() {
        let line = "GET /v1/code/inference Bearer eyJhbGciOi.payloadpayload.signature failed";
        let redacted = redact(line);
        assert!(!redacted.contains("payloadpayload"));
        assert!(redacted.contains("failed"));
    }

    #[test]
    fn an_assistant_turn_that_only_calls_tools_sends_a_null_content() {
        let message = ApiMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "read_file".into(),
                input: serde_json::json!({ "path": "src/lib.rs" }),
            }],
            tool_call_id: None,
        };
        let json = serde_json::to_value(&message).unwrap();
        assert!(json.get("content").is_none());
        assert_eq!(json["tool_calls"][0]["name"], "read_file");

        // A plain message carries neither field, rather than empty ones.
        let plain = serde_json::to_value(ApiMessage::text("user", "hi")).unwrap();
        assert!(plain.get("tool_calls").is_none());
        assert!(plain.get("tool_call_id").is_none());
    }

    #[test]
    fn stream_events_parse_into_their_shapes() {
        assert_eq!(
            parse_stream_event("text_delta", r#"{"delta":"I'll look at"}"#),
            StreamEvent::TextDelta {
                delta: "I'll look at".into()
            }
        );
        assert_eq!(
            parse_stream_event(
                "tool_call",
                r#"{"id":"call_1","name":"read_file","input":{"path":"a.rs"}}"#
            ),
            StreamEvent::ToolCall(ToolCall {
                id: "call_1".into(),
                name: "read_file".into(),
                input: serde_json::json!({ "path": "a.rs" }),
            })
        );
        match parse_stream_event(
            "done",
            r#"{"stop_reason":"tool_use","tokens_used":{"input":1200,"output":340},"model":"claude-sonnet"}"#,
        ) {
            StreamEvent::Done {
                stop_reason,
                tokens_used,
                model,
            } => {
                assert_eq!(stop_reason, "tool_use");
                assert_eq!((tokens_used.input, tokens_used.output), (1200, 340));
                assert_eq!(model, "claude-sonnet");
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_event_is_ignored_rather_than_fatal() {
        // The server may add events without a protocol bump.
        assert_eq!(
            parse_stream_event("thinking", r#"{"delta":"…"}"#),
            StreamEvent::Unknown
        );
        assert_eq!(
            parse_stream_event("text_delta", "not json"),
            StreamEvent::Unknown
        );
    }

    #[test]
    fn a_mid_stream_failure_carries_the_status_the_router_needs() {
        let event = parse_stream_event(
            "error",
            r#"{"type":"provider_error","provider_status":503,"message":"upstream down"}"#,
        );
        let StreamEvent::Error {
            provider_status,
            message,
            kind,
        } = event
        else {
            panic!("expected Error");
        };
        assert_eq!((provider_status, kind.as_str()), (503, "provider_error"));

        let router = crate::router::Router::new(crate::router::RouterConfig::default());
        assert_eq!(router.decide(provider_status, &message), Decision::Failover);
    }

    #[test]
    fn the_provider_status_in_the_body_wins_over_the_envelope() {
        let (status, message) = parse_error_body(
            r#"{"error":{"type":"rate_limited","provider_status":429,"message":"slow down"}}"#,
        );
        assert_eq!(status, Some(429));
        assert_eq!(message, "slow down");
    }

    #[test]
    fn an_unrecognised_error_body_is_passed_through_not_guessed_at() {
        let (status, message) = parse_error_body("<html>502 Bad Gateway</html>");
        assert_eq!(status, None);
        assert_eq!(message, "<html>502 Bad Gateway</html>");

        // The older `{"error": "..."}` shape the rest of the API uses.
        let (status, message) = parse_error_body(r#"{"error":"Profile not found"}"#);
        assert_eq!(status, None);
        assert_eq!(message, "Profile not found");
    }

    #[test]
    fn tool_calls_translate_into_openai_shape_and_back() {
        let call = ToolCall {
            id: "call_1".into(),
            name: "read_file".into(),
            input: serde_json::json!({ "path": "src/lib.rs" }),
        };
        let assistant = ApiMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: vec![call.clone()],
            tool_call_id: None,
        };

        // Out: `input` becomes a JSON *string* in `function.arguments`.
        let wire = openai_message(&assistant);
        assert_eq!(wire["tool_calls"][0]["type"], "function");
        assert_eq!(wire["tool_calls"][0]["function"]["name"], "read_file");
        assert_eq!(
            wire["tool_calls"][0]["function"]["arguments"],
            serde_json::json!(r#"{"path":"src/lib.rs"}"#)
        );

        // Back: the string is parsed into an object again.
        let completion = serde_json::json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "function": { "name": "read_file", "arguments": "{\"path\":\"src/lib.rs\"}" }
                    }]
                }
            }]
        });
        let parsed = parse_openai_completion(&completion);
        assert_eq!(parsed.tool_calls, vec![call]);
        assert_eq!(parsed.stop_reason, "tool_calls");
        assert_eq!(parsed.content, "");
    }

    #[test]
    fn a_tool_result_message_carries_its_call_id() {
        let wire = openai_message(&ApiMessage::tool_result("call_1", "fn main() {}"));
        assert_eq!(wire["role"], "tool");
        assert_eq!(wire["tool_call_id"], "call_1");
        assert_eq!(wire["content"], "fn main() {}");
    }

    #[test]
    fn malformed_tool_arguments_are_kept_rather_than_dropped() {
        let completion = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{ "id": "c", "function": { "name": "search", "arguments": "{oops" } }]
                }
            }]
        });
        let parsed = parse_openai_completion(&completion);
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].input["_malformed_arguments"], "{oops");
    }

    #[test]
    fn a_structured_summary_renders_failures_ahead_of_decisions() {
        let summary = SessionSummary {
            approach: "Fixed off-by-one in window boundary check".into(),
            key_decisions: vec!["Used Instant::now() for monotonic guarantees".into()],
            errors_encountered: vec!["Initial fix overflowed on window.iter().count()".into()],
            ..Default::default()
        };
        let rendered = summary.render();
        assert!(rendered.starts_with("Fixed off-by-one"));
        assert!(rendered.find("Went wrong").unwrap() < rendered.find("Decided").unwrap());
    }
}
