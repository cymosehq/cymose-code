# Cymose Code

An agentic coding tool that treats work as a **graph of short sessions** instead
of one endless chat, and routes across **several models** so a single provider's
rate limit doesn't stop your day.

Two clients, one core:

- **Cymose Code for Terminal** — a TUI (Rust + [Ratatui](https://ratatui.rs)).
- **Cymose Code for VS Code** — an extension that drives the same core as a
  sidecar process, so a session started in the terminal resumes in the editor
  and the other way round.

> Status: **pre-alpha.** The workspace builds and the session store is real;
> the agent loop and the model router are still being filled in. See
> [Roadmap](#roadmap).

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

## Install

Nothing is published yet. From source:

```sh
cargo install --path crates/cymose-cli
cymose login
cymose init            # link this directory to a workspace
cymose                 # open the TUI
```

## CLI

```
cymose init                    link the current directory to a workspace
cymose new "add redis cache"   start a session
cymose new --from <id> "..."   start one that inherits a specific session
cymose resume <id>             continue a session
cymose list                    show the session tree
cymose diff <id> <id>          compare two approaches
cymose promote <id>            send the outcome to Cymose Web
cymose sidecar                 JSON-RPC over stdio (used by the extension)
```

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

Cymose Code needs a Cymose account today: the model calls go through the Cymose
API. Bring-your-own-key, which removes that dependency, is on the roadmap.

## Roadmap

| Version | Scope |
|---------|-------|
| v0.1 | Terminal: session store, TUI, agent loop (read/write/run/search), router with fallback, context inheritance, summariser |
| v0.2 | VS Code: session tree, agent panel, native diff, sidecar transport, CodeLens |
| v0.3 | Sync: Web ↔ Terminal ↔ VS Code, promote from both clients, `explore` visible everywhere |
| v0.4 | Code graph via tree-sitter: symbols and call/import edges feeding the context builder |
| later | BYOK — talk to providers directly, no Cymose account |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). One rule worth repeating up front: this
repository is public and the rest of Cymose is not, so no credentials, no
server-side business logic, no schema — check your diff before you push.

## Licence

[Apache-2.0](LICENSE). "Cymose" is a project name, not part of the grant — see
[NOTICE](NOTICE).
