//! The agent's tools: read, write, search, run.
//!
//! Two rules hold everywhere in this module. Every path is resolved against
//! the workspace root and rejected if it escapes — a model that has been told
//! about `../../.ssh` should get an error, not a file. And every output is
//! capped, because one `cat` of a vendored bundle otherwise costs a whole
//! context window.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Output cap per tool call, in bytes.
const MAX_OUTPUT: usize = 64 * 1024;
/// Cap on search hits, so a common word doesn't return the repository.
const MAX_HITS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    ReadFile { path: String },
    WriteFile { path: String, content: String },
    Search { query: String },
    RunCommand { command: String },
}

impl ToolCall {
    pub fn name(&self) -> &'static str {
        match self {
            ToolCall::ReadFile { .. } => "read_file",
            ToolCall::WriteFile { .. } => "write_file",
            ToolCall::Search { .. } => "search",
            ToolCall::RunCommand { .. } => "run_command",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub content: String,
    /// True when the cap cut something off. The model is told, so it can
    /// narrow the request instead of assuming it saw everything.
    pub truncated: bool,
    /// Files this call changed, for the session's summary and diff.
    #[serde(default)]
    pub files_touched: Vec<String>,
}

impl ToolOutput {
    fn text(content: String) -> Self {
        let (content, truncated) = cap(content);
        ToolOutput {
            content,
            truncated,
            files_touched: Vec::new(),
        }
    }
}

/// Which commands may run without asking the user.
///
/// The default asks about everything. An allowlist is the user's to widen, and
/// a broad one is a broad grant — `run_command` executes in their working
/// directory with their privileges.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandPolicy {
    /// Matched against the first word of the command, exactly. `cargo` allows
    /// `cargo test`; it does not allow `sudo cargo`. Empty by default — every
    /// command is asked about until the user says otherwise.
    #[serde(default)]
    pub allow: Vec<String>,
}

impl CommandPolicy {
    pub fn permits(&self, command: &str) -> bool {
        match command.split_whitespace().next() {
            Some(program) => self.allow.iter().any(|a| a == program),
            None => false,
        }
    }
}

pub struct Toolbox {
    root: PathBuf,
    policy: CommandPolicy,
}

impl Toolbox {
    pub fn new(root: impl Into<PathBuf>, policy: CommandPolicy) -> Self {
        Toolbox {
            root: root.into(),
            policy,
        }
    }

    pub fn execute(&self, call: &ToolCall) -> Result<ToolOutput> {
        match call {
            ToolCall::ReadFile { path } => self.read_file(path),
            ToolCall::WriteFile { path, content } => self.write_file(path, content),
            ToolCall::Search { query } => self.search(query),
            ToolCall::RunCommand { command } => self.run_command(command),
        }
    }

    /// Resolves a tool-supplied path inside the workspace.
    ///
    /// Lexical, not `canonicalize`: a write target doesn't exist yet, so
    /// canonicalising it would fail before it could be checked. Symlinks that
    /// point outside the workspace are therefore still reachable — an
    /// unresolved gap, tracked as such rather than papered over.
    fn resolve(&self, path: &str) -> Result<PathBuf> {
        let requested = Path::new(path);
        if requested.is_absolute() {
            return Err(Error::PathEscapesWorkspace(requested.to_path_buf()));
        }

        let mut out = PathBuf::new();
        for component in requested.components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        return Err(Error::PathEscapesWorkspace(requested.to_path_buf()));
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(Error::PathEscapesWorkspace(requested.to_path_buf()))
                }
            }
        }
        Ok(self.root.join(out))
    }

    fn read_file(&self, path: &str) -> Result<ToolOutput> {
        let full = self.resolve(path)?;
        Ok(ToolOutput::text(std::fs::read_to_string(full)?))
    }

    fn write_file(&self, path: &str, content: &str) -> Result<ToolOutput> {
        let full = self.resolve(path)?;
        if let Some(dir) = full.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&full, content)?;
        Ok(ToolOutput {
            content: format!("wrote {} bytes to {path}", content.len()),
            truncated: false,
            files_touched: vec![path.to_string()],
        })
    }

    /// Plain substring search over the workspace's text files. Deliberately
    /// simple: the code graph (v0.4) is what will make this smart, and a
    /// half-clever regex layer now would be thrown away then.
    fn search(&self, query: &str) -> Result<ToolOutput> {
        let mut hits = Vec::new();
        let mut truncated = false;
        self.walk(&self.root.clone(), query, &mut hits, &mut truncated)?;
        let mut out = ToolOutput::text(hits.join("\n"));
        out.truncated |= truncated;
        Ok(out)
    }

    fn walk(
        &self,
        dir: &Path,
        query: &str,
        hits: &mut Vec<String>,
        truncated: &mut bool,
    ) -> Result<()> {
        const SKIP: &[&str] = &[".git", "target", "node_modules", "dist", "out", ".cymose"];

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            // An unreadable directory is not worth failing a search over.
            Err(_) => return Ok(()),
        };
        for entry in entries.flatten() {
            if hits.len() >= MAX_HITS {
                *truncated = true;
                return Ok(());
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && name != ".env.example" || SKIP.contains(&name.as_str()) {
                continue;
            }
            if path.is_dir() {
                self.walk(&path, query, hits, truncated)?;
                continue;
            }
            // read_to_string fails on binaries, which is the filter we want.
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let rel = path
                .strip_prefix(&self.root)
                .unwrap_or(&path)
                .display()
                .to_string();
            for (i, line) in text.lines().enumerate() {
                if line.contains(query) {
                    hits.push(format!("{rel}:{}: {}", i + 1, line.trim()));
                    if hits.len() >= MAX_HITS {
                        *truncated = true;
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    /// Runs a shell command in the workspace root.
    ///
    /// A command the policy doesn't allow is refused here rather than queued:
    /// the core has no way to ask a question, so the client prompts the user
    /// and widens the policy if they agree.
    fn run_command(&self, command: &str) -> Result<ToolOutput> {
        if !self.policy.permits(command) {
            return Err(Error::Refused(format!(
                "`{command}` is not in the allowed-command list; approve it to run it"
            )));
        }

        let output = if cfg!(windows) {
            Command::new("cmd")
                .args(["/C", command])
                .current_dir(&self.root)
                .output()?
        } else {
            Command::new("sh")
                .args(["-c", command])
                .current_dir(&self.root)
                .output()?
        };

        // stdout and stderr both go to the model: a failing build says what is
        // wrong on stderr, and hiding it would make the next turn guess.
        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        let err = String::from_utf8_lossy(&output.stderr);
        if !err.is_empty() {
            text.push_str("\n--- stderr ---\n");
            text.push_str(&err);
        }
        text.push_str(&format!(
            "\n--- exit: {} ---",
            output.status.code().unwrap_or(-1)
        ));
        Ok(ToolOutput::text(text))
    }
}

fn cap(mut content: String) -> (String, bool) {
    if content.len() <= MAX_OUTPUT {
        return (content, false);
    }
    // Cut on a char boundary — a truncated multibyte sequence would panic.
    let mut cut = MAX_OUTPUT;
    while !content.is_char_boundary(cut) {
        cut -= 1;
    }
    content.truncate(cut);
    content.push_str("\n… truncated");
    (content, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("cymose-agent-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn toolbox(dir: &Temp, allow: &[&str]) -> Toolbox {
        Toolbox::new(
            dir.0.clone(),
            CommandPolicy {
                allow: allow.iter().map(|s| s.to_string()).collect(),
            },
        )
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = Temp::new();
        let tb = toolbox(&dir, &[]);
        let written = tb
            .execute(&ToolCall::WriteFile {
                path: "src/limiter.rs".into(),
                content: "fn try_acquire() {}".into(),
            })
            .unwrap();
        assert_eq!(written.files_touched, vec!["src/limiter.rs".to_string()]);

        let read = tb
            .execute(&ToolCall::ReadFile {
                path: "src/limiter.rs".into(),
            })
            .unwrap();
        assert_eq!(read.content, "fn try_acquire() {}");
    }

    #[test]
    fn paths_cannot_escape_the_workspace() {
        let dir = Temp::new();
        let tb = toolbox(&dir, &[]);
        for path in ["../secrets.env", "a/../../secrets.env", "/etc/passwd"] {
            let err = tb
                .execute(&ToolCall::ReadFile { path: path.into() })
                .unwrap_err();
            assert!(
                matches!(err, Error::PathEscapesWorkspace(_)),
                "{path} should have been rejected, got {err:?}"
            );
        }
        // Staying inside while using .. is fine.
        tb.execute(&ToolCall::WriteFile {
            path: "a/b/../c.txt".into(),
            content: "ok".into(),
        })
        .unwrap();
        assert!(dir.0.join("a/c.txt").exists());
    }

    #[test]
    fn commands_outside_the_policy_are_refused() {
        let dir = Temp::new();
        let tb = toolbox(&dir, &["echo"]);
        let err = tb
            .execute(&ToolCall::RunCommand {
                command: "rm -rf /".into(),
            })
            .unwrap_err();
        assert!(matches!(err, Error::Refused(_)));

        // An allowlisted program is not a licence for a different one.
        assert!(!tb.policy.permits("sudo echo hi"));
        assert!(tb.policy.permits("echo hi"));
    }

    #[test]
    #[cfg(unix)]
    fn an_allowed_command_reports_stdout_and_exit_code() {
        let dir = Temp::new();
        let tb = toolbox(&dir, &["echo"]);
        let out = tb
            .execute(&ToolCall::RunCommand {
                command: "echo hello".into(),
            })
            .unwrap();
        assert!(out.content.contains("hello"));
        assert!(out.content.contains("exit: 0"));
    }

    #[test]
    fn search_finds_matches_and_skips_build_output() {
        let dir = Temp::new();
        let tb = toolbox(&dir, &[]);
        tb.execute(&ToolCall::WriteFile {
            path: "src/limiter.rs".into(),
            content: "fn try_acquire() {}\nfn release() {}".into(),
        })
        .unwrap();
        std::fs::create_dir_all(dir.0.join("target")).unwrap();
        std::fs::write(dir.0.join("target/build.rs"), "fn try_acquire() {}").unwrap();

        let out = tb
            .execute(&ToolCall::Search {
                query: "try_acquire".into(),
            })
            .unwrap();
        assert!(out.content.contains("src/limiter.rs:1"));
        assert!(!out.content.contains("target/"));
    }

    #[test]
    fn output_is_capped_on_a_char_boundary() {
        let (content, truncated) = cap("é".repeat(MAX_OUTPUT));
        assert!(truncated);
        assert!(content.ends_with("… truncated"));
    }
}
