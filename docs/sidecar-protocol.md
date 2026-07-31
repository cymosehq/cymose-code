# Sidecar protocol

`cymose sidecar` speaks JSON-RPC 2.0 over stdin/stdout, one message per line
(newline-delimited JSON, not `Content-Length` framing). stderr carries logs and
is never part of the protocol.

The VS Code extension is the only consumer today. Anything the TUI can do must
be expressible here.

## Handshake

The client calls `initialize` first. The server replies with the protocol
version it implements; a client that doesn't recognise it must refuse to
continue and tell the user to update, rather than probing method by method.

```jsonc
--> {"jsonrpc":"2.0","id":1,"method":"initialize",
     "params":{"client":"vscode","client_version":"0.2.0","protocol":1}}
<-- {"jsonrpc":"2.0","id":1,
     "result":{"protocol":1,"core_version":"0.1.0","workspace":null}}
```

`protocol` is bumped only on a breaking change. Additive fields and new
notification types do not bump it, so both sides must ignore what they don't
recognise.

## Methods

| Method | Params | Result |
|--------|--------|--------|
| `initialize` | client, client_version, protocol | protocol, core_version, workspace |
| `workspace.open` | path | workspace id, session tree |
| `session.tree` | — | nodes with id, title, status, parent |
| `session.new` | title, parent (optional) | session id |
| `session.resume` | id | session id, inherited context summary |
| `model.list` | — | chain, active model |
| `shutdown` | — | — |

Documented, and answering `-32004` until they are built:

| Method | Params | Result |
|--------|--------|--------|
| `session.prompt` | id, text | accepted; output arrives as notifications |
| `session.cancel` | id | — |
| `session.diff` | id_a, id_b | unified diff per file |
| `session.promote` | id | Web node id |
| `model.switch` | model | active model |

`session.prompt` is the one that matters: the core runs turns already, but this
loop is synchronous, and streaming a turn means pushing notifications from
another task. That is the change this becomes async for. Until then the
terminal client is the only one that can run a turn.

## Notifications

Server to client, no id, fire and forget:

| Notification | Payload |
|--------------|---------|
| `event/token` | session, text |
| `event/tool` | session, tool, args, phase (`start` \| `end`), result |
| `event/diff` | session, path, unified diff |
| `event/status` | session, status (`running` \| `done` \| `failed`) |
| `event/model` | session, model, reason (e.g. `fallback`) |
| `event/usage` | session, input tokens, output tokens |
| `event/log` | level, message |

## Errors

Standard JSON-RPC error objects. Codes below `-32000` are reserved by the spec;
core errors start at `-32000`:

| Code | Meaning |
|------|---------|
| `-32000` | no workspace open |
| `-32001` | session not found |
| `-32002` | not authenticated |
| `-32003` | store is locked by another client |
| `-32004` | not implemented in this build |

## Lifetime

One sidecar per VS Code window. The extension owns the process: it spawns it on
activation, kills it on deactivate, and restarts it with backoff if it exits.
A sidecar that loses its stdin exits — an orphan holding the store open is
worse than a dead one.
