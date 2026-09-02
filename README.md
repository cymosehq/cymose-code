# Cymose Code

Plugins that put a **Cymose session graph** on a coding harness you already use.

This is not a coding agent. The harness runs the loop, the tools, and the model. Cymose is the map: short sessions as nodes, ancestor summaries as context you can actually read.

The graph lives in the harness process. The focused node's map is injected into the system prompt when DSH exposes `systemPrompt`. Summaries, explore, diff, combine, pick, and promote are written by **that harness's model**. No Cymose account, no Cymose API — only the host's limits.

To keep the graph after the process exits, call `cymose_dump` and let the **harness** save the JSON (its own file tools). Call `cymose_load` with that JSON at the start of a later session.

The point is **seeing what you already tried** — including the path that failed — not shaving input tokens.

One graph. One adapter per harness. **DeepSeek Harness (dsh) is first**; others come after that one is real.

```
cymose-code/
  src/     graph IR and store (shared)
  dsh/     DeepSeek Harness plugin
  claude/  later
  cursor/  later
```

## DeepSeek Harness

```sh
dsh plugin add github:cymosehq/cymose-code
```

Or from a checkout:

```sh
dsh plugin add link:/absolute/path/to/cymose-code
```

Optional plugin config: `namespace` (in-process name if you keep more than one graph).

### Permissions

This plugin does not open files, sockets, or credentials. Install has no `prepare` / `install` lifecycle scripts.

### Tools (dsh)

| Tool | What it is for |
|------|----------------|
| `cymose_tree` | Map of nodes, focus, summaries |
| `cymose_branch` | New node; inherits the parent chain |
| `cymose_focus` | Pick the active node |
| `cymose_inherit` | Ancestor text to read before repeating work |
| `cymose_mark` | `done` / `failed` / `dead-end` |
| `cymose_summarize` | Store a summary the host model wrote |
| `cymose_explore` | Fork several sibling approaches |
| `cymose_diff` | Two nodes' summaries side by side |
| `cymose_combine` | Write a synthesis onto a node |
| `cymose_promote` | Fold a child's outcome onto its parent |
| `cymose_pick` | Copy summaries onto another node, labeled |
| `cymose_dump` | JSON snapshot of the in-process graph |
| `cymose_load` | Restore a snapshot |

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
