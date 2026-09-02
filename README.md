# Cymose Code

Plugins that put a **Cymose session graph** on a coding harness you already use.

This is not a coding agent. The harness runs the loop, the tools, and the model. Cymose is the map: short sessions as nodes, ancestor summaries as context you can actually read.

The graph lives in the adapter process. On DeepSeek Harness the focused node's map is injected into the system prompt when DSH exposes `systemPrompt`. On MCP, that map is in the server instructions plus `cymose_tree` / `cymose_inherit`. Summaries, explore, diff, combine, pick, and promote are written by **that harness's model**. No Cymose account, no Cymose API — only the host's limits.

To keep the graph after the process exits, call `cymose_dump` and let the **harness** save the JSON (its own file tools). Call `cymose_load` with that JSON at the start of a later session.

The point is **seeing what you already tried** — including the path that failed — not shaving input tokens.

One graph. One adapter per harness. **DeepSeek Harness (dsh)** and **MCP** (Cursor, Claude Code, and anything else that speaks MCP stdio) are first.

```
cymose-code/
  src/     graph IR and store (shared)
  dsh/     DeepSeek Harness plugin
  mcp/     MCP stdio server (Cursor, Claude Code, …)
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

## MCP (Cursor, Claude Code, …)

Same tools, same in-process graph, no extra npm packages. The server speaks JSON-RPC on stdio (newline JSON, or `Content-Length` framing).

From a checkout after `npm install && npm run build`:

```sh
node lib/mcp/stdio.js
```

Claude Code / Claude Desktop `mcpServers` example (absolute path to this repo):

```json
{
  "mcpServers": {
    "cymose": {
      "command": "node",
      "args": ["/absolute/path/to/cymose-code/lib/mcp/stdio.js"]
    }
  }
}
```

Cursor: Settings → MCP → add the same command. The graph lasts as long as that MCP process; dump/load if you want it after a restart.

## Develop

```sh
npm install
npm test
npm run build
```

`dsh` loads `cymose-code/dsh` from this package. MCP hosts run `cymose-code/mcp` via `node lib/mcp/stdio.js`.

## Licence

Apache-2.0. "Cymose" is a project name, not part of the grant — see [NOTICE](NOTICE).
