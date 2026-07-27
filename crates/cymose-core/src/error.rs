use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no workspace is open")]
    NoWorkspace,

    #[error("session {0} not found")]
    SessionNotFound(String),

    #[error("not authenticated — run `cymose login`")]
    NotAuthenticated,

    #[error("the session store is locked by another Cymose client")]
    StoreLocked,

    #[error("{0} is not implemented in this build")]
    NotImplemented(&'static str),

    /// One attempt failed upstream. Carries the status the provider returned
    /// (the server passes it through rather than normalising it) because that
    /// is what the router decides on — see [`crate::router::Router::decide`].
    #[error("upstream returned {status}: {message}")]
    Upstream { status: u16, message: String },

    /// Every model in the chain was exhausted. Carries what each one said, so
    /// the user sees "rate limited, rate limited, 500" rather than a bare
    /// failure at the end of a long run.
    #[error("every model in the chain failed: {0}")]
    ChainExhausted(String),

    /// The server refused in a way retrying cannot fix (out of allowance,
    /// region). The message is the server's own and is shown verbatim.
    #[error("{0}")]
    Refused(String),

    #[error("path {0} is outside the workspace")]
    PathEscapesWorkspace(PathBuf),

    #[error("store: {0}")]
    Store(#[from] rusqlite::Error),

    #[error("api: {0}")]
    Http(#[from] reqwest::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("config: {0}")]
    Config(String),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl Error {
    /// JSON-RPC error code for the sidecar. Codes below -32000 are reserved by
    /// the JSON-RPC spec; ours start there. Keep in sync with
    /// docs/sidecar-protocol.md.
    pub fn rpc_code(&self) -> i32 {
        match self {
            Error::NoWorkspace => -32000,
            Error::SessionNotFound(_) => -32001,
            Error::NotAuthenticated => -32002,
            Error::StoreLocked => -32003,
            Error::NotImplemented(_) => -32004,
            _ => -32099,
        }
    }
}
