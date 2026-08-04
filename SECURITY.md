# Security

## Reporting

Email **security@cymose.app** with a description, affected version, and
reproduction steps. We aim to acknowledge within three working days.

Please report privately first — including anything published in this repository
that looks like a credential or an internal detail. Do not open a public issue
for it.

## Scope

In scope: the terminal client, the VS Code extension, the sidecar protocol, and
how either client handles tokens and local data.

Out of scope here: the Cymose API and web app. Report those to the same address;
they are separate services.

## What this software does with your data

- The session store is local: source code, prompts, model output, and file
  snapshots. Nothing in this repository uploads it.
- The auth token goes to the OS keychain when one is available, otherwise to
  `~/.config/cymose/credentials.json` at mode `0600`. It is not written to the
  session store and is redacted from logs.
- `run_command` executes commands in your working directory on your behalf. It
  asks before running by default; the allowlist that skips the prompt is yours
  to configure, and a broad one is a broad grant.
- Inference goes to the Cymose API, which means the prompt and the file
  contents a turn includes leave the machine. Sessions are otherwise local
  until you `promote` one.
