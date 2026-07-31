# Architecture

```
crates/cymose-core     library — everything that isn't a pixel
crates/cymose-cli      the `cymose` binary — TUI, subcommands, sidecar server
extension/             VS Code extension — UI only, speaks JSON-RPC to the binary
```

## One core, two clients

The core is implemented once, in Rust. The VS Code extension does not
reimplement it: it spawns `cymose sidecar` as a child process and talks JSON-RPC
over stdio.

The alternative — porting the session graph, router and agent loop to
TypeScript — was rejected. Those three are exactly where a silent divergence
hurts most: the same prompt would produce different contexts, different
failover, and different summaries depending on which client you happened to
open, and the bug would be invisible until a user compared them.

The price is real and paid in CI: the extension has to ship a binary per
platform inside its `.vsix`, and a mismatched sidecar version has to be
detected and reported rather than left to fail at the first RPC. The sidecar
handshake carries a protocol version for that reason.

## Core modules

| Module | Responsibility |
|--------|----------------|
| `store` | SQLite: workspaces, sessions, messages, summaries, artifacts |
| `context` | Builds a session's starting context from ancestor summaries |
| `router` | Model chain, failover policy, per-task pins |
| `agent` | The tools themselves: read_file, write_file, run_command, search |
| `runner` | The agent loop: stream a turn, run the tools it asks for, repeat |
| `summarize` | Turns a finished session into the summary its children inherit |
| `api` | Client for the Cymose API (auth, inference, sync) |
| `auth` | The stored token — `~/.config/cymose/credentials.toml`, mode `0600` |
| `config` | `~/.config/cymose/config.toml` plus per-workspace overrides |

Rules that keep the split honest:

- `cymose-core` never prints, never reads a terminal, never assumes a TTY. It
  returns values and emits events; a client decides how they look.
- Anything a user can do in the TUI must be reachable through the sidecar
  protocol, or the extension can't do it either.
- The store schema is the contract between clients. Change it through a
  migration, never in place — see [session-store.md](session-store.md).

## Event flow for one turn

```
client                sidecar/core            Cymose API
  │  session.prompt ──▶ │
  │                     │ context.build (ancestor summaries)
  │                     │ router.select ────────▶ inference
  │  ◀── event: token   │  ◀───────────────────── stream
  │  ◀── event: tool    │ agent.execute (read/write/run/search)
  │  ◀── event: diff    │ store.append
  │  ◀── result         │
```

Events are pushed as JSON-RPC notifications, so both clients render a turn
while it is happening rather than after it finishes.

## What is not here

The Cymose API is a separate, private service. This repository contains a
client for its public wire contract ([api-contract.md](api-contract.md)) and
nothing about how it answers: no provider mix behind a model name, no credit
accounting, no prompts it runs server-side. If something in this repo appears
to need one of those, it is a sign the logic belongs on the server.
