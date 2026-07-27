# Cymose Code

An agentic coding tool that treats work as a **graph of short sessions** instead
of one endless chat, and routes across **several models** so a single provider's
rate limit doesn't stop your day.

Two clients, one core:

- **Cymose Code for Terminal** — a TUI (Rust + [Ratatui](https://ratatui.rs)).
- **Cymose Code for VS Code** — an extension that drives the same core as a
  sidecar process, so a session started in the terminal resumes in the editor
  and the other way round.
The core is one Rust implementation. Every client is a thin layer over
`cymose sidecar` speaking JSON-RPC, so none of them can quietly disagree about
what a session is.

## 0.1 beta — read this before you install

**This is a 0.1 beta. It runs on your own OpenRouter key (BYOK) and nothing
else.** There is no Cymose account, no billing, no server of ours in the path
of a turn: your key goes to OpenRouter, and you see every cent of it on your
own OpenRouter dashboard.

What that means concretely:

- **You need an [OpenRouter](https://openrouter.ai) key.** Put it in
  `OPENROUTER_API_KEY`. Without one, nothing runs.
- **Integration with [Cymose Web](https://cymose.dev) comes later.** The canvas
  where the plan lives, `promote` back to a conclusion node, sync between
  machines — that is the next milestone, and none of it is in this build. The
  client already carries the backend it will use; it is switched off.
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
| TUI: tree, session detail, creating sessions | **works** |
| Tools — read, write, search, run (path jail, command allowlist) | **works** |
| Model chain and failover policy | **works** |
| One turn, BYOK, non-streamed | **works** |
| Streaming turns | not yet |
| The agent loop end to end (`cymose new` runs a task by itself) | **not yet** |
| `diff`, `promote`, VS Code panel actions | not yet |
| Cymose Web sync | later milestone |

If you install this expecting to hand it a task and walk away, you will be
disappointed today. If you want to look at how a session graph is built and
tell us where it's wrong, now is a good time.

## Install

Nothing is published yet. From source:

```sh
export OPENROUTER_API_KEY=sk-or-v1-...
cargo install --path crates/cymose-cli
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
cymose promote <id>            send the outcome to Cymose Web (later milestone)
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

None of that is in 0.1. Today the two are separate products: Cymose Code runs
on your own key, locally, and knows nothing about the web canvas. Linking them
— a canvas node that opens as a coding session, and `promote` sending the diff
back as a conclusion — is the next milestone.

## Roadmap

| Version | Scope |
|---------|-------|
| **v0.1 beta (here)** | BYOK only. Session store, TUI, tools, router, context inheritance. Agent loop and streaming still landing |
| v0.2 | VS Code: session tree, agent panel, native diff, sidecar transport, CodeLens |
| v0.3 | Sync: Web ↔ Terminal ↔ VS Code, promote from both clients, `explore` visible everywhere |
| v0.4 | Code graph via tree-sitter: symbols and call/import edges feeding the context builder |
| v0.5 | Cymose Web sync from every client, `explore` visible everywhere |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). One rule worth repeating up front: this
repository is public and the rest of Cymose is not, so no credentials, no
server-side business logic, no schema — check your diff before you push.

## Licence

[Apache-2.0](LICENSE). "Cymose" is a project name, not part of the grant — see
[NOTICE](NOTICE).
