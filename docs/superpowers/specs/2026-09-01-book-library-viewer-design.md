# A library of mdbooks: repository discovery, a viewer, and the shell

- **Date:** 2026-09-01
- **Status:** Approved (design) — pending implementation plan
- **Scope owner:** gpui-yaams repo (`github.com/jlgerber/gpui-yaams`)
- **Follow-up owner:** usdlite (separate change; see [Sequencing](#sequencing))

## Background

`yaams-webview` opens **one page**. It is used by usdlite to show documentation
in a floating WebKit window, and `book/src/docs-window.md` documents the pattern
for showing *more* than one page — a generated shell with a persistent bar over
an iframe — extracted from yaams-tk2.

What does not exist anywhere is a **library**: a way to write and build books
independently of any application, put them somewhere, and have a viewer find
them. Today each consumer hard-codes its own set:

| | usdlite | yaams-tk2 |
|---|---|---|
| Books live | compiled into the binary (`rust-embed`, 3 books) | on disk, `<books_dir>/<name>/book.toml` (6 books) |
| Served by | a lazy in-process `tiny_http` on an ephemeral loopback port, mounted by prefix | an axum portal under one origin, or a static export |
| Discovery | a hand-enumerated `DocBook` enum | `yaams-books`: scan for `book.toml`, title/description from it |
| Landing page | none — one window per book | a generated page with `_portal/thumbs/<book>.<ext>` covers |

So the card-per-book library already exists **once**, in yaams-tk2, keyed to that
repo's conventions; and the app that most wants one — usdlite — cannot have it,
because its books are compiled in and have no directory to discover.

The goal is a viewer that stands alone, that an application can launch, and that
finds its books by **discovery rather than hard-coded source**, so a book (the
gpui book, say) can be shared between contexts without either context owning it.

## Measured facts that constrain the design

Everything below was verified in this session rather than assumed, and several
decisions are forced by them.

**mdbook 0.5.4 rejects unknown configuration.** `[book] id = "…"` fails the build
(`unknown field 'id', expected one of title, authors, description, src, language,
text-direction`), and a custom top-level table fails too (`unknown field
'library', expected one of book, build, rust, output, preprocessor`). An
`[output.library]` table parses, but mdbook then tries to run an `mdbook-library`
backend binary and fails the build unless `optional = true` — which builds, but
logs a `WARN` on every run and creates a stray `book/library/` directory.

> **Consequence:** a book cannot carry custom metadata. No id, no tags, no cover
> declaration, no "which contexts is this for". Everything the library needs
> beyond mdbook's six `[book]` keys must be found by **convention** or supplied
> from **outside** the book.

**`file://` cannot do this job.** usdlite's `docs.rs` records that serving over
HTTP is required so mdBook's full-text search index and relative links work, as
browsers refuse `fetch` from `file://`. Independently, `docs-window.md` records a
measurement against a real `WebKit2.WebView`: a `file://` page *can* embed
another `file://` page in an iframe, but `iframe.contentDocument` is `null`,
because WebKit gives every `file://` document an opaque origin.

**gpui is the cost.** From the rez build in this repo: `yaams-webview-demo` is
**1.7 MB**; `yaams-gallery` and `yaams-resources` are **61 MB** and **56 MB**.
Same repo, same profile — the difference is gpui and its renderer.

**A child outlives its parent.** On Linux a child process is not killed when its
parent dies; it is reparented to init. A host that spawns a viewer and kills it
on shutdown covers the graceful path only.

## Goals

- Books are **authored and built independently** of any application, and
  discovered at runtime.
- The **same book** can appear in several contexts without either hard-coding it.
- A **standalone viewer** is the primary artifact; applications launch it.
- A **library page**: one card per book, optional cover, click through to read.
- A **way back** to the library from inside a book.

## Non-goals

- Cross-library full-text search. Each mdbook ships its own index and that works
  inside the frame; indexing across books is out of scope.
- Rebuilding books. The viewer serves what is already built; it never runs
  `mdbook`. (This is why honouring `build-dir` is safe here — see below.)
- Native chrome. The viewer has no menu bar, no keybindings, no dialogs.
- An in-memory / embedded book source. Books are always on disk.

## Architecture

Three artifacts, all **gpui-free**. Nothing in this feature links gpui, so the
upgrade cadence this repo drives cannot break the documentation viewer.

| Crate | Contains | Depends on |
|---|---|---|
| `yaams-booklib` | the model: roots, discovery, shadowing, filtering, covers | `toml`, std |
| `yaams-bookserve` | the HTTP server and shell generation — **server only** | `yaams-booklib`, a small HTTP server |
| `yaams-books` | the viewer binary: server + window + lifetime | above, plus `yaams-webview` |

`yaams-books` is a **feature-gated `[[bin]]` inside `yaams-bookserve`**, following
the existing `yaams-resources` precedent: that binary lives in `yaams-ui` behind a
`standalone` feature which pulls in the optional `gpui_platform`, so no ordinary
build produces it and library consumers never pay for it. Here the optional
dependency is `yaams-webview`, keeping `wry`/`tao`/GTK out of `yaams-bookserve`.

`yaams-webview` itself is **unchanged**. It keeps its four dependencies and its
one-window-one-page shape.

### Why server-only

The split costs about five lines in the binary and buys the only testability this
feature can have. `docs-window.md` is explicit that anything touching `WebWindow`
needs GTK and a display and can therefore only be a manual checklist — but shell
rendering is a pure function. Server-only means a test can point the server at a
tempdir of fake books and assert the real contract over HTTP: title ordering,
first-root-wins shadowing, the dead card for a missing filtered title, the
directory name appearing only on collision, percent-encoded routes.

It also keeps a headless use free rather than a rewrite: serving a repository to
an ordinary browser, or over `ssh -L` to a machine with no display.

It is the same seam yaams-tk2 already cut, for the same reason — its `yaams-books`
crate exists "so consumers can find books without linking axum".

## The model — `yaams-booklib`

### A repository root

A root is a directory holding **source trees with their built output inside**:

```
<root>/
├── gpui/
│   ├── book.toml
│   ├── cover.png          ← optional
│   ├── src/
│   └── book/              ← or wherever [build] build-dir points
└── usdlite-user/
    ├── book.toml
    └── book/
```

A directory is a book iff it contains a `book.toml`. `[build] build-dir` **is
honoured**, so a book that renders to `book/html` (as this repo's own does) is
discovered as-is.

> yaams-tk2 deliberately does *not* support `build-dir`, because it **rebuilds**
> books and the output directory would land inside its staleness walk, making the
> book its own newest input and never rebuild again. The viewer only serves, so
> that reason does not transfer.

### The search path

Roots come from **`YAAMS_BOOK_PATH`**, a stacking list, overridable by a CLI flag.
With neither set, an XDG default root (`$XDG_DATA_HOME/…`, else `~/.local/share/…`)
so a bare run finds whatever was deployed locally.

- Roots **stack**, and the **first root wins**: a book present in an earlier root
  shadows the same book in later ones. This matches the house rule everywhere
  else here — the yaams config search path is first-root-wins, and usdlite
  searches `USDLITE_ASSET_PATH` entries first so a user's entry shadows the
  shipped one.
- A root that **does not exist** is skipped **with a log line** — not silently
  (a typo or an unmounted NFS root should leave a trace) and not fatally (a
  package that isn't resolved on this machine must not stop the viewer).

### Identity

A book's identity is its **`[book] title`**, falling back to the **directory
name** when the title is missing or empty. (`docs-window.md` records that an
empty label renders as an invisible, zero-width link.)

Consequences, all deliberate:

- Shadowing across roots compares **titles**.
- Two directories in the **same root** with the same title **both appear** —
  there is no root order to break the tie, and hiding one would be a lie about
  what is installed.
- Titles are display text: they contain spaces and punctuation, so they are
  **never** used as a route key (see [Routing](#routing)).

### Filtering

A context may supply an **explicit include-list of titles**. Roots select
coarsely — a context points at the roots it wants — and the list refines.

A listed title that no root provides renders as a **dead card saying it is
missing**. This follows `docs-window.md`'s first trap: list the entry, but render
it inert, because a link to content that is not there opens a `file://` 404 in a
window with **no back button**.

> **Derived, flag if wrong:** the same rule should apply to a discovered book
> whose *built output* is absent — the card lists it, inert, rather than linking
> into nothing. This was not decided explicitly; it is the consistent reading of
> the same trap.

### Covers

An optional **`cover.<ext>` inside the book's own directory**, beside
`book.toml`, probed across a few extensions. It travels with the book, so a book
shared between contexts carries its cover into all of them — which a central
`thumbs/` directory per root (yaams-tk2's convention) could not do, and which
would in any case key awkwardly on titles containing spaces.

With no cover, the viewer **generates one per book: an initial and a colour
derived from the title**, so uncovered books still look distinct and the page
never has a hole in it.

## The server — `yaams-bookserve`

- Binds **`127.0.0.1` on an ephemeral port**, one server per viewer process,
  serving every root on the path. usdlite's precedent: the server is idle in
  `recv()` until a page is requested, so it costs nothing while open.
- **HTTP, not `file://`** — this is what makes mdBook's search index and relative
  links work, and what makes the shell same-origin with the books it frames.

### Routing

Each book is mounted under **root index + directory name** — `/2/gpui/…` — which
is unique across the whole path even when titles collide, and is derived from
names rather than display text.

Every segment is **percent-encoded to the unreserved set**, per `docs-window.md`'s
second trap: HTML-escaping is wrong for an `href` (a directory named
`ev" onmouseover=alert(1) x` closed the attribute and produced a live event
handler), and a `#` or `?` silently truncates the path.

### The shell

`GET /` renders the library page. The shell is generated **per request**:

- It cannot go stale. A book built while the viewer is open appears on refresh —
  which is the authoring loop this feature exists for.
- Nothing is written, so **read-only roots work** (a root shipped inside a rez
  package, or an NFS mount).
- Existence is checked at render time, so the "never link content that is not
  there" rule is evaluated against the filesystem as it is now.
- It sidesteps both bugs `docs-window.md` records for a disk-written shell: a
  shell generated once listed a later-built book as unavailable forever, and an
  unmarked `index.html` overwrote a file that was not ours. Neither failure mode
  exists if nothing is ever written.
- There is also no honest root to write a shell *into*: the merged, shadowed view
  spans the whole path and belongs to no single root.

The scan cost is a directory listing plus N small `book.toml` reads per page
load, against an otherwise idle server. It only becomes a question at hundreds of
books.

### The page

One card per book, sorted **alphabetically by title**, each showing:

- the **cover**, or the generated initial-and-colour graphic;
- the **title**;
- the **`[book] description`** from `book.toml`;
- the **directory name**, *only* when another book in the same root shares its
  title — the one thing that tells two identical cards apart.

### Reading a book: shell + iframe

Clicking a card loads the book into an **iframe** under a **persistent bar**.
The bar carries **"← Library"** and **the current book's title**, tracked as the
reader moves between chapters — readable now because HTTP made the shell and the
frame same-origin, which `file://` forbade.

Rejected alternatives, and why:

- **Injecting a back button into each page server-side** means rewriting every
  HTML response — parsing output we do not control, coexisting with mdbook's own
  JS, and breaking on every mdbook release. Worse, the button lives *inside* the
  page: the first external link (to docs.rs, say) strands the user in a
  chrome-less window. The bar is a separate document and survives it.
- **Per-book opt-in** (a theme override or `additional-js` in each `book.toml`)
  fails precisely when someone adds a book and forgets, and the failure is
  silent. It also cannot work for books we do not own.

The cost is that a chapter is not deep-linkable — the window's URL stays the
shell's. Accepted; revisit only if a real requirement for bookmarkable chapter
URLs appears.

## The viewer — `yaams-books`

Roughly:

```rust
let url = yaams_bookserve::start(roots, filter)?;   // http://127.0.0.1:PORT/
let win = yaams_webview::WebWindow::open(opts, WebContent::Url(url))?;
// exit when the user closes the window, or when the parent pipe reaches EOF
```

CLI flags override `YAAMS_BOOK_PATH` and supply the include-list.

## Embedding: out of process, tied by a pipe

An application (usdlite first) **launches the `yaams-books` binary** rather than
linking the crates. No server in the app, no crate dependency, and the same code
path serves standalone and embedded use.

The viewer must **not outlive its host**. Killing the child on graceful shutdown
covers only the graceful path — on Linux the child is reparented to init if the
host segfaults or is `SIGKILL`ed, leaving an orphaned window with no owner.

So: the host holds the **write end of an inherited pipe**, and the viewer watches
the read end and exits on **EOF**. This fires on any death, needs no signals, and
avoids `prctl(PR_SET_PDEATHSIG)`'s sharp edge (it triggers on the spawning
*thread's* death, not the process's).

## Packaging

The gpui-yaams rez package gains:

- **`yaams-books` in `tools`**, built like `yaams-resources` — the feature must be
  named explicitly in `rez_build.sh`, since no ordinary build produces the binary.
- **Its own book shipped as a discoverable root.** The package currently installs
  the rendered book to `{root}/book` — built HTML only, with no `book.toml` beside
  it — which is *not* discoverable under this design. It must instead ship
  `{root}/books/<dir>/` with the `book.toml` and its output, and `commands()` must
  **append** that to `YAAMS_BOOK_PATH` (appended, so a user's own roots win — the
  house rule for every other path list here).

The result is that `rez env gpui_yaams -- yaams-books` opens with something in it.

## Testing

**Testable, and worth testing** — all over HTTP against a tempdir of fake books,
no display required:

- discovery: `book.toml` recognition, `build-dir` honoured, missing root skipped;
- identity: title used, directory-name fallback for missing/empty titles;
- shadowing: first root wins across roots; both shown within one root;
- filtering: include-list applied; a missing title renders as a dead card;
- the page: title ordering, description present, directory name shown only on
  collision, generated cover when no `cover.<ext>`;
- routing: root index + directory name, percent-encoded.

**Not testable — a written manual checklist instead**, per `docs-window.md`:
opening the window, clicking a card, following an external link and returning via
the bar, close-then-reopen, the no-books-anywhere case, and the parent-dies case
(both graceful and `SIGKILL`).

## Documentation obligations

Per CLAUDE.md, documentation ships with the feature:

- a **book chapter** covering the repository layout, `YAAMS_BOOK_PATH`, the
  filter, covers, and how an application launches the viewer;
- a **revision of `docs-window.md`**, which currently documents the `file://`
  shell pattern this supersedes for the library case. It should keep its measured
  facts and traps — they are still true and still cited here — while pointing at
  the viewer as the answer for "a library of books", and remaining the answer for
  "one app, a fixed handful of pages, no server".
- **no gallery entry**: nothing lands in `yaams-ui`.

## Sequencing

**First cut, this repo:** `yaams-booklib`, `yaams-bookserve`, the `yaams-books`
binary, the book chapter, the `docs-window.md` revision, and the rez packaging
above. The viewer ships standalone and is useful on its own.

**Separately, usdlite:** delete `docs.rs` and its `rust-embed` + `tiny_http`
server; retire `dev-docs` as a *compile-time* switch in favour of shipping (or
not) a directory; teach its rez package to install its books into a root; rewire
the Help menu to spawn `yaams-books` with the pipe. This is a bigger change than
it sounds and is deliberately not on the critical path for the viewer.

## Assumptions carried

Stated so they can be corrected rather than discovered:

1. One server per viewer process, `127.0.0.1`, ephemeral port.
2. A discovered book whose built output is missing renders as a dead card, the
   same as a filtered title that is absent.
3. The HTTP server dependency is a small blocking one in the `tiny_http` mould —
   usdlite's precedent — not an async runtime.
