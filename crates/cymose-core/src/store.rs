//! SQLite session store — the contract between the two clients.
//!
//! One file per machine holds every workspace, so a session created in the
//! terminal is a row the extension can already see. The schema therefore
//! changes only through [`MIGRATIONS`], which is append-only: a shipped
//! migration is never edited, because a user with an existing store will not
//! re-run it and the clients would then disagree about the schema.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::session::{Message, Role, Session, SessionStatus, Summary, TreeNode};

/// Append only. Index in this list is the schema version.
const MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE workspaces (
        id          TEXT PRIMARY KEY,
        root_path   TEXT NOT NULL UNIQUE,
        name        TEXT NOT NULL,
        web_node_id TEXT,
        created_at  TEXT NOT NULL
    );

    CREATE TABLE sessions (
        id           TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
        parent_id    TEXT REFERENCES sessions(id) ON DELETE SET NULL,
        title        TEXT NOT NULL,
        status       TEXT NOT NULL,
        model        TEXT,
        web_node_id  TEXT,
        created_at   TEXT NOT NULL,
        ended_at     TEXT
    );
    CREATE INDEX idx_sessions_workspace ON sessions(workspace_id);
    CREATE INDEX idx_sessions_parent    ON sessions(parent_id);

    CREATE TABLE messages (
        id         TEXT PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        role       TEXT NOT NULL,
        content    TEXT NOT NULL,
        tokens_in  INTEGER NOT NULL DEFAULT 0,
        tokens_out INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL
    );
    CREATE INDEX idx_messages_session ON messages(session_id);

    CREATE TABLE summaries (
        session_id    TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
        text          TEXT NOT NULL,
        files_touched TEXT NOT NULL,
        outcome       TEXT NOT NULL,
        created_at    TEXT NOT NULL
    );

    CREATE TABLE artifacts (
        id         TEXT PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        path       TEXT NOT NULL,
        before     TEXT,
        after      TEXT,
        created_at TEXT NOT NULL
    );
    CREATE INDEX idx_artifacts_session ON artifacts(session_id);
    "#];

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Default location: `$XDG_DATA_HOME/cymose/sessions.db`, falling back to
    /// `~/.local/share`, and `%APPDATA%\cymose` on Windows.
    pub fn default_path() -> Result<PathBuf> {
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        };
        let base = base.ok_or_else(|| Error::Config("no home directory".into()))?;
        Ok(base.join("cymose").join("sessions.db"))
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        Self::configure(&conn)?;
        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    fn configure(conn: &Connection) -> Result<()> {
        // WAL so a terminal and an editor can read while the other writes.
        // query_row rather than pragma_update: setting journal_mode returns a
        // row, which pragma_update treats as an error.
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        // A turn streams for a while, but writes are per chunk-flush, so a few
        // seconds is generous. Past it the caller gets StoreLocked and can say
        // something useful instead of hanging.
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute("PRAGMA foreign_keys=ON", [])?;
        Ok(())
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)",
            [],
        )?;
        let current: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |r| r.get(0),
        )?;

        for (i, sql) in MIGRATIONS.iter().enumerate() {
            let version = i as i64 + 1;
            if version <= current {
                continue;
            }
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![version],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    // ---- workspaces -------------------------------------------------------

    /// Links a directory to a workspace, reusing the existing one if the path
    /// is already known — `cymose init` in an initialised directory is a
    /// no-op, not a second workspace over the same files.
    pub fn ensure_workspace(&self, root: &Path, name: &str) -> Result<String> {
        let root = root.to_string_lossy().to_string();
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM workspaces WHERE root_path = ?1",
                params![root],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO workspaces (id, root_path, name, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, root, name, now()],
        )?;
        Ok(id)
    }

    pub fn workspace_for_path(&self, root: &Path) -> Result<Option<String>> {
        let root = root.to_string_lossy().to_string();
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM workspaces WHERE root_path = ?1",
                params![root],
                |r| r.get(0),
            )
            .optional()?)
    }

    // ---- sessions ---------------------------------------------------------

    pub fn create_session(
        &self,
        workspace_id: &str,
        title: &str,
        parent_id: Option<&str>,
    ) -> Result<Session> {
        // Resolve before inserting, for two reasons. A session whose parent
        // doesn't exist would silently inherit nothing, which looks like a bad
        // summariser rather than a bad id. And `parent_id` may be a short id
        // pasted out of a listing — storing that verbatim writes a foreign key
        // that points at nothing, which the database catches and the user
        // cannot act on.
        let parent_id = match parent_id {
            Some(parent) => Some(self.session(parent)?.id),
            None => None,
        };
        let session = Session {
            id: Uuid::new_v4().to_string(),
            workspace_id: workspace_id.to_string(),
            parent_id,
            title: title.to_string(),
            status: SessionStatus::Pending,
            model: None,
            web_node_id: None,
            created_at: Utc::now(),
            ended_at: None,
        };
        self.conn.execute(
            "INSERT INTO sessions (id, workspace_id, parent_id, title, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session.id,
                session.workspace_id,
                session.parent_id,
                session.title,
                session.status.as_str(),
                session.created_at.to_rfc3339(),
            ],
        )?;
        Ok(session)
    }

    /// A session by id, or by any unambiguous prefix of one.
    ///
    /// Prefixes because every list this program prints shows a short id — a
    /// full uuid four times over is a wall, not a list — and an id you cannot
    /// paste back into the next command is a decoration. Git set this
    /// expectation and everyone has it.
    ///
    /// Exact match is tried first, so a full id never pays for the scan and can
    /// never be ambiguous. A prefix matching two sessions is an error rather
    /// than a guess: picking one would silently branch from the wrong place,
    /// and the whole point of this tool is that branches inherit.
    pub fn session(&self, id: &str) -> Result<Session> {
        if let Some(session) = self
            .conn
            .query_row(
                "SELECT id, workspace_id, parent_id, title, status, model, web_node_id, created_at, ended_at
                 FROM sessions WHERE id = ?1",
                params![id],
                row_to_session,
            )
            .optional()?
        {
            return Ok(session);
        }

        // Two rows are enough to know it's ambiguous; no reason to read more.
        let mut stmt = self.conn.prepare(
            "SELECT id, workspace_id, parent_id, title, status, model, web_node_id, created_at, ended_at
             FROM sessions WHERE id LIKE ?1 || '%' LIMIT 2",
        )?;
        let mut matches = stmt
            .query_map(params![id], row_to_session)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        match matches.len() {
            1 => Ok(matches.remove(0)),
            0 => Err(Error::SessionNotFound(id.to_string())),
            _ => Err(Error::AmbiguousSession(id.to_string())),
        }
    }

    pub fn set_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        let ended = status.is_terminal().then(|| Utc::now().to_rfc3339());
        let changed = self.conn.execute(
            "UPDATE sessions SET status = ?2, ended_at = COALESCE(?3, ended_at) WHERE id = ?1",
            params![id, status.as_str(), ended],
        )?;
        if changed == 0 {
            return Err(Error::SessionNotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn set_model(&self, id: &str, model: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET model = ?2 WHERE id = ?1",
            params![id, model],
        )?;
        Ok(())
    }

    /// The whole tree for a workspace, oldest first, with each node's summary
    /// text inlined so a client can draw the tree without a second round trip.
    pub fn tree(&self, workspace_id: &str) -> Result<Vec<TreeNode>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.parent_id, s.title, s.status, s.model, m.text, s.created_at
             FROM sessions s
             LEFT JOIN summaries m ON m.session_id = s.id
             WHERE s.workspace_id = ?1
             ORDER BY s.created_at ASC",
        )?;
        let rows = stmt.query_map(params![workspace_id], |r| {
            Ok(TreeNode {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                title: r.get(2)?,
                status: parse_status(r.get::<_, String>(3)?),
                model: r.get(4)?,
                summary: r.get(5)?,
                created_at: parse_time(r.get::<_, String>(6)?),
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Walks parent links from `id` upwards. The session itself is not
    /// included, and the walk stops at a cycle rather than looping — the schema
    /// shouldn't allow one, but a corrupted store is not worth hanging over.
    pub fn ancestors(&self, id: &str) -> Result<Vec<Session>> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut cursor = self.session(id)?.parent_id;
        while let Some(parent_id) = cursor {
            if !seen.insert(parent_id.clone()) {
                break;
            }
            let parent = self.session(&parent_id)?;
            cursor = parent.parent_id.clone();
            out.push(parent);
        }
        Ok(out)
    }

    pub fn children(&self, id: &str) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, workspace_id, parent_id, title, status, model, web_node_id, created_at, ended_at
             FROM sessions WHERE parent_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![id], row_to_session)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    // ---- messages ---------------------------------------------------------

    pub fn append_message(
        &self,
        session_id: &str,
        role: Role,
        content: &str,
        tokens_in: u32,
        tokens_out: u32,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO messages (id, session_id, role, content, tokens_in, tokens_out, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, session_id, role.as_str(), content, tokens_in, tokens_out, now()],
        )?;
        Ok(id)
    }

    pub fn messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, tokens_in, tokens_out, created_at
             FROM messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |r| {
            Ok(Message {
                id: r.get(0)?,
                session_id: r.get(1)?,
                role: Role::parse(&r.get::<_, String>(2)?).unwrap_or(Role::User),
                content: r.get(3)?,
                tokens_in: r.get(4)?,
                tokens_out: r.get(5)?,
                created_at: parse_time(r.get::<_, String>(6)?),
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    // ---- summaries --------------------------------------------------------

    pub fn put_summary(&self, summary: &Summary) -> Result<()> {
        self.conn.execute(
            "INSERT INTO summaries (session_id, text, files_touched, outcome, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(session_id) DO UPDATE SET
                text = excluded.text,
                files_touched = excluded.files_touched,
                outcome = excluded.outcome,
                created_at = excluded.created_at",
            params![
                summary.session_id,
                summary.text,
                serde_json::to_string(&summary.files_touched)?,
                summary.outcome.as_str(),
                summary.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn summary(&self, session_id: &str) -> Result<Option<Summary>> {
        Ok(self
            .conn
            .query_row(
                "SELECT session_id, text, files_touched, outcome, created_at
                 FROM summaries WHERE session_id = ?1",
                params![session_id],
                |r| {
                    Ok(Summary {
                        session_id: r.get(0)?,
                        text: r.get(1)?,
                        files_touched: serde_json::from_str(&r.get::<_, String>(2)?)
                            .unwrap_or_default(),
                        outcome: parse_status(r.get::<_, String>(3)?),
                        created_at: parse_time(r.get::<_, String>(4)?),
                    })
                },
            )
            .optional()?)
    }
}

fn row_to_session(r: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        parent_id: r.get(2)?,
        title: r.get(3)?,
        status: parse_status(r.get::<_, String>(4)?),
        model: r.get(5)?,
        web_node_id: r.get(6)?,
        created_at: parse_time(r.get::<_, String>(7)?),
        ended_at: r.get::<_, Option<String>>(8)?.map(parse_time),
    })
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

/// An unparsable status means the row was written by a newer client. Treat it
/// as pending rather than failing the whole tree read.
fn parse_status(s: String) -> SessionStatus {
    SessionStatus::parse(&s).unwrap_or(SessionStatus::Pending)
}

fn parse_time(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    #[test]
    fn workspace_is_reused_for_the_same_path() {
        let s = store();
        let a = s.ensure_workspace(Path::new("/tmp/proj"), "proj").unwrap();
        let b = s.ensure_workspace(Path::new("/tmp/proj"), "proj").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn ancestors_walk_from_child_to_root() {
        let s = store();
        let ws = s.ensure_workspace(Path::new("/tmp/proj"), "proj").unwrap();
        let root = s.create_session(&ws, "rate limiter", None).unwrap();
        let child = s
            .create_session(&ws, "token bucket", Some(&root.id))
            .unwrap();
        let grandchild = s.create_session(&ws, "v2", Some(&child.id)).unwrap();

        let chain: Vec<_> = s
            .ancestors(&grandchild.id)
            .unwrap()
            .into_iter()
            .map(|x| x.title)
            .collect();
        assert_eq!(chain, vec!["token bucket", "rate limiter"]);
    }

    #[test]
    fn creating_a_session_under_a_missing_parent_fails() {
        let s = store();
        let ws = s.ensure_workspace(Path::new("/tmp/proj"), "proj").unwrap();
        let err = s.create_session(&ws, "orphan", Some("nope")).unwrap_err();
        assert!(matches!(err, Error::SessionNotFound(_)));
    }

    #[test]
    fn a_short_id_from_a_listing_resolves_back_to_its_session() {
        // The bug this exists for: every listing prints an 8-character id, and
        // pasting one into `--from` used to say "not found".
        let s = Store::open_in_memory().unwrap();
        let ws = s.ensure_workspace(Path::new("/tmp/p"), "p").unwrap();
        let session = s.create_session(&ws, "rate limiting", None).unwrap();

        let short = &session.id[..8];
        assert_eq!(s.session(short).unwrap().id, session.id);
        // A full id still works, and takes the exact path.
        assert_eq!(s.session(&session.id).unwrap().id, session.id);
        assert!(matches!(
            s.session("zzzzzzzz").unwrap_err(),
            Error::SessionNotFound(_)
        ));
    }

    #[test]
    fn branching_from_a_short_id_stores_the_full_one() {
        // The follow-on bug: resolution succeeded and then the prefix itself
        // was written as parent_id, so the insert failed on the foreign key —
        // with a message about constraints that tells the user nothing.
        let s = Store::open_in_memory().unwrap();
        let ws = s.ensure_workspace(Path::new("/tmp/p"), "p").unwrap();
        let root = s.create_session(&ws, "rate limiting", None).unwrap();

        let child = s
            .create_session(&ws, "token bucket", Some(&root.id[..8]))
            .unwrap();
        assert_eq!(child.parent_id.as_deref(), Some(root.id.as_str()));
        // And the tree agrees, which is what the context builder walks.
        assert_eq!(s.ancestors(&child.id).unwrap().len(), 1);
    }

    #[test]
    fn an_ambiguous_prefix_is_an_error_rather_than_a_guess() {
        // Branching from the wrong parent inherits the wrong context, and the
        // user would have no reason to suspect it.
        let s = Store::open_in_memory().unwrap();
        let ws = s.ensure_workspace(Path::new("/tmp/p"), "p").unwrap();
        s.create_session(&ws, "a", None).unwrap();
        s.create_session(&ws, "b", None).unwrap();
        // Every uuid here starts with a hex digit, so the empty-ish prefix that
        // matches everything is the shared one: "".
        assert!(matches!(
            s.session("").unwrap_err(),
            Error::AmbiguousSession(_)
        ));
    }

    #[test]
    fn finishing_a_session_stamps_ended_at() {
        let s = store();
        let ws = s.ensure_workspace(Path::new("/tmp/proj"), "proj").unwrap();
        let session = s.create_session(&ws, "auth bug", None).unwrap();
        assert!(session.ended_at.is_none());

        s.set_status(&session.id, SessionStatus::Done).unwrap();
        let reloaded = s.session(&session.id).unwrap();
        assert_eq!(reloaded.status, SessionStatus::Done);
        assert!(reloaded.ended_at.is_some());
    }

    #[test]
    fn summaries_are_upserted_and_show_up_in_the_tree() {
        let s = store();
        let ws = s.ensure_workspace(Path::new("/tmp/proj"), "proj").unwrap();
        let session = s.create_session(&ws, "rate limiter", None).unwrap();

        for text in ["first pass", "sliding window works, merged"] {
            s.put_summary(&Summary {
                session_id: session.id.clone(),
                text: text.into(),
                files_touched: vec!["src/limiter.rs".into()],
                outcome: SessionStatus::Done,
                created_at: Utc::now(),
            })
            .unwrap();
        }

        let stored = s.summary(&session.id).unwrap().unwrap();
        assert_eq!(stored.text, "sliding window works, merged");
        assert_eq!(stored.files_touched, vec!["src/limiter.rs".to_string()]);

        let tree = s.tree(&ws).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(
            tree[0].summary.as_deref(),
            Some("sliding window works, merged")
        );
    }

    #[test]
    fn migrations_are_idempotent() {
        let dir = std::env::temp_dir().join(format!("cymose-test-{}", Uuid::new_v4()));
        let path = dir.join("sessions.db");
        let ws = {
            let s = Store::open(&path).unwrap();
            s.ensure_workspace(Path::new("/tmp/proj"), "proj").unwrap()
        };
        let reopened = Store::open(&path).unwrap();
        assert_eq!(
            reopened.workspace_for_path(Path::new("/tmp/proj")).unwrap(),
            Some(ws)
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
