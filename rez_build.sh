#!/usr/bin/env bash
#
# rez build for md-librarian: cargo build --release → install the viewer and the
# webview demo into the package's bin/, plus the rendered book as a library
# root. The version comes from Cargo.toml via package.py's @early() reader.
#
# Invoked as `build_command = "bash {root}/rez_build.sh"`. rez sets:
#   REZ_BUILD_SOURCE_PATH  — the repo root (where Cargo.toml lives)
#   REZ_BUILD_INSTALL_PATH — where to install (with -i / on release)
#   REZ_BUILD_INSTALL      — "1" when installing, else "0"
#
# rez is for DEPLOYMENT. The developer loop is `just build`, `just test`,
# `just run`, `just install`.
#
# Knobs:
#   MD_LIBRARIAN_RUN_TESTS=1  also run `cargo test --workspace` before installing.
#   MD_LIBRARIAN_SKIP_BOOK=1  do not render/ship the book even if mdbook is present.
set -euo pipefail

SRC="${REZ_BUILD_SOURCE_PATH:-$(pwd)}"
cd "$SRC"

# --- toolchain ---------------------------------------------------------------
# rez builds in a CLEAN environment, so nothing from a login shell is on PATH.
if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
fi
export PATH="$HOME/.cargo/bin:$PATH"

command -v cargo >/dev/null 2>&1 || {
    echo "rez_build: 'cargo' not found — install Rust (rustup); it is the whole build." >&2
    exit 1
}

# mdbook is OPTIONAL: a package built without it is a complete tools package,
# just without the docs. Decide (and say so) up front rather than after the
# compile.
ship_book=1
if [ "${MD_LIBRARIAN_SKIP_BOOK:-0}" = "1" ]; then
    ship_book=0
    echo "rez_build: MD_LIBRARIAN_SKIP_BOOK=1 — the book will not be shipped"
elif ! command -v mdbook >/dev/null 2>&1; then
    ship_book=0
    echo "rez_build: 'mdbook' not found — the book will not be shipped (cargo install mdbook to include it)"
fi

# --- version sanity ----------------------------------------------------------
# rez has no pre-release concept and sorts MORE tokens HIGHER, so `X.Y.Z.beta.N`
# outranks the stable `X.Y.Z` forever. Warn loudly rather than let someone
# discover it after `rez release` — and name the actual fix, which is NOT simply
# dropping the suffix: a bare `X.Y.Z` still loses to `X.Y.Z.beta.N`.
proj_version="$(python3 -c "import tomllib;print(tomllib.load(open('Cargo.toml','rb'))['workspace']['package']['version'])" 2>/dev/null || echo "")"
case "$proj_version" in
  *-*) base="${proj_version%%-*}"
       next="${base%.*}.$(( ${base##*.} + 1 ))"
       echo "rez_build: WARNING — version '$proj_version' is a pre-release." >&2
       echo "  rez sorts '${proj_version//-/.}' ABOVE '$base', so releasing at" >&2
       echo "  '$base' would leave this beta shadowing it permanently." >&2
       echo "  Build it (-i) for testing; release as '$next' — increment the" >&2
       echo "  patch when you drop the suffix, do not just remove it." >&2 ;;
esac

# --- compile -----------------------------------------------------------------
# --locked: the package must build the dependency set Cargo.lock records.
#
# Two invocations: the workspace (which produces the `md-librarian` binary),
# and the webview demo, which is a cargo EXAMPLE and so is not built by the
# first call.
echo "rez_build: cargo build --release — workspace (md-librarian)"
cargo build --release --locked --workspace

echo "rez_build: cargo build --release — webview demo (example help)"
cargo build --release --locked -p md-librarian-webview --example help

# --- optional: the workspace test suite --------------------------------------
if [ "${MD_LIBRARIAN_RUN_TESTS:-0}" = "1" ]; then
    echo "rez_build: MD_LIBRARIAN_RUN_TESTS=1 — cargo test --workspace"
    cargo test --workspace --locked
fi

# --- install (only when rez is installing, not a bare build) -----------------
if [ "${REZ_BUILD_INSTALL:-0}" != "1" ]; then
    echo "rez_build: compile-only (REZ_BUILD_INSTALL != 1) — nothing installed"
    exit 0
fi

# `set -u` catches an UNSET variable, not an empty one — and an empty install
# path would make `dest` /bin and the book wipe `rm -rf /books`.
: "${REZ_BUILD_INSTALL_PATH:?rez_build: REZ_BUILD_INSTALL_PATH is empty}"

dest="$REZ_BUILD_INSTALL_PATH/bin"
mkdir -p "$dest"
install -m 0755 "target/release/md-librarian" "$dest/md-librarian"
# The example is renamed on the way in: `help` is far too generic for a shared
# PATH, and package.py's `tools` lists the prefixed name.
install -m 0755 "target/release/examples/help" "$dest/md-librarian-webview-demo"
echo "rez_build: installed 2 tools → $dest"

# --- the cargo-test helper ---------------------------------------------------
# package.py's `cargo_test` test runs this out of the installed package, against
# a source checkout the caller points it at.
mkdir -p "$REZ_BUILD_INSTALL_PATH/scripts"
install -m 0755 "$SRC/scripts/rez-cargo-test.sh" "$REZ_BUILD_INSTALL_PATH/scripts/rez-cargo-test.sh"

# --- the book, as a discoverable library root --------------------------------
# Rendered fresh rather than copied from book/html, so a stale local render can
# never be what ships.
#
# The layout is what `md-librarian` scans for — a root of book directories,
# each holding its `book.toml` beside its output — NOT bare HTML: discovery keys
# on `book.toml`, and the title, description and `build-dir` all come out of it.
# `build-dir = "html"` in our book.toml is why the output lands in `html/`.
# package.py APPENDS this root to MD_LIBRARIAN_PATH, so a user's own roots
# shadow it.
if [ "$ship_book" = "1" ]; then
    mdbook build book
    books_dest="$REZ_BUILD_INSTALL_PATH/books/md-librarian"
    rm -rf "$REZ_BUILD_INSTALL_PATH/books"
    mkdir -p "$books_dest"
    install -m 0644 "$SRC/book/book.toml" "$books_dest/book.toml"
    cp -r "$SRC/book/html" "$books_dest/html"
    echo "rez_build: book shipped as a library root → $books_dest"
fi

echo
echo "──────────────────────────────────────────────────────────────"
echo " rez_build — DONE"
echo "   tools -> $dest"
[ "$ship_book" = "1" ] && echo "   books -> $REZ_BUILD_INSTALL_PATH/books (an MD_LIBRARIAN_PATH root)"
echo "──────────────────────────────────────────────────────────────"
