#!/usr/bin/env bash
#
# Run the workspace's own test suite from inside a rez environment.
#
# This backs package.py's explicit `cargo_test` test:
#
#     cd /path/to/md-librarian && rez test md_librarian cargo_test
#
# The rez package ships BINARIES, not crates, so there is nothing to `cargo test`
# inside {root} — the suite needs a source checkout. Find one in this order:
#
#   1. $1                — an explicit path (rez test ... -- <path> passes extra args)
#   2. $MD_LIBRARIAN_SRC   — for a fixed checkout (CI, a shared build box)
#   3. the current directory — the usual case: run rez test from the repo root
#
# Anything after the source path is passed through to cargo, so
# `rez test md_librarian cargo_test -- -p md-librarian` narrows the run.
set -euo pipefail

src=""
if [ "$#" -gt 0 ] && [ -d "${1:-}" ]; then
    src="$1"
    shift
elif [ -n "${MD_LIBRARIAN_SRC:-}" ]; then
    src="$MD_LIBRARIAN_SRC"
else
    src="$(pwd)"
fi

if [ ! -f "$src/Cargo.toml" ] || ! grep -q '^\[workspace\]' "$src/Cargo.toml"; then
    cat >&2 <<MSG
rez-cargo-test: no md-librarian workspace at '$src'.

The rez package ships the built tools, not the crates, so the test suite needs a
source checkout. Either run this from the repository root:

    cd /path/to/md-librarian && rez test md_librarian cargo_test

or point it at one:

    MD_LIBRARIAN_SRC=/path/to/md-librarian rez test md_librarian cargo_test
MSG
    exit 1
fi

# rez resolves in a clean environment, so rustup's shims are not on PATH.
if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
fi
export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || {
    echo "rez-cargo-test: 'cargo' not found — install Rust (rustup)." >&2
    exit 1
}

cd "$src"
echo "rez-cargo-test: cargo test --workspace --locked in $src"
# --locked for the same reason the build uses it: test what Cargo.lock records.
exec cargo test --workspace --locked "$@"
