# Cymose Code

Plugins that put a **Cymose session graph** on a coding harness you already use.

This is not a coding agent. The harness runs the loop, the tools, and the model. Cymose is the map: short sessions as nodes, ancestor summaries as context you can actually read, the Web canvas as the plan.

The point is **seeing what you already tried** — including the path that failed — not shaving input tokens.

One graph. One adapter per harness. **DeepSeek Harness (dsh) is first**; others come after that one is real.

```
cymose-code/
  src/     graph IR, store, Cymose API (shared)
  dsh/     DeepSeek Harness plugin
  claude/  later
  cursor/  later
```

The previous Cymose Code (own TUI, sidecar, `/v1/code/inference`) is archived as `cymose-code-legacy`. This repository replaces it.

## DeepSeek Harness

```sh
dsh plugin add github:cymosehq/cymose-code
```

Or from a checkout:

```sh
dsh plugin add link:/absolute/path/to/cymose-code
```

Set `token` on the plugin config (bundle overlay or profile `cordis.patch.yml`). Create a token at [web.cymose.app](https://web.cymose.app) → Settings → Connected apps.

Graph file: `<workspace>/.cymose/graph.json`.

### Tools (dsh)

| Tool | What it is for |
|------|----------------|
| `cymose_tree` | Map of nodes, focus, who has a summary |
| `cymose_branch` | New node; inherits the parent chain |
| `cymose_focus` | Pick the active node |
| `cymose_inherit` | Ancestor text to read before repeating work |
| `cymose_mark` | `done` / `failed` / `dead-end` |
| `cymose_summarize` | Store a summary children will inherit (needs token) |
| `cymose_sync` | Read-only Web canvas tree (needs token) |
| `cymose_whoami` | Token check + credits |

A child of a `failed` / `dead-end` node still sees that ancestor. That is the product.

## Develop

```sh
npm install
npm test
npm run build
```

`dsh` loads `cymose-code/dsh` from this package.

## Licence

Apache-2.0. "Cymose" is a project name, not part of the grant — see [NOTICE](NOTICE).
