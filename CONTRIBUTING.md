# Contributing

## How this code gets written

The code here is written by an AI coding agent working from the maintainer's
direction — see [the note in the README](README.md#this-code-was-written-by-an-ai).
The maintainer is not a programmer and does not hand-write the code.

Two things follow for you as a contributor:

- **Review carries more weight here than usual.** A pull request that points at
  a wrong assumption is worth more than one that fixes a typo, and there is no
  senior author who already caught the obvious things.
- **Use whatever tools you like, and say so.** AI-assisted contributions are
  welcome on the same terms as any other: you are responsible for what you
  submit, you have read it, and you can explain why it is correct. That bar
  applies to the maintainer too.

## Before anything else: this repository is public

The rest of Cymose is not. Contributions and reviews are expected to keep the
line:

- No credentials, project identifiers, or infrastructure ids — of ours or your
  own.
- No server-side business logic: pricing, quotas, rate-limit algorithms, the
  provider behind a model name, prompts the API runs.
- No database schema or admin endpoint shapes.

There is no automated check for this — read your own diff before you push. The
rule is written out in [CLAUDE.md](CLAUDE.md) and applies to humans and agents
equally.

If you found something in a shipped release that shouldn't be public, don't open
a PR — see [SECURITY.md](SECURITY.md).

## Setup

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

The extension needs Node 20+:

```sh
npm --prefix extension install
npm --prefix extension run compile
```

Then open the repo in VS Code and press F5 to launch an Extension Development
Host against the sidecar built from `crates/cymose-cli`.

## Where code goes

Logic in `cymose-core`, presentation in `cymose-cli` or `extension/`. A feature
that exists in only one client is a bug in the making — see
[docs/architecture.md](docs/architecture.md). If the extension needs something,
add a sidecar method for it rather than a TypeScript implementation of it.

## Style

Rust: `rustfmt` defaults, clippy clean at `-D warnings`. Prefer returning
errors over `unwrap`; `cymose-core` has no `println!`.

Comments explain the decision, not the syntax. If a line looks wrong and isn't,
say why — that comment is worth ten that restate the code.

Commit messages: imperative mood, one concern per commit. Reference an issue
when there is one.

## Tests

Anything touching the store, the context builder, or the router needs a test.
The store tests run against an in-memory SQLite instance; the router tests use a
fake provider that can be told to return `429`. Don't write a test that needs
network or a Cymose account — CI has neither.
