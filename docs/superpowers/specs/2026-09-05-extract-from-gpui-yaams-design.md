# md-librarian: extracting the mdbook library viewer from gpui-yaams

- **Date:** 2026-09-05
- **Status:** Approved (design) — pending implementation plan
- **Source:** `github.com/jlgerber/gpui-yaams` at `v0.27.0-beta.3`
  (crates `yaams-booklib`, `yaams-bookserve`, `yaams-webview`)
- **Owner:** this repo (`github.com/jlgerber/md-librarian`)

## Background

gpui-yaams grew a self-contained feature that has nothing to do with gpui: a
**library of mdbooks** — discovery of book roots on a search path
(`yaams-booklib`), a loopback HTTP server that renders a card page and a
persistent bar over an iframe (`yaams-bookserve`), a floating WebKit window
(`yaams-webview`), and the `yaams-books` binary that ties them together. The
original design is recorded in
[`2026-09-01-book-library-viewer-design.md`](./2026-09-01-book-library-viewer-design.md),
copied here from gpui-yaams as background; it remains the authority on *why*
the viewer is shaped the way it is.

Two things argue for moving it out:

1. **None of it depends on gpui.** The webview crate depends on `tao`, `wry`
   and `glib`; the other two on `toml`, `tiny_http` and `tracing`. Living in
   the gpui workspace only ties their release cadence to gpui pin bumps.
2. **The consumers want a viewer, not a widget library.** usdlite launches the
   viewer as a separate process and imports discovery only to answer "is that
   book installed?". Pulling that through a gpui workspace's tag is more
   coupling than the dependency deserves.

There is also a latent collision: yaams-tk2 has its own, unrelated crate named
`yaams-books`. Renaming on the way out removes it.

## Goals

- md-librarian is **self-contained**: it builds from crates.io dependencies
  alone, with no dependency on gpui-yaams, gpui, or gpui-component, and no
  `[patch]` block.
- The **behaviour and public API shapes are unchanged**. Only names move, so
  the later usdlite / yaams-tk2 migrations are path renames.
- The repo is **releasable and deployable on its own**: one workspace version,
  a rez package, an mdbook, a justfile, CI.

## Non-goals (separate tasks)

- Removing the three crates from gpui-yaams.
- Migrating usdlite or yaams-tk2 to md-librarian.
- Cutting a first release tag.
- Any functional change to discovery, the shell, the server, or the window.

## Decisions

| Question | Decision |
|---|---|
| Which crates move | all three: booklib, bookserve, webview |
| Git history | fresh copy; one import commit citing the source tag |
| Naming | md-librarian names throughout (crates, binary, env var, XDG dir, rez package) |
| Layout | four crates; the binary in its own crate, no `standalone` feature |
| Env var / default dir | `MD_LIBRARIAN_PATH` / `$XDG_DATA_HOME/md-librarian/books` |
| Scope of this task | copy only; gpui-yaams and consumers untouched |

## Workspace layout

```text
md-librarian/
├── Cargo.toml                       workspace; version 0.1.0; edition 2024; resolver 2
├── crates/
│   ├── md-librarian/                discovery         (from yaams-booklib)
│   ├── md-librarian-serve/          server + shell    (from yaams-bookserve, lib only)
│   ├── md-librarian-webview/        wry/tao window    (from yaams-webview, with examples/)
│   └── md-librarian-cli/            the `md-librarian` binary (from src/bin/yaams-books.rs)
├── book/                            mdbook, title "md-librarian", build-dir = "html"
├── docs/agents/                     issue-tracker, triage-labels, domain
├── docs/superpowers/specs/          this spec + the 2026-09-01 background spec
├── scripts/rez-cargo-test.sh
├── package.py, rez_build.sh, REZ.md
├── justfile
├── CLAUDE.md, CHANGELOG.md, README.md, LICENSE (MIT)
└── .github/workflows/ci.yml
```

### Crate: `md-librarian` (discovery)

Source: `crates/yaams-booklib/src/lib.rs` (538 lines, 14 unit tests).
Dependencies: `toml`, `tracing`; dev: `tempfile`.

Public surface, unchanged except the constant's value:

```rust
pub const BOOK_PATH_VAR: &str = "MD_LIBRARIAN_PATH";
pub struct Book { /* root_index, dir_name, title, description, ... */ }
impl Book { pub fn is_built(&self) -> bool }
pub enum Entry { Book(Book), Missing(String) }
impl Entry { pub fn title(&self) -> &str }
pub fn roots(cli: &[PathBuf]) -> Vec<PathBuf>;
pub fn default_root() -> PathBuf;   // $XDG_DATA_HOME/md-librarian/books, else ~/.local/share/md-librarian/books
pub fn library(roots: &[PathBuf], include: Option<&[String]>) -> Vec<Entry>;
pub fn book_at(roots: &[PathBuf], root_index: usize, dir_name: &str) -> Option<Book>;
pub fn discover_root(root: &Path, index: usize) -> Vec<Book>;
```

This is the crate consumers import (`md_librarian::library(...)`). It is
deliberately the bare project name because it is the one crate an application
is expected to depend on.

### Crate: `md-librarian-serve`

Source: `crates/yaams-bookserve/src/{lib.rs,shell.rs}` and
`tests/serving.rs` (14 integration tests). Dependencies: `md-librarian`,
`anyhow`, `tiny_http`, `tracing`; dev: `tempfile`.

The `standalone` feature, the `[[bin]]` table and the optional
`clap` / `tracing-subscriber` / webview dependencies are **removed**; the
binary moves to its own crate. Public surface unchanged:

```rust
pub mod shell;                      // url_segment, html_escape, book_prefix, generated_cover, shell_document, grid_document
pub struct Server;  impl Server { pub fn url(&self) -> &str }
pub fn start(roots: Vec<PathBuf>, include: Option<Vec<String>>) -> Result<Server>;
```

The server thread is renamed from `yaams-bookserve` to `md-librarian-serve`.

### Crate: `md-librarian-webview`

Source: `crates/yaams-webview/src/lib.rs` (923 lines) and the four examples
(`help`, `first_paint`, `reopen`, `close_teardown`). Dependencies: `anyhow`,
`glib 0.18`, `tao 0.34`, `wry 0.53` — the manifest comments explaining why
`glib` is named come along verbatim.

Public surface unchanged: `WebContent`, `WebWindowOptions`, `WebWindow::{open,
navigate, set_title, focus, is_open}`. The GTK thread is renamed from
`yaams-webview-gtk` to `md-librarian-gtk`; error strings that name the crate
follow.

### Crate: `md-librarian-cli`

Source: `crates/yaams-bookserve/src/bin/yaams-books.rs` (177 lines), as
`src/main.rs`. Dependencies: `md-librarian`, `md-librarian-serve`,
`md-librarian-webview`, `anyhow`, `clap` (derive), `tracing`,
`tracing-subscriber` (env-filter).

```toml
[[bin]]
name = "md-librarian"
path = "src/main.rs"
```

CLI flags unchanged: `--root <DIR>` (repeatable), `--include <TITLE>`
(repeatable), `--book <TITLE>`, `--exit-on-stdin-close`, `--parent-pipe <FD>`,
`--no-window`. The help text names `MD_LIBRARIAN_PATH` and the new default
directory. The crate-level doc keeps the lifetime contract (stdin pipe, EOF
exits) and adds one line: extracted from gpui-yaams `yaams-books` at
`v0.27.0-beta.3`.

This is the **only** crate that links wry/tao/GTK. The library crates stay
display-free by construction, which is what the `standalone` feature used to
buy at the cost of a flag every packager had to know about.

## Rename table

Mechanical renames, applied across sources, tests, examples, book, docs and
packaging:

| Was | Becomes |
|---|---|
| `yaams-booklib` / `yaams_booklib` | `md-librarian` / `md_librarian` |
| `yaams-bookserve` / `yaams_bookserve` | `md-librarian-serve` / `md_librarian_serve` |
| `yaams-webview` / `yaams_webview` | `md-librarian-webview` / `md_librarian_webview` |
| `yaams-books` (binary) | `md-librarian` |
| `YAAMS_BOOK_PATH` | `MD_LIBRARIAN_PATH` |
| `$XDG_DATA_HOME/yaams/books` | `$XDG_DATA_HOME/md-librarian/books` |
| thread `yaams-webview-gtk` | `md-librarian-gtk` |
| thread `yaams-bookserve` | `md-librarian-serve` |
| rez package `gpui_yaams` | `md_librarian` |
| `GPUI_YAAMS_SKIP_BOOK`, `GPUI_YAAMS_RUN_TESTS`, `GPUI_YAAMS_SRC` | `MD_LIBRARIAN_SKIP_BOOK`, `MD_LIBRARIAN_RUN_TESTS`, `MD_LIBRARIAN_SRC` |
| `REZ_GPUI_YAAMS_ROOT` | `REZ_MD_LIBRARIAN_ROOT` |
| `yaams-webview-demo` (rez tool) | `md-librarian-webview-demo` |
| `{root}/books/gpui-yaams/` | `{root}/books/md-librarian/` |

**Kept as historical prose**, deliberately: the contrast with yaams-tk2's
`YAAMS_BOOKS_DIR` (it explains the `_PATH` suffix), links to gpui-yaams issues
(`#47` first-paint, the Wayland dmabuf diagnosis), and references to yaams-tk2
as the origin of the documentation-window pattern. A final
`grep -rn yaams` over the tree must show only these.

## Book

`book/book.toml`: title `md-librarian`, `build-dir = "html"`, navy theme,
`git-repository-url` pointing at this repo. Rendering to `html/` is not
cosmetic: it is the case the discovery code honours `[build] build-dir` for,
and the rez package ships this book as the default library root, so the
viewer dogfoods its own book.

Chapters (`book/src/SUMMARY.md`):

1. **Introduction** — new, short: what a library root is, the four crates, the
   one command.
2. **Getting started** — new: install (`cargo install` / rez), `MD_LIBRARIAN_PATH`,
   launching from an application with `--exit-on-stdin-close`, the `--book`
   deep link.
3. **A library of books** — from `books.md`, renamed.
4. **The webview window** — from `yaams-webview.md`, renamed.
5. **Building a documentation window** — from `docs-window.md`, renamed. Kept
   because it is the measured basis for the shell design.
6. **Releasing** — adapted from gpui-yaams: single workspace version, `vX.Y.Z`
   tags, no gpui-pin section.
7. **rez packaging** — adapted from `rez.md`.

## Repo scaffolding

- **`CLAUDE.md`** — working notes: what the repo is, the "documentation is part
  of the feature" rule (book chapter + `mdbook build` for every public change;
  no gallery here), and the surviving gotchas: the guarded
  `WEBKIT_DISABLE_DMABUF_RENDERER` workaround, and that the CLI crate is the
  only place GTK may enter. Points at `docs/agents/`.
- **`docs/agents/`** — `issue-tracker.md`, `triage-labels.md`, `domain.md`
  copied with the repo name changed.
- **`CHANGELOG.md`** — Keep a Changelog format, `[Unreleased]` with one entry:
  extracted from gpui-yaams `v0.27.0-beta.3`, the rename table in brief.
- **`README.md`** — replaces the one-liner: what it is, the command, where
  books go.
- **`LICENSE`** — MIT, matching the source workspace's `license = "MIT"`.
- **`justfile`** — `default`, `build`, `test`, `run *args` (viewer with
  `--root .` so the repo's own book shows; sets the guarded WebKit env var),
  `docs`, `docs-build`, `install` (`cargo install --path crates/md-librarian-cli --locked --force`),
  `rez-build`, `rez-build-isolated`, `rez-test`, `rez-run`.

## rez package

`package.py` (name `md_librarian`, version read from the workspace
`Cargo.toml` via the same `@early()` reader), `rez_build.sh`, `REZ.md`,
`scripts/rez-cargo-test.sh`, all adapted from gpui-yaams:

- **tools**: `md-librarian`, `md-librarian-webview-demo` (the `help` example
  under a prefixed name).
- **books root**: `mdbook build book` → `{root}/books/md-librarian/{book.toml,html/}`;
  `commands()` **appends** `{root}/books` to `MD_LIBRARIAN_PATH` (first-root-wins,
  so the shipped copy is the fallback). Skipped with a message when `mdbook`
  is absent or `MD_LIBRARIAN_SKIP_BOOK=1`.
- **env**: `WEBKIT_DISABLE_DMABUF_RENDERER=1`, guarded so an existing value
  wins, with the same "temporary, not a property of this package" comment.
- **tests**: `tools_on_path` (default), `books_root` (default),
  `books_serve` (explicit; `timeout 5 md-librarian --no-window | head -1`),
  `cargo_test` (explicit; needs a source checkout, resolved via arg →
  `MD_LIBRARIAN_SRC` → cwd).

## CI

`.github/workflows/ci.yml`, on push to `main` and on pull requests, ubuntu-latest:

1. `apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev` (what wry /
   tao need to compile; the exact list is confirmed during implementation
   against a first run).
2. `cargo fmt --all --check`
3. `cargo test --workspace` (no display needed: the webview crate has no tests
   and its examples are not built by `cargo test`).
4. `mdbook build book` via the `peaceiris/actions-mdbook` action or a cargo
   install, whichever is faster in practice.

## Error handling

Unchanged from the source. Discovery logs and skips a missing root; the server
returns 404 for unknown paths and a dead card for a listed-but-absent title; the
CLI exits non-zero if the server cannot bind, and exits 0 with the URL printed
when the window fails to open (the "serve only" fallback the Wayland workaround
exists for).

## Testing and verification

- `cargo test --workspace` passes: 14 discovery unit tests, 14 serving
  integration tests. The test that hard-codes a book named `gpui-yaams` with
  `build-dir = "html"` is renamed to `md-librarian` — same case, this repo's
  own book.
- `cargo build --workspace --examples` compiles the four webview examples.
- `grep -rn -i yaams` over the tree (excluding `target/`) lists only the
  historical references enumerated above.
- `just run` opens the viewer on the repo's own book (manual, one look).
- `just rez-build-isolated` into a scratch prefix succeeds and
  `rez test md_librarian` (default tests) passes from that prefix.
- `cd book && mdbook build` succeeds with no broken-link warnings.

## Sequencing after this task

1. Tag md-librarian `v0.1.0`.
2. usdlite: replace the `yaams-booklib` / `yaams-webview` git deps with
   `md-librarian` at that tag, rename the import paths, switch `package.py`
   from `YAAMS_BOOK_PATH` / `yaams-books` to `MD_LIBRARIAN_PATH` /
   `md-librarian`, and depend on the `md_librarian` rez package.
3. yaams-tk2: replace the `yaams-webview` git dep with `md-librarian-webview`.
4. gpui-yaams: remove the three crates, the three chapters, the rez tools and
   tests, and the `books` / `install-books` recipes; release as a breaking
   minor with a CHANGELOG pointer to md-librarian.
