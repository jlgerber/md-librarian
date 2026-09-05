# Getting started

## Install

From a checkout:

```sh
just install          # cargo install --path crates/md-librarian-cli
```

Or resolve the rez package, which also ships this book as a root:

```sh
rez env md_librarian -- md-librarian
```

The binary links WebKitGTK, so those libraries must be installed either way; `--no-window` is what removes the need for a *display*.

## Where books go

A **root** is a directory whose subdirectories each hold a `book.toml` beside
their built output. Roots come from `--root`, else **`MD_LIBRARIAN_PATH`** (a
colon-separated, first-wins list), else `$XDG_DATA_HOME/md-librarian/books`.

```sh
mdbook build ~/src/my-book                      # renders into ~/src/my-book/book
MD_LIBRARIAN_PATH=~/src md-librarian            # ~/src is the root; my-book is found
```

## Launching from an application

Launch the viewer **out of process** and hand it a pipe so it exits when your
process does — including on `SIGKILL`, which a shutdown hook cannot cover:

```rust
use std::process::{Child, ChildStdin, Command, Stdio};

/// Hold this for as long as the app runs. Dropping it closes the pipe and the
/// viewer exits on its own.
pub struct Viewer {
    child: Child,
    _keepalive: ChildStdin,
}

pub fn open_library() -> std::io::Result<Viewer> {
    let mut child = Command::new("md-librarian")
        .arg("--exit-on-stdin-close")
        // Optional: open straight onto one book rather than the library.
        // .args(["--book", "usdlite changelog"])
        .stdin(Stdio::piped())
        .spawn()?;
    let keepalive = child.stdin.take().expect("stdin was piped");
    Ok(Viewer { child, _keepalive: keepalive })
}
```

To know whether a book is installed before offering it in a menu, depend on
the discovery crate only:

```toml
[dependencies]
md-librarian = { git = "https://github.com/jlgerber/md-librarian", tag = "v0.1.0" }
```

```rust
use md_librarian::{library, roots, Entry};

let roots = roots(&[]);                 // --root wins, else MD_LIBRARIAN_PATH, else XDG
let installed = library(&roots, None).into_iter().any(|e| match e {
    Entry::Book(b) => b.title == "usdlite user guide" && b.is_built(),
    Entry::Missing { .. } => false,
});
```

## If the window never appears

On some Wayland driver/compositor combinations GTK reports
`Error 71 (Protocol error)` and every window dies. Set
`WEBKIT_DISABLE_DMABUF_RENDERER=1`; the rez package does this for you. The
diagnosis is in [A library of books](./library.md#known-issue-the-window-never-appears-wayland--explicit-sync).
