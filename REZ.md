# Deploying md-librarian as a rez package

`package.py` ships md-librarian as a [rez](https://github.com/AcademySoftwareFoundation/rez)
package named **`md_librarian`** (rez reads `-` as the name/version separator,
so the hyphenated repo name is not a legal package name).

This repo is a **library** first: usdlite and yaams-tk2 consume the crates as
cargo git dependencies pinned to a `vX.Y.Z` tag, and rez changes nothing about
that — a rez package holds built artifacts, not crates. What the package adds
is the runnable half: the viewer, the webview demo, and the rendered book as a
library root, deployable and resolvable like any other tool package.

The developer loop is unchanged: `just build`, `just test`, `just run`,
`just install`.

## What the package contains

| Tool | Crate | What it is |
|---|---|---|
| `md-librarian` | `md-librarian-cli` | the book library viewer (serves an `MD_LIBRARIAN_PATH` of mdbooks) |
| `md-librarian-webview-demo` | `md-librarian-webview` | the floating WebKit help window (`examples/help.rs`, renamed because `help` is too generic for a shared `PATH`) |

`md-librarian` (discovery) and `md-librarian-serve` have no runnable target of
their own; they are exercised through the viewer and by the `cargo_test` test.

Also installed:

- **the book, as a discoverable library root** — rendered fresh by `mdbook build`
  into `{root}/books/md-librarian/`, with its `book.toml` and its `cover.svg`
  beside its `html/` output. That layout is what the viewer scans for.
  Skipped, with a message, when `mdbook` is absent or
  `MD_LIBRARIAN_SKIP_BOOK=1`.
- **`{root}/scripts/rez-cargo-test.sh`**, which backs the `cargo_test` test.

`commands()` prepends `{root}/bin` to `PATH` and **appends** `{root}/books` to
`MD_LIBRARIAN_PATH` — appended, because the search path is first-root-wins, so
what we ship is the fallback and a user's own root shadows it.

`commands()` also exports **`WEBKIT_DISABLE_DMABUF_RENDERER=1`**, guarded so a
value already in the environment wins. That is a **temporary workaround, not a
property of this package**: GTK/WebKit's dmabuf path violates the Wayland
explicit-sync protocol on some driver/compositor combinations and the
compositor closes the connection, so the viewer starts, prints its URL, and
shows no window. Remove it once that combination is fixed — see the book's
"Known issue: the window never appears" for the diagnosis.

## Dependencies

`requires = []`. The viewer links WebKitGTK and GTK3 at runtime — the graphical
session itself, i.e. **site infrastructure**, not rez packages. The Rust
toolchain is a **build-time** requirement only; `rez_build.sh` sources
`~/.cargo/env` because rez builds in a clean environment.

## Building

```sh
rez build -i          # ~/packages
rez release           # a real deployment
```

Every `cargo` invocation passes `--locked`, so the package builds exactly what
`Cargo.lock` records.

| Variable | Effect |
|---|---|
| `MD_LIBRARIAN_RUN_TESTS=1` | also run `cargo test --workspace` before installing |
| `MD_LIBRARIAN_SKIP_BOOK=1` | do not render or ship the book even if `mdbook` is installed |

### Isolated build + test

Does not touch `~/packages`:

```sh
PKG=/tmp/rez-md-librarian
rez build -i --prefix "$PKG"
rez test md_librarian --paths "$PKG:$(rez config packages_path | sed 's/^ *- *//' | tr '\n' ':')"
rez env md_librarian --paths "$PKG:…" -- md-librarian --no-window
```

## Running

```sh
rez env md_librarian -- md-librarian                # the library, opening on the shipped book
rez env md_librarian -- md-librarian --no-window    # serve only; prints the URL (works over ssh -L)
rez env md_librarian -- md-librarian-webview-demo
```

## Tests

```sh
rez test md_librarian                      # the default tests
rez test md_librarian --list               # everything, including explicit ones
```

| Test | Runs by default | What it checks |
|---|---|---|
| `tools_present` | yes | both tools resolve on `PATH` |
| `books_root` | yes | the shipped book is a real root (`book.toml` beside `html/index.html`) *and* is on `MD_LIBRARIAN_PATH` |
| `books_serve` | no | `md-librarian --no-window` binds and prints its URL — end to end, but it binds a port |
| `cargo_test` | no | the workspace's own `cargo test --workspace --locked` |

`cargo_test` needs a **source checkout** — the package ships binaries, not
crates. The shipped script takes the source directory from its argument, then
`$MD_LIBRARIAN_SRC`, then the current directory:

```sh
cd /path/to/md-librarian && rez test md_librarian cargo_test
rez test md_librarian cargo_test -- -p md-librarian     # extra args go to cargo
```

## Versioning

The version comes from `Cargo.toml`'s `[workspace.package]` via an `@early()`
reader in `package.py`, baked into the installed package at build time.

Cargo requires three-component semver, so a development round carries
`X.Y.Z-beta.N`; the reader translates that to rez's dot-separated `X.Y.Z.beta.N`.
Beware: **rez has no pre-release concept and sorts more tokens higher**, so
`0.1.0.beta.1` outranks a later stable `0.1.0` permanently. `rez_build.sh`
warns when it sees a pre-release version, and the fix is to release at the
*next* patch (`0.1.1`), not to simply drop the suffix.
