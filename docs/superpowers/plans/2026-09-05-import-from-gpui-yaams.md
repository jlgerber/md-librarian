# md-librarian: import from gpui-yaams — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `~/src/md-librarian` a self-contained, releasable Rust workspace holding the mdbook library viewer that today lives in gpui-yaams (`yaams-booklib`, `yaams-bookserve`, `yaams-webview`, the `yaams-books` binary), renamed to md-librarian names, with its own book, rez package, justfile and CI.

**Architecture:** Four crates: `md-librarian` (discovery), `md-librarian-serve` (loopback server + generated shell), `md-librarian-webview` (wry/tao floating window), `md-librarian-cli` (the `md-librarian` binary, the only crate that links GTK). Files are copied from gpui-yaams at tag `v0.27.0-beta.3` and renamed with one sed script; behaviour, public API shapes and CLI flags are unchanged.

**Tech Stack:** Rust 2024 edition (rustc 1.98), crates.io only: toml, tracing, tiny_http, anyhow, clap 4, tracing-subscriber, tao 0.34, wry 0.53, glib 0.18, tempfile (dev). mdbook for the book. rez for packaging. GitHub Actions for CI.

**Spec:** `docs/superpowers/specs/2026-09-05-extract-from-gpui-yaams-design.md` (this repo). Background: `docs/superpowers/specs/2026-09-01-book-library-viewer-design.md`.

## Global Constraints

- Source of truth for every copied file: `~/src/gpui-yaams` at tag `v0.27.0-beta.3`. Verified at plan time: `main` is byte-identical to that tag for every file this plan copies. Task 1 re-checks.
- Workspace version `0.1.0`, `edition = "2024"`, `license = "MIT"`, `resolver = "2"`, `authors = ["Jonathan Gerber <jlgerber@gmail.com>"]`.
- No dependency on gpui, gpui-component, or gpui-yaams. No `[patch]` block.
- Names: env var `MD_LIBRARIAN_PATH`; default root `$XDG_DATA_HOME/md-librarian/books`; binary `md-librarian`; GTK thread `md-librarian-gtk`; server thread `md-librarian-serve`; rez package `md_librarian`; rez knobs `MD_LIBRARIAN_SKIP_BOOK`, `MD_LIBRARIAN_RUN_TESTS`, `MD_LIBRARIAN_SRC`; rez tools `md-librarian`, `md-librarian-webview-demo`; shipped book root `{root}/books/md-librarian/`.
- Historical references stay: yaams-tk2's `YAAMS_BOOKS_DIR` contrast, links to `github.com/jlgerber/gpui-yaams/issues/...`, mentions of yaams-tk2 and usdlite as origins.
- Every rename goes through `docs/superpowers/plans/rename-from-yaams.sed` (committed beside this plan). Never hand-rename what the script covers; hand edits are only for the lines this plan names.
- All work on branch `feat/import-from-gpui-yaams` in `~/src/md-librarian`. Commit after every task with the trailer:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ
  ```
- Shell convention in every task: `SRC=~/src/gpui-yaams`, `DST=~/src/md-librarian`, `RENAME=$DST/docs/superpowers/plans/rename-from-yaams.sed`.

---

## File map

| Path in md-librarian | Origin | Responsibility |
|---|---|---|
| `Cargo.toml` | new | workspace: members, shared package metadata |
| `crates/md-librarian/{Cargo.toml,src/lib.rs}` | `crates/yaams-booklib` | discovery: roots, titles, shadowing, covers |
| `crates/md-librarian-serve/{Cargo.toml,src/lib.rs,src/shell.rs,tests/serving.rs}` | `crates/yaams-bookserve` (lib + tests only) | loopback server and generated shell |
| `crates/md-librarian-webview/{Cargo.toml,src/lib.rs,examples/*.rs}` | `crates/yaams-webview` | floating WebKit window |
| `crates/md-librarian-cli/{Cargo.toml,src/main.rs}` | `crates/yaams-bookserve/src/bin/yaams-books.rs` | the `md-librarian` binary |
| `book/` | `book/src/{books,yaams-webview,docs-window}.md` + new chapters | the mdbook |
| `README.md`, `CLAUDE.md`, `CHANGELOG.md`, `LICENSE`, `.gitignore` | new / adapted | repo scaffolding |
| `docs/agents/*.md` | `docs/agents/*.md` | agent conventions |
| `justfile` | new | developer recipes |
| `package.py`, `rez_build.sh`, `REZ.md`, `scripts/rez-cargo-test.sh` | adapted | rez packaging |
| `.github/workflows/ci.yml` | new | CI |

---

### Task 1: Workspace skeleton and the discovery crate

**Files:**
- Create: `Cargo.toml`, `.gitignore` (replace), `LICENSE`
- Create: `crates/md-librarian/Cargo.toml`
- Create: `crates/md-librarian/src/lib.rs` (copied + renamed from `$SRC/crates/yaams-booklib/src/lib.rs`)

**Interfaces:**
- Produces: crate `md-librarian` with `pub const BOOK_PATH_VAR: &str = "MD_LIBRARIAN_PATH"`, `pub struct Book`, `pub enum Entry { Book(Book), Missing { title: String } }`, `pub fn roots(cli: &[PathBuf]) -> Vec<PathBuf>`, `pub fn default_root() -> PathBuf`, `pub fn library(roots: &[PathBuf], include: Option<&[String]>) -> Vec<Entry>`, `pub fn book_at(roots: &[PathBuf], root_index: usize, dir_name: &str) -> Option<Book>`, `pub fn discover_root(root: &Path, index: usize) -> Vec<Book>`. Later tasks import it as `md_librarian::…`.

- [ ] **Step 1: Confirm the source matches the tag, and branch**

```bash
SRC=~/src/gpui-yaams; DST=~/src/md-librarian
git -C $SRC diff --stat v0.27.0-beta.3 HEAD -- crates/yaams-booklib crates/yaams-bookserve crates/yaams-webview book/src/books.md book/src/docs-window.md book/src/yaams-webview.md package.py rez_build.sh scripts/rez-cargo-test.sh REZ.md docs/agents
cd $DST && git checkout -b feat/import-from-gpui-yaams
```
Expected: the diff prints nothing. If it prints anything, copy from the tag instead: `git -C $SRC show v0.27.0-beta.3:<path>` in place of `cp` throughout this plan.

- [ ] **Step 2: Write the workspace manifest**

`$DST/Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = [
    "crates/md-librarian",
    "crates/md-librarian-serve",
    "crates/md-librarian-webview",
    "crates/md-librarian-cli",
]

[workspace.package]
edition = "2024"
version = "0.1.0"
license = "MIT"
authors = ["Jonathan Gerber <jlgerber@gmail.com>"]
repository = "https://github.com/jlgerber/md-librarian"
```

Cargo will refuse to build until all four members exist. For this task only, temporarily list just `"crates/md-librarian"`; Tasks 2–4 each add their member line back. (Ordering the list as above from the start and adding the directories one task at a time is equivalent; do whichever keeps `cargo test` green at each commit.)

- [ ] **Step 3: Replace `.gitignore` and add the license**

`$DST/.gitignore`:
```
/target
/book/html

# rez build scratch (`rez build` writes build.rxt and its logs here).
/build
```

`$DST/LICENSE`: the MIT license text with `Copyright (c) 2026 Jonathan Gerber`:
```
MIT License

Copyright (c) 2026 Jonathan Gerber

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 4: Copy and rename the discovery crate**

```bash
mkdir -p $DST/crates/md-librarian/src
cp $SRC/crates/yaams-booklib/src/lib.rs $DST/crates/md-librarian/src/lib.rs
sed -i -f $DST/docs/superpowers/plans/rename-from-yaams.sed $DST/crates/md-librarian/src/lib.rs
grep -n -i "yaams" $DST/crates/md-librarian/src/lib.rs
```
Expected leftovers, all prose and all intentional: "the same rule the yaams config search path", "yaams-tk2's `YAAMS_BOOKS_DIR`", "Unlike yaams-tk2's equivalent", "yaams-tk2's `just install-docs`". Anything else means the sed missed a form; fix the sed script, not the file.

Confirm the constant and the default directory came through:
```bash
grep -n 'BOOK_PATH_VAR: &str\|join("md-librarian").join("books")\|md-librarian/books' $DST/crates/md-librarian/src/lib.rs
```
Expected: `pub const BOOK_PATH_VAR: &str = "MD_LIBRARIAN_PATH";`, `data.join("md-librarian").join("books")`, and the doc line naming `$XDG_DATA_HOME/md-librarian/books`.

- [ ] **Step 5: Write the crate manifest**

`$DST/crates/md-librarian/Cargo.toml`:
```toml
[package]
name = "md-librarian"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
publish = false
description = "Discovery for a library of mdbooks: repository roots on a stacking search path, title identity, shadowing, filtering and covers. Pure — no server, no window."

[dependencies]
toml = "0.8"
tracing = "0.1"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 6: Run the discovery tests**

```bash
cd $DST && cargo test -p md-librarian 2>&1 | tail -5
```
Expected: `test result: ok. 14 passed; 0 failed`. The test `build_dir_is_honoured_so_a_book_rendering_to_html_is_found` now creates a book directory named `md-librarian` (the sed renamed the literal); that is the intended case, this repo's own book.

- [ ] **Step 7: Commit**

```bash
cd $DST && cargo fmt --all && git add -A && git commit -m "feat: workspace skeleton and md-librarian discovery crate

Imported from gpui-yaams v0.27.0-beta.3 crates/yaams-booklib, renamed via
docs/superpowers/plans/rename-from-yaams.sed. MD_LIBRARIAN_PATH replaces
YAAMS_BOOK_PATH; the default root is \$XDG_DATA_HOME/md-librarian/books.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```

---

### Task 2: The serve crate

**Files:**
- Modify: `Cargo.toml` (add member `crates/md-librarian-serve` if not already listed)
- Create: `crates/md-librarian-serve/Cargo.toml`
- Create: `crates/md-librarian-serve/src/lib.rs`, `src/shell.rs` (from `$SRC/crates/yaams-bookserve/src/`)
- Create: `crates/md-librarian-serve/tests/serving.rs` (from `$SRC/crates/yaams-bookserve/tests/serving.rs`)

**Interfaces:**
- Consumes: `md_librarian::{book_at, library, Book, Entry, BOOK_PATH_VAR}`.
- Produces: `md_librarian_serve::start(roots: Vec<PathBuf>, include: Option<Vec<String>>) -> anyhow::Result<Server>`, `Server::url(&self) -> &str`, and `md_librarian_serve::shell::{url_segment, html_escape, book_prefix, generated_cover, shell_document, grid_document}`.

- [ ] **Step 1: Copy and rename**

```bash
mkdir -p $DST/crates/md-librarian-serve/src $DST/crates/md-librarian-serve/tests
cp $SRC/crates/yaams-bookserve/src/lib.rs $SRC/crates/yaams-bookserve/src/shell.rs $DST/crates/md-librarian-serve/src/
cp $SRC/crates/yaams-bookserve/tests/serving.rs $DST/crates/md-librarian-serve/tests/
sed -i -f $RENAME $DST/crates/md-librarian-serve/src/*.rs $DST/crates/md-librarian-serve/tests/serving.rs
grep -rn -i "yaams" $DST/crates/md-librarian-serve
```
Expected: no output. (The one intentional yaams-tk2 mention in the shell is about `BOOK_PATH_VAR`, which the sed already turned into `MD_LIBRARIAN_PATH`.)

- [ ] **Step 2: Hand-edit the "Server only" doc paragraph**

The `standalone` feature no longer exists. In `crates/md-librarian-serve/src/lib.rs`, replace the paragraph under `//! # Server only` (it begins `//! Nothing here opens a window.`) with:

```rust
//! Nothing here opens a window. That lives in the `md-librarian` binary (the
//! `md-librarian-cli` crate), which is the only crate in the workspace that
//! links `wry`/`tao`/GTK — so the shell can be tested by fetching `/` with no
//! display, and a repository can be served to an ordinary browser or over
//! `ssh -L`.
```

Verify: `grep -n standalone $DST/crates/md-librarian-serve/src/lib.rs` prints nothing.

- [ ] **Step 3: Write the manifest**

`$DST/crates/md-librarian-serve/Cargo.toml`:
```toml
[package]
name = "md-librarian-serve"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
publish = false
description = "Serves a library of mdbooks over a loopback HTTP origin: a generated card page, a persistent bar over an iframe, and the books themselves. Server only — the window lives in the `md-librarian` binary."

[dependencies]
md-librarian = { path = "../md-librarian" }
anyhow = "1"
tiny_http = "0.12"
tracing = "0.1"

[dev-dependencies]
tempfile = "3"
```

Add `"crates/md-librarian-serve",` to the workspace `members` if it is not there.

- [ ] **Step 4: Run the serving tests**

```bash
cd $DST && cargo test -p md-librarian-serve 2>&1 | grep "test result"
```
Expected: two lines, the integration suite `14 passed; 0 failed` and the (empty) unit suite `0 passed`. The tests bind a loopback port; no display is needed.

- [ ] **Step 5: Commit**

```bash
cd $DST && cargo fmt --all && git add -A && git commit -m "feat: md-librarian-serve — loopback server and shell

Imported from gpui-yaams v0.27.0-beta.3 crates/yaams-bookserve (library and
tests only; the binary moves to md-librarian-cli, so the standalone feature
is gone). Server thread renamed md-librarian-serve.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```

---

### Task 3: The webview crate and its examples

**Files:**
- Modify: `Cargo.toml` (add member `crates/md-librarian-webview`)
- Create: `crates/md-librarian-webview/Cargo.toml`
- Create: `crates/md-librarian-webview/src/lib.rs`, `examples/{help,first_paint,reopen,close_teardown}.rs` (from `$SRC/crates/yaams-webview/`)

**Interfaces:**
- Produces: `md_librarian_webview::{WebContent, WebWindowOptions, WebWindow}` with `WebWindow::open(opts: WebWindowOptions, content: WebContent) -> anyhow::Result<WebWindow>`, `navigate(&self, WebContent)`, `set_title(&self, impl Into<String>)`, `focus(&self)`, `is_open(&self) -> bool`.

- [ ] **Step 1: Copy and rename**

```bash
mkdir -p $DST/crates/md-librarian-webview/src $DST/crates/md-librarian-webview/examples
cp $SRC/crates/yaams-webview/src/lib.rs $DST/crates/md-librarian-webview/src/
cp $SRC/crates/yaams-webview/examples/*.rs $DST/crates/md-librarian-webview/examples/
sed -i -f $RENAME $DST/crates/md-librarian-webview/src/lib.rs $DST/crates/md-librarian-webview/examples/*.rs
grep -rn -i "yaams" $DST/crates/md-librarian-webview
```
Expected: exactly one line, the issue link `https://github.com/jlgerber/gpui-yaams/issues/47` in `src/lib.rs`. Confirm the thread name: `grep -n '"md-librarian-gtk"' src/lib.rs` shows the `.name(...)` call.

- [ ] **Step 2: Add the provenance note to the crate doc**

The source is full of `#43` / `#47` and "Changed in 0.23.x" — gpui-yaams issue numbers and versions. Insert, immediately after the first line of `crates/md-librarian-webview/src/lib.rs` (the `//!` title line), a blank `//!` line followed by:

```rust
//! Extracted from gpui-yaams at v0.27.0-beta.3. Issue numbers in this crate
//! (`#43`, `#47`) and "Changed in 0.2x" notes refer to
//! <https://github.com/jlgerber/gpui-yaams>, where it was developed.
```

- [ ] **Step 3: Write the manifest**

`$DST/crates/md-librarian-webview/Cargo.toml`, keeping the source's dependency comments (copy them from `$SRC/crates/yaams-webview/Cargo.toml` verbatim; they explain why `glib` is named):
```toml
[package]
name = "md-librarian-webview"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
publish = false
description = "Floating WebKit (wry) window driven from any Rust app — help/docs rendering without an external browser"

[dependencies]
anyhow = "1"
# (paste the glib/gtk explanation comment block from the source manifest here)
glib = "0.18"
tao = "0.34"
wry = "0.53"
```

Add `"crates/md-librarian-webview",` to the workspace `members`.

- [ ] **Step 4: Build the crate and its examples**

```bash
cd $DST && cargo build -p md-librarian-webview --examples 2>&1 | tail -3
```
Expected: `Finished`. If the link step fails for a missing system library, install `webkit2gtk-4.1` / `gtk3` dev packages (on Arch: `webkit2gtk-4.1`, `gtk3`); on this machine they are present because gpui-yaams builds. There are no tests in this crate.

- [ ] **Step 5: Commit**

```bash
cd $DST && cargo fmt --all && git add -A && git commit -m "feat: md-librarian-webview — floating WebKit window

Imported from gpui-yaams v0.27.0-beta.3 crates/yaams-webview with its four
examples. GTK thread renamed md-librarian-gtk; issue numbers in the source
keep referring to gpui-yaams, noted in the crate doc.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```

---

### Task 4: The CLI crate — the `md-librarian` binary

**Files:**
- Modify: `Cargo.toml` (add member `crates/md-librarian-cli`)
- Create: `crates/md-librarian-cli/Cargo.toml`
- Create: `crates/md-librarian-cli/src/main.rs` (from `$SRC/crates/yaams-bookserve/src/bin/yaams-books.rs`)

**Interfaces:**
- Consumes: `md_librarian::roots`, `md_librarian_serve::{start, shell::url_segment}`, `md_librarian_webview::{WebWindow, WebWindowOptions, WebContent}`.
- Produces: binary `md-librarian` with flags `--root <DIR>` (repeatable), `--include <TITLE>` (repeatable), `--book <TITLE>`, `--exit-on-stdin-close`, `--parent-pipe <FD>`, `--no-window`. Tasks 6, 8 and 9 invoke it.

- [ ] **Step 1: Copy and rename**

```bash
mkdir -p $DST/crates/md-librarian-cli/src
cp $SRC/crates/yaams-bookserve/src/bin/yaams-books.rs $DST/crates/md-librarian-cli/src/main.rs
sed -i -f $RENAME $DST/crates/md-librarian-cli/src/main.rs
grep -n -i "yaams" $DST/crates/md-librarian-cli/src/main.rs
```
Expected: no output.

- [ ] **Step 2: Hand edits**

(a) The default `RUST_LOG` filter. The sed turned the old `yaams_books=info,yaams_bookserve=info,yaams_booklib=info` into a string with `md_librarian` twice. Replace that string literal with:
```rust
"md_librarian=info,md_librarian_serve=info,md_librarian_webview=info"
```

(b) Provenance. Insert after the first doc line (`//! \`md-librarian\` — the standalone book library viewer.`):
```rust
//!
//! Extracted from gpui-yaams (`yaams-books`) at v0.27.0-beta.3; the CLI is
//! unchanged apart from the names.
```

(c) Check the help text came through: `grep -n "MD_LIBRARIAN_PATH\|md-librarian/books" src/main.rs` shows the `long_about` and the `--root` doc.

- [ ] **Step 3: Write the manifest**

`$DST/crates/md-librarian-cli/Cargo.toml`:
```toml
[package]
name = "md-librarian-cli"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
publish = false
description = "The `md-librarian` binary: serves a library of mdbooks and opens a window on it. The only crate in the workspace that links wry/tao/GTK."

[[bin]]
name = "md-librarian"
path = "src/main.rs"
# The lib crate is also called md-librarian; keep `cargo doc --workspace`
# from writing both into target/doc/md_librarian/.
doc = false

[dependencies]
md-librarian = { path = "../md-librarian" }
md-librarian-serve = { path = "../md-librarian-serve" }
md-librarian-webview = { path = "../md-librarian-webview" }
anyhow = "1"
clap = { version = "4", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

Add `"crates/md-librarian-cli",` to the workspace `members`. The list now has all four entries in the order given in Task 1.

- [ ] **Step 4: Build and smoke-test without a display**

```bash
cd $DST && cargo build --workspace 2>&1 | tail -2
cargo run -q --bin md-librarian -- --help | grep -n "MD_LIBRARIAN_PATH\|md-librarian/books"
mkdir -p /tmp/mdl-smoke/one && printf '[book]\ntitle = "One"\n' > /tmp/mdl-smoke/one/book.toml && mkdir -p /tmp/mdl-smoke/one/book && echo '<h1>one</h1>' > /tmp/mdl-smoke/one/book/index.html
timeout 5 cargo run -q --bin md-librarian -- --root /tmp/mdl-smoke --no-window | head -1
```
Expected: `--help` shows both names; the last command prints one line `http://127.0.0.1:<port>/` and `timeout` ends it (exit 124 is fine).

- [ ] **Step 5: Full workspace test and commit**

```bash
cd $DST && cargo test --workspace 2>&1 | grep "test result" 
cargo fmt --all && git add -A && git commit -m "feat: md-librarian-cli — the md-librarian binary

The former yaams-books bin from gpui-yaams v0.27.0-beta.3, in its own crate
instead of behind a standalone feature: the library crates stay free of
wry/tao/GTK by construction.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```
Expected before the commit: every `test result` line says `0 failed`; 14 + 14 tests passed in total.

---

### Task 5: The book

**Files:**
- Create: `book/book.toml`, `book/src/SUMMARY.md`
- Create: `book/src/introduction.md`, `book/src/getting-started.md`, `book/src/releasing.md`, `book/src/rez.md` (new)
- Create: `book/src/library.md` (from `$SRC/book/src/books.md`), `book/src/webview.md` (from `yaams-webview.md`), `book/src/docs-window.md` (from `docs-window.md`)

**Interfaces:**
- Consumes: the binary and crate names from Tasks 1–4.
- Produces: `book/html/` when built; `book/book.toml` with `build-dir = "html"`, which Task 8's rez build ships as the library root.

- [ ] **Step 1: book.toml and SUMMARY**

`$DST/book/book.toml`:
```toml
[book]
title = "md-librarian"
authors = ["Jonathan Gerber"]
description = "A library of mdbooks: discovery on a search path, a loopback server with a card page, and a floating WebKit viewer."
src = "src"

[build]
# Not cosmetic: discovery honours [build] build-dir, and this repo's own book
# is the test case for it. The rez package ships this directory as a root.
build-dir = "html"

[output.html]
default-theme = "navy"
preferred-dark-theme = "navy"
git-repository-url = "https://github.com/jlgerber/md-librarian"
```

`$DST/book/src/SUMMARY.md`:
```markdown
# Summary

- [Introduction](./introduction.md)
- [Getting started](./getting-started.md)
- [A library of books](./library.md)
- [The webview window](./webview.md)
- [Building a documentation window](./docs-window.md)
- [Releasing](./releasing.md)
- [rez packaging](./rez.md)
```

- [ ] **Step 2: Copy and rename the three existing chapters**

```bash
mkdir -p $DST/book/src
cp $SRC/book/src/books.md        $DST/book/src/library.md
cp $SRC/book/src/yaams-webview.md $DST/book/src/webview.md
cp $SRC/book/src/docs-window.md  $DST/book/src/docs-window.md
sed -i -f $RENAME $DST/book/src/library.md $DST/book/src/webview.md $DST/book/src/docs-window.md
# the chapter file was renamed, so fix intra-book links
sed -i 's#\./md-librarian-webview\.md#./webview.md#g; s#\./books\.md#./library.md#g' $DST/book/src/*.md
grep -n -i "yaams" $DST/book/src/library.md $DST/book/src/webview.md $DST/book/src/docs-window.md
```
Expected leftovers, all intentional (verified by a dry run at plan time): yaams-tk2 mentions (build-dir contrast, `YAAMS_BOOKS_DIR`, "extracted from yaams-tk2 (yaams-tk2#489)", "In yaams-tk2 exactly one book"), "the same rule as the yaams config search path", the three `[#43]`/`[#45]`/`[#47]` link definitions to gpui-yaams issues, and the `yaams-ui` gallery mention in webview.md (edited next). Anything else: fix the sed script.

- [ ] **Step 3: Hand edits to the copied chapters**

`library.md`:
- The paragraph starting "`md-librarian-serve` deliberately does **not** open a window" mentions "behind the crate's `standalone` feature". Replace that paragraph with:
  ```markdown
  `md-librarian-serve` deliberately does **not** open a window: that lives in the
  `md-librarian` binary (the `md-librarian-cli` crate), the only crate that links
  `wry`/`tao`/GTK — so the pages can be tested with no display, and a repository
  can be served to an ordinary browser or over `ssh -L`.
  ```
- The command block near the top lists `md-librarian --include gpui` and `--book gpui`; leave them, `gpui` is just an example title.

`webview.md`:
- Title line `# md-librarian-webview` stays. Replace the first paragraph ("A **floating WebKit window** (wry) a gpui app can open and drive — built for usdlite's help rendering (usdlite #1302)…") with:
  ```markdown
  A **floating WebKit window** (wry) any Rust app can open and drive — built
  for usdlite's help rendering (usdlite #1302): show docs in our own process
  instead of punting to an external browser. Extracted from gpui-yaams at
  v0.27.0-beta.3; the `[#43]` / `[#47]` links below and the "Changed in 0.2x"
  notes refer to that repository's history.
  ```
- The paragraph "Because it is not a gpui widget, it is deliberately **not in the `yaams-ui` gallery** — its live reference is its own example:" becomes "Its live reference is its own example:".
- The section "Why a floating window, not a pane" discusses docking into a gpui window. Keep it verbatim: it is measured history and consumers are gpui apps.

`docs-window.md`: the gpui `Global` example stays; it documents how a gpui host holds the handle and is still correct.

- [ ] **Step 4: Write the new chapters**

`$DST/book/src/introduction.md`:
```markdown
# md-librarian

A **library of mdbooks**: put built books under one or more directories, and
`md-librarian` finds them, serves them over a loopback origin, and opens a window
with a card per book, a persistent bar, and a way back.

```text
md-librarian                                  # MD_LIBRARIAN_PATH, else the XDG default
md-librarian --root ~/books --root /opt/books # explicit roots; the first wins
md-librarian --book "usdlite user guide"      # open straight onto one book
md-librarian --no-window                      # serve only; prints the URL
```

Four crates, one binary:

| Crate | What it is | Links GTK? |
|---|---|---|
| `md-librarian` | discovery: what counts as a book, which roots, which copy wins | no |
| `md-librarian-serve` | the loopback server and the generated shell page | no |
| `md-librarian-webview` | a floating WebKit (wry) window | yes |
| `md-librarian-cli` | the `md-librarian` binary, tying the three together | yes |

An application depends on `md-librarian` alone to ask "is that book installed?",
and launches the binary out of process to show it — see
[Getting started](./getting-started.md). The design and its measured
constraints are in [A library of books](./library.md).

This project was extracted from
[gpui-yaams](https://github.com/jlgerber/gpui-yaams) at v0.27.0-beta.3, where
it was `yaams-booklib`, `yaams-bookserve`, `yaams-webview` and `yaams-books`.
```

`$DST/book/src/getting-started.md`:
```markdown
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

The binary needs WebKitGTK at runtime; `--no-window` does not.

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
```

`$DST/book/src/releasing.md`:
```markdown
# Releasing

md-librarian releases as a **single unit**: all four crates inherit
`version.workspace = true`, and each release is a git tag `vX.Y.Z` that
consumers pin.

1. Verify: `just test`, `just docs-build`.
2. Bump `workspace.package.version` in the root `Cargo.toml` per semver.
3. Move the `[Unreleased]` entries in `CHANGELOG.md` under `## [X.Y.Z] - YYYY-MM-DD`.
4. Commit and push `main`.
5. Tag and push:
   ```sh
   git tag -a vX.Y.Z -m "vX.Y.Z — <summary>"
   git push origin vX.Y.Z
   ```
6. In each consumer, bump the `tag`:
   ```toml
   md-librarian = { git = "https://github.com/jlgerber/md-librarian", tag = "vX.Y.Z" }
   ```

Versioning: **patch** for internal fixes, **minor** for additive API or new CLI
flags, **major** for breaking API, a renamed flag, or a change to the
`MD_LIBRARIAN_PATH` / root-layout contract.
```

`$DST/book/src/rez.md`:
```markdown
# rez packaging

`package.py` ships this repo as the rez package **`md_librarian`** (rez reads
`-` as the name/version separator, so the hyphenated name is not legal). The
package holds the built tools and the rendered book, not the crates: another
Rust workspace depends on the crates as cargo git dependencies pinned to a tag.

| Tool | Crate | What it is |
|---|---|---|
| `md-librarian` | `md-librarian-cli` | the library viewer |
| `md-librarian-webview-demo` | `md-librarian-webview` | the `help` example, under a name safe for a shared PATH |

The package also installs this book as a discoverable root at
`{root}/books/md-librarian/` and **appends** `{root}/books` to
`MD_LIBRARIAN_PATH`, so a resolve opens with something in it and a user's own
roots still win.

```sh
just rez-build                 # rez build -i into ~/packages
just rez-build-isolated        # into /tmp/rez-md-librarian, touching nothing
just rez-test                  # the default tests: tools on PATH, the book root
rez test md_librarian books_serve   # binds a port; explicit
rez test md_librarian cargo_test    # the workspace suite; needs a source checkout
```

`REZ.md` at the repository root has the full description: the knobs
(`MD_LIBRARIAN_RUN_TESTS`, `MD_LIBRARIAN_SKIP_BOOK`), the pre-release
versioning trap, and the `WEBKIT_DISABLE_DMABUF_RENDERER` workaround.
```

- [ ] **Step 5: Build the book**

```bash
cd $DST/book && mdbook build 2>&1 | tail -3 && ls html/index.html
grep -rn "yaams-webview.md\|books.md" $DST/book/src/*.md
```
Expected: `mdbook build` reports no warnings about missing files; `html/index.html` exists; the grep prints nothing (every intra-book link was rewritten). Open `html/library.html` in a browser if in doubt about the anchors.

- [ ] **Step 6: Commit**

```bash
cd $DST && git add -A && git commit -m "docs: the md-librarian book

Chapters imported from gpui-yaams (books, yaams-webview, docs-window) and
renamed; new introduction, getting-started, releasing and rez chapters.
build-dir = html so the repo's own book exercises discovery's build-dir
support and ships as the rez package's library root.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```

---

### Task 6: Repo scaffolding — README, CLAUDE.md, CHANGELOG, agent docs, justfile

**Files:**
- Create: `README.md` (replace), `CLAUDE.md`, `CHANGELOG.md`, `justfile`
- Create: `docs/agents/issue-tracker.md`, `docs/agents/triage-labels.md`, `docs/agents/domain.md` (from `$SRC/docs/agents/`)

**Interfaces:**
- Consumes: the binary from Task 4 (`just run`), the book from Task 5 (`just docs`).
- Produces: recipes `build`, `test`, `run`, `docs`, `docs-build`, `install`, `rez-build`, `rez-build-isolated`, `rez-test`, `rez-run` that Task 8 and the README refer to.

- [ ] **Step 1: README**

`$DST/README.md`:
```markdown
# md-librarian

A library of mdbooks: discovery on a search path, a loopback server with a card
page and a persistent bar, and a floating WebKit viewer.

```sh
cargo install --path crates/md-librarian-cli
MD_LIBRARIAN_PATH=~/books md-librarian          # every book under ~/books
md-librarian --book "usdlite user guide"        # straight onto one book
md-librarian --no-window                        # serve only; prints the URL
```

A **root** is a directory of books, each a subdirectory holding `book.toml`
beside its built output. Roots stack; the first one wins.

- `md-librarian` — discovery (what an application depends on)
- `md-librarian-serve` — the server and the generated shell
- `md-librarian-webview` — the floating WebKit window
- `md-librarian-cli` — the `md-librarian` binary

Docs: `just docs` (mdbook). Packaging: `REZ.md`. Extracted from
[gpui-yaams](https://github.com/jlgerber/gpui-yaams) at v0.27.0-beta.3.
```

- [ ] **Step 2: CLAUDE.md**

`$DST/CLAUDE.md`:
```markdown
# md-librarian — working notes

`md-librarian` is the mdbook **library viewer**: `md-librarian` (discovery, pure),
`md-librarian-serve` (loopback server + generated shell), `md-librarian-webview`
(floating wry/WebKit window) and `md-librarian-cli` (the `md-librarian` binary).
Extracted from gpui-yaams at v0.27.0-beta.3; usdlite and yaams-tk2 consume it.

## Documentation is part of the feature

**Every public change MUST be documented in the mdbook in the same change**:

- Extend the relevant chapter under `book/src/` (`library.md` for discovery and
  serving, `webview.md` for the window, `getting-started.md` for host wiring),
  and keep `book/src/SUMMARY.md` current.
- Every new public function or CLI flag gets a **usage snippet** with the real
  signature.
- Run `just docs-build` before committing. The book is also the rez package's
  shipped library root, so a broken book ships.

## Gotchas

- **Only `md-librarian-cli` may link wry/tao/GTK.** `md-librarian` and
  `md-librarian-serve` are display-free by construction; that is what lets
  their tests run headless and lets an application depend on discovery without
  paying for WebKit. Do not add the webview crate as a dependency of either.
- **Wayland explicit-sync.** On some driver/compositor combinations every
  WebKitGTK window dies with `Error 71 (Protocol error)`. `just run` and the
  rez package default `WEBKIT_DISABLE_DMABUF_RENDERER=1` (guarded, so an
  explicit value wins). It is a workaround for other people's bug; see
  `book/src/library.md` → "Known issue".
- **Roots stack, first wins.** `MD_LIBRARIAN_PATH` is a list; the rez package
  *appends* its own book so a user's roots shadow it. Keep that direction.
- **Identity is the title.** mdbook rejects unknown `[book]` keys, so a book
  cannot carry an id. Do not design around one.
- **Issue numbers in the webview crate** (`#43`, `#47`) refer to gpui-yaams.

## Agent skills

### Issue tracker

GitHub issues in `jlgerber/md-librarian`, via `gh`. See `docs/agents/issue-tracker.md`.

### Triage labels

See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root (created lazily).
See `docs/agents/domain.md`.
```

- [ ] **Step 3: CHANGELOG**

`$DST/CHANGELOG.md`:
```markdown
# Changelog

All notable changes to **md-librarian** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
(pre-1.0: minor versions may add features and make breaking changes).

md-librarian releases as a **single unit** — all crates share one
`workspace.package.version`, and each release is a git tag `vX.Y.Z` that
consumers pin.

## [Unreleased]

### Added

- **Extracted from gpui-yaams v0.27.0-beta.3.** The mdbook library viewer —
  `yaams-booklib`, `yaams-bookserve`, `yaams-webview` and the `yaams-books`
  binary — moves here unchanged in behaviour, renamed:

  | gpui-yaams | md-librarian |
  |---|---|
  | `yaams-booklib` | `md-librarian` |
  | `yaams-bookserve` | `md-librarian-serve` |
  | `yaams-webview` | `md-librarian-webview` |
  | `yaams-books` (bin, behind `standalone`) | `md-librarian` (bin, crate `md-librarian-cli`) |
  | `YAAMS_BOOK_PATH` | `MD_LIBRARIAN_PATH` |
  | `$XDG_DATA_HOME/yaams/books` | `$XDG_DATA_HOME/md-librarian/books` |
  | rez `gpui_yaams` | rez `md_librarian` |

  The `standalone` feature is gone: the binary has its own crate, so the
  library crates are free of wry/tao/GTK by construction. Public API shapes
  and CLI flags are unchanged. Earlier history lives in gpui-yaams'
  CHANGELOG (0.22.0 through 0.27.0-beta.3).
```

- [ ] **Step 4: Agent docs**

```bash
mkdir -p $DST/docs/agents
cp $SRC/docs/agents/issue-tracker.md $SRC/docs/agents/triage-labels.md $SRC/docs/agents/domain.md $DST/docs/agents/
sed -i -f $RENAME $DST/docs/agents/*.md
```
Then in `docs/agents/domain.md` replace the crate tree in the "File structure" block and the sentence after it:
```
└── crates/
    ├── md-librarian/
    ├── md-librarian-serve/
    ├── md-librarian-webview/
    └── md-librarian-cli/
```
and: "The four crates are one pipeline (discover → serve → show) sharing one domain, not separate bounded contexts, so they take one glossary between them." Also change "what each crate is for, the documentation-is-part-of-the-feature rule, and the gpui gotchas" to "... and the gotchas", and "one chapter per crate" to "one chapter per concern". Verify: `grep -n "yaams\|gpui" $DST/docs/agents/*.md` prints nothing.

- [ ] **Step 5: justfile**

`$DST/justfile`:
```make
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
```

- [ ] **Step 6: Verify the recipes and commit**

```bash
cd $DST && just --list && just test 2>&1 | grep "test result" && just docs-build | tail -1
```
Expected: the list shows every recipe above; all `test result` lines say `0 failed`; mdbook builds. `just run` is a manual check: it should open a window showing a card titled "md-librarian" (the repo's own book, because `--root .` and `book/` holds `book.toml` beside `html/`). Do it once if a display is available; do not block on it otherwise.

```bash
git add -A && git commit -m "chore: README, CLAUDE.md, CHANGELOG, agent docs and justfile

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```

---

### Task 7: CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `cargo test --workspace`, `mdbook build book`.

- [ ] **Step 1: Write the workflow**

`$DST/.github/workflows/ci.yml`:
```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      # wry / tao link WebKitGTK and GTK3 even though no test opens a window.
      - name: System libraries for wry/tao
        run: |
          sudo apt-get update
          sudo apt-get install -y --no-install-recommends \
            libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libssl-dev pkg-config

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt

      - uses: Swatinem/rust-cache@v2

      - name: fmt
        run: cargo fmt --all --check

      - name: test
        run: cargo test --workspace --locked

      - name: build examples
        run: cargo build --workspace --examples --locked

  book:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: peaceiris/actions-mdbook@v2
        with:
          mdbook-version: latest
      - run: mdbook build book
```

- [ ] **Step 2: Validate locally what can be validated**

```bash
cd $DST && cargo fmt --all --check && cargo test --workspace --locked 2>&1 | grep "test result" && cargo build --workspace --examples --locked 2>&1 | tail -1
```
Expected: fmt clean, all tests `0 failed`, examples `Finished`. `Cargo.lock` must be committed for `--locked` to work in CI (it was created by Task 1's first build; confirm with `git ls-files Cargo.lock`).

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "ci: fmt, test, examples and mdbook build on ubuntu

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```

The apt package list is a best guess for wry 0.53 / tao 0.34; if the first CI run fails on a missing `.pc` file, add the named package and amend. That is the one thing this plan cannot verify offline.

---

### Task 8: rez package

**Files:**
- Create: `package.py`, `rez_build.sh`, `REZ.md`
- Create: `scripts/rez-cargo-test.sh` (from `$SRC/scripts/rez-cargo-test.sh`)

**Interfaces:**
- Consumes: the `md-librarian` binary and the `help` example (Tasks 3–4), the book (Task 5), the justfile recipes (Task 6).
- Produces: rez package `md_librarian` with tools `md-librarian`, `md-librarian-webview-demo`; env `MD_LIBRARIAN_PATH` appended with `{root}/books`; tests `tools_present`, `books_root`, `books_serve`, `cargo_test`.

- [ ] **Step 1: The cargo-test helper**

```bash
mkdir -p $DST/scripts
cp $SRC/scripts/rez-cargo-test.sh $DST/scripts/rez-cargo-test.sh
sed -i -f $RENAME $DST/scripts/rez-cargo-test.sh
chmod +x $DST/scripts/rez-cargo-test.sh
grep -n "yaams\|gpui" $DST/scripts/rez-cargo-test.sh
```
Expected: only the comment `# --locked for the same reason the build uses it: the gpui / gpui-component` line. Replace that two-line comment with `# --locked for the same reason the build uses it: test what Cargo.lock records.`. Also `rez test md_librarian cargo_test -- -p yaams-frameset` in the header comment becomes `-- -p md-librarian`.

- [ ] **Step 2: package.py**

`$DST/package.py`:
```python
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
```

- [ ] **Step 3: rez_build.sh**

`$DST/rez_build.sh` (mode 0755):
```bash
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
```

- [ ] **Step 4: REZ.md**

`$DST/REZ.md`:
```markdown
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
  into `{root}/books/md-librarian/`, with its `book.toml` beside its `html/`
  output. That layout is what the viewer scans for. Skipped, with a message,
  when `mdbook` is absent or `MD_LIBRARIAN_SKIP_BOOK=1`.
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
```

- [ ] **Step 5: Isolated build and the default tests**

```bash
cd $DST && chmod +x rez_build.sh && just rez-build-isolated 2>&1 | tail -8
PKG=/tmp/rez-md-librarian
PATHS="$PKG:$(rez config packages_path | sed 's/^ *- *//' | tr '\n' ':')"
rez test md_librarian --paths "$PATHS" 2>&1 | tail -5
rez test md_librarian --paths "$PATHS" books_serve 2>&1 | tail -3
rez env md_librarian --paths "$PATHS" -- sh -c 'echo "$MD_LIBRARIAN_PATH"; ls "$REZ_MD_LIBRARIAN_ROOT/bin" "$REZ_MD_LIBRARIAN_ROOT/books/md-librarian"'
```
Expected: the build ends with the DONE banner listing `tools ->` and `books ->`; `tools_present` and `books_root` pass; `books_serve` passes; the env check prints a path ending in `/books`, the two tools, and `book.toml html`.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "build: rez package md_librarian

package.py, rez_build.sh, REZ.md and scripts/rez-cargo-test.sh adapted from
gpui-yaams: ships md-librarian and md-librarian-webview-demo, and the book
as a library root appended to MD_LIBRARIAN_PATH.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```

---

### Task 9: Final audit

**Files:** none new; fixes only if the audit finds something.

- [ ] **Step 1: The yaams grep**

```bash
cd $DST && grep -rn -i "yaams\|gpui" --exclude-dir=target --exclude-dir=.git --exclude-dir=html . | grep -v "docs/superpowers/"
```
Every line must be one of:
- a link to `github.com/jlgerber/gpui-yaams` (issues, or the provenance mentions in README, CLAUDE.md, CHANGELOG, introduction.md, webview.md, and the crate docs of `md-librarian-webview` and `md-librarian-cli`);
- a yaams-tk2 or usdlite mention as origin or contrast (`YAAMS_BOOKS_DIR`, `yaams-tk2#489`, "yaams config search path", `just install-docs`, usdlite #1302);
- the gpui docking discussion in `webview.md` and the gpui `Global` host example in `docs-window.md`;
- `rename-from-yaams.sed` and this plan (excluded above).

Anything else is a missed rename: add the form to `rename-from-yaams.sed` if it is mechanical, re-run the sed over the affected files, and note the addition in the commit.

- [ ] **Step 2: Everything green**

```bash
cd $DST && cargo fmt --all --check && cargo test --workspace --locked 2>&1 | grep "test result" && cargo build --workspace --examples --locked 2>&1 | tail -1 && (cd book && mdbook build 2>&1 | tail -1) && git status --short
```
Expected: fmt clean; `14 passed` twice, `0 failed` everywhere; examples build; book builds; working tree clean (or only audit fixes, which get committed).

- [ ] **Step 3: Commit any fixes and open the PR**

```bash
cd $DST && git add -A && git commit -m "chore: audit fixes after the import" 2>/dev/null || true
git push -u origin feat/import-from-gpui-yaams
gh pr create --title "Import the mdbook library viewer from gpui-yaams" --body "$(cat <<'EOF'
Imports yaams-booklib, yaams-bookserve, yaams-webview and the yaams-books binary from gpui-yaams v0.27.0-beta.3 as md-librarian, md-librarian-serve, md-librarian-webview and md-librarian-cli. Behaviour, public API shapes and CLI flags are unchanged; names, the env var (MD_LIBRARIAN_PATH), the default root and the rez package (md_librarian) are new.

Spec: docs/superpowers/specs/2026-09-05-extract-from-gpui-yaams-design.md
Plan: docs/superpowers/plans/2026-09-05-import-from-gpui-yaams.md

Not in this PR: removing the crates from gpui-yaams, migrating usdlite / yaams-tk2, tagging v0.1.0.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ
EOF
)"
```
Then watch the first CI run; the apt package list in Task 7 is the one thing that could not be verified locally.

---

## Self-review against the spec

- Workspace, four crates, no gpui, no patch: Tasks 1–4. ✔
- Rename table, including thread names, rez knobs, `REZ_MD_LIBRARIAN_ROOT`, `{root}/books/md-librarian/`: the sed script plus Tasks 4, 8. ✔
- Kept historical prose: enumerated in Task 9 Step 1. ✔
- Book with seven chapters, `build-dir = "html"`, git-repository-url: Task 5. ✔
- CLAUDE.md, docs/agents, CHANGELOG, README, LICENSE, justfile: Tasks 1, 6. ✔
- rez package with two tools, appended root, guarded WebKit env, four tests: Task 8. ✔
- CI (fmt, test, examples, mdbook via peaceiris action): Task 7. ✔
- Verification list (28 tests, examples build, yaams grep, `just run`, isolated rez build, mdbook build): Tasks 4–9. ✔
- Type consistency: `Entry::Missing { title }` is used in getting-started.md as `Entry::Missing { .. }`, matching Task 1's interface. `md_librarian_serve::start(Vec<PathBuf>, Option<Vec<String>>)` matches the CLI's call. ✔
