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
- **Account first, BYOK second — reversed again, and this is the settled
  answer.** The project began "cloud first", flipped to BYOK-only for 0.1, and
  has now flipped back. Each move was right at the time and the reasoning is
  worth keeping, because the same argument decides the next one.

  BYOK-only was chosen because an agent burns an order of magnitude more per
  session than a chat turn, and a free tier funded from one shared balance
  would have lasted days. It was also the shortest path to something people
  could run: a beta needing an account, a payment rail and a working backend
  has three ways to fail before anybody sees the product.

  Both of those have since been answered rather than avoided. There is a
  payment rail, it has taken real money, and there are plans — so the thing
  funding an agent turn is a subscription, which is what it always had to be.
  The free-tier problem doesn't arise: **Cymose Code requires an account on an
  active plan.** Not a free tier with small limits — none. An agent session is
  not a thing that fits inside a free allowance, and pretending otherwise ships
  a product that stops working halfway through the first task.

  BYOK survives as the second path, not the first. The plan is the licence to
  use the client; the key decides whose credit the tokens come out of. Someone
  on Max who wants Opus on their own OpenRouter account can have that, and
  still gets the summariser, the sync, and a client that is being maintained.

  The gate is one `if` in an Apache-2.0 repository and anybody who wants it
  gone can have it gone in a minute. That is not what it's for. It's for the
  honest majority, and for stating the deal plainly: the client is open, the
  service behind it is paid for.

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
