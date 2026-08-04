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
use cymose_core::api::{Account, Backend, Client};
use cymose_core::context::ContextBuilder;
use cymose_core::{Config, Credentials, Store};

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
    /// Sign in to Cymose
    Login {
        /// Paste a token instead of being prompted (for scripts and CI).
        #[arg(long)]
        token: Option<String>,
    },
    /// Forget the stored token
    Logout,
    /// Show who you're signed in as and what's left
    Whoami,
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
    /// Read the tree you planned in Cymose Web
    Sync,
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
            let root = std::env::current_dir()?;
            // Before the terminal goes into raw mode: an error printed from
            // inside the alternate screen is unreadable, and "you need a plan"
            // is exactly the message that must not arrive that way.
            let (account_client, account) = authenticated(&root).await?;
            // The plan gate has passed; now decide whose credit the turns come
            // out of. See `turn_client`.
            let client = turn_client(account_client);
            let store = Store::open(&store_path)?;
            let workspace = store
                .workspace_for_path(&root)?
                .context("this directory is not linked to a workspace — run `cymose init`")?;

            // One session per launch. The transcript in front of you is a
            // node in the graph like any other, so a branch opened from it
            // later inherits what happened here.
            let session = store.create_session(&workspace, "terminal session", None)?;
            let config = Config::load(Some(&root))?;
            let router = cymose_core::router::Router::new(config.router.clone());
            let model = router
                .head(cymose_core::router::TaskKind::Code)?
                .to_string();

            tui::run(tui::Context {
                store,
                workspace,
                session_id: session.id,
                account,
                client,
                toolbox: cymose_core::agent::Toolbox::new(root.clone(), config.agent.clone()),
                model,
                runtime: tokio::runtime::Handle::current(),
            })
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
            authenticated(&root).await?;
            let workspace = workspace(&store, &root)?;
            let session = store.create_session(&workspace, &title, parent.as_deref())?;
            println!("session {} — {}", session.id, session.title);

            let inherited = ContextBuilder::new(&store).build(&session.id)?;
            if inherited.is_empty() {
                // Two different situations, and calling both "a root session"
                // tells someone who just branched that their branch didn't
                // take. It did — its parent simply hasn't been summarised yet,
                // because nothing has run in it.
                if session.parent_id.is_some() {
                    println!("(nothing to inherit yet — the parent has no summary until a session there finishes)");
                } else {
                    println!("(nothing inherited — this is a root session)");
                }
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
            authenticated(&root).await?;
            let session = store.session(&id)?;
            println!(
                "{} — {} [{}]",
                session.id,
                session.title,
                session.status.as_str()
            );
            let inherited = ContextBuilder::new(&store).build(&session.id)?;
            print!("{}", inherited.render());
            // `resume` prints what a session inherits; it does not open a
            // transcript. Turns happen in the TUI, so say where to go rather
            // than leaving the reader waiting for a prompt that never comes.
            println!("\nRun `cymose` in this directory to continue the session.");
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
            // Sign-in is done and `Client::promote` exists; what is missing is
            // on the other side. `/v1/promote` promotes a *web* node: it takes
            // a workspace id and reads that workspace's messages back out of
            // the server. A Code session has neither, so there is nothing for
            // it to post yet — the route has to learn to accept an outcome
            // before this can be wired.
            anyhow::bail!(
                "promote of {} has nowhere to go yet: {}/v1/promote promotes a Cymose Web node, \
                 and a local session isn't one. Sending a session's outcome up is the next \
                 milestone — see docs/api-contract.md.",
                session.id,
                config.api.base_url
            );
        }

        Command::Login { token } => {
            let config = Config::load(Some(&root))?;
            let token = match token {
                Some(token) => token,
                None => prompt_for_token(&config)?,
            };
            let client = client_with(&config, token.clone());
            // Verify before storing. A token saved without a check is a token
            // that fails later, somewhere less convenient to explain.
            let account = client.account().await?;
            let path = Credentials::save(&token)?;
            println!("signed in — {} plan", account.plan_label());
            println!("token stored in {}", path.display());
            if !account.may_use_code() {
                println!();
                println!("{}", upgrade_notice(&account));
            }
        }

        Command::Logout => {
            Credentials::clear()?;
            println!("signed out.");
        }

        Command::Whoami => {
            let (_, account) = authenticated(&root).await?;
            println!("{} plan", account.plan_label());
            println!(
                "standard credits: {} / {}",
                account.standard_used, account.standard_cap
            );
            println!(
                "premium credits:  {} / {}",
                account.premium_used,
                account.premium_cap + account.premium_extra
            );
            if !account.may_use_code() {
                println!();
                println!("{}", upgrade_notice(&account));
            }
        }

        Command::Sync => {
            // Read-only. Pull before push: nothing here writes back, so there
            // is no conflict resolution to get wrong, and the direction that
            // hurts every day — planning in the browser and rebuilding the
            // context here by hand — is the one this fixes.
            let (client, _) = authenticated(&root).await?;
            let tree = client.sync_tree().await?;
            if tree.nodes.is_empty() {
                println!("nothing on the web yet.");
            } else {
                print_web_tree(&tree, None, 0);
                println!(
                    "\n{} node(s). Read-only — nothing here is written back.",
                    tree.nodes.len()
                );
            }
        }

        Command::Sidecar => unreachable!("handled in main"),
    }
    Ok(())
}

/// A client pointed at the API with a token in hand.
fn client_with(config: &Config, token: String) -> Client {
    Client::new(Backend::Cymose {
        base_url: config.api.base_url.clone(),
        token,
        // Device identity exists for the per-device rate limits on the metered
        // routes. One value for every CLI install is deliberate for now: the
        // account behind the bearer token is what those limits key on for a
        // paid plan, and inventing a per-machine id here would be a second
        // identity to keep stable across reinstalls for no gain today.
        device_id: "cymose-cli".into(),
    })
}

/// Which client actually runs the turns.
///
/// The plan is the licence to use the client; the key decides whose credit the
/// tokens come out of. With `OPENROUTER_API_KEY` set, turns go straight to
/// OpenRouter on the user's own account and nothing of ours is in the path —
/// which is also why the summariser has no server prompt to call in that mode
/// (see docs/spec.md §8). Without it, they go through the Cymose backend on
/// the account that has already been checked.
fn turn_client(account_client: Client) -> Client {
    match std::env::var("OPENROUTER_API_KEY") {
        Ok(key) if !key.trim().is_empty() => Client::openrouter(key.trim()),
        _ => account_client,
    }
}

/// Sign-in plus the plan gate, in the order the user experiences them.
///
/// Cymose Code runs on an account with an active plan. That is a reversal of
/// 0.1's BYOK-only stance and it is deliberate: an agent turn costs an order
/// of magnitude more than a chat turn, so the thing funding it has to be a
/// subscription rather than a shared free pool. Bringing your own provider key
/// is still supported, and still needs the account — the plan is the licence
/// to use the client, the key decides whose credit the tokens come out of.
async fn authenticated(root: &std::path::Path) -> Result<(Client, Account)> {
    let config = Config::load(Some(root))?;
    let Some(token) = Credentials::load(&config)? else {
        anyhow::bail!(
            "not signed in.\n\n\
             Cymose Code runs on your Cymose account. Run `cymose login`, or see \
             {}/pricing if you don't have a plan yet.",
            landing_url()
        );
    };

    let client = client_with(&config, token);
    let account = client.account().await?;
    if !account.may_use_code() {
        anyhow::bail!("{}", upgrade_notice(&account));
    }
    Ok((client, account))
}

/// Said the same way everywhere it comes up, because a paywall that phrases
/// itself differently in three places reads as three different problems.
fn upgrade_notice(account: &Account) -> String {
    format!(
        "Cymose Code needs an active plan. This account is on {}.\n\n\
         Pro is $19/month and Max is $49, tax included — both include Code, the \
         VS Code extension, and the web canvas. {}/pricing",
        account.plan_label(),
        landing_url()
    )
}

fn landing_url() -> String {
    std::env::var("CYMOSE_LANDING_URL").unwrap_or_else(|_| "https://cymose.app".into())
}

/// Reads a token from the terminal.
///
/// A paste rather than a browser round trip: a device-code flow needs a route
/// on the API that doesn't exist yet, and shipping a worse login now would
/// mean two logins to support later. The prompt says exactly where to get it.
fn prompt_for_token(config: &Config) -> Result<String> {
    use std::io::{BufRead, Write};

    println!(
        "Sign in at {}/account and copy your CLI token.",
        landing_url()
    );
    println!("(It's the same account as the web app. Nothing is sent anywhere else.)");
    print!("\nToken: ");
    std::io::stdout().flush()?;

    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let token = line.trim().to_string();
    if token.is_empty() {
        anyhow::bail!("no token entered");
    }
    let _ = config;
    Ok(token)
}

fn workspace(store: &Store, root: &std::path::Path) -> Result<String> {
    store
        .workspace_for_path(root)?
        .context("this directory is not linked to a workspace — run `cymose init`")
}

/// The Web tree, same shape as the local one so the two read alike. Deliberately
/// prints what a node *decided* — its promoted conclusion and the notes pinned
/// to it — rather than its transcript, which the export does not carry.
fn print_web_tree(tree: &cymose_core::api::SyncTree, parent: Option<&str>, depth: usize) {
    for node in tree
        .nodes
        .iter()
        .filter(|n| n.parent_id.as_deref() == parent)
    {
        let short = &node.id[..node.id.len().min(8)];
        println!(
            "{:indent$}• {} — {short}",
            "",
            node.title,
            indent = depth * 2
        );
        if let Some(digest) = node.promoted_digest.as_deref().map(str::trim) {
            if !digest.is_empty() {
                let line = digest.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                println!("{:indent$}  ↑ {line}", "", indent = depth * 2);
            }
        }
        for note in &node.notes {
            let state = if note.in_context { "" } else { " (off)" };
            println!(
                "{:indent$}  ▤ {}{state}",
                "",
                note.title,
                indent = depth * 2
            );
        }
        print_web_tree(tree, Some(&node.id), depth + 1);
    }
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
