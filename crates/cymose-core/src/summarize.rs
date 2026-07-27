//! Turns a finished session into the summary its children inherit.
//!
//! This is the pressure point of the whole design: a summary that loses the
//! reason an approach failed makes the next session repeat it, and the graph
//! stops being worth anything.
//!
//! The wording that gets that out of a model is the server's, not ours — the
//! client sends the transcript and the verdict and gets a structured
//! [`SessionSummary`] back. Keeping the prompt server-side means it can be
//! improved without shipping a new binary, and it keeps prompt text out of a
//! public repository.

use chrono::Utc;

use crate::api::{ApiMessage, Client, SummarizeRequest};
use crate::error::Result;
use crate::router::{Router, TaskKind};
use crate::session::{Message, Role, SessionStatus, Summary};
use crate::store::Store;

pub struct Summarizer<'a> {
    store: &'a Store,
    client: &'a Client,
    router: &'a Router,
}

impl<'a> Summarizer<'a> {
    pub fn new(store: &'a Store, client: &'a Client, router: &'a Router) -> Self {
        Summarizer {
            store,
            client,
            router,
        }
    }

    /// Summarises a session and stores the result.
    ///
    /// `outcome` is the caller's, not the model's: whether the work succeeded
    /// is a fact the client already knows, and letting a summariser infer it
    /// from a transcript is how a failed attempt gets inherited as a success.
    pub async fn run(&self, session_id: &str, outcome: SessionStatus) -> Result<Summary> {
        let session = self.store.session(session_id)?;
        let messages = self.store.messages(session_id)?;

        let summary = self
            .client
            .summarize(&SummarizeRequest {
                session_id: session_id.to_string(),
                task: session.title.clone(),
                transcript: messages
                    .iter()
                    .map(|m| ApiMessage::text(m.role.as_str(), m.content.clone()))
                    .collect(),
                outcome: outcome.as_str().to_string(),
                // A pin is a deliberate choice; no pin lets the server use its
                // cheap default, which is the point of summarising at all.
                model: self.router.pinned(TaskKind::Summarize).map(str::to_string),
            })
            .await?;

        let stored = Summary {
            session_id: session_id.to_string(),
            text: summary.render(),
            // The server reads file names out of the transcript; the client
            // watched the tools run. Neither is complete on its own — a write
            // the model never mentioned is only in the second, a file it
            // discussed but reached through a command is only in the first.
            files_touched: merge(summary.files_touched, files_touched(&messages)),
            outcome,
            created_at: Utc::now(),
        };
        self.store.put_summary(&stored)?;
        Ok(stored)
    }
}

/// Files the session wrote, taken from its tool output.
fn files_touched(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .filter(|m| m.role == Role::Tool)
        .filter_map(|m| serde_json::from_str::<serde_json::Value>(&m.content).ok())
        .filter_map(|v| {
            v.get("files_touched").and_then(|f| f.as_array()).map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
        })
        .flatten()
        .collect()
}

fn merge(a: Vec<String>, b: Vec<String>) -> Vec<String> {
    let mut all: Vec<String> = a.into_iter().chain(b).collect();
    all.sort();
    all.dedup();
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn tool_message(content: &str) -> Message {
        Message {
            id: "m".into(),
            session_id: "s".into(),
            role: Role::Tool,
            content: content.into(),
            tokens_in: 0,
            tokens_out: 0,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn touched_files_are_collected_from_tool_output() {
        let messages = vec![
            tool_message(r#"{"files_touched":["src/limiter.rs"]}"#),
            tool_message(r#"{"files_touched":["src/lib.rs"]}"#),
            tool_message("not json at all"),
        ];
        assert_eq!(
            files_touched(&messages),
            vec!["src/limiter.rs", "src/lib.rs"]
        );
    }

    #[test]
    fn a_session_that_wrote_nothing_reports_no_files() {
        assert!(files_touched(&[]).is_empty());
    }

    #[test]
    fn the_two_file_lists_are_unioned_not_chosen_between() {
        let from_server = vec!["src/limiter.rs".to_string(), "tests/limiter.rs".to_string()];
        let from_tools = vec!["src/limiter.rs".to_string(), "src/lib.rs".to_string()];
        assert_eq!(
            merge(from_server, from_tools),
            vec!["src/lib.rs", "src/limiter.rs", "tests/limiter.rs"]
        );
    }
}
