# Cymose Code for VS Code

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
Those are blocked on the core's agent loop and the API's inference route — see
[../docs/api-contract.md](../docs/api-contract.md).

## Packaging

Release builds bundle a `cymose` binary per platform under `bin/`, which is why
`.vscodeignore` keeps `bin/**` while excluding everything else that isn't
compiled output.
