#!/bin/sh
# Cymose Code installer.
#
#   curl -fsSL https://cymose.dev/install.sh | sh
#
# Downloads the release binary for this platform, verifies it against the
# published checksums, and puts it on your PATH. No build toolchain, no cargo,
# no sudo unless the install directory needs it.
#
# Set CYMOSE_INSTALL_DIR to choose where it lands; the default is ~/.local/bin,
# which is on PATH for most shells and never needs root.

set -eu

REPO="cymosehq/cymose-code"
INSTALL_DIR="${CYMOSE_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required"; }
need curl
need tar

# Rust target triples, which is what the release assets are named after.
case "$(uname -s)" in
	Linux)  os="unknown-linux-gnu" ;;
	Darwin) os="apple-darwin" ;;
	*) die "unsupported OS: $(uname -s). On Windows use the PowerShell command on https://cymose.dev/code/terminal" ;;
esac

case "$(uname -m)" in
	x86_64|amd64) arch="x86_64" ;;
	arm64|aarch64) arch="aarch64" ;;
	*) die "unsupported architecture: $(uname -m)" ;;
esac

# There is no aarch64 Linux build yet. Saying so beats a 404 from curl.
if [ "$os" = "unknown-linux-gnu" ] && [ "$arch" = "aarch64" ]; then
	die "no prebuilt binary for aarch64 Linux yet — build from source: https://github.com/$REPO"
fi

target="${arch}-${os}"
asset="cymose-${target}.tar.gz"
base="https://github.com/$REPO/releases/latest/download"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "Downloading $asset…"
curl -fsSL "$base/$asset" -o "$tmp/$asset" || die "no release asset for $target yet"

# Verify. A pipe-to-shell installer that doesn't check what it downloaded is
# asking you to trust the network as well as the project.
if curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS" 2>/dev/null; then
	if command -v sha256sum >/dev/null 2>&1; then
		expected="$(grep " $asset\$" "$tmp/SHA256SUMS" | awk '{print $1}')"
		actual="$(sha256sum "$tmp/$asset" | awk '{print $1}')"
	elif command -v shasum >/dev/null 2>&1; then
		expected="$(grep " $asset\$" "$tmp/SHA256SUMS" | awk '{print $1}')"
		actual="$(shasum -a 256 "$tmp/$asset" | awk '{print $1}')"
	else
		expected=""; actual=""
		say "warning: no sha256 tool found; skipping checksum verification"
	fi
	if [ -n "$expected" ] && [ "$expected" != "$actual" ]; then
		die "checksum mismatch — refusing to install"
	fi
else
	say "warning: no SHA256SUMS in the release; skipping checksum verification"
fi

tar xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$INSTALL_DIR"
mv "$tmp/cymose" "$INSTALL_DIR/cymose"
chmod +x "$INSTALL_DIR/cymose"

say ""
say "Installed cymose to $INSTALL_DIR/cymose"

# Being on PATH is the difference between installed and usable, and the failure
# is silent otherwise — the user types `cymose` and gets "command not found"
# from a successful install.
case ":$PATH:" in
	*":$INSTALL_DIR:"*) ;;
	*)
		say ""
		say "$INSTALL_DIR is not on your PATH. Add this to your shell profile:"
		say "  export PATH=\"$INSTALL_DIR:\$PATH\""
		;;
esac

say ""
say "Next: export OPENROUTER_API_KEY=sk-or-v1-…   then   cymose init && cymose"
