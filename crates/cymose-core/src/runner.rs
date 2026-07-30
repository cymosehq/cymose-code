//! The agent loop: prompt in, streamed answer and tool calls out.
//!
//! This is the piece that makes Cymose Code an agent rather than a chat box.
//! It streams a turn, runs whatever tools the model asked for, feeds the
//! results back, and goes round again until the model stops asking for
//! anything — which is the whole shape of a coding agent, and the only part of
//! it that has to be identical in the terminal and in VS Code.
//!
//! No rendering. It emits events; a client decides what a tool call looks like
//! on screen. That rule is what keeps the two clients from disagreeing about
//! what the agent did.

use serde_json::json;

use crate::agent::{ToolCall as Tool, Toolbox};
use crate::api::{ApiMessage, Client, InferenceRequest, StreamEvent, ToolCall, ToolSchema, Usage};
use crate::error::{Error, Result};

/// How many model→tools→model round trips one prompt may take.
///
/// A loop with no ceiling is a loop that can spend a subscription on a single
/// prompt, and models do get stuck retrying the same failing command. Twenty
/// is past anything a human-scale task needs and well short of a runaway.
const MAX_STEPS: usize = 20;

/// Per-turn output ceiling. Generous for an explanation plus a file, and a
/// bound on what one runaway step can cost.
const MAX_TOKENS: u32 = 8192;

/// What the client is told, as it happens.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A piece of the answer. Arrives token by token.
    Text(String),
    /// The model asked for a tool. Emitted before it runs, so a slow command
    /// doesn't look like a hang.
    ToolStarted { name: String, detail: String },
    ToolFinished {
        name: String,
        truncated: bool,
        files_touched: Vec<String>,
    },
    /// Refused locally — outside the workspace, or a command the policy
    /// doesn't allow. The model is told too, so it can try something else.
    ToolRefused { name: String, reason: String },
    /// One model in the chain failed and another is being tried.
    ModelSwitched { from: String, to: String },
}

/// What a finished turn leaves behind.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub text: String,
    pub files_touched: Vec<String>,
    pub steps: usize,
    pub tokens: Usage,
    /// True when the loop hit MAX_STEPS rather than the model finishing. The
    /// caller should say so — an answer cut off at a ceiling reads like a
    /// complete one otherwise.
    pub hit_step_limit: bool,
}

/// The tools the model is offered. Descriptions are part of the contract: a
/// model that misuses a tool is usually a model that was told the wrong thing
/// about it.
pub fn tool_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "read_file".into(),
            description: "Read a file from the workspace. Paths are relative to the workspace root; anything outside it is refused.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
            }),
        },
        ToolSchema {
            name: "write_file".into(),
            description: "Write a file in the workspace, creating or replacing it. Send the file's full new contents.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                },
                "required": ["path", "content"],
            }),
        },
        ToolSchema {
            name: "search".into(),
            description: "Search the workspace for a string. Returns matching lines with their files.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"],
            }),
        },
        ToolSchema {
            name: "run_command".into(),
            description: "Run a shell command in the workspace. Only commands the user has allowed will run; anything else is refused and you should carry on without it.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "command": { "type": "string" } },
                "required": ["command"],
            }),
        },
    ]
}

/// Turns the model's `{name, input}` into the tool enum the toolbox executes.
///
/// A malformed call is an error the *model* is told about rather than one that
/// stops the turn: models routinely send a nearly-right argument, and the
/// recovery is to say so and let it try again.
fn to_tool(call: &ToolCall) -> std::result::Result<Tool, String> {
    let field = |key: &str| -> std::result::Result<String, String> {
        call.input
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("`{}` needs a string `{key}`", call.name))
    };
    match call.name.as_str() {
        "read_file" => Ok(Tool::ReadFile {
            path: field("path")?,
        }),
        "write_file" => Ok(Tool::WriteFile {
            path: field("path")?,
            content: field("content")?,
        }),
        "search" => Ok(Tool::Search {
            query: field("query")?,
        }),
        "run_command" => Ok(Tool::RunCommand {
            command: field("command")?,
        }),
        other => Err(format!("no such tool: {other}")),
    }
}

/// A one-line description of what a call is about to do, for the client to
/// print. The path or the command, not the whole file being written.
fn describe(tool: &Tool) -> String {
    match tool {
        Tool::ReadFile { path } => path.clone(),
        Tool::WriteFile { path, content } => format!("{path} ({} bytes)", content.len()),
        Tool::Search { query } => query.clone(),
        Tool::RunCommand { command } => command.clone(),
    }
}

/// Everything one prompt needs.
///
/// A struct rather than eight parameters: at that count the call site is a row
/// of bare `&str`s where swapping two of them compiles and produces a session
/// that reports the wrong model.
pub struct Turn<'a> {
    pub client: &'a Client,
    pub toolbox: &'a Toolbox,
    /// Logging and billing only — the server keeps no state between calls.
    pub session_id: &'a str,
    pub model: &'a str,
    pub system: &'a str,
    /// The conversation so far. The prompt is appended to it.
    pub history: Vec<ApiMessage>,
    pub prompt: &'a str,
}

impl Turn<'_> {
    /// Runs the prompt to completion.
    ///
    /// The returned Outcome carries the final text, so a caller that streamed
    /// it doesn't have to reassemble it from the events.
    pub async fn run<F>(self, mut on_event: F) -> Result<Outcome>
    where
        F: FnMut(AgentEvent),
    {
        let Turn {
            client,
            toolbox,
            session_id,
            model,
            system,
            history,
            prompt,
        } = self;

        let mut messages = Vec::with_capacity(history.len() + 2);
        messages.push(ApiMessage::text("system", system));
        messages.extend(history);
        messages.push(ApiMessage::text("user", prompt));

        let mut outcome = Outcome::default();

        for step in 0..MAX_STEPS {
            outcome.steps = step + 1;

            let request = InferenceRequest {
                session_id: session_id.to_string(),
                model: model.to_string(),
                messages: messages.clone(),
                tools: tool_schemas(),
                max_tokens: MAX_TOKENS,
                stream: true,
            };

            // Collected as the stream arrives. The text is emitted immediately and
            // kept, because the model's own turn has to go back into `messages`.
            let mut turn_text = String::new();
            let mut calls: Vec<ToolCall> = Vec::new();
            let mut failure: Option<Error> = None;

            client
                .inference_stream(&request, |event| match event {
                    StreamEvent::TextDelta { delta } => {
                        turn_text.push_str(&delta);
                        on_event(AgentEvent::Text(delta));
                    }
                    StreamEvent::ToolCall(call) => calls.push(call),
                    StreamEvent::Done { tokens_used, .. } => {
                        outcome.tokens.input += tokens_used.input;
                        outcome.tokens.output += tokens_used.output;
                    }
                    StreamEvent::Error {
                        provider_status,
                        message,
                        ..
                    } => {
                        // Mid-stream failure: there is no HTTP status left to fail
                        // on, so the one the provider reported is carried through
                        // and treated exactly like a status would have been.
                        failure = Some(Error::Upstream {
                            status: provider_status,
                            message,
                        });
                    }
                    StreamEvent::Unknown => {}
                })
                .await?;

            if let Some(error) = failure {
                return Err(error);
            }

            outcome.text = turn_text.clone();

            // No tools asked for: the model is finished talking.
            if calls.is_empty() {
                return Ok(outcome);
            }

            messages.push(ApiMessage::assistant_tool_calls(&turn_text, &calls));

            for call in &calls {
                let tool = match to_tool(call) {
                    Ok(tool) => tool,
                    Err(reason) => {
                        on_event(AgentEvent::ToolRefused {
                            name: call.name.clone(),
                            reason: reason.clone(),
                        });
                        messages.push(ApiMessage::tool_result(&call.id, reason));
                        continue;
                    }
                };

                on_event(AgentEvent::ToolStarted {
                    name: call.name.clone(),
                    detail: describe(&tool),
                });

                // Tools shell out and touch the disk. Doing that on the async
                // runtime's thread stalls every other task in the process,
                // including the stream of the next step.
                let result = tokio::task::block_in_place(|| toolbox.execute(&tool));

                match result {
                    Ok(output) => {
                        for path in &output.files_touched {
                            if !outcome.files_touched.contains(path) {
                                outcome.files_touched.push(path.clone());
                            }
                        }
                        on_event(AgentEvent::ToolFinished {
                            name: call.name.clone(),
                            truncated: output.truncated,
                            files_touched: output.files_touched.clone(),
                        });
                        let body = if output.truncated {
                            format!("{}\n\n[output truncated]", output.content)
                        } else {
                            output.content
                        };
                        messages.push(ApiMessage::tool_result(&call.id, body));
                    }
                    Err(error) => {
                        // A refused path or a disallowed command is not a crash —
                        // it is information the model can act on. Telling it beats
                        // ending the turn, which would leave the user with half a
                        // task and no explanation.
                        let reason = error.to_string();
                        on_event(AgentEvent::ToolRefused {
                            name: call.name.clone(),
                            reason: reason.clone(),
                        });
                        messages.push(ApiMessage::tool_result(&call.id, reason));
                    }
                }
            }
        }

        outcome.hit_step_limit = true;
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_advertised_tool_can_be_built_from_its_own_schema() {
        // The schemas and the parser are two descriptions of one contract, and
        // nothing else checks that they agree.
        for schema in tool_schemas() {
            let props = schema.input_schema.get("properties").unwrap();
            let input = serde_json::Value::Object(
                props
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(|k| (k.clone(), json!("x")))
                    .collect(),
            );
            let call = ToolCall {
                id: "1".into(),
                name: schema.name.clone(),
                input,
            };
            assert!(
                to_tool(&call).is_ok(),
                "{} is advertised but can't be parsed",
                schema.name
            );
        }
    }

    #[test]
    fn a_missing_argument_becomes_a_message_for_the_model() {
        // Not an error that ends the turn: models send nearly-right arguments
        // constantly, and the recovery is to say so.
        let call = ToolCall {
            id: "1".into(),
            name: "read_file".into(),
            input: json!({}),
        };
        let err = to_tool(&call).unwrap_err();
        assert!(
            err.contains("path"),
            "the message must name the field: {err}"
        );
    }

    #[test]
    fn an_unknown_tool_is_reported_rather_than_guessed_at() {
        let call = ToolCall {
            id: "1".into(),
            name: "rm_rf".into(),
            input: json!({}),
        };
        assert!(to_tool(&call).unwrap_err().contains("rm_rf"));
    }
}
