//! Model chain with failover.
//!
//! The router decides *which* model a turn asks for and what to do when the
//! answer is a rate limit. It knows nothing about which provider serves a
//! model name — that resolution happens server-side, and keeping it there is
//! what lets the chain be reordered without a client release.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// What kind of work a turn is, so cheap models can take the routine parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    /// The default: writing and editing code.
    Code,
    /// Design questions, where a stronger model earns its cost.
    Architect,
    /// End-of-session summarisation — high volume, low difficulty.
    Summarize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    /// Ordered. The first entry is what a turn asks for; the rest are what it
    /// falls back to, in order.
    pub chain: Vec<String>,
    /// Overrides the head of the chain for a kind of task. A pinned model that
    /// fails still falls back through the chain.
    #[serde(default)]
    pub pin: HashMap<TaskKind, String>,
}

impl Default for RouterConfig {
    fn default() -> Self {
        RouterConfig {
            chain: vec![
                "claude-sonnet".into(),
                "deepseek-coder".into(),
                "glm-4.5".into(),
                "qwen-coder".into(),
            ],
            pin: HashMap::new(),
        }
    }
}

/// What the router decides to do about a failed attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Try again on this model — a transient failure that isn't the model's
    /// fault.
    Retry,
    /// Move to the next model in the chain.
    Failover,
    /// Stop. The message is the server's and is shown as-is.
    Refuse(String),
    /// Stop, and the caller should re-authenticate first.
    Reauthenticate,
}

pub struct Router {
    config: RouterConfig,
}

impl Router {
    pub fn new(config: RouterConfig) -> Self {
        Router { config }
    }

    pub fn chain(&self) -> &[String] {
        &self.config.chain
    }

    /// The model pinned to this kind of task, if any. `None` means "no
    /// opinion" — which for a server-side default is the answer to send.
    pub fn pinned(&self, kind: TaskKind) -> Option<&str> {
        self.config.pin.get(&kind).map(String::as_str)
    }

    /// The model a turn of this kind starts on.
    pub fn head(&self, kind: TaskKind) -> Result<&str> {
        if let Some(pinned) = self.config.pin.get(&kind) {
            return Ok(pinned);
        }
        self.config
            .chain
            .first()
            .map(String::as_str)
            .ok_or_else(|| Error::Config("router chain is empty".into()))
    }

    /// The next model to try after `current` failed.
    ///
    /// A pinned model is not usually in the chain, so falling back from one
    /// starts at the top of the chain rather than nowhere.
    pub fn next_after(&self, current: &str) -> Option<&str> {
        match self.config.chain.iter().position(|m| m == current) {
            Some(i) => self.config.chain.get(i + 1).map(String::as_str),
            None => self.config.chain.first().map(String::as_str),
        }
    }

    /// Failover is driven by status code, not by matching on message text —
    /// provider wording changes without warning, status codes don't.
    pub fn decide(&self, status: u16, body: &str) -> Decision {
        match status {
            401 => Decision::Reauthenticate,
            // Out of allowance, or a region the service can't serve. Another
            // model won't change either answer.
            402 | 451 | 403 => Decision::Refuse(body.to_string()),
            408 | 409 | 425 => Decision::Retry,
            429 => Decision::Failover,
            s if s >= 500 => Decision::Failover,
            _ => Decision::Refuse(body.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn router() -> Router {
        Router::new(RouterConfig::default())
    }

    #[test]
    fn the_chain_is_walked_in_order() {
        let r = router();
        assert_eq!(r.head(TaskKind::Code).unwrap(), "claude-sonnet");
        assert_eq!(r.next_after("claude-sonnet"), Some("deepseek-coder"));
        assert_eq!(r.next_after("glm-4.5"), Some("qwen-coder"));
        assert_eq!(r.next_after("qwen-coder"), None);
    }

    #[test]
    fn a_pinned_model_falls_back_into_the_chain() {
        let mut config = RouterConfig::default();
        config
            .pin
            .insert(TaskKind::Summarize, "some-cheap-model".into());
        let r = Router::new(config);

        assert_eq!(r.head(TaskKind::Summarize).unwrap(), "some-cheap-model");
        assert_eq!(r.head(TaskKind::Code).unwrap(), "claude-sonnet");
        // Not in the chain: fall back to its head rather than giving up.
        assert_eq!(r.next_after("some-cheap-model"), Some("claude-sonnet"));
    }

    #[test]
    fn rate_limits_and_server_errors_fail_over() {
        let r = router();
        assert_eq!(r.decide(429, ""), Decision::Failover);
        assert_eq!(r.decide(500, ""), Decision::Failover);
        assert_eq!(r.decide(503, ""), Decision::Failover);
    }

    #[test]
    fn refusals_are_passed_through_verbatim() {
        let r = router();
        let msg = "You've used this month's allowance.";
        assert_eq!(r.decide(402, msg), Decision::Refuse(msg.into()));
        assert_eq!(
            r.decide(451, "Not available in your region."),
            Decision::Refuse("Not available in your region.".into())
        );
        assert_eq!(r.decide(401, ""), Decision::Reauthenticate);
    }

    #[test]
    fn an_empty_chain_is_a_config_error_not_a_panic() {
        let r = Router::new(RouterConfig {
            chain: vec![],
            pin: HashMap::new(),
        });
        assert!(r.head(TaskKind::Code).is_err());
    }
}
