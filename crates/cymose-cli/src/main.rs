//! The `cymose` binary: terminal client, scripting CLI, and the sidecar the
//! VS Code extension drives.
//!
//! Everything here is presentation and process plumbing. Behaviour lives in
//! `cymose-core`, so the extension gets the same answers this does.

mod sidecar;
mod tui;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use cymose_core::context::ContextBuilder;
use cymose_core::{Config, Store};

#[derive(Parser)]
#[command(
    name = "cymose",
    version,
    about = "Cymose Code — sessions as a graph, models as a chain"
)]
struct Cli {
    /// Session store to use. Defaults to the shared one, which is what lets a
    /// session started here resume in VS Code.
    #[arg(long, global = true)]
    store: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Link the current directory to a workspace
    Init,
    /// Start a session
    New {
        title: String,
        /// Inherit from a specific session
        #[arg(long = "from")]
        parent: Option<String>,
    },
    /// Show what a session inherits, and continue it
    Resume { id: String },
    /// Print the session tree
    List,
    /// Compare two sessions
    Diff { a: String, b: String },
    /// Send a session's outcome to Cymose Web
    Promote { id: String },
    /// Serve JSON-RPC over stdio (used by the VS Code extension)
    Sidecar,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let store_path = match cli.store {
        Some(path) => path,
        None => Store::default_path()?,
    };

    match cli.command {
        // The sidecar owns stdio, so it opens the store itself and never
        // prints anything that isn't a protocol message.
        Some(Command::Sidecar) => sidecar::serve(&store_path),
        Some(command) => run(command, &store_path).await,
        // Bare `cymose` is the TUI — the common case shouldn't need a verb.
        None => {
            let store = Store::open(&store_path)?;
            let root = std::env::current_dir()?;
            let workspace = store
                .workspace_for_path(&root)?
                .context("this directory is not linked to a workspace — run `cymose init`")?;
            tui::run(store, workspace)
        }
    }
}

async fn run(command: Command, store_path: &std::path::Path) -> Result<()> {
    let store = Store::open(store_path)?;
    let root = std::env::current_dir()?;

    match command {
        Command::Init => {
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "workspace".into());
            let id = store.ensure_workspace(&root, &name)?;
            println!("workspace {name} ({id})");
            println!("store: {}", store_path.display());
        }

        Command::New { title, parent } => {
            let workspace = workspace(&store, &root)?;
            let session = store.create_session(&workspace, &title, parent.as_deref())?;
            println!("session {} — {}", session.id, session.title);

            let inherited = ContextBuilder::new(&store).build(&session.id)?;
            if inherited.is_empty() {
                println!("(nothing inherited — this is a root session)");
            } else {
                println!("\ninherits {} session(s):", inherited.items.len());
                for item in &inherited.items {
                    println!("  [{}] {}", item.outcome.as_str(), item.title);
                }
                if inherited.dropped > 0 {
                    println!(
                        "  ({} older session(s) dropped for context budget)",
                        inherited.dropped
                    );
                }
            }
        }

        Command::Resume { id } => {
            let session = store.session(&id)?;
            println!(
                "{} — {} [{}]",
                session.id,
                session.title,
                session.status.as_str()
            );
            let inherited = ContextBuilder::new(&store).build(&session.id)?;
            print!("{}", inherited.render());
            // Turns need the inference route, which the API doesn't serve yet.
            // Saying so beats a spinner that never resolves.
            println!("\nagent turns are not wired up yet — see docs/api-contract.md");
        }

        Command::List => {
            let workspace = workspace(&store, &root)?;
            let nodes = store.tree(&workspace)?;
            if nodes.is_empty() {
                println!("no sessions yet — `cymose new \"...\"`");
            }
            print_tree(&nodes, None, 0);
        }

        Command::Diff { a, b } => {
            store.session(&a)?;
            store.session(&b)?;
            anyhow::bail!("diff needs session artifacts, which the agent loop does not record yet");
        }

        Command::Promote { id } => {
            let session = store.session(&id)?;
            let config = Config::load(Some(&root))?;
            anyhow::bail!(
                "promote of {} would post to {}/v1/promote, which needs `cymose login` (not implemented yet)",
                session.id,
                config.api.base_url
            );
        }

        Command::Sidecar => unreachable!("handled in main"),
    }
    Ok(())
}

fn workspace(store: &Store, root: &std::path::Path) -> Result<String> {
    store
        .workspace_for_path(root)?
        .context("this directory is not linked to a workspace — run `cymose init`")
}

/// Roots first, then their children beneath them. The tree is small enough
/// that a recursive filter beats building an index.
fn print_tree(nodes: &[cymose_core::session::TreeNode], parent: Option<&str>, depth: usize) {
    for node in nodes.iter().filter(|n| n.parent_id.as_deref() == parent) {
        let mark = match node.status {
            cymose_core::SessionStatus::Done => "✓",
            cymose_core::SessionStatus::Failed => "✗",
            cymose_core::SessionStatus::Running => "⟳",
            cymose_core::SessionStatus::Pending => "·",
        };
        let short = &node.id[..node.id.len().min(8)];
        println!(
            "{:indent$}{mark} {} — {short}",
            "",
            node.title,
            indent = depth * 2
        );
        if let Some(summary) = &node.summary {
            println!("{:indent$}  → {summary}", "", indent = depth * 2);
        }
        print_tree(nodes, Some(&node.id), depth + 1);
    }
}
