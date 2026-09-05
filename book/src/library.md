# A library of books

`md-librarian` and `md-librarian-serve` turn a directory of mdbooks into a
browsable library, and the `md-librarian` binary is the standalone viewer that
opens it.

This is the answer for **many books, discovered at runtime**. For one app with a
fixed handful of pages, [Building a documentation window](./docs-window.md) is
still the smaller answer — and most of its traps are quoted here, because they
apply just as much.

```text
md-librarian                                  # MD_LIBRARIAN_PATH, else the XDG default
md-librarian --root ~/books --root /opt/books # explicit roots; the first wins
md-librarian --include gpui                   # only these titles
md-librarian --book gpui                      # open straight onto one book
md-librarian --no-window                      # serve only; prints the URL
```

## What counts as a book

A **root** is a directory of book source trees with their built output inside.
A subdirectory is a book **iff** it holds a `book.toml`:

```text
<root>/
├── gpui/
│   ├── book.toml
│   ├── cover.png          <- optional
│   ├── src/
│   └── book/              <- or wherever [build] build-dir points
└── usdlite-user/
    ├── book.toml
    └── book/
```

`[build] build-dir` **is** honoured, so a book that renders to `html/` — as this
repo's own does — is found as-is. (yaams-tk2's equivalent refuses `build-dir`
because it *rebuilds* books and the output would land inside its staleness walk.
The viewer only serves, so the hazard does not transfer.)

## Where books are found

Roots come from `--root`, else the **`MD_LIBRARIAN_PATH`** environment variable — a
stacking, colon-separated list — else `$XDG_DATA_HOME/md-librarian/books`.

- Roots **stack**, and the **first one wins**: a book present in an earlier root
  shadows the same book later on the path. This is the same rule as the yaams
  config search path and `USDLITE_ASSET_PATH`, so a user's own root shadows one
  shipped inside a package.
- A root that does not exist is **skipped with a log line** — loud enough to
  explain a missing book, quiet enough that an unresolved package does not stop
  the viewer.

Note the deliberate contrast with yaams-tk2's `YAAMS_BOOKS_DIR`, which names a
*single* directory: `_PATH` is a list, `_DIR` is one. The suffix is the
cardinality, so a machine mid-migration can carry both without confusion.

## Identity is the title

A book is identified by its **`[book] title`**, falling back to the **directory
name** when that is missing or empty (an empty label renders as an invisible,
zero-width link).

This is forced by mdbook, not chosen freely: mdbook 0.5 **rejects unknown
configuration**. `[book] id = "…"` fails the build with `unknown field 'id'`, and
so does a custom top-level table. A book cannot carry an id, a tag, or a cover
declaration — so anything beyond mdbook's six `[book]` keys must come from a
*convention* or from *outside* the book.

Two consequences worth knowing:

- Shadowing across roots compares titles.
- Two directories in the **same** root may share a title. Both are shown — there
  is no root order to break the tie, and hiding one would misreport what is
  installed — and each card then shows its directory name, which is the only
  thing that tells them apart.

Titles are never used to build URLs. A book is addressed by **root index +
directory name** (`/0/gpui/…`), percent-encoded to the unreserved set.

## Covers

An optional **`cover.<ext>`** beside `book.toml` (`png`, `svg`, `jpg`, `jpeg`,
`webp`). It lives inside the book rather than in a per-root index, so a book
shared between contexts carries its cover into all of them.

With no cover, the viewer generates one: the title's initial on a colour derived
from the whole title. It is deterministic, so a book looks the same everywhere,
and adding a book never recolours the others.

## Filtering

A context can pass an explicit include-list of **titles**:

```sh
md-librarian --root /opt/books --include gpui --include "usdlite user guide"
```

Roots select coarsely, the list refines. A listed title that no root provides is
shown as a **dead card saying so** rather than silently dropped — a typo or an
unmounted root should be visible.

## Opening straight onto a book

`--book <TITLE>` starts with the frame already on that book instead of the
library — what an application's *Help → User Guide* should do, rather than
making the user pick a card first.

It is still **inside the shell**, so the bar and the way back to the library are
there from the first frame. Under the hood it is a query the shell honours:

```sh
md-librarian --book "usdlite changelog"
# opens http://127.0.0.1:PORT/?book=usdlite%20changelog
```

Keeping it in the URL rather than in the server's state means the server stays
stateless, the choice is visible, and `--no-window` prints something a browser
can use directly.

A title that no root provides — or one whose book is not built — **opens the
library instead**, with a log line saying so. A caller cannot know what is
installed on the machine it is running on, so this is a preference rather than a
demand, and the fallback is the page that shows what *is* there.

## The page

`/` serves a **shell**: a bar that never unloads, over an iframe. `/_grid` serves
the library itself, which is what the frame shows first. Clicking a card loads
the book *into the frame*, so the bar — carrying "← Library" and the current
book's title — survives everything the book does, including a link out to an
external site. In a window with no back button and no address bar, that bar is
the only way home.

The shell is regenerated **on every request**. A book built while the viewer is
open appears on refresh, read-only roots (an NFS mount, or a root inside a rez
package) work, and there is no written page to go stale or to overwrite someone
else's `index.html`.

Everything is served over a **loopback HTTP origin**, not `file://`. That is what
makes mdBook's full-text search and relative links work at all, and what lets the
bar read the frame — under `file://` each document gets an opaque origin and
`iframe.contentDocument` is `null`.

## Using the crates directly

Discovery is a pure function of what is on disk:

```rust
use std::path::PathBuf;
use md_librarian::{Entry, library, roots};

// --root wins, else MD_LIBRARIAN_PATH, else the XDG default.
let roots: Vec<PathBuf> = roots(&[]);
for entry in library(&roots, None) {
    match entry {
        Entry::Book(b) => println!("{} ({})", b.title, b.dir.display()),
        Entry::Missing { title } => println!("{title} — not found in any root"),
    }
}
```

Serving is one call, and returns the URL to point a window at:

```rust
let server = md_librarian_serve::start(roots, None)?;   // -> anyhow::Result<Server>
println!("{}", server.url());                        // http://127.0.0.1:PORT/
# Ok::<(), anyhow::Error>(())
```

`md-librarian-serve` deliberately does **not** open a window: that lives in the
`md-librarian` binary (the `md-librarian-cli` crate), the only crate that links
`wry`/`tao`/GTK — so the pages can be tested with no display, and a repository
can be served to an ordinary browser or over `ssh -L`.

## Host wiring: launching the viewer from an app

An application launches `md-librarian` **out of process**. No crate dependency, no
server inside the app, and the same code path standalone and embedded.

The one thing to get right is lifetime. On Linux a child is **not** killed when
its parent dies — it is reparented to init — so killing it from your shutdown
path covers only the graceful case. Instead, give the viewer a pipe and let it
watch for EOF: the write end closes when your process dies *for any reason*,
including `SIGKILL`.

The pipe every host already has is stdin:

```rust
use std::process::{Child, ChildStdin, Command, Stdio};

/// Hold this for as long as the app runs. Dropping it closes the pipe, and the
/// viewer exits on its own; so does an outright crash, which is the point.
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

No `unsafe`, no libc, nothing to inherit by hand. (`--parent-pipe <FD>` exists
for a host that needs stdin for something else, but that host must make the
descriptor survive `exec` itself — Rust sets `CLOEXEC` on descriptors it creates,
so `std::io::pipe()` alone will *not* be inherited.)

Reuse one viewer rather than spawning one per book: `md-librarian-webview`'s
[thread cost](./webview.md#thread-cost) applies to the viewer process too,
and the library is the way between books anyway.

## Known issue: the window never appears (Wayland + explicit sync)

The server starts, prints its URL, and then:

```text
Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.
```

**This is not the viewer.** It is GTK/WebKit's dmabuf path violating the
explicit-sync protocol, and it takes down *every* `md-librarian-webview` window in the
same way — usdlite's help windows included. `WAYLAND_DEBUG=1` shows the actual
violation:

```text
wl_display#1.error(wp_linux_drm_syncobj_surface_v1#60, 4, "Missing acquire timeline")
```

The surface is committed without an acquire timeline point and the compositor
closes the connection. Observed on an NVIDIA RTX 3090 (`nvidia-open-dkms`
610.57.04) under Hyprland 0.56.2 with `webkit2gtk-4.1` 2.52.6 — with no driver
or package update involved, and no leaked GPU state; it simply started happening
mid-session.

The fix is one environment variable, which keeps the window on native Wayland:

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 md-librarian --root ~/books
```

Verified three runs of each: **without it, 3/3 die; with it, 3/3 survive**, and
`hyprctl clients` reports the window `mapped: true`, `xwayland: false`. Three
other things also work, in rough order of preference:

| | Effect |
|---|---|
| `WEBKIT_DISABLE_COMPOSITING_MODE=1` | drops WebKit's compositing path |
| `GDK_GL=disable` | takes GDK off GL entirely |
| `GDK_BACKEND=x11` | routes through XWayland; logs harmless `Failed to create GBM buffer` noise |

There is no compositor-side switch to reach for any more: Hyprland removed the
`render:explicit_sync` option (0.56 answers `no such option`).

**To confirm it is the environment rather than your code**, run the crate's own
example, which involves none of this machinery:

```sh
cargo run -p md-librarian-webview --example help
```

If that dies the same way, nothing in your application is implicated.

For a permanent fix, set the variable wherever your session's environment is
defined rather than per command — but note it is a **workaround for a
toolkit/driver/compositor interaction**, not something this code can fix, and it
should be dropped once the underlying combination is repaired.

## What is tested, and what is a checklist

Testable, and tested — over real HTTP against a temporary root, with no display:
discovery and `build-dir`, the title fallback, first-root-wins shadowing, both
copies of a same-titled pair, the include-list and its dead card, an unbuilt book
listed but never linked, percent-encoded directory names, refusal to serve
outside a book, and covers both real and generated.

Not testable, so keep it as a manual checklist: opening the window, clicking a
card, following an external link and returning via the bar, close-then-reopen,
the no-books-anywhere case, and the parent-dies case — both a graceful exit and
`kill -9`.
