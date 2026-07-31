# Cymose Code for VS Code

> **0.1 beta.** Needs a Cymose account on a paid plan (Pro or Max) — the same
> subscription that covers the web canvas. Setting `OPENROUTER_API_KEY` sends
> turns to your own [OpenRouter](https://openrouter.ai) account instead of
> spending plan credits; the plan is still what licenses the client.
> The code in this repository is written by an AI agent; see
> [the note in the root README](../README.md#this-code-was-written-by-an-ai).

The editor client. It does not implement the session graph, the model router,
or the agent loop — it drives `cymose sidecar` over JSON-RPC, so it and the
terminal client always agree.

## Working on it

```sh
npm install
npm run compile
cargo build            # from the repo root, builds the core the extension drives
```

Press F5 to open an Extension Development Host. With no bundled binary present
the extension falls back to `cymose` on PATH, so `cargo build` plus a reload is
the edit cycle. `cymose.corePath` overrides that if you want a specific build.

## What works today

- Session tree in the sidebar, with each session's summary on hover
- New session, inheriting from the workspace's graph
- Showing what a session inherits, in the output channel

Not yet: running a turn, native diff between two sessions, CodeLens, promote.
The agent loop and the inference route both exist and the terminal client runs
turns on them; what is missing here is the sidecar half — `session.prompt`,
`session.cancel` and `session.diff` still answer "not implemented in this
build", because streaming a turn needs the sidecar to push notifications from
another task. See [../docs/sidecar-protocol.md](../docs/sidecar-protocol.md).

## Packaging

Release builds bundle a `cymose` binary per platform under `bin/`, which is why
`.vscodeignore` keeps `bin/**` while excluding everything else that isn't
compiled output.
