# rez package for md-librarian.
#
# md-librarian is a LIBRARY repo first — usdlite and yaams-tk2 consume the crates
# as cargo git dependencies pinned to a tag, and that is unchanged by this file.
# What a rez package adds is the runnable half: the viewer binary, the webview
# demo, and the rendered book as a discoverable library root, deployable and
# resolvable like any other tool package.
#
# So: `rez env md_librarian` is how you RUN the viewer without a checkout;
# `rez build -i` is how you BUILD it into a package; `rez test md_librarian` is
# how you TEST — smoke tests against the installed tools by default, and the
# workspace's own `cargo test` via the explicit `cargo_test` test, run from a
# checkout (see REZ.md).
#
# The rez package name is `md_librarian`, not `md-librarian`: rez reads `-` as
# the name/version separator, so a hyphen is not a legal package name.
#
# NOTE: package.py is evaluated both at build (source tree present) and at
# resolve (only this installed file present), so version is derived by an
# @early() reader that runs at BUILD time only.

name = "md_librarian"


# Read the version from Cargo.toml's [workspace.package] — the single source of
# truth, the same value the `vX.Y.Z` tags and the CHANGELOG headings carry.
# @early evaluates ONCE at build/release time and its return is BAKED into the
# installed package, so resolve time never re-reads Cargo.toml. rez does not
# expose the package's own path to @early, but a build runs from the package
# directory, so Cargo.toml sits in the working directory. `tomllib` is stdlib
# on the Python 3.11+ rez runs under.
@early()
def version():
    import os
    import tomllib

    cargo_toml = os.path.join(os.getcwd(), "Cargo.toml")
    if not os.path.isfile(cargo_toml):
        raise RuntimeError(
            "md_librarian package.py: Cargo.toml not found in %s — run `rez build` "
            "/ `rez release` from the repository root (where package.py lives)."
            % os.getcwd()
        )
    with open(cargo_toml, "rb") as f:
        cargo = tomllib.load(f)
    # Cargo requires three-component semver, so a development round carries the
    # pre-release form `X.Y.Z-beta.N`; rez wants dot-separated tokens
    # (`X.Y.Z.beta.N`, which sorts ABOVE the bare `X.Y.Z` — see REZ.md).
    # Release versions contain no `-` and pass through unchanged.
    return cargo["workspace"]["package"]["version"].replace("-", ".")


authors = ["Jonathan Gerber"]

description = (
    "A library of mdbooks: discovery on a search path, a loopback server with "
    "a card page, and a floating WebKit viewer — packaged as the md-librarian "
    "tool, the webview demo, and this project's own book as a library root."
)

# Nothing to resolve. The viewer links WebKitGTK and GTK3, which are SITE
# INFRASTRUCTURE (the graphical session itself), not rez packages. The Rust
# toolchain is a BUILD-time requirement only, and rez_build.sh finds it via
# rustup; see REZ.md.
requires = []

# One entry per runnable thing the workspace produces:
#
#   md-librarian              md-librarian-cli      the book library viewer
#   md-librarian-webview-demo md-librarian-webview  the floating help window (examples/help.rs)
#
# The demo is a cargo EXAMPLE upstream — `cargo run -p md-librarian-webview
# --example help` in a checkout — installed here under a prefixed name because
# `help` is far too generic to put on a shared PATH. `md-librarian` (discovery)
# and `md-librarian-serve` have no runnable target of their own; they are
# exercised through the viewer and by the `cargo_test` test.
tools = [
    "md-librarian",
    "md-librarian-webview-demo",
]

build_command = "bash {root}/rez_build.sh"


def commands():
    # The installed viewer and demo.
    env.PATH.prepend("{root}/bin")

    # This repo's own book, shipped as a discoverable library ROOT (not just
    # rendered HTML): {root}/books/md-librarian/ holds the book.toml beside its
    # output, which is what `md-librarian` scans for. So a resolve opens with
    # something in it rather than an empty library.
    #
    # APPENDED, never prepended. The search path is first-root-wins, so
    # appending puts the shipped copy LAST — a user's own root, or a site's,
    # shadows it. What we ship is the fallback, not the override.
    env.MD_LIBRARIAN_PATH.append("{root}/books")

    # WORKAROUND, and deliberately a temporary one: GTK/WebKit's dmabuf path
    # violates the Wayland explicit-sync protocol on some driver/compositor
    # combinations, and the compositor answers by closing the connection —
    #
    #   wp_linux_drm_syncobj_surface_v1 error 4, "Missing acquire timeline"
    #   Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.
    #
    # Every window the webview opens dies that way, so `md-librarian` starts,
    # prints its URL, and shows nothing. Seen on NVIDIA (nvidia-open-dkms
    # 610.57.04) under Hyprland 0.56.2 with webkit2gtk-4.1 2.52.6; there is no
    # compositor-side switch left, since Hyprland 0.56 removed
    # render:explicit_sync. See book/src/library.md for the full diagnosis.
    #
    # This costs WebKit's dmabuf fast path and keeps the window on native
    # Wayland. It is NOT a property of this package — it is a bug in a
    # combination of other people's software — so REMOVE IT once that
    # combination is fixed.
    #
    # Guarded rather than assigned, so an explicit value from a user or a site
    # always wins: a variable that names a policy should defer to one that is
    # already set.
    if "WEBKIT_DISABLE_DMABUF_RENDERER" not in env:
        env.WEBKIT_DISABLE_DMABUF_RENDERER = "1"


tests = {
    # Every tool resolves on PATH from the resolved env — a smoke test of the
    # install plus commands(). Runs by default under `rez test`.
    "tools_present": {
        "command": "command -v md-librarian && command -v md-librarian-webview-demo",
        "run_on": "default",
    },
    # The shipped book is actually a discoverable ROOT (book.toml beside its
    # output) and is on the search path — the two halves of "a resolve opens
    # with something in it". Cheap and display-free, so it runs by default.
    "books_root": {
        "command": (
            "test -f \"$REZ_MD_LIBRARIAN_ROOT/books/md-librarian/book.toml\" "
            "&& test -f \"$REZ_MD_LIBRARIAN_ROOT/books/md-librarian/html/index.html\" "
            "&& case \":$MD_LIBRARIAN_PATH:\" in *\":$REZ_MD_LIBRARIAN_ROOT/books:\"*) true ;; "
            "*) echo \"books root not on MD_LIBRARIAN_PATH\" >&2; false ;; esac"
        ),
        "run_on": "default",
    },
    # The viewer really binds and serves: --no-window prints its URL to stdout
    # and nothing else, so matching that line is an end-to-end check of the
    # install plus commands(). `timeout` bounds it because the viewer serves
    # until killed, by design. Explicit rather than default: it binds a port.
    "books_serve": {
        "command": "timeout 5 md-librarian --no-window | head -1 | grep -q '^http://127.0.0.1:'",
        "run_on": "explicit",
    },
    # The workspace's own test suite. Explicit, and it needs a SOURCE CHECKOUT:
    # this package ships binaries, not crates. The script takes the source
    # directory from its argument, then $MD_LIBRARIAN_SRC, then the current
    # directory — so the usual invocation is `rez test md_librarian cargo_test`
    # from the repo root.
    "cargo_test": {
        "command": "bash {root}/scripts/rez-cargo-test.sh",
        "run_on": "explicit",
    },
}
