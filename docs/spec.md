# Cymose Code — concept

Two clients of one ecosystem for writing and editing code with an agent:
a terminal TUI (Rust + Ratatui) and a VS Code extension (TypeScript). Both sit
on the same core.

The audience is developers tired of one vendor's rate limits and bills, who
want the agentic experience without the lock-in.

## 1. Sessions are nodes, not one long chat

The observation the product is built on: nobody keeps a 1000-message session
with a coding agent. People open a new session per task or bugfix — and the new
session has no memory of the previous ones, so context gets re-explained by
hand and old mistakes get repeated.

So every session is a node in a tree. When a session is created, the context
builder assembles a compact context out of the *summaries* of its parents and
siblings — never their full transcripts.

```
● auth bug          [done]
│  └→ summary: "JWT expiry fixed, src/auth.rs"
│
● rate limiter      [done]
│  ├─● try token bucket   [failed] → "race condition"
│  └─● try sliding window [done]   → "works, merged"
│
● rate limiter v2   [active]
   └→ inherits: both rate-limiter summaries + auth
      knows token bucket failed and sliding window landed
```

This is identical in both clients: a session started in the terminal is visible
in VS Code and vice versa, because both read and write one session store.

## 2. Model routing

Configured once, applied everywhere. A chain of models with automatic failover
on rate limit, error, or timeout, and per-task pinning (routine fixes to a cheap
model, architecture to a strong one).

## 3. Shared core

Both clients are UI over one core:

```
┌─────────────── Core ────────────────────┐
│  Session store (SQLite)                  │
│  Context builder (summary inheritance)   │
│  Summariser (after a session ends)       │
│  Model router (fallback chain)           │
│  Agent loop (read/write/run/search)      │
│  Sync client (→ Cymose Web)              │
└───────────────┬───────────────┬──────────┘
                │               │
      ┌─────────▼──────┐ ┌──────▼─────────┐
      │  Terminal TUI  │ │  VS Code ext   │
      │  (Rust binary) │ │  (TS, webview) │
      └────────────────┘ └────────────────┘
```

The core exists once, in Rust. The extension drives it as a sidecar process
over JSON-RPC — see [architecture.md](architecture.md) for why, and
[sidecar-protocol.md](sidecar-protocol.md) for the wire format.

## 4. Terminal client

Three panes: a session tree on the left, an editor/diff view in the centre, an
agent log plus prompt line along the bottom.

```
┌─ Cymose Code ──────────────────────────────────────────────┐
│  WORKSPACE: my-project          MODEL: claude → deepseek ⚡ │
├──────────────┬─────────────────────────────────────────────┤
│  TREE        │  EDITOR / DIFF / CHAT                       │
│  ● rate lim  │  fn try_acquire(&self) -> bool { ... }      │
│  ├─● A ✓     │  +12 -3                                     │
│  └─● B ⟳     │  [e] explore [p] promote [s] switch model   │
├──────────────┴─────────────────────────────────────────────┤
│  > fix the failing test in sliding_window                  │
└────────────────────────────────────────────────────────────┘
```

Keys (draft):

```
Navigation: j/k ↑/↓, h/l ←/→, Enter, Tab
Actions:    n new session, r resume, d diff, e explore (3 approaches),
            p promote to Web, s switch model, m merge
Git:        gs status, gc commit, gch checkout
General:    : command, i input, Esc, q
```

Stack: `ratatui`, `crossterm`, `syntect`, `tokio`, `reqwest`, `serde`,
`rusqlite`, `tui-textarea`, `anyhow`, `clap`.

## 5. VS Code client

Not a TUI emulated inside the IDE — the editor already has files, git and a
terminal, so the extension uses native surfaces:

- **Sidebar** — the session tree, in the Activity Bar (`TreeDataProvider`).
- **Native diff** — VS Code's own diff editor, not a custom renderer.
- **Inline actions** — right-click a selection: "Explore 3 ways", "Fix this",
  "Ask Cymose".
- **CodeLens** — above a function, the status of the last session that touched
  it ("✓ tested, 2 sessions ago").
- **Status bar** — active model, fallback indicator, session token spend.

The agent panel is a webview (the log needs richer layout than a tree can
give); everything that fits a native component uses one.

## 6. Relationship to Cymose Web

Plan on one screen, code on the other, always in sync.

```
Cymose Web (canvas)                  Cymose Code (terminal / VS Code)
───────────────────                  ────────────────────────────────
"needs a rate limiter"    ──sync──▶  session: rate-limiter
  ├─ branch: token bucket ◀─sync──     ├─ branch A → failed (race)
  └─ branch: sliding win  ◀─sync──     └─ branch B → passed, promoted
  conclusion node created
  automatically on promote
```

Flow: sketch hypotheses on the canvas → mark a node as a code task and link it
to a project → open a session against that node in either client (the context
builder pulls the task text and its discussion as starting context) → the agent
works → `promote` sends summary and diff back as a conclusion node → the canvas
shows done / failed / in progress, and several code sessions can be compared as
branches.

## 7. Decisions taken

Three questions that were open at the start of the project and are now settled:

- **Sidecar, not a duplicated core.** One Rust implementation; the extension is
  a client. Cost: per-platform binaries in CI, shipped inside the `.vsix`.
  Rejected alternative: a TypeScript port of the core, which guarantees the two
  clients eventually disagree about something subtle.
- **Apache-2.0.** Patent grant, and an explicit position on the project name
  (see [NOTICE](../NOTICE)).
- **BYOK first, cloud later — reversed.** This started as "cloud first": an
  account, and turns through the Cymose API. Two things changed it. A coding
  agent burns an order of magnitude more per session than a chat turn, so a
  free tier funded out of one shared balance would last days, not months. And a
  beta that needs an account, a payment rail and a working backend has three
  ways to fail before anyone sees the product. 0.1 is BYOK only: the user's own
  OpenRouter key, straight to OpenRouter. The Cymose backend is built (the API
  serves `/v1/code/*`) and switched off here; it is what the web integration
  will run on.

## 8. Still open

- **Store contention.** Both clients may have the same SQLite file open on the
  same machine. WAL plus a short busy timeout covers the common case; whether
  live cross-client updates need a change feed (file watch, or a socket owned by
  one sidecar) is undecided. See [session-store.md](session-store.md).
- **`explore` in code.** Real git branches (native for the VS Code diff view,
  awkward for running three agents in one working directory) versus file
  snapshots in the store (easy to parallelise, needs its own diff plumbing).
  Leaning towards snapshots with an opt-in export to branches.
- **Offline behaviour.** What a session does when the network is down: refuse,
  queue, or degrade.
- **Where the summariser's prompt lives under BYOK.** With the Cymose backend
  the server owns it, which is how it improves without a client release. On a
  user's own key there is no server in the path, so the client has to carry a
  prompt — and then two implementations can disagree about what a summary is.
