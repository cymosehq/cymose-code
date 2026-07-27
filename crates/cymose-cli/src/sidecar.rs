//! JSON-RPC 2.0 over stdio — the transport the VS Code extension uses.
//!
//! Newline-delimited, one message per line. stderr is for logs and is never
//! part of the protocol; anything written to stdout that isn't a message will
//! desynchronise the client, which is why nothing else in this binary prints
//! when the sidecar is running.
//!
//! The loop is synchronous on purpose. Every method implemented so far is a
//! store read or write that finishes in microseconds, and a synchronous loop
//! cannot interleave a half-written response into the stream. Streaming turns
//! will need notifications pushed from another task; that is the point at
//! which this becomes async, and not before.

use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::Result;
use cymose_core::context::ContextBuilder;
use cymose_core::{Store, CORE_VERSION, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

pub fn serve(store_path: &Path) -> Result<()> {
    let store = Store::open(store_path)?;
    let mut workspace: Option<String> = None;

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                write(
                    &mut stdout,
                    &error_response(Value::Null, -32700, &format!("parse error: {e}")),
                )?;
                continue;
            }
        };

        if request.method == "shutdown" {
            if let Some(id) = request.id {
                write(&mut stdout, &ok_response(id, json!(null)))?;
            }
            return Ok(());
        }

        let outcome = dispatch(&store, &mut workspace, &request);

        // A request without an id is a notification: no reply, even on error.
        let Some(id) = request.id else { continue };
        let response = match outcome {
            Ok(result) => ok_response(id, result),
            Err(e) => error_response(id, e.code, &e.message),
        };
        write(&mut stdout, &response)?;
    }
    Ok(())
}

struct Failure {
    code: i32,
    message: String,
}

impl From<cymose_core::Error> for Failure {
    fn from(e: cymose_core::Error) -> Self {
        Failure {
            code: e.rpc_code(),
            message: e.to_string(),
        }
    }
}

fn dispatch(
    store: &Store,
    workspace: &mut Option<String>,
    request: &Request,
) -> std::result::Result<Value, Failure> {
    let params = &request.params;

    match request.method.as_str() {
        "initialize" => Ok(json!({
            "protocol": PROTOCOL_VERSION,
            "core_version": CORE_VERSION,
            "workspace": workspace,
        })),

        "workspace.open" => {
            let path = string_param(params, "path")?;
            let root = std::path::PathBuf::from(&path);
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "workspace".into());
            // Opening initialises: the extension activating in a directory is
            // the same intent as `cymose init` in it.
            let id = store
                .ensure_workspace(&root, &name)
                .map_err(Failure::from)?;
            *workspace = Some(id.clone());
            let tree = store.tree(&id).map_err(Failure::from)?;
            Ok(json!({ "workspace": id, "tree": tree }))
        }

        "session.tree" => {
            let id = current(workspace)?;
            let tree = store.tree(&id).map_err(Failure::from)?;
            Ok(json!({ "tree": tree }))
        }

        "session.new" => {
            let id = current(workspace)?;
            let title = string_param(params, "title")?;
            let parent = params.get("parent").and_then(Value::as_str);
            let session = store
                .create_session(&id, &title, parent)
                .map_err(Failure::from)?;
            let inherited = ContextBuilder::new(store)
                .build(&session.id)
                .map_err(Failure::from)?;
            Ok(json!({
                "session": session,
                "inherited": inherited.items.iter().map(|i| json!({
                    "session": i.session_id,
                    "title": i.title,
                    "outcome": i.outcome,
                })).collect::<Vec<_>>(),
                "dropped": inherited.dropped,
            }))
        }

        "session.resume" => {
            let session_id = string_param(params, "id")?;
            let session = store.session(&session_id).map_err(Failure::from)?;
            let inherited = ContextBuilder::new(store)
                .build(&session.id)
                .map_err(Failure::from)?;
            Ok(json!({ "session": session, "context": inherited.render() }))
        }

        "model.list" => {
            let config = cymose_core::Config::load(None).map_err(|e| Failure {
                code: -32099,
                message: e.to_string(),
            })?;
            Ok(json!({ "chain": config.router.chain, "active": config.router.chain.first() }))
        }

        // Documented, not yet built. A specific "not implemented" beats
        // "method not found", which would send the extension looking for a
        // version mismatch that isn't there.
        "session.prompt" | "session.cancel" | "session.diff" | "session.promote"
        | "model.switch" => Err(Failure {
            code: -32004,
            message: format!("{} is not implemented in this build", request.method),
        }),

        other => Err(Failure {
            code: -32601,
            message: format!("unknown method {other}"),
        }),
    }
}

fn current(workspace: &Option<String>) -> std::result::Result<String, Failure> {
    workspace.clone().ok_or_else(|| Failure {
        code: -32000,
        message: "no workspace is open".into(),
    })
}

fn string_param(params: &Value, name: &str) -> std::result::Result<String, Failure> {
    params
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Failure {
            code: -32602,
            message: format!("missing parameter `{name}`"),
        })
}

fn ok_response(id: Value, result: Value) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn error_response(id: Value, code: i32, message: &str) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(RpcError {
            code,
            message: message.to_string(),
        }),
    }
}

fn write(out: &mut impl Write, response: &Response) -> Result<()> {
    serde_json::to_writer(&mut *out, response)?;
    out.write_all(b"\n")?;
    // The extension is waiting on this line; a buffered response looks like a
    // hung sidecar.
    out.flush()?;
    Ok(())
}
