# Releasing

Everything below is driven by a tag. Nothing publishes on a normal push — a
release should be a decision, not a side effect of merging.

```sh
# 1. Bump the version in three places, or the release lies about itself:
#      Cargo.toml            [workspace.package] version
#      extension/package.json  version
#      extension/manifest?    (none — VS Code reads package.json)
# 2. Commit, tag, push.
git tag v0.1.0-beta.1
git push origin v0.1.0-beta.1
```

The `release` workflow then:

1. builds `cymose` for macOS (arm64 + x86_64), Linux x86_64 and Windows x86_64;
2. bundles all four binaries into the VS Code extension and packages a `.vsix`;
3. publishes the extension to the Marketplace **if** `VSCE_PAT` is set;
4. attaches every archive, the `.vsix` and a `SHA256SUMS` file to a GitHub
   release.

A tag containing `beta` or `rc` is marked as a prerelease, which keeps
`releases/latest` — the URL `scripts/install.sh` fetches — pointing at the last
stable build.

## Publishing the VS Code extension

One-time setup. The Marketplace is not GitHub: it authenticates through Azure
DevOps, which is the part that surprises everyone.

1. **Create an Azure DevOps organisation** at
   [dev.azure.com](https://dev.azure.com) with the account you want to own the
   listing. The organisation itself is never seen by users.
2. **Create a Personal Access Token**: User settings → Personal access tokens →
   New token. Set **Organization: All accessible organizations** — a token
   scoped to one organisation fails with a misleading 401 — and scope
   **Marketplace → Manage**. Copy it; it is shown once.
3. **Create the publisher** at
   [marketplace.visualstudio.com/manage](https://marketplace.visualstudio.com/manage/createpublisher).
   The publisher id must equal `publisher` in `extension/package.json`
   (`cymose`).
4. **Add the token to this repository**: Settings → Secrets and variables →
   Actions → New repository secret, named `VSCE_PAT`.

After that, tagging publishes. Without the secret the workflow still builds and
attaches the `.vsix`, so a release is never blocked on it — you can upload that
file by hand at the manage page above.

To publish from your own machine instead:

```sh
cd extension
npx @vscode/vsce login cymose      # paste the PAT
npx @vscode/vsce publish --no-dependencies
```

**Before the first publish**, the listing needs an icon (128×128 PNG,
`extension/icon.png`, referenced as `"icon"` in `package.json`) and a
`repository` field — the Marketplace rejects a package without the first and
warns about the second. Neither exists yet.

## Checklist for a release that isn't a beta

- [ ] `cargo test --all` and `cargo clippy --all-targets -- -D warnings` pass
- [ ] `npm --prefix extension run compile` passes
- [ ] The works/doesn't table in [README.md](README.md) is still true
- [ ] `CHANGELOG` entry, if one exists by then
- [ ] The version is the same in `Cargo.toml` and `extension/package.json`
