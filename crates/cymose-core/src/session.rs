use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A unit of work. Sessions form a tree: `parent_id` is the edge, and a child
/// inherits its ancestors' [`Summary`] — never their messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub workspace_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub status: SessionStatus,
    /// The model the last turn actually ran on, which is not necessarily the
    /// head of the chain — a fallback records itself here.
    pub model: Option<String>,
    /// The Cymose Web node this session was opened from, if any.
    pub web_node_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Pending,
    Running,
    Done,
    /// A failed session is kept, not deleted: "token bucket deadlocks under
    /// contention" is the most useful thing a sibling session can inherit.
    Failed,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionStatus::Pending => "pending",
            SessionStatus::Running => "running",
            SessionStatus::Done => "done",
            SessionStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(SessionStatus::Pending),
            "running" => Some(SessionStatus::Running),
            "done" => Some(SessionStatus::Done),
            "failed" => Some(SessionStatus::Failed),
            _ => None,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, SessionStatus::Done | SessionStatus::Failed)
    }
}

/// What a finished session hands to its children. This — not the transcript —
/// is the unit of inheritance, and the reason a deep tree stays inside a
/// reasonable context budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub session_id: String,
    /// A few sentences: what was attempted, what happened, what to avoid.
    pub text: String,
    pub files_touched: Vec<String>,
    pub outcome: SessionStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: Role,
    pub content: String,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "system" => Some(Role::System),
            "user" => Some(Role::User),
            "assistant" => Some(Role::Assistant),
            "tool" => Some(Role::Tool),
            _ => None,
        }
    }
}

/// One node of the tree as a client draws it: no message bodies, so the whole
/// tree is cheap to send over the sidecar on every refresh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub status: SessionStatus,
    pub model: Option<String>,
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
}
