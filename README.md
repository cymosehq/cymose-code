# Cymose Code

An agentic coding tool that treats work as a **graph of short sessions** instead
of one endless chat, and routes across **several models** so a single provider's
rate limit doesn't stop your day. — [cymose.app/code](https://cymose.app/code)

Two clients, one core:

- **Cymose Code for Terminal** — a TUI (Rust + [Ratatui](https://ratatui.rs)).
- **Cymose Code for VS Code** — an extension that drives the same core as a
  sidecar process, so a session started in the terminal resumes in the editor
  and the other way round.
The core is one Rust implementation. Every client is a thin layer over
`cymose sidecar` speaking JSON-RPC, so none of them can quietly disagree about
what a session is.

## 0.1 beta — read this before you install

**This is a 0.1 beta, and it needs a Cymose account on a paid plan.** Pro or
Max — the same subscription that covers the web canvas and the VS Code
extension. There is no free tier for Code, and that is deliberate rather than
stingy: an agent session costs an order of magnitude more than a chat turn, and
a free allowance it could fit inside would run out halfway through the first
real task.

What that means concretely:

- **Sign in first.** `cymose login`, then paste the token from your account
  page. `cymose whoami` says which plan you're on and what's left of it.
- **Bringing your own key is supported, and it is the second path, not the
  first.** Set `OPENROUTER_API_KEY` and turns go to OpenRouter on your account
  instead of spending Cymose credits. It still needs the plan: the
  subscription is the licence to use the client, the key only decides whose
  credit the tokens come out of.
- **The gate is one `if` in an Apache-2.0 repository.** Anybody who wants it
  gone can have it gone in a minute, and we know that. It's there for the
  honest majority, and to state the deal plainly — the client is open, the
  service behind it is paid for.
- **[Cymose Web](https://cymose.app) integration reads only.** `cymose sync`
  prints the tree you planned in the browser — titles, promoted conclusions,
  pinned notes. It never writes anything back, and it reads the tree from your
  account whether or not turns are going through us.
- **`promote` back to a conclusion node, and syncing sessions between machines,
  come later.** The write direction needs revisions and merge rules; reading is
  most of the value and has none of that risk.
- **Not everything works yet.** Honest inventory below.

## This code was written by an AI

Every line in this repository was written by an AI coding agent (Claude, via
Claude Code) working from my direction. I decide what gets built, review it at
the level of behaviour and product, and test it. I do not hand-write the code,
and I could not have written this by hand — I am not a programmer.

This is stated up front because you deserve to know what you are reading before
you run it, not after:

- **Read it before you trust it.** There is no experienced human author who
  reviewed every line for correctness or security. The tests in this repo are
  real and they pass, but a passing test suite is not a code review.
- **`run_command` executes commands on your machine.** It asks first by
  default. Look at what the allowlist does before widening it.
- **Bugs here are my responsibility, not the model's.** Report them and they
  get fixed. "The AI wrote it" is an explanation, never an excuse.

If that disqualifies the project for you, that is a reasonable call and no hard
feelings. If it doesn't: the design decisions are argued in the comments and in
[docs/](docs/), and I would rather be judged on whether those decisions are
right than on who typed them.

## Why sessions form a graph

Nobody runs a thousand-message session with a coding agent. People open a new
one per task — and then the new session knows nothing, so they re-explain the
codebase and watch the model repeat a mistake it already made an hour ago.

Cymose Code makes every session a node. A new session inherits the *summaries*
of its parents, not their transcripts:

```
● auth bug            [done]   → "JWT expiry fixed, src/auth.rs"
● rate limiter        [done]
  ├─● token bucket    [failed] → "race condition under contention"
  └─● sliding window  [done]   → "works, merged"
● rate limiter v2     [active]
     inherits both summaries: knows token bucket failed, sliding window landed
```

Carrying a compact summary instead of the full log is what keeps a long-running
project inside a sane context budget — the saving grows with the depth of the
tree, and it is the reason a fresh session starts already knowing what failed.

## Model routing

Configure a chain once; both clients use it. On a rate limit, timeout, or
provider error, the next model picks the turn up without interrupting the run.

```toml
[router]
chain = ["claude-sonnet", "deepseek-coder", "glm-4.5", "qwen-coder"]

[router.pin]
summarize = "glm-4.5"      # routine work on a cheap model
architect  = "claude-sonnet"
```

### What works, and what doesn't

| | |
|---|---|
| Session graph, summary inheritance, SQLite store | **works** |
| TUI: transcript, tools running, creating sessions | **works** |
| Tools — read, write, search, run (path jail, command allowlist) | **works** |
| Model chain and failover policy | **works** |
| Streaming turns, on both backends | **works** |
| The agent loop — a prompt runs tools until it's done | **works** |
| `cymose sync` — read the Web tree | **works** (read-only) |
| Summaries on your own OpenRouter key | not yet (the prompt is the server's) |
| `diff` — needs session artifacts, which aren't recorded yet | not yet |
| `promote` — needs a server route that accepts a session | not yet |
| VS Code panel actions (prompt, cancel, diff) | not yet |

The agent loop runs in the TUI. The VS Code extension can browse the session
tree but cannot yet drive a turn, so the editor half is genuinely behind the
terminal half today.

## Install

**Terminal — one line, no toolchain:**

```sh
curl -fsSL https://cymose.app/install.sh | sh
```

It downloads the release binary for your platform, checks it against the
published `SHA256SUMS`, and puts it in `~/.local/bin` (override with
`CYMOSE_INSTALL_DIR`). Windows PowerShell:

```powershell
irm https://cymose.app/install.ps1 | iex
```

Then:

```sh
cymose login           # paste the token from your account page
cymose init            # link this directory to a workspace
cymose                 # open the TUI
```

Optionally, to spend your own OpenRouter credit instead of the plan's:

```sh
export OPENROUTER_API_KEY=sk-or-v1-...
```

**VS Code:** search for **Cymose Code** in the Extensions view (`Ctrl`/`Cmd` +
`Shift` + `X`) and install it — the extension bundles the core binary, so
there is nothing else to set up. Not published yet: until it is, grab the
`.vsix` from [Releases](https://github.com/cymosehq/cymose-code/releases) and
run **Extensions: Install from VSIX…** from the command palette.

**From source**, if you'd rather build it:

```sh
git clone https://github.com/cymosehq/cymose-code
cd cymose-code
cargo install --path crates/cymose-cli
```

Releasing, and how the Marketplace publish works: [RELEASING.md](RELEASING.md).

## CLI

```
cymose login                   sign in with the token from your account page
cymose logout                  forget the stored token
cymose whoami                  which plan you're on, and what's left of it
cymose init                    link the current directory to a workspace
cymose new "add redis cache"   start a session
cymose new --from <id> "..."   start one that inherits a specific session
cymose resume <id>             show what a session inherits
cymose list                    show the session tree
cymose sync                    read the tree you planned in Cymose Web
cymose diff <id> <id>          compare two approaches (later milestone)
cymose promote <id>            send the outcome to Cymose Web (later milestone)
cymose sidecar                 JSON-RPC over stdio (used by the extension)
```

Bare `cymose` opens the TUI, which is where turns actually run.

## VS Code

The extension is deliberately thin — a sidebar tree, a webview for the agent
log, native diffs, CodeLens showing which session last touched a function. All
of it talks to `cymose sidecar` over JSON-RPC, so behaviour can't drift between
the two clients. See [docs/sidecar-protocol.md](docs/sidecar-protocol.md).

## Cymose Web

Cymose Web is the non-linear canvas where the plan lives — hypotheses,
branches, conclusions. Cymose Code is where those turn into diffs. Mark a node
as a code task and it opens as a session here; `promote` sends the result back
as a conclusion node. Plan on one screen, implementation on the other.

Half of that is in 0.1. `cymose sync` reads the tree you planned in the
browser, so the plan is at least visible from here. The other half — a canvas
node that opens as a coding session, and `promote` sending the diff back as a
conclusion — is the next milestone.

## Roadmap

| Version | Scope |
|---------|-------|
| **v0.1 beta (here)** | Account + plan gate, BYOK as the second path. Session store, TUI, tools, router, context inheritance, agent loop, streaming, read-only `cymose sync` |
| v0.2 | VS Code: session tree, agent panel, native diff, sidecar transport, CodeLens |
| v0.3 | `promote` from both clients, and session artifacts so `cymose diff` works |
| v0.4 | Code graph via tree-sitter: symbols and call/import edges feeding the context builder |
| v0.5 | Write-direction sync between Web, Terminal and VS Code; `explore` visible everywhere |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). One rule worth repeating up front: this
repository is public and the rest of Cymose is not, so no credentials, no
server-side business logic, no schema — check your diff before you push.

## Licence

[Apache-2.0](LICENSE). "Cymose" is a project name, not part of the grant — see
[NOTICE](NOTICE).
