# Backends

## 0.1 talks to OpenRouter, not to us

The 0.1 beta is BYOK: `api::Backend::OpenRouter` sends an OpenAI-compatible
chat completion straight to `openrouter.ai` with the user's own key. Nothing of
ours is in the path of a turn — no account, no metering, no service to be down
— and the translation between this crate's wire format and OpenAI's lives in
`api::openai_message` / `api::parse_openai_completion`.

The rest of this document describes the **Cymose backend**: implemented on the
API, not switched on here, and what the Cymose Web integration will run on.
`api::Backend::Cymose` returns `Error::NotImplemented` in this build.

## The Cymose backend (later milestone)

The server is proprietary and lives in a separate repository. What follows is
the wire contract only: what this client sends and what it expects back. How
the server produces an answer — which provider serves a model name, what a turn
costs, what prompt it summarises with — is deliberately not described here and
must not be encoded in this repo.

Base URL is configuration, not a constant. It defaults to the production host
and can be overridden per workspace (`api.base_url`) so a contributor can point
at a local server.

## The split

**Routing lives in the client.** `cymose-core::router` holds the chain, the
pins and the failover policy. A request names exactly one model. When it comes
back `429`, the client picks the next model itself and sends a new request.

The server therefore never sees a chain, never retries, and never substitutes a
model. Its one job on the inference path is to relay: resolve the model name to
a provider, forward the call, pass the answer or the failure back.

**Tools run in the client.** The server relays a `tool_call` from the model;
`cymose-core::agent` executes it, under the workspace path jail and the command
allowlist, and sends the result back as the next message. The server never
touches the user's files.

**Failures pass through.** The client's failover is driven by status code, so
the server must preserve the provider's status rather than normalise everything
into a `500`. A `429` that arrives as a `500` costs the user a working fallback.

## Auth

`cymose login` obtains a user token and stores it in the OS keychain, falling
back to `~/.config/cymose/credentials.json` at mode `0600`. Every call carries:

```
Authorization: Bearer <token>
Content-Type: application/json
X-Cymose-Device: <stable per-install id>
```

The token is a bearer credential with a finite lifetime. The client refreshes
it on `401` once, then asks the user to log in again. It is never logged, never
written to the session store, and never included in a bug report — see
`api::redact`.

## Endpoints

| Route | Used by | Shape |
|-------|---------|-------|
| `POST /v1/code/inference` | every agent turn | SSE stream |
| `POST /v1/code/summarize` | the summariser, when a session ends | synchronous JSON |
| `POST /v1/promote` | `cymose promote` — outcome to a Web conclusion node | synchronous JSON |
| `GET /v1/sync/tree` | reading the Web tree into a local store | synchronous JSON |

`/v1/code/*` is implemented on the API. Both routes accept `stream: false` and
answer with one JSON body, which is what `api::Client::inference` sends and
what makes the contract testable with `curl`.

**Streaming works.** `api::Client::inference_stream` reads SSE and hands the
caller one event at a time; a turn takes tens of seconds and a client that
shows nothing until the end is indistinguishable from one that has hung. Both
backends stream — this route with the events below, and OpenRouter with
OpenAI's format, translated at the transport so nothing above it knows which
key paid.

Frames are buffered until the blank line that ends them. A chunk boundary
lands mid-JSON often enough to matter and never on a fast local connection,
which is exactly the kind of bug that ships.

### `POST /v1/code/inference`

```jsonc
{
  "session_id": "uuid",     // logging and billing only — see below
  "model": "claude-sonnet", // exactly one; the server resolves it to a provider
  "messages": [
    { "role": "system", "content": "..." },
    { "role": "user", "content": "fix the failing test in sliding_window" },
    { "role": "assistant", "content": null, "tool_calls": [
      { "id": "call_1", "name": "read_file", "input": { "path": "src/limiter.rs" } }
    ] },
    { "role": "tool", "tool_call_id": "call_1", "content": "fn try_acquire() { … }" }
  ],
  "tools": [
    { "name": "read_file", "description": "…", "input_schema": { /* JSON Schema */ } }
  ],
  "max_tokens": 4096,
  "stream": true
}
```

`content` is `null` on an assistant turn that only calls tools — the client
omits the field rather than sending an empty string, because providers treat
those differently.

**The server keeps no context between requests.** `session_id` is for logging
and billing. The transcript lives in the client's SQLite store and the relevant
part of it is sent every turn — which is also what makes a session resumable
from either client, and what lets a child session start from summaries instead
of a log.

Response is `text/event-stream`:

```
event: text_delta
data: {"delta": "I'll look at the failing test…"}

event: tool_call
data: {"id": "call_1", "name": "read_file", "input": {"path": "tests/window.rs"}}

event: done
data: {"stop_reason": "tool_use", "tokens_used": {"input": 1200, "output": 340}, "model": "claude-sonnet"}
```

An unknown event type is ignored, so the server can add events without a
protocol bump.

### `POST /v1/code/summarize`

Synchronous — it is short, and nothing is waiting on it.

```jsonc
{
  "session_id": "uuid",
  "task": "fix the failing test in sliding_window",
  "transcript": [ /* messages, same shape as above */ ],
  "outcome": "done",        // the client's verdict, not the model's
  "model": "glm-4.5"        // optional; omitted means "use your cheap default"
}
```

`outcome` is sent, not inferred. Whether the work succeeded is a fact the
client already knows, and a summariser guessing it from a transcript is how a
failed attempt gets inherited as a success — the one failure that makes the
whole session graph untrustworthy.

The response must be structured output (the provider's JSON-schema or
function-calling mode), not prose the client has to scrape:

```json
{
  "task": "fix the failing test in sliding_window",
  "files_touched": ["src/sliding_window.rs", "tests/sliding_window_test.rs"],
  "approach": "Fixed off-by-one in the window boundary check",
  "outcome": "success",
  "key_decisions": ["Used Instant::now() rather than SystemTime for monotonicity"],
  "errors_encountered": ["Initial fix overflowed on window.iter().count()"],
  "tokens_used": 8400
}
```

`errors_encountered` is the field that earns the summary its place: a child
session inherits it ahead of the decisions, because what went wrong is what
must not be repeated. The client unions `files_touched` with what it watched
its own tools write — neither list is complete alone.

The prompt that produces this is the server's. It stays there so it can be
improved without a client release, and so prompt text stays out of a public
repository.

### Errors

Before the stream starts, a failure is an HTTP status with:

```json
{ "error": { "type": "rate_limited", "provider_status": 429, "message": "…" } }
```

After the stream has started, it is a final event, since the status is already
sent:

```
event: error
data: {"type": "provider_error", "provider_status": 503, "message": "…"}
```

Either way the client reads `provider_status` and acts on it:

| Status | Client behaviour |
|--------|------------------|
| `401` | refresh once, then re-authenticate |
| `402` | out of allowance — surface the server's message verbatim, do not retry |
| `403`, `451` | refusal — surface verbatim, do not retry |
| `408`, `409`, `425` | retry the same model |
| `429`, `5xx` | advance to the next model in the chain |

`402`, `403` and `451` messages are shown as sent. The client does not
paraphrase them and must not try to reconstruct why the server said no.

### `GET /v1/sync/tree`

The account's whole tree, in the shape every client is expected to store it in.
No body; the bearer token identifies the caller and the server returns only
that caller's nodes.

```jsonc
{
  "version": 1,
  "synced_at": "2026-07-29T02:14:00.000Z",
  "nodes": [
    {
      "id": "uuid",
      "parent_id": null,             // null = a root node
      "title": "rate limiting",
      "inherited_summary": "…",      // context frozen from ancestors at branch time
      "promoted_digest": "…",        // conclusions promoted up from this node's branches
      "pinned": false,
      "position": { "x": 120, "y": 40 },   // null = never dragged; lay it out yourself
      "notes": [
        { "id": "uuid", "title": "wire protocol", "in_context": true, "updated_at": "…" }
      ],
      "created_at": "…"
    }
  ]
}
```

Read-only, and only structure: titles, summaries, note titles. **No message
bodies and no note bodies.** A client draws the tree from this response and
fetches the transcript for the one node the user actually opened — returning
every message would grow the payload without bound for exactly the accounts
that sync the most.

`version` is checked before parsing. Three clients read this route on three
release schedules, so a client that meets a version it doesn't know must say so
rather than guess at a tree.

`synced_at` is advisory today. It is returned from the first version so clients
can start recording it, because the write direction will need it.

There is no write direction yet, deliberately: pull before push. An export has
no conflict resolution to get wrong, and reading the Web tree into a local
store is most of what synchronisation is for. Writing back needs revisions,
tombstones and merge rules, and designing those under the pressure of having
already shipped the easy half is how a sync protocol goes bad.

This route needs a Cymose account. It is not part of a turn: 0.1 sends
inference straight to OpenRouter on the user's own key regardless of whether
anything here is ever called.
