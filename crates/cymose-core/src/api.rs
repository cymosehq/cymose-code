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

    /// The assistant's own turn when it asked for tools.
    ///
    /// Content is null rather than empty when the model said nothing before
    /// calling — some providers reject an empty string in that position, and
    /// "" and null mean different things to all of them.
    pub fn assistant_tool_calls(text: &str, calls: &[ToolCall]) -> Self {
        ApiMessage {
            role: "assistant".into(),
            content: (!text.trim().is_empty()).then(|| text.to_string()),
            tool_calls: calls.to_vec(),
            tool_call_id: None,
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

/// Pulls complete SSE frames out of a buffer, leaving any partial one behind.
///
/// The thing this exists for: a frame is delimited by a blank line, and the
/// network has no idea about that. A 4KB read can end mid-JSON, and the next
/// one carries the rest. Parsing whatever arrived and hoping is how streams
/// drop tokens under load and nowhere else — it works perfectly on a fast
/// local connection and corrupts one message in fifty over a bad hotel wifi.
///
/// Returns the frames it could complete; `buffer` keeps the remainder.
fn drain_sse_frames(buffer: &mut String) -> Vec<(String, String)> {
    let mut frames = Vec::new();
    // Servers disagree about line endings, and a stream that works on one and
    // not the other is a bug nobody can reproduce.
    let normalised = buffer.replace("\r\n", "\n");
    *buffer = normalised;

    while let Some(end) = buffer.find("\n\n") {
        let raw: String = buffer.drain(..end + 2).collect();
        let mut event = String::new();
        let mut data = String::new();
        for line in raw.lines() {
            if let Some(rest) = line.strip_prefix("event:") {
                event = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("data:") {
                // Multiple data: lines in one frame concatenate, per the spec.
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim_start());
            }
            // Comments (": keep-alive") and unknown fields are ignored, which
            // is what keeps a proxy's heartbeat from looking like an event.
        }
        if !data.is_empty() {
            frames.push((event, data));
        }
    }
    frames
}

/// One chunk of an OpenAI-style stream, translated into our own event.
///
/// BYOK talks to OpenRouter directly, and OpenRouter speaks OpenAI's format:
/// no `event:` line, deltas under `choices[0].delta`, and a `[DONE]` sentinel
/// instead of a terminating event. Translating here rather than teaching the
/// rest of the crate two formats is the same call `openai_message` already
/// makes for the non-streaming path.
fn parse_openai_chunk(data: &str) -> Option<StreamEvent> {
    if data.trim() == "[DONE]" {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    let choice = value.get("choices")?.get(0)?;

    if let Some(delta) = choice
        .get("delta")
        .and_then(|d| d.get("content"))
        .and_then(|c| c.as_str())
    {
        if !delta.is_empty() {
            return Some(StreamEvent::TextDelta {
                delta: delta.to_string(),
            });
        }
    }

    if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str()) {
        return Some(StreamEvent::Done {
            stop_reason: reason.to_string(),
            tokens_used: value
                .get("usage")
                .and_then(|u| serde_json::from_value(u.clone()).ok())
                .unwrap_or_default(),
            model: value
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or_default()
                .to_string(),
        });
    }

    // A keep-alive chunk, or a role-only first delta. Neither is an event.
    None
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

/// The signed-in account, as `GET /v1/credits` describes it.
///
/// Deliberately the same payload the web app reads. A second endpoint that
/// answered "which plan is this" would be a second thing to keep true.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Account {
    pub plan: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub standard_used: u32,
    #[serde(default)]
    pub standard_cap: u32,
    #[serde(default)]
    pub premium_used: u32,
    #[serde(default)]
    pub premium_cap: u32,
    #[serde(default)]
    pub premium_extra: u32,
    #[serde(default)]
    pub standard_reset_at: Option<String>,
}

impl Account {
    /// Whether this account may use Cymose Code at all.
    ///
    /// The client is Apache-2.0 and the check is one `if` — anybody who wants
    /// to remove it can, in about a minute. That is not what the gate is for.
    /// It is for the honest majority, and for saying plainly what the deal is:
    /// the client is open, the service behind it is paid for.
    pub fn may_use_code(&self) -> bool {
        self.is_admin || matches!(self.plan.as_str(), "pro" | "max")
    }

    /// What to call the plan in a sentence.
    pub fn plan_label(&self) -> &str {
        match self.plan.as_str() {
            "pro" => "Pro",
            "max" => "Max",
            _ => "Free",
        }
    }
}

/// Tree format this build speaks. Sent by the server as `version`.
pub const SYNC_VERSION: u32 = 1;

/// One node of the Web tree, as `GET /v1/sync/tree` returns it.
///
/// Structure and summaries only — no message bodies, no note bodies. A client
/// draws the tree from this and fetches the transcript for the one node the
/// user actually opened; returning everything would grow without bound for
/// exactly the accounts that sync the most.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SyncNode {
    pub id: String,
    /// `None` is a root node.
    pub parent_id: Option<String>,
    pub title: String,
    /// Context frozen from the ancestors at the moment this branch was made.
    pub inherited_summary: Option<String>,
    /// Conclusions promoted up from this node's own branches.
    pub promoted_digest: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    /// `None` means the node has never been dragged on the Web canvas, and a
    /// client is free to lay it out however it likes.
    #[serde(default)]
    pub position: Option<SyncPosition>,
    #[serde(default)]
    pub notes: Vec<SyncNote>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct SyncPosition {
    pub x: f64,
    pub y: f64,
}

/// A notebook pinned to a node. Title only — see [`SyncNode`].
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SyncNote {
    pub id: String,
    pub title: String,
    /// Whether the Web reads this note into the model's context.
    #[serde(default)]
    pub in_context: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SyncTree {
    pub version: u32,
    /// Advisory in v1. Returned from the first version so clients can start
    /// recording it, because the write direction will need it.
    #[serde(default)]
    pub synced_at: Option<String>,
    pub nodes: Vec<SyncNode>,
}

impl SyncTree {
    /// Root nodes, in the order the server sent them.
    pub fn roots(&self) -> impl Iterator<Item = &SyncNode> {
        self.nodes.iter().filter(|n| n.parent_id.is_none())
    }

    /// Direct children of a node.
    pub fn children_of<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a SyncNode> {
        self.nodes
            .iter()
            .filter(move |n| n.parent_id.as_deref() == Some(id))
    }
}

/// Where a turn is sent.
///
/// Cymose is the default: the account that has already passed the plan gate
/// pays for the turn, and the server owns the summariser's prompt so it can be
/// improved without a client release.
///
/// OpenRouter is the second path. With `OPENROUTER_API_KEY` set, turns go
/// straight to the user's own OpenRouter account with nothing of ours in the
/// path — the plan is still the licence to use the client, the key only
/// decides whose credit the tokens come out of. Nothing of ours in the path
/// also means no server-side summariser to call; see docs/spec.md §8.
#[derive(Debug, Clone)]
pub enum Backend {
    OpenRouter {
        api_key: String,
    },
    Cymose {
        base_url: String,
        token: String,
        device_id: String,
    },
}

const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

#[derive(Clone)]
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

    /// One agent turn, buffered.
    ///
    /// For callers that want the finished result and nothing in between —
    /// scripts, the summariser, tests. Anything with a person watching should
    /// use [`Client::inference_stream`], which is the same turn delivered as
    /// it happens.
    pub async fn inference(&self, req: &InferenceRequest) -> Result<InferenceResult> {
        if req.stream {
            return Err(Error::NotImplemented(
                "buffered inference with stream: true — call inference_stream",
            ));
        }
        match &self.backend {
            Backend::OpenRouter { api_key } => self.openrouter_inference(req, api_key).await,
            // `stream: false` is a documented mode of the route, which is what
            // makes the contract testable with curl — so this sends it rather
            // than refusing a shape the server accepts.
            Backend::Cymose { .. } => self.post("/v1/code/inference", req).await,
        }
    }

    /// One agent turn, streamed.
    ///
    /// The callback is invoked per event as it arrives, which is the whole
    /// point: an agent turn takes tens of seconds, and a client that shows
    /// nothing until the end is indistinguishable from one that has hung.
    ///
    /// Both backends stream. The Cymose route sends our own `event:`/`data:`
    /// pairs; OpenRouter sends OpenAI's format, translated on the way in, so
    /// nothing above this line has to know which key paid for the turn.
    pub async fn inference_stream<F>(&self, req: &InferenceRequest, mut on_event: F) -> Result<()>
    where
        F: FnMut(StreamEvent),
    {
        use futures_util::StreamExt;

        let (response, openai) = match &self.backend {
            Backend::OpenRouter { api_key } => {
                if api_key.trim().is_empty() {
                    return Err(Error::NotAuthenticated);
                }
                let body = self.openrouter_body(req, true);
                let response = self
                    .http
                    .post(OPENROUTER_URL)
                    .bearer_auth(api_key)
                    .header("HTTP-Referer", "https://cymose.dev")
                    .header("X-Title", "Cymose Code")
                    .json(&body)
                    .send()
                    .await?;
                (response, true)
            }
            Backend::Cymose {
                base_url,
                token,
                device_id,
            } => {
                let mut body = serde_json::to_value(req)?;
                body["stream"] = serde_json::Value::Bool(true);
                let response = self
                    .http
                    .post(format!(
                        "{}/v1/code/inference",
                        base_url.trim_end_matches('/')
                    ))
                    .bearer_auth(token)
                    .header("X-Cymose-Device", device_id)
                    .json(&body)
                    .send()
                    .await?;
                (response, false)
            }
        };

        let status = response.status();
        if !status.is_success() {
            // Fail before a single byte is handed to the caller: an error that
            // arrives after half an answer has been drawn is one the client has
            // to unpaint.
            let text = response.text().await.unwrap_or_default();
            let (provider_status, message) = parse_error_body(&text);
            return Err(match provider_status.unwrap_or(status.as_u16()) {
                401 => Error::NotAuthenticated,
                402 | 403 | 451 => Error::Refused(message),
                other => Error::Upstream {
                    status: other,
                    message,
                },
            });
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = stream.next().await {
            // Bytes, not str: a multi-byte character can straddle a chunk
            // boundary, and from_utf8_lossy on each chunk in isolation would
            // turn it into two replacement characters.
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            for (event, data) in drain_sse_frames(&mut buffer) {
                let parsed = if openai {
                    match parse_openai_chunk(&data) {
                        Some(event) => event,
                        None => continue,
                    }
                } else {
                    parse_stream_event(&event, &data)
                };
                on_event(parsed);
            }
        }
        Ok(())
    }

    /// The BYOK turn: OpenAI-compatible chat completions, straight to
    /// OpenRouter.
    ///
    /// The translation lives here rather than in the caller because the wire
    /// format this crate speaks is deliberately not OpenAI's — `input` as a
    /// parsed object rather than a JSON string is one less place for a tool
    /// call to get double-encoded — and the agent loop should not have to know
    /// which backend it is talking to.
    /// The OpenAI-shaped request body, used by both the streamed and the
    /// non-streamed BYOK path. Kept in one place because two copies of this
    /// translation is how a streamed turn quietly stops sending tools.
    fn openrouter_body(&self, req: &InferenceRequest, stream: bool) -> serde_json::Value {
        serde_json::json!({
            "model": req.model,
            "messages": req.messages.iter().map(openai_message).collect::<Vec<_>>(),
            "max_tokens": req.max_tokens,
            "stream": stream,
            // Ask for a usage record on the final chunk. Without it a streamed
            // turn reports zero tokens, and the router's budget arithmetic
            // silently works off nothing.
            "stream_options": if stream { serde_json::json!({ "include_usage": true }) } else { serde_json::Value::Null },
            "tools": req.tools.iter().map(|t| serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                },
            })).collect::<Vec<_>>(),
        })
    }

    async fn openrouter_inference(
        &self,
        req: &InferenceRequest,
        api_key: &str,
    ) -> Result<InferenceResult> {
        if api_key.trim().is_empty() {
            return Err(Error::NotAuthenticated);
        }

        let body = self.openrouter_body(req, false);

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
            // BYOK puts no server in the path, so there is no server-side
            // prompt to call. Carrying one here instead would mean two
            // implementations that can disagree about what a summary is —
            // still an open question, deliberately not answered by guessing.
            // See docs/spec.md §8.
            Backend::OpenRouter { .. } => Err(Error::NotImplemented("BYOK summaries")),
            Backend::Cymose { .. } => self.post("/v1/code/summarize", req).await,
        }
    }

    /// Sends a session's outcome up to Cymose Web. Needs the Cymose backend by
    /// definition — there is nothing to promote to without it.
    pub async fn promote(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        self.post("/v1/promote", body).await
    }

    /// Who the caller is, what they pay for, and what is left.
    ///
    /// The gate every entry point runs before doing anything expensive. It is
    /// one round trip against a route the server already serves, so there is no
    /// separate "is my token good" endpoint to keep in sync with this one.
    pub async fn account(&self) -> Result<Account> {
        self.get("/v1/credits").await
    }

    /// The account's Web tree, read into whatever this client stores.
    ///
    /// Read-only on purpose. Pull before push: an export has no conflict
    /// resolution to get wrong, and reading the Web tree is most of what
    /// synchronisation is for from here — you planned it in the browser and
    /// the session opens against the node you planned. Writing back needs
    /// revisions, tombstones and merge rules, and those should not be designed
    /// after the easy half has already shipped.
    ///
    /// Needs the Cymose backend: there is no tree to read on a bare provider
    /// key. A client built from `OPENROUTER_API_KEY` alone gets
    /// [`Error::NotImplemented`] from [`Client::get`].
    pub async fn sync_tree(&self) -> Result<SyncTree> {
        let tree: SyncTree = self.get("/v1/sync/tree").await?;
        // Three clients read this route on three release schedules. A build
        // that meets a version it does not know must say so rather than guess
        // at a tree — a mis-parsed parent pointer is a reparented branch, and
        // the user would find out by seeing their work in the wrong place.
        if tree.version != SYNC_VERSION {
            return Err(Error::Upstream {
                status: 505,
                message: format!(
                    "this build speaks tree format v{SYNC_VERSION}, the server sent v{} — update Cymose",
                    tree.version
                ),
            });
        }
        Ok(tree)
    }

    /// A read from the Cymose backend. The GET twin of [`Client::post`], and
    /// refuses the same way on a BYOK client: these routes are the account's,
    /// and a provider key is not an account.
    async fn get<R: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<R> {
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
            .get(format!("{}{path}", base_url.trim_end_matches('/')))
            .bearer_auth(token)
            .header("X-Cymose-Device", device_id)
            .send()
            .await?;

        let status = response.status();
        if status.is_success() {
            return Ok(response.json::<R>().await?);
        }

        let text = response.text().await.unwrap_or_default();
        let (provider_status, message) = parse_error_body(&text);
        Err(match provider_status.unwrap_or(status.as_u16()) {
            401 => Error::NotAuthenticated,
            402 | 403 | 451 => Error::Refused(message),
            other => Error::Upstream {
                status: other,
                message,
            },
        })
    }

    /// A call to the Cymose backend. Refuses on a BYOK client: the routes it
    /// targets are the account's, and there is no account behind a bare
    /// provider key.
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
///
/// Fields the server adds beyond these (`model`, `tokens_used`,
/// `credits_charged`) are ignored rather than rejected, so it can report more
/// about a turn without a protocol bump.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct InferenceResult {
    #[serde(default)]
    pub content: String,
    /// Tools the model wants run. The client runs them — see [`crate::agent`].
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
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

    #[test]
    fn a_frame_split_across_chunks_is_held_until_it_is_whole() {
        // The failure this prevents: parse what arrived, and a message cut
        // mid-JSON is dropped. It works on a fast local connection and loses
        // one message in fifty over bad wifi, which is the worst kind of bug
        // to own.
        let mut buffer = String::new();

        buffer.push_str("event: text_delta\ndata: {\"delta\":\"hel");
        assert!(
            drain_sse_frames(&mut buffer).is_empty(),
            "a half-arrived frame must not be parsed"
        );

        buffer.push_str("lo\"}\n\n");
        let frames = drain_sse_frames(&mut buffer);
        assert_eq!(frames.len(), 1);
        assert!(matches!(
            parse_stream_event(&frames[0].0, &frames[0].1),
            StreamEvent::TextDelta { delta } if delta == "hello"
        ));
        assert!(buffer.is_empty());
    }

    #[test]
    fn several_frames_in_one_chunk_all_come_out() {
        let mut buffer = String::from(
            "event: text_delta\ndata: {\"delta\":\"a\"}\n\n             event: text_delta\ndata: {\"delta\":\"b\"}\n\n             event: text_delta\ndata: {\"delta\":\"c\"}\n\n",
        );
        let frames = drain_sse_frames(&mut buffer);
        assert_eq!(frames.len(), 3);
        assert!(buffer.is_empty());
    }

    #[test]
    fn crlf_and_keepalive_comments_are_survivable() {
        // Different servers and proxies disagree about both, and a stream that
        // works against one and not the other is unreproducible.
        let mut buffer = String::from(
            ": keep-alive\r\n\r\nevent: text_delta\r\ndata: {\"delta\":\"x\"}\r\n\r\n",
        );
        let frames = drain_sse_frames(&mut buffer);
        assert_eq!(frames.len(), 1, "the heartbeat is not an event");
        assert_eq!(frames[0].1, "{\"delta\":\"x\"}");
    }

    #[test]
    fn openrouter_chunks_translate_into_our_events() {
        // BYOK speaks OpenAI's format: no event line, deltas under choices.
        let delta = parse_openai_chunk(r#"{"choices":[{"delta":{"content":"hi"}}]}"#);
        assert!(matches!(delta, Some(StreamEvent::TextDelta { delta }) if delta == "hi"));

        // A role-only opener and a keep-alive are not events.
        assert!(parse_openai_chunk(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#).is_none());
        assert!(parse_openai_chunk("[DONE]").is_none());

        let done = parse_openai_chunk(
            r#"{"model":"anthropic/claude-sonnet-5","choices":[{"finish_reason":"stop","delta":{}}],"usage":{"input_tokens":10,"output_tokens":3}}"#,
        );
        match done {
            Some(StreamEvent::Done {
                stop_reason, model, ..
            }) => {
                assert_eq!(stop_reason, "stop");
                assert_eq!(model, "anthropic/claude-sonnet-5");
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn a_tree_export_parses_into_a_navigable_graph() {
        // The documented v1 shape, verbatim from docs/api-contract.md. The
        // point of pinning it in a test is that three clients read this route
        // and the contract is the only thing keeping them agreeing.
        let body = r#"{
            "version": 1,
            "synced_at": "2026-07-29T02:14:00.000Z",
            "nodes": [
                {
                    "id": "root",
                    "parent_id": null,
                    "title": "rate limiting",
                    "inherited_summary": null,
                    "promoted_digest": "sliding window won",
                    "pinned": true,
                    "position": { "x": 120.0, "y": 40.0 },
                    "notes": [
                        { "id": "n1", "title": "wire protocol", "in_context": true, "updated_at": "t" }
                    ],
                    "created_at": "t"
                },
                {
                    "id": "child",
                    "parent_id": "root",
                    "title": "token bucket",
                    "inherited_summary": "the parent is about rate limiting",
                    "promoted_digest": null,
                    "pinned": false,
                    "position": null,
                    "notes": [],
                    "created_at": "t"
                }
            ]
        }"#;

        let tree: SyncTree = serde_json::from_str(body).expect("v1 export should parse");
        assert_eq!(tree.version, SYNC_VERSION);
        assert_eq!(tree.nodes.len(), 2);

        let roots: Vec<_> = tree.roots().collect();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].id, "root");
        assert_eq!(roots[0].notes[0].title, "wire protocol");
        assert_eq!(roots[0].position, Some(SyncPosition { x: 120.0, y: 40.0 }));

        let children: Vec<_> = tree.children_of("root").collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].title, "token bucket");
        // Never dragged on the canvas — the client lays this one out itself.
        assert!(children[0].position.is_none());
    }

    #[test]
    fn a_node_with_no_notes_key_still_parses() {
        // Older servers, and any future response that omits an empty list. A
        // missing notes array must mean "no notes", not a parse failure that
        // loses the whole tree.
        let node: SyncNode = serde_json::from_str(
            r#"{"id":"a","parent_id":null,"title":"t","inherited_summary":null,
                "promoted_digest":null,"created_at":"t"}"#,
        )
        .expect("optional fields should default");
        assert!(node.notes.is_empty());
        assert!(!node.pinned);
        assert!(node.position.is_none());
    }

    #[tokio::test]
    async fn syncing_without_the_cymose_backend_is_unimplemented() {
        // A provider key is not an account, so there is no tree to read.
        let client = Client::openrouter("sk-or-v1-whatever");
        assert!(matches!(
            client.sync_tree().await,
            Err(Error::NotImplemented("the Cymose backend"))
        ));
    }

    #[tokio::test]
    async fn summarising_without_the_cymose_backend_is_unimplemented() {
        // The summariser's prompt is the server's. On a bare provider key
        // there is no server in the path, so this refuses rather than
        // inventing a second prompt — see docs/spec.md §8.
        let client = Client::openrouter("sk-or-v1-whatever");
        let req = SummarizeRequest {
            session_id: "s".into(),
            task: "t".into(),
            transcript: vec![ApiMessage::text("user", "hi")],
            outcome: "done".into(),
            model: None,
        };
        assert!(matches!(
            client.summarize(&req).await,
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
