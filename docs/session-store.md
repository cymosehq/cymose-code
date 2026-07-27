# Session store

SQLite, one file per machine at `~/.local/share/cymose/sessions.db` (respecting
`XDG_DATA_HOME`; `%APPDATA%\cymose` on Windows). Workspaces are rows, not
files, so one store holds every project.

The schema is the contract between the TUI and the extension. It is created and
migrated by `store::migrate`, which runs on every open and is idempotent.

## Tables

```sql
workspaces (id, root_path, name, web_node_id, created_at)
sessions   (id, workspace_id, parent_id, title, status, model, web_node_id,
            created_at, ended_at)
messages   (id, session_id, role, content, tokens_in, tokens_out, created_at)
summaries  (session_id, text, files_touched, outcome, created_at)
artifacts  (id, session_id, path, before, after, created_at)
```

- `sessions.parent_id` is the tree edge. A root session has `NULL`.
- `sessions.status` is one of `pending`, `running`, `done`, `failed`.
- `summaries` is what a child inherits. `messages` is not — the whole point of
  the design is that transcripts stay where they were written.
- `artifacts` holds the before/after snapshots that make `cymose diff` work
  without touching git.

## Concurrency

Both clients can have the store open at once — a terminal in one window, VS
Code in another. The store is opened in WAL mode with a busy timeout, so
concurrent readers and one writer are fine. What is not solved yet:

- **Change notification.** A session created in the terminal does not push
  itself into an open VS Code tree; the extension currently polls on focus.
  Options are a file watch on the WAL, or electing a single sidecar as the
  writer that others subscribe to. Undecided.
- **Long writes.** A turn that streams for a minute must not hold a write
  transaction for a minute. Messages are appended per chunk-flush, not per turn.

## Migrations

`store::MIGRATIONS` is an append-only list. Each entry is applied once and
recorded in `schema_version`. Never edit a migration that has shipped — a user
with an existing store will not re-run it, and the two clients will disagree
about the schema, which is the one failure mode this design is supposed to
prevent.

## Privacy

The store contains source code, prompts, model output and file snapshots. It is
local, it is in `.gitignore`, and nothing in this repository uploads it. What
crosses the network is what the user explicitly promotes, plus the inference
calls a turn requires.
