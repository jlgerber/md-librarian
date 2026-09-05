# List the available recipes (default when you run bare `just`).
default:
    @just --list

# Build all crates in the workspace.
build:
    cargo build --workspace

# Run the tests (headless: the library crates never link GTK).
test:
    cargo test --workspace

# WEBKIT_DISABLE_DMABUF_RENDERER works around a Wayland explicit-sync violation
# in GTK/WebKit that kills every WebKitGTK window ("Error 71 (Protocol error)");
# see book/src/library.md. Defaulted, not forced, so an explicit value wins.

# Open the viewer on this repo's own book (build it with docs-build first); extra args pass through.
run *args:
    WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}" \
      cargo run --bin md-librarian -- --root . {{ args }}

# Run the webview demo (the `help` example).
webview-demo *args:
    WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}" \
      cargo run -p md-librarian-webview --example help -- {{ args }}

# Serve the docs (mdbook) with live-reload, opening a browser.
docs:
    mdbook serve book --open

# Build the docs to book/html.
docs-build:
    mdbook build book

# Install the viewer to ~/.cargo/bin.
install:
    cargo install --path crates/md-librarian-cli --bin md-librarian --locked --force

# --- rez packaging (see REZ.md) ----------------------------------------------

# Build + install the rez package (md_librarian) into ~/packages.
rez-build:
    rez build -i

# Build the rez package into an isolated prefix, touching nothing in ~/packages.
rez-build-isolated prefix="/tmp/rez-md-librarian":
    rez build -i --prefix {{ prefix }}

# Run the package's rez tests — bare for the defaults, or name one (`just rez-test cargo_test`).
rez-test *tests:
    rez test md_librarian {{ tests }}

# Run a packaged tool from a resolve: `just rez-run md-librarian --no-window`.
rez-run tool *args:
    rez env md_librarian -- {{ tool }} {{ args }}
