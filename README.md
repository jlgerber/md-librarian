# md-librarian

A viewer for a **library of mdbooks**.

Point it at one or more directories of built [mdbook](https://rust-lang.github.io/mdBook/)
books and it finds them, serves them over a loopback HTTP origin, and opens a
window with a card per book, a persistent bar across the top, and a way back to
the library from any page. Because the books are served rather than opened as
`file://`, mdbook's full-text search and relative links work as they do on the
web, and the bar can tell which book and chapter you are reading.

It is built for applications that want a *Help → Documentation* menu without
compiling their docs into the binary: the application launches `md-librarian`
as a separate process, and any book installed on the machine shows up.

## Quick start

```sh
cargo install --path crates/md-librarian-cli      # needs WebKitGTK, see Building
mkdir -p ~/books && cp -r ~/src/my-book ~/books/  # a book with its build output inside
md-librarian build ~/src/my-book --into ~/books   # or let it build and install the book
MD_LIBRARIAN_PATH=~/books md-librarian
```

A **root** is a directory whose subdirectories are books. A subdirectory is a
book if it holds a `book.toml`; its built output is found beside it, honouring
`[build] build-dir` if the book sets one:

```text
~/books/
├── my-book/
│   ├── book.toml
│   ├── cover.png         # optional; png, svg, jpg, jpeg, webp
│   ├── src/
│   └── book/             # or wherever build-dir points
└── another-book/
    ├── book.toml
    └── html/
```

Roots come from `--root`, else the **`MD_LIBRARIAN_PATH`** environment
variable (a colon-separated list), else `$XDG_DATA_HOME/md-librarian/books`.
Roots stack and the first one wins, so a user's own copy of a book shadows one
shipped by a package. A book is identified by its `[book] title`, falling back
to the directory name.

### Covers

To give a book a cover on the card page, put an image named **`cover.<ext>`**
beside its `book.toml`, next to `src/`, not inside it:

```text
my-book/
├── book.toml
├── cover.png
└── src/
```

The extensions looked for are `png`, `svg`, `jpg`, `jpeg` and `webp`, in that
order; the first one present wins. The cover lives inside the book rather than
in a per-root index, so a book copied to another root brings its cover along.
Nothing goes in `book.toml`: mdbook rejects keys it does not know, so a cover
cannot be declared there.

**Size**: the card page shows the cover in a landscape box 150px tall and
roughly 240 to 350px wide, depending on how many columns fit the window, and
crops it to fill (`object-fit: cover`), so the edges are what gets lost. Make
it **16:9**, at least **640 × 360** so it stays sharp on HiDPI screens; the
generated cover uses the same 320 × 180 proportions. Keep the subject centred,
since a wider window trims the top and bottom and a narrower one trims the
sides.

A book without a cover gets a generated one, the title's initial on a colour
derived from the whole title. It is deterministic, so the same book looks the
same in every library, and adding a book never recolours the others.

## Command line

```sh
md-librarian                                  # MD_LIBRARIAN_PATH, else the XDG default
md-librarian --root ~/books --root /opt/books # explicit roots; the first wins
md-librarian --include "User Guide"           # only these titles
md-librarian --book "User Guide"              # open straight onto one book
md-librarian --no-window                      # serve only; prints the URL
md-librarian build                            # mdbook-build every stale book on the roots
md-librarian build ~/src/foo/docs --into ~/books   # build one book, install a slim copy
```

| Flag | Effect |
|---|---|
| `--root <DIR>` | A root; repeatable. Overrides `MD_LIBRARIAN_PATH`. Earlier wins. |
| `--include <TITLE>` | Show only these titles; repeatable. A listed title no root provides is shown as a dead card rather than silently dropped. |
| `--book <TITLE>` | Open onto this book instead of the library, with the bar and the way back still there. Falls back to the library if the book is missing. |
| `--no-window` | Serve without a window and print the URL. For headless machines, `ssh -L`, or driving the pages from a normal browser. |
| `--exit-on-stdin-close` | Exit when stdin reaches EOF, which is how an application ties the viewer's lifetime to its own. |
| `--parent-pipe <FD>` | Like the above, watching an inherited pipe instead of stdin. |

`build` takes optional book directories, the same `--root` and `--include`,
plus `--force` (rebuild everything) and `--into <ROOT>` (install a slim copy
of each book: `book.toml`, cover, rendered output). It needs `mdbook` on
`PATH`. The [library chapter](book/src/library.md#building-the-library) has
the staleness rule and what `--into` refuses.

## Building

The workspace is plain Rust. Two crates link WebKitGTK through
[wry](https://github.com/tauri-apps/wry) and [tao](https://github.com/tauri-apps/tao),
so the system needs the development packages for **webkit2gtk 4.1** and
**GTK 3** to build the viewer:

| Distribution | Packages |
|---|---|
| Arch | `webkit2gtk-4.1 gtk3` |
| Debian / Ubuntu | `libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libssl-dev pkg-config` |

The two library crates, `md-librarian` and `md-librarian-serve`, have no GTK
dependency and their tests run headless.

### With cargo

```sh
cargo build --workspace                       # everything, including the binary
cargo test --workspace                        # 29 tests, no display needed
cargo run --bin md-librarian -- --root .      # the viewer, on this repo's own book
cargo build -p md-librarian-webview --examples
cargo install --path crates/md-librarian-cli  # puts `md-librarian` in ~/.cargo/bin
```

`cargo run --bin md-librarian -- --root .` finds `book/book.toml` because the
repository's own documentation is laid out as a library root. Build it first
with `mdbook build book`.

### With just

[just](https://github.com/casey/just) wraps the same commands with the
project's conventions:

```sh
just                 # list the recipes
just build           # cargo build --workspace
just test            # cargo test --workspace
just docs-build      # mdbook build book   (the docs, and the book `just run` opens)
just docs            # mdbook serve, with live reload
just run             # the viewer on this repo's own book; args pass through
just webview-demo    # the floating-window example on its own
just install         # cargo install the viewer
```

`just run` sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` unless it is already set;
see [Known issue](#known-issue-the-window-never-appears) for why.

### With rez

The repository is also a [rez](https://github.com/AcademySoftwareFoundation/rez)
package named **`md_librarian`** (rez does not allow a hyphen in a package
name). The package holds the built viewer, the webview demo, and this
repository's rendered documentation as a library root, which it appends to
`MD_LIBRARIAN_PATH` so a fresh resolve opens with something in it.

```sh
rez build -i                              # build and install into ~/packages
just rez-build-isolated                   # the same, into /tmp/rez-md-librarian
rez test md_librarian                     # the default tests: tools on PATH, book root present
rez test md_librarian books_serve         # binds a port and checks the URL is printed
rez env md_librarian -- md-librarian      # run it from a resolve
```

`rez_build.sh` needs `cargo` (via rustup) and, optionally, `mdbook`; without
mdbook the package is built without the documentation root. Set
`MD_LIBRARIAN_RUN_TESTS=1` to run the test suite during the build, or
`MD_LIBRARIAN_SKIP_BOOK=1` to skip the documentation. [REZ.md](REZ.md) covers
the rest, including the pre-release versioning trap rez has.

## Using it from an application

Launch the viewer out of process and hand it a pipe. When your process exits,
however it exits, the pipe closes and the viewer follows:

```rust
use std::process::{Child, ChildStdin, Command, Stdio};

pub struct Viewer {
    child: Child,
    _keepalive: ChildStdin, // dropping this closes the pipe; the viewer exits
}

pub fn open_library() -> std::io::Result<Viewer> {
    let mut child = Command::new("md-librarian")
        .arg("--exit-on-stdin-close")
        .stdin(Stdio::piped())
        .spawn()?;
    let keepalive = child.stdin.take().expect("stdin was piped");
    Ok(Viewer { child, _keepalive: keepalive })
}
```

To ask whether a book is installed before offering it in a menu, depend on the
discovery crate alone. It pulls in no server and no GTK:

```toml
[dependencies]
md-librarian = { git = "https://github.com/jlgerber/md-librarian", tag = "v0.1.0" }
```

```rust
use md_librarian::{library, roots, Entry};

let installed = library(&roots(&[]), None).into_iter().any(|e| match e {
    Entry::Book(b) => b.title == "User Guide" && b.is_built(),
    Entry::Missing { .. } => false,
});
```

The [Getting started](book/src/getting-started.md) chapter has the details.

## Crates

| Crate | What it is | Links GTK? |
|---|---|---|
| `md-librarian` | discovery: roots, titles, shadowing, covers | no |
| `md-librarian-serve` | the loopback server and the generated shell page | no |
| `md-librarian-webview` | a floating WebKit window, reusable on its own | yes |
| `md-librarian-cli` | the `md-librarian` binary, tying the three together | yes |

## Known issue: the window never appears

On some Wayland driver and compositor combinations GTK reports
`Error 71 (Protocol error) dispatching to Wayland display` and every WebKitGTK
window dies at once. Set `WEBKIT_DISABLE_DMABUF_RENDERER=1`. The justfile and
the rez package do this for you; the diagnosis is in the
[library chapter](book/src/library.md#known-issue-the-window-never-appears-wayland--explicit-sync).

## Documentation

The full documentation is an mdbook under [`book/`](book/src/SUMMARY.md):
build it with `just docs-build`, or open it in the viewer itself with `just run`.

## License and provenance

MIT. Extracted from [gpui-yaams](https://github.com/jlgerber/gpui-yaams) at
v0.27.0-beta.3, where these crates were developed as `yaams-booklib`,
`yaams-bookserve`, `yaams-webview` and `yaams-books`.
