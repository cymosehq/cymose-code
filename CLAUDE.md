# Cymose Code

Two clients (terminal TUI in Rust, VS Code extension in TypeScript) over one
core. See [docs/architecture.md](docs/architecture.md) before making structural
changes, and [docs/spec.md](docs/spec.md) for the product concept.

## This repository is public

Everything committed here is published under Apache-2.0. The sibling Cymose
repositories are proprietary; this one is the only public surface, so the
boundary has to be enforced by hand at every commit.

Never write into this repo:

- Credentials or project identifiers of any kind — Supabase project refs or
  keys, Cloudflare account/KV/R2 ids, AI Gateway names, provider API keys,
  billing (Dodo/Polar/Paddle) product ids, tokens or webhook secrets.
- Server-side business logic copied out of the API: credit pricing and daily
  spend ceilings, per-device rate-limit algorithms, free-tier fuse thresholds,
  the provider mix behind a model name, prompt text used for server-side
  summarisation or routing, geo/sanctions rules.
- Database schema, RLS policy, or admin/webhook endpoint shapes.
- Unreleased pricing, roadmap dates, revenue numbers, or customer names.

What may live here: the public wire contract this client needs in order to
talk to the Cymose API — base URL, documented `/v1/…` routes, request and
response JSON, error codes, auth header shape. Nothing about how the server
answers. If a fix seems to need a private detail, the fix belongs on the API
side; open an issue there instead of encoding the detail here.

Nothing enforces this automatically. Check the diff before every commit —
`git diff --cached` — with the list above in mind.

## Layout

- `crates/cymose-core` — session store (SQLite), context builder, model
  router, agent loop, API client. No UI, no terminal, no `println!`.
- `crates/cymose-cli` — the `cymose` binary: TUI, CLI subcommands, and
  `cymose sidecar` (JSON-RPC over stdio).
- `extension/` — VS Code extension. Thin: UI, commands, and a client for the
  sidecar. Logic that clients share goes in `cymose-core`, not here.

No client may reimplement core behaviour. If two of them would disagree about
something, that something belongs behind the sidecar protocol
([docs/sidecar-protocol.md](docs/sidecar-protocol.md)).

0.1 requires a Cymose account on a paid plan. `cymose login` stores a token in
`~/.config/cymose/credentials.toml` (never in `config.toml` — that file gets
shared); every entry point runs `authenticated()`, which fetches the account
and refuses if the plan doesn't include Code.

BYOK is the second path, not the first: `OPENROUTER_API_KEY` sends turns to
OpenRouter on the user's own account, and still needs the plan. The
subscription is the licence to use the client; the key decides whose credit is
spent. See docs/spec.md §7 for why this reversed twice and why this is the
settled answer — change it only with that argument in hand.

## Commands

| Command | Purpose |
|---------|---------|
| `cargo build` | Build the workspace |
| `cargo test` | Run tests |
| `cargo clippy --all-targets -- -D warnings` | Lint (CI gate) |
| `cargo fmt --all` | Format |
| `cargo run -- tui` | Run the TUI against a local store |
| `npm --prefix extension run compile` | Build the extension |

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- The `graphify` binary is not on PATH. If `graphify ...` fails with "command not found", use the full path `/var/data/python/bin/graphify ...` instead.
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
