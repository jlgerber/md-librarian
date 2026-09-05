# `md-librarian build` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `build` subcommand to the `md-librarian` binary that runs `mdbook build` on every stale book on the search path (or on given book directories) and can install slim copies into a root with `--into`.

**Architecture:** Staleness (`src_dir`, `newest_input`, `is_stale`) and a public `read_book(dir)` go into the discovery crate beside `is_built`, pure filesystem, unit-tested on temp dirs. The CLI crate's `main.rs` splits into `main.rs` (clap types, dispatch), `serve.rs` (the existing path, moved verbatim) and `build.rs` (selection, the `mdbook` subprocess, the install copy, the summary). No new dependencies.

**Tech Stack:** Rust 2024, clap 4 derive (`Subcommand`, `Args`), std `File::set_modified` for test fixtures, `mdbook` as an external process, tempfile (dev).

**Spec:** `docs/superpowers/specs/2026-09-05-build-subcommand-design.md` (this repo). Read it first; every task below argues from it.

## Global Constraints

- The bare `md-librarian` command, all six existing flags, and their behaviour are unchanged. `md-librarian --root x --no-window` must still print exactly the URL on stdout.
- Existing public items of `md-librarian` (discovery) keep their shapes: `BOOK_PATH_VAR`, `Book` (all existing pub fields), `Entry`, `roots`, `default_root`, `library`, `book_at`, `discover_root`, `Book::is_built`, `Entry::title`. Additions only.
- No new crate dependencies. `mdbook` is found on `PATH` and run as a subprocess; the `mdbook` crate is never linked.
- The discovery crate never spawns a process. Only `md-librarian-cli` does.
- Staleness rule (spec): stale = not built, or `newest_input()` newer than `build_dir/index.html`. Equal mtimes are up to date. `newest_input() == None` on a built book is up to date. Inputs are `book.toml`, everything under the src dir (`[book] src`, default `src`), everything under `dir/theme` if present; never descend into `build_dir`.
- Install rule (spec): destination `ROOT/<dir_name>/` holds exactly `book.toml`, `cover.<ext>` if any, and the output at the same relative `build-dir` path. Copy only when the destination `index.html` is missing or older than the source's. Replace, never merge. Refuse (log, continue, no exit-code change) when `build_dir` is absolute or escapes the book dir, or when the destination exists without a `book.toml`. Skip silently when destination is the source.
- Exit code: `1` if any book failed to build or an install hit an I/O error; `0` otherwise. Summary line on stderr: `built N, up to date M, failed K`.
- Logs on stderr via `tracing`; stdout stays clean.
- Branch `feat/build-subcommand` in `~/src/md-librarian` (already exists, holds the spec). Commit after every task with the trailer:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ
  ```
- Run `cargo fmt --all` before each commit. `cargo test --workspace` must stay green after every task (29 tests before this plan).

---

## File map

| Path | Change | Responsibility |
|---|---|---|
| `crates/md-librarian/src/lib.rs` | modify | `Meta.src`, private `Book.src`, `Book::{src_dir,newest_input,is_stale}`, `pub fn read_book`, tests |
| `crates/md-librarian-cli/src/main.rs` | rewrite | crate doc, clap `Cli`/`Command`, tracing init, dispatch |
| `crates/md-librarian-cli/src/serve.rs` | create | `ServeArgs`, `run`, `watch_eof`, `park` (moved from main.rs) |
| `crates/md-librarian-cli/src/build.rs` | create | `BuildArgs`, `select`, `run`, `which_mdbook`, `mdbook_build`, `install`, tests |
| `crates/md-librarian-cli/tests/build.rs` | create | end-to-end: runs the binary against a fixture book |
| `crates/md-librarian-cli/Cargo.toml` | modify | `tempfile` dev-dependency |
| `.github/workflows/ci.yml` | modify | mdbook on the `test` job |
| `book/src/library.md`, `book/src/getting-started.md`, `README.md`, `CHANGELOG.md`, `CLAUDE.md` | modify | docs |

---

### Task 1: Staleness and `read_book` in the discovery crate

**Files:**
- Modify: `crates/md-librarian/src/lib.rs` (struct `Book` ~line 63, `impl Book` ~line 104, `struct Meta` ~line 324, `read_meta` ~line 335, `read_book` ~line 291, tests module at the end)

**Interfaces:**
- Produces:
  - `pub fn read_book(dir: &Path) -> Option<Book>` — `None` unless `dir/book.toml` is a file; `root_index` is 0, `ambiguous` false.
  - `impl Book { pub fn src_dir(&self) -> PathBuf; pub fn newest_input(&self) -> Option<std::time::SystemTime>; pub fn is_stale(&self) -> bool }`
  - `Book` gains a **private** field `src: PathBuf` (the joined source dir). External crates never construct `Book` literals (verified: none in `md-librarian-serve`), so this is not a break.

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` in `crates/md-librarian/src/lib.rs`, after the existing helpers (`book`, `built`, `titles`):

```rust
    use std::time::{Duration, SystemTime};

    /// A fixed instant so tests never depend on the wall clock.
    fn t0() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    /// Set a file's mtime (directories cannot be opened for writing on Linux,
    /// so only files are stamped; `newest_input` only looks at files).
    fn stamp(path: &Path, at: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    fn write(path: &Path, body: &str, at: SystemTime) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
        stamp(path, at);
    }

    #[test]
    fn read_book_needs_a_book_toml() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("plain")).unwrap();
        assert!(read_book(&tmp.path().join("plain")).is_none());
        let dir = book(tmp.path(), "guide", "[book]\ntitle = \"Guide\"\n");
        let b = read_book(&dir).expect("a book.toml makes a book");
        assert_eq!(b.title, "Guide");
        assert_eq!(b.dir_name, "guide");
        assert_eq!(b.root_index, 0);
    }

    #[test]
    fn src_dir_honours_book_src_and_defaults_to_src() {
        let tmp = tempfile::tempdir().unwrap();
        let a = book(tmp.path(), "a", "[book]\ntitle = \"A\"\nsrc = \"docs\"\n");
        let b = book(tmp.path(), "b", "[book]\ntitle = \"B\"\n");
        assert_eq!(read_book(&a).unwrap().src_dir(), a.join("docs"));
        assert_eq!(read_book(&b).unwrap().src_dir(), b.join("src"));
    }

    #[test]
    fn newest_input_covers_toml_src_and_theme_but_never_the_build_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // build-dir deliberately INSIDE src: the walk must step over it.
        let dir = book(
            tmp.path(),
            "g",
            "[book]\ntitle = \"G\"\n\n[build]\nbuild-dir = \"src/out\"\n",
        );
        stamp(&dir.join("book.toml"), t0());
        write(&dir.join("src/intro.md"), "# hi", t0() + Duration::from_secs(10));
        write(&dir.join("theme/x.css"), "b{}", t0() + Duration::from_secs(20));
        write(&dir.join("src/out/index.html"), "<h1/>", t0() + Duration::from_secs(999));
        let b = read_book(&dir).unwrap();
        assert_eq!(b.newest_input(), Some(t0() + Duration::from_secs(20)));
    }

    #[test]
    fn an_unbuilt_book_is_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = book(tmp.path(), "g", "[book]\ntitle = \"G\"\n");
        assert!(read_book(&dir).unwrap().is_stale());
    }

    #[test]
    fn stale_when_a_source_is_newer_than_index_html() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = book(tmp.path(), "g", "[book]\ntitle = \"G\"\n");
        stamp(&dir.join("book.toml"), t0());
        write(&dir.join("book/index.html"), "<h1/>", t0() + Duration::from_secs(10));
        write(&dir.join("src/intro.md"), "# hi", t0() + Duration::from_secs(20));
        assert!(read_book(&dir).unwrap().is_stale());
    }

    #[test]
    fn fresh_when_index_html_is_newer_than_every_input() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = book(tmp.path(), "g", "[book]\ntitle = \"G\"\n");
        stamp(&dir.join("book.toml"), t0());
        write(&dir.join("src/intro.md"), "# hi", t0() + Duration::from_secs(10));
        write(&dir.join("book/index.html"), "<h1/>", t0() + Duration::from_secs(20));
        assert!(!read_book(&dir).unwrap().is_stale());
    }

    #[test]
    fn equal_mtimes_are_up_to_date() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = book(tmp.path(), "g", "[book]\ntitle = \"G\"\n");
        stamp(&dir.join("book.toml"), t0());
        write(&dir.join("src/intro.md"), "# hi", t0());
        write(&dir.join("book/index.html"), "<h1/>", t0());
        assert!(!read_book(&dir).unwrap().is_stale());
    }

    #[test]
    fn a_built_book_with_no_readable_inputs_is_up_to_date() {
        let tmp = tempfile::tempdir().unwrap();
        // The book directory does not exist at all, so nothing is readable;
        // only the output does. `newest_input` is None and that means fresh.
        let out = tmp.path().join("out");
        write(&out.join("index.html"), "<h1/>", t0());
        let ghost = tmp.path().join("ghost");
        let b = Book {
            title: "Ghost".into(),
            dir_name: "ghost".into(),
            dir: ghost.clone(),
            build_dir: out,
            description: String::new(),
            cover: None,
            root_index: 0,
            ambiguous: false,
            src: ghost.join("src"),
        };
        assert_eq!(b.newest_input(), None);
        assert!(!b.is_stale());
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cd ~/src/md-librarian && cargo test -p md-librarian 2>&1 | grep -E "^error|cannot find|no field|test result" | head`
Expected: compile errors: `cannot find function \`read_book\``, `no method named \`src_dir\``, `no field \`src\``.

- [ ] **Step 3: Implement**

(a) `Meta` gains `src`:
```rust
/// The four keys read out of a `book.toml`.
#[derive(Default)]
struct Meta {
    title: Option<String>,
    description: Option<String>,
    build_dir: Option<String>,
    src: Option<String>,
}
```
and in `read_meta`'s final struct literal add:
```rust
        src: str_at("book", "src").filter(|s| !s.is_empty()),
```
Update the doc comment above `read_meta` to say `[book] title`/`description`/`src` and `[build] build-dir`.

(b) `Book` gains the private field. Add after `ambiguous`:
```rust
    /// The source directory: `dir` joined with `[book] src` (default `src`).
    /// Private because it is derived; read it through [`Book::src_dir`].
    src: PathBuf,
```
Add a constant beside `DEFAULT_BUILD_DIR`:
```rust
/// mdbook's default source directory, used when `book.toml` sets no `[book] src`.
const DEFAULT_SRC_DIR: &str = "src";
```
In the private `read_book(dir: &Path, root_index: usize) -> Book` function, set `src: dir.join(meta.src.as_deref().unwrap_or(DEFAULT_SRC_DIR)),` in the `Book { .. }` literal. Rename that private function to `read_book_in_root` (both call sites: `discover_root` and this new public wrapper) so the public name is free.

(c) The public constructor, placed right after `discover_root`:
```rust
/// Read one book directory directly, outside any root.
///
/// `None` unless `dir/book.toml` is a file. The book is read exactly as a root
/// entry would be (title fallback, `build-dir`, cover, `src`), with
/// `root_index` 0 and `ambiguous` false — there is no root to be ambiguous in.
pub fn read_book(dir: &Path) -> Option<Book> {
    dir.join("book.toml")
        .is_file()
        .then(|| read_book_in_root(dir, 0))
}
```

(d) The three methods, added to `impl Book` after `is_built`:
```rust
    /// The source directory: `dir` joined with `[book] src`, default `src`.
    pub fn src_dir(&self) -> PathBuf {
        self.src.clone()
    }

    /// The newest modification time among the build inputs: `book.toml`,
    /// every file under [`Book::src_dir`], and every file under `dir/theme`
    /// if it exists.
    ///
    /// Never descends into `build_dir`, so a build-dir placed inside `src`
    /// cannot make a book its own newest input. An unreadable input is skipped
    /// at debug level rather than failing: a permission problem should not
    /// turn a whole library into "stale". `None` when nothing was readable.
    pub fn newest_input(&self) -> Option<std::time::SystemTime> {
        let mut newest: Option<std::time::SystemTime> = None;
        let mut consider = |path: &Path| match std::fs::metadata(path).and_then(|m| m.modified()) {
            Ok(m) => newest = Some(newest.map_or(m, |n| n.max(m))),
            Err(e) => tracing::debug!(path = %path.display(), error = %e, "input unreadable; ignored"),
        };
        consider(&self.dir.join("book.toml"));
        for top in [self.src_dir(), self.dir.join("theme")] {
            if top.is_dir() {
                walk_files(&top, &self.build_dir, &mut consider);
            }
        }
        newest
    }

    /// Whether a build is needed: not built, or an input is newer than the
    /// rendered `index.html`. Equal times are up to date, and a built book
    /// with no readable inputs is up to date.
    pub fn is_stale(&self) -> bool {
        if !self.is_built() {
            return true;
        }
        let built_at = match std::fs::metadata(self.build_dir.join("index.html")).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => return true,
        };
        match self.newest_input() {
            Some(input) => input > built_at,
            None => false,
        }
    }
```
and a private free function next to `read_meta`:
```rust
/// Call `f` on every file under `dir`, recursively, never entering `skip`.
fn walk_files(dir: &Path, skip: &Path, f: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        tracing::debug!(dir = %dir.display(), "directory unreadable; ignored");
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == skip {
            continue;
        }
        if path.is_dir() {
            walk_files(&path, skip, f);
        } else {
            f(&path);
        }
    }
}
```

(e) The crate-level doc comment (top of `lib.rs`) has a short list of what the crate does; add one line: "and whether a book is stale relative to its sources ([`Book::is_stale`]), which is what `md-librarian build` asks."

- [ ] **Step 4: Run the tests**

Run: `cd ~/src/md-librarian && cargo test -p md-librarian 2>&1 | grep "test result"`
Expected: `test result: ok. 22 passed; 0 failed` (14 existing + 8 new). Then `cargo test --workspace 2>&1 | grep "test result"` — every line `0 failed` (the serve crate constructs no `Book` literals, so it still compiles).

- [ ] **Step 5: Commit**

```bash
cd ~/src/md-librarian && cargo fmt --all && git add crates/md-librarian/src/lib.rs && git commit -m "feat(discovery): Book::{src_dir,newest_input,is_stale} and read_book

Staleness is a pure filesystem question, so it lives beside is_built:
inputs are book.toml, [book] src (default src) and theme/, never the
build dir. read_book(dir) reads one book outside any root, for the build
subcommand's positional arguments.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```

---

### Task 2: The `build` subcommand — selection, mdbook, summary

**Files:**
- Rewrite: `crates/md-librarian-cli/src/main.rs`
- Create: `crates/md-librarian-cli/src/serve.rs`, `crates/md-librarian-cli/src/build.rs`
- Create: `crates/md-librarian-cli/tests/build.rs`
- Modify: `crates/md-librarian-cli/Cargo.toml` (add `[dev-dependencies] tempfile = "3"`)

**Interfaces:**
- Consumes: `md_librarian::{read_book, roots, library, Book, Entry}`, `Book::is_stale`.
- Produces: `build::BuildArgs { dirs, root, include, force, into }`, `build::select(&BuildArgs) -> Vec<Book>`, `build::run(BuildArgs) -> anyhow::Result<i32>`, `build::which_mdbook() -> anyhow::Result<PathBuf>`, `build::mdbook_build(&Path, &Book) -> anyhow::Result<()>`. Task 3 adds `install` and wires `into`; in this task `into` is parsed but unused (a `let _ = args.into;` keeps clippy quiet and the flag visible in `--help`).

- [ ] **Step 1: Write the failing end-to-end tests**

`crates/md-librarian-cli/tests/build.rs`:
```rust
//! End-to-end tests for `md-librarian build`, driving the real binary.
//!
//! The ones that need `mdbook` skip with a message when it is not on PATH;
//! CI installs it so they run there.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_md-librarian"))
}

fn mdbook_on_path() -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join("mdbook").is_file()))
        .unwrap_or(false)
}

/// A minimal buildable book at `root/<name>`.
fn fixture(root: &Path, name: &str, title: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("book.toml"), format!("[book]\ntitle = \"{title}\"\n")).unwrap();
    std::fs::write(dir.join("src/SUMMARY.md"), "# Summary\n\n- [Intro](intro.md)\n").unwrap();
    std::fs::write(dir.join("src/intro.md"), "# Intro\n\nhello\n").unwrap();
    dir
}

#[test]
fn help_lists_the_build_subcommand_and_the_serve_flags_still_exist() {
    let out = bin().arg("--help").output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("build"), "{text}");
    assert!(text.contains("--no-window"), "{text}");
    let out = bin().args(["build", "--help"]).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    for flag in ["--root", "--include", "--force", "--into", "[DIR]"] {
        assert!(text.contains(flag), "missing {flag} in:\n{text}");
    }
}

#[test]
fn build_fails_up_front_without_mdbook() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), "guide", "Guide");
    let out = bin()
        .args(["build", "--root"])
        .arg(tmp.path())
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("cargo install mdbook"), "{err}");
    assert!(!tmp.path().join("guide/book/index.html").exists(), "nothing must be built");
}

#[test]
fn build_renders_stale_books_and_then_reports_up_to_date() {
    if !mdbook_on_path() {
        eprintln!("skipping: mdbook not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let guide = fixture(tmp.path(), "guide", "Guide");

    let out = bin().args(["build", "--root"]).arg(tmp.path()).output().unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "{err}");
    assert!(guide.join("book/index.html").is_file(), "{err}");
    assert!(err.contains("built 1, up to date 0, failed 0"), "{err}");

    let out = bin().args(["build", "--root"]).arg(tmp.path()).output().unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "{err}");
    assert!(err.contains("built 0, up to date 1, failed 0"), "{err}");

    let out = bin().args(["build", "--force", "--root"]).arg(tmp.path()).output().unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("built 1, up to date 0, failed 0"), "{err}");
}

#[test]
fn build_takes_book_directories_directly_and_reports_a_non_book() {
    if !mdbook_on_path() {
        eprintln!("skipping: mdbook not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let guide = fixture(tmp.path(), "guide", "Guide");
    let plain = tmp.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();

    let out = bin().arg("build").arg(&guide).arg(&plain).output().unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "a non-book argument is an error line, not a failure: {err}");
    assert!(guide.join("book/index.html").is_file());
    assert!(err.contains("plain"), "the non-book path must be named: {err}");
    assert!(err.contains("built 1, up to date 0, failed 0"), "{err}");
}

#[test]
fn a_failing_book_sets_the_exit_code_but_the_others_still_build() {
    if !mdbook_on_path() {
        eprintln!("skipping: mdbook not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let good = fixture(tmp.path(), "good", "Good");
    // A book whose SUMMARY.md points at a chapter that is a directory, which
    // mdbook refuses to read.
    let bad = fixture(tmp.path(), "bad", "Bad");
    std::fs::remove_file(bad.join("src/intro.md")).unwrap();
    std::fs::create_dir_all(bad.join("src/intro.md")).unwrap();

    let out = bin().args(["build", "--root"]).arg(tmp.path()).output().unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(good.join("book/index.html").is_file(), "{err}");
    assert!(err.contains("built 1, up to date 0, failed 1"), "{err}");
}
```

Add to `crates/md-librarian-cli/Cargo.toml` after `[dependencies]`:
```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Run to see them fail**

Run: `cd ~/src/md-librarian && cargo test -p md-librarian-cli --test build 2>&1 | grep -E "test |panicked|test result" | head -20`
Expected: the help test fails (no `build` in `--help`), the without-mdbook test fails (`build` is an unknown argument so the exit is non-zero but stderr lacks `cargo install mdbook`), and the mdbook-dependent tests fail on the missing `index.html`.

- [ ] **Step 3: Move the serve path into `serve.rs`**

Create `crates/md-librarian-cli/src/serve.rs` containing, in this order:
1. A module doc: `//! The default path: serve the library and open a window on it.`
2. `use std::path::PathBuf; use clap::Args;`
3. The existing six fields from the old `Cli` struct, verbatim with their doc comments, in
   ```rust
   #[derive(Args, Debug)]
   pub struct ServeArgs { /* root, include, book, exit_on_stdin_close, parent_pipe, no_window */ }
   ```
4. `pub fn run(cli: ServeArgs) -> anyhow::Result<()>` whose body is everything the old `main` did AFTER tracing init, verbatim: the two watcher `if`s, roots/include/server/url, the `no_window` branch, the `WebWindow::open` match, `Ok(())`.
5. The existing `watch_eof` and `park` functions, verbatim, made `fn` (private to this module).

- [ ] **Step 4: Write `build.rs`**

`crates/md-librarian-cli/src/build.rs`:
```rust
//! `md-librarian build`: bring every stale book up to date with `mdbook`.
//!
//! Selection reuses discovery, so "build what I would see" holds: the same
//! roots, the same first-root-wins shadowing, the same `--include` filter.
//! `mdbook` is the user's own, found on PATH and run as a subprocess; this
//! binary never links it.

use std::path::{Path, PathBuf};

use clap::Args;
use md_librarian::{Book, Entry};

#[derive(Args, Debug)]
pub struct BuildArgs {
    /// Book directories (each holding a book.toml) to build directly. With
    /// none given, every book the library would show is built.
    #[arg(value_name = "DIR")]
    pub dirs: Vec<PathBuf>,

    /// A repository root; repeatable. Overrides MD_LIBRARIAN_PATH. Earlier
    /// wins. Ignored when DIR is given.
    #[arg(long, value_name = "DIR")]
    pub root: Vec<PathBuf>,

    /// Only these book titles; repeatable. A title no root provides is a
    /// warning, not a failure.
    #[arg(long, value_name = "TITLE")]
    pub include: Vec<String>,

    /// Rebuild every selected book, stale or not.
    #[arg(long)]
    pub force: bool,

    /// After building, install a slim copy of each book (book.toml, cover,
    /// rendered output) at ROOT/<dir-name>/.
    #[arg(long, value_name = "ROOT")]
    pub into: Option<PathBuf>,
}

/// The books to build, in library order.
///
/// With `dirs`, each must hold a `book.toml`; one that does not is logged and
/// skipped. Without, this is exactly what discovery would show.
pub fn select(args: &BuildArgs) -> Vec<Book> {
    if !args.dirs.is_empty() {
        return args
            .dirs
            .iter()
            .filter_map(|dir| {
                let found = md_librarian::read_book(dir);
                if found.is_none() {
                    tracing::error!(path = %dir.display(), "not a book: no book.toml here");
                }
                found
            })
            .collect();
    }
    let roots = md_librarian::roots(&args.root);
    let include = (!args.include.is_empty()).then_some(args.include.as_slice());
    md_librarian::library(&roots, include)
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Book(b) => Some(b),
            Entry::Missing { title } => {
                tracing::warn!(title, "no root provides this book; nothing to build");
                None
            }
        })
        .collect()
}

/// Run the subcommand. Returns the process exit code: 1 if any book failed to
/// build, else 0.
pub fn run(args: BuildArgs) -> anyhow::Result<i32> {
    let mdbook = which_mdbook()?;
    let books = select(&args);
    let _ = &args.into; // wired in the install step
    let (mut built, mut fresh, mut failed) = (0u32, 0u32, 0u32);
    for book in &books {
        if !args.force && !book.is_stale() {
            tracing::info!(title = %book.title, "up to date");
            fresh += 1;
            continue;
        }
        match mdbook_build(&mdbook, book) {
            Ok(()) => {
                tracing::info!(title = %book.title, dir = %book.dir.display(), "built");
                built += 1;
            }
            Err(e) => {
                tracing::error!(title = %book.title, dir = %book.dir.display(), error = %e, "build failed");
                failed += 1;
            }
        }
    }
    eprintln!("built {built}, up to date {fresh}, failed {failed}");
    Ok(if failed > 0 { 1 } else { 0 })
}

/// `mdbook` on PATH, or an error naming how to install it.
pub fn which_mdbook() -> anyhow::Result<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join("mdbook"))
        .find(|p| p.is_file())
        .ok_or_else(|| anyhow::anyhow!("`mdbook` not found on PATH — install it with `cargo install mdbook`"))
}

/// `mdbook build <dir>`, with mdbook's own output passed straight through.
/// mdbook resolves `build-dir` relative to the book root it is given, so the
/// current directory is left alone.
pub fn mdbook_build(mdbook: &Path, book: &Book) -> anyhow::Result<()> {
    let status = std::process::Command::new(mdbook)
        .arg("build")
        .arg(&book.dir)
        .status()?;
    anyhow::ensure!(status.success(), "mdbook exited with {status}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(root: &Path, name: &str, body: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("book.toml"), body).unwrap();
        dir
    }

    fn args() -> BuildArgs {
        BuildArgs { dirs: vec![], root: vec![], include: vec![], force: false, into: None }
    }

    #[test]
    fn select_from_roots_uses_discovery_order_and_include() {
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "z", "[book]\ntitle = \"Alpha\"\n");
        book(tmp.path(), "a", "[book]\ntitle = \"Beta\"\n");
        let mut a = args();
        a.root = vec![tmp.path().to_path_buf()];
        let titles: Vec<String> = select(&a).into_iter().map(|b| b.title).collect();
        assert_eq!(titles, vec!["Alpha", "Beta"]);
        a.include = vec!["Beta".into(), "Nope".into()];
        let titles: Vec<String> = select(&a).into_iter().map(|b| b.title).collect();
        assert_eq!(titles, vec!["Beta"], "Missing entries are warned about, not built");
    }

    #[test]
    fn select_from_dirs_skips_non_books_and_ignores_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let guide = book(tmp.path(), "guide", "[book]\ntitle = \"Guide\"\n");
        let plain = tmp.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let other_root = tempfile::tempdir().unwrap();
        book(other_root.path(), "elsewhere", "[book]\ntitle = \"Elsewhere\"\n");
        let mut a = args();
        a.dirs = vec![guide.clone(), plain];
        a.root = vec![other_root.path().to_path_buf()];
        let got = select(&a);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].dir, guide);
    }

    #[test]
    fn which_mdbook_reports_how_to_install() {
        let saved = std::env::var_os("PATH");
        // SAFETY: tests in this module run single-threaded with respect to PATH
        // (this is the only test touching it), and it is restored below.
        unsafe { std::env::set_var("PATH", "") };
        let err = which_mdbook().unwrap_err().to_string();
        unsafe {
            match saved {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
        }
        assert!(err.contains("cargo install mdbook"), "{err}");
    }
}
```

- [ ] **Step 5: Rewrite `main.rs`**

```rust
//! `md-librarian` — the standalone book library viewer, and its builder.
//!
//! Extracted from gpui-yaams (`yaams-books`) at v0.27.0-beta.3; the serve
//! path is unchanged apart from the names. The `build` subcommand is new here.
//!
//! ```text
//! md-librarian                                  # MD_LIBRARIAN_PATH, else the XDG default
//! md-librarian --root ~/books --root /opt/books # explicit roots, first wins
//! md-librarian --include gpui --include usdlite # only these titles
//! md-librarian --book gpui                      # open straight onto one book
//! md-librarian --no-window                      # serve only; prints the URL
//! md-librarian build                            # mdbook-build every stale book on the roots
//! md-librarian build ~/src/foo/docs --into ~/books   # build one book, install a copy
//! ```
//!
//! The serve path and its lifetime contract (`--exit-on-stdin-close`,
//! `--parent-pipe`) are in [`serve`]; building is in [`build`].

mod build;
mod serve;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    about = "Browse a library of mdbooks",
    long_about = "Serves every book found on the search path and opens a window on the library.\n\n\
                  Roots come from --root, else MD_LIBRARIAN_PATH (a stacking, colon-separated \
                  list), else $XDG_DATA_HOME/md-librarian/books. Earlier roots shadow later ones.\n\n\
                  `md-librarian build` brings the books on those roots up to date with mdbook."
)]
struct Cli {
    #[command(flatten)]
    serve: serve::ServeArgs,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run `mdbook build` on every stale book on the search path, or on the
    /// given book directories; optionally install slim copies into a root.
    Build(build::BuildArgs),
}

fn main() -> anyhow::Result<()> {
    // Logs go to STDERR so that `--no-window`'s stdout is exactly the URL and
    // nothing else — it is meant to be read by a script.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "md_librarian=info,md_librarian_serve=info,md_librarian_webview=info".into()
            }),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Some(Command::Build(args)) => std::process::exit(build::run(args)?),
        None => serve::run(cli.serve),
    }
}
```

Keep the old crate doc's "# Lifetime" section: move it to the top of `serve.rs`'s module doc, verbatim, so the stdin-pipe contract is still documented where the code is.

- [ ] **Step 6: Run everything**

Run: `cd ~/src/md-librarian && cargo build --workspace 2>&1 | grep -E "warning|error" ; cargo test --workspace 2>&1 | grep "test result"`
Expected: no warnings; the CLI's unit suite `3 passed`, the CLI's `build` integration suite `5 passed` (mdbook is installed on this machine), discovery `22 passed`, serve `14 passed`, doctest `1 passed`, everything `0 failed`.

Then the unchanged-behaviour check:
```bash
timeout 5 cargo run -q --bin md-librarian -- --root /nonexistent --no-window | head -1
```
Expected: exactly one `http://127.0.0.1:` line on stdout.

- [ ] **Step 7: Commit**

```bash
cd ~/src/md-librarian && cargo fmt --all && git add crates/md-librarian-cli && git commit -m "feat(cli): md-librarian build

A clap subcommand beside the unchanged serve path. Selection reuses
discovery (same roots, shadowing and --include), stale books are rebuilt
with the user's own mdbook found on PATH, failures are logged and counted,
and the run ends with one summary line and exit 1 if anything failed.
main.rs splits into main / serve / build.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```

---

### Task 3: `--into ROOT` — the slim install copy

**Files:**
- Modify: `crates/md-librarian-cli/src/build.rs` (add `Installed`, `install`, `copy_tree`, `same_dir`; wire into `run`; tests)
- Modify: `crates/md-librarian-cli/tests/build.rs` (one more end-to-end test)

**Interfaces:**
- Consumes: `Book { dir, dir_name, build_dir, cover }`.
- Produces: `pub enum Installed { Copied, UpToDate, SameDir, Refused(String) }`, `pub fn install(book: &Book, root: &Path) -> anyhow::Result<Installed>`. `Err` means an I/O failure (counts toward exit 1); `Refused` is a logged policy refusal (does not).

- [ ] **Step 1: Write the failing unit tests**

Append inside `mod tests` in `build.rs`:
```rust
    fn built(dir: &Path, sub: &str, index: &str) {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        std::fs::write(dir.join(sub).join("index.html"), index).unwrap();
        std::fs::write(dir.join(sub).join("style.css"), "b{}").unwrap();
    }

    #[test]
    fn install_copies_toml_cover_and_output_at_the_relative_build_dir() {
        let src = tempfile::tempdir().unwrap();
        let dir = book(src.path(), "guide", "[book]\ntitle = \"Guide\"\n\n[build]\nbuild-dir = \"html\"\n");
        std::fs::write(dir.join("cover.png"), b"png").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/intro.md"), "# hi").unwrap();
        built(&dir, "html", "<h1>v1</h1>");
        let b = md_librarian::read_book(&dir).unwrap();
        let root = tempfile::tempdir().unwrap();

        assert_eq!(install(&b, root.path()).unwrap(), Installed::Copied);
        let dest = root.path().join("guide");
        assert_eq!(std::fs::read_to_string(dest.join("book.toml")).unwrap(), std::fs::read_to_string(dir.join("book.toml")).unwrap());
        assert_eq!(std::fs::read(dest.join("cover.png")).unwrap(), b"png");
        assert_eq!(std::fs::read_to_string(dest.join("html/index.html")).unwrap(), "<h1>v1</h1>");
        assert!(dest.join("html/style.css").is_file());
        assert!(!dest.join("src").exists(), "sources are not installed");
        // The installed copy is itself a discoverable, built book.
        let installed = md_librarian::read_book(&dest).unwrap();
        assert!(installed.is_built());
        assert_eq!(installed.cover, Some(dest.join("cover.png")));
    }

    #[test]
    fn install_is_a_replace_and_then_up_to_date() {
        let src = tempfile::tempdir().unwrap();
        let dir = book(src.path(), "g", "[book]\ntitle = \"G\"\n");
        built(&dir, "book", "<h1>v1</h1>");
        std::fs::write(dir.join("book/old-chapter.html"), "gone soon").unwrap();
        let b = md_librarian::read_book(&dir).unwrap();
        let root = tempfile::tempdir().unwrap();
        assert_eq!(install(&b, root.path()).unwrap(), Installed::Copied);
        assert!(root.path().join("g/book/old-chapter.html").is_file());

        // Nothing changed at the source: no copy.
        assert_eq!(install(&b, root.path()).unwrap(), Installed::UpToDate);

        // Rebuild without the old chapter, newer than the install.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::remove_file(dir.join("book/old-chapter.html")).unwrap();
        std::fs::write(dir.join("book/index.html"), "<h1>v2</h1>").unwrap();
        assert_eq!(install(&b, root.path()).unwrap(), Installed::Copied);
        assert!(!root.path().join("g/book/old-chapter.html").exists(), "replace, not merge");
        assert_eq!(std::fs::read_to_string(root.path().join("g/book/index.html")).unwrap(), "<h1>v2</h1>");
    }

    #[test]
    fn install_refuses_a_destination_that_is_not_a_book() {
        let src = tempfile::tempdir().unwrap();
        let dir = book(src.path(), "g", "[book]\ntitle = \"G\"\n");
        built(&dir, "book", "<h1/>");
        let b = md_librarian::read_book(&dir).unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("g/precious")).unwrap();
        match install(&b, root.path()).unwrap() {
            Installed::Refused(why) => assert!(why.contains("book.toml"), "{why}"),
            other => panic!("expected Refused, got {other:?}"),
        }
        assert!(root.path().join("g/precious").is_dir(), "nothing was deleted");
    }

    #[test]
    fn install_refuses_a_build_dir_outside_the_book() {
        let src = tempfile::tempdir().unwrap();
        let dir = book(src.path(), "g", "[book]\ntitle = \"G\"\n\n[build]\nbuild-dir = \"../out\"\n");
        built(src.path(), "out", "<h1/>");
        let b = md_librarian::read_book(&dir).unwrap();
        assert!(b.is_built(), "the escaping build-dir is honoured for serving");
        let root = tempfile::tempdir().unwrap();
        match install(&b, root.path()).unwrap() {
            Installed::Refused(why) => assert!(why.contains("build-dir"), "{why}"),
            other => panic!("expected Refused, got {other:?}"),
        }
        assert!(!root.path().join("g").exists());
    }

    #[test]
    fn install_into_the_books_own_root_is_a_no_op() {
        let root = tempfile::tempdir().unwrap();
        let dir = book(root.path(), "g", "[book]\ntitle = \"G\"\n");
        built(&dir, "book", "<h1/>");
        let b = md_librarian::read_book(&dir).unwrap();
        assert_eq!(install(&b, root.path()).unwrap(), Installed::SameDir);
        assert!(dir.join("book/index.html").is_file(), "untouched");
    }
```

And append to `tests/build.rs`:
```rust
#[test]
fn build_into_installs_a_slim_copy() {
    if !mdbook_on_path() {
        eprintln!("skipping: mdbook not on PATH");
        return;
    }
    let src = tempfile::tempdir().unwrap();
    let guide = fixture(src.path(), "guide", "Guide");
    std::fs::write(guide.join("cover.svg"), "<svg/>").unwrap();
    let lib = tempfile::tempdir().unwrap();
    let into = lib.path().join("books");

    let out = bin().arg("build").arg(&guide).arg("--into").arg(&into).output().unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "{err}");
    let dest = into.join("guide");
    assert!(dest.join("book.toml").is_file(), "{err}");
    assert!(dest.join("cover.svg").is_file());
    assert!(dest.join("book/index.html").is_file());
    assert!(!dest.join("src").exists());
    assert!(err.contains("installed"), "{err}");

    // Second run: built book is fresh, install is fresh, nothing copied.
    let out = bin().arg("build").arg(&guide).arg("--into").arg(&into).output().unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("built 0, up to date 1, failed 0"), "{err}");
    assert!(!err.contains("installed"), "{err}");
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cd ~/src/md-librarian && cargo test -p md-librarian-cli 2>&1 | grep -E "^error|cannot find|test result" | head`
Expected: compile errors: `cannot find function \`install\``, `cannot find type \`Installed\``.

- [ ] **Step 3: Implement `install`**

Add to `build.rs` above the tests module:
```rust
/// What [`install`] did for one book.
#[derive(Debug, PartialEq, Eq)]
pub enum Installed {
    /// A fresh copy was written.
    Copied,
    /// The destination's `index.html` is at least as new as the source's.
    UpToDate,
    /// The destination *is* the source directory; building in place was enough.
    SameDir,
    /// A policy refusal (not an I/O error): the reason, for the log. Does not
    /// affect the exit code — the build itself succeeded.
    Refused(String),
}

/// Install a slim copy of a built book at `root/<dir_name>/`: `book.toml`,
/// the cover if any, and the rendered output at the same relative
/// `build-dir` path, so the copied `book.toml` still points at it.
///
/// A replace, never a merge, so chapters removed from the source do not
/// linger. `Err` is an I/O failure; policy refusals come back as
/// [`Installed::Refused`] so the caller can log them without failing the run.
pub fn install(book: &Book, root: &Path) -> anyhow::Result<Installed> {
    let Ok(rel) = book.build_dir.strip_prefix(&book.dir) else {
        return Ok(Installed::Refused(format!(
            "build-dir {} is not inside the book directory; the layout cannot be preserved",
            book.build_dir.display()
        )));
    };
    if rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Ok(Installed::Refused(format!(
            "build-dir {} escapes the book directory; the layout cannot be preserved",
            rel.display()
        )));
    }
    std::fs::create_dir_all(root)?;
    let dest = root.join(&book.dir_name);
    if same_dir(&dest, &book.dir) {
        return Ok(Installed::SameDir);
    }

    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let src_index = book.build_dir.join("index.html");
    if let (Some(s), Some(d)) = (mtime(&src_index), mtime(&dest.join(rel).join("index.html"))) {
        if d >= s {
            return Ok(Installed::UpToDate);
        }
    }

    if dest.exists() {
        if !dest.join("book.toml").is_file() {
            return Ok(Installed::Refused(format!(
                "{} exists but holds no book.toml; not replacing something this tool did not make",
                dest.display()
            )));
        }
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::create_dir_all(&dest)?;
    std::fs::copy(book.dir.join("book.toml"), dest.join("book.toml"))?;
    if let Some(cover) = &book.cover {
        if let Some(name) = cover.file_name() {
            std::fs::copy(cover, dest.join(name))?;
        }
    }
    copy_tree(&book.build_dir, &dest.join(rel))?;
    Ok(Installed::Copied)
}

/// Whether two paths name the same existing directory (`dest` may not exist).
fn same_dir(dest: &Path, src: &Path) -> bool {
    match (dest.canonicalize(), src.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Recursive copy of a directory tree. Symlinks are followed (copied as files).
fn copy_tree(from: &Path, to: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
```

Then wire it into `run`. Replace the `let _ = &args.into;` line and the `continue` in the up-to-date branch so both the built and the fresh book reach the install step:
```rust
    for book in &books {
        if !args.force && !book.is_stale() {
            tracing::info!(title = %book.title, "up to date");
            fresh += 1;
        } else {
            match mdbook_build(&mdbook, book) {
                Ok(()) => {
                    tracing::info!(title = %book.title, dir = %book.dir.display(), "built");
                    built += 1;
                }
                Err(e) => {
                    tracing::error!(title = %book.title, dir = %book.dir.display(), error = %e, "build failed");
                    failed += 1;
                    continue;
                }
            }
        }
        if let Some(root) = &args.into {
            match install(book, root) {
                Ok(Installed::Copied) => tracing::info!(title = %book.title, root = %root.display(), "installed"),
                Ok(Installed::UpToDate) => tracing::debug!(title = %book.title, "install up to date"),
                Ok(Installed::SameDir) => tracing::debug!(title = %book.title, "already in that root"),
                Ok(Installed::Refused(why)) => tracing::error!(title = %book.title, "not installed: {why}"),
                Err(e) => {
                    tracing::error!(title = %book.title, error = %e, "install failed");
                    failed += 1;
                }
            }
        }
    }
```
and update `run`'s doc comment: "Returns the process exit code: 1 if any book failed to build or an install hit an I/O error, else 0."

- [ ] **Step 4: Run the tests**

Run: `cd ~/src/md-librarian && cargo test -p md-librarian-cli 2>&1 | grep "test result"`
Expected: unit suite `8 passed`, integration suite `6 passed`, `0 failed`. Then `cargo test --workspace 2>&1 | grep -c "0 failed"` prints the number of suites (8) and no line says otherwise.

- [ ] **Step 5: Commit**

```bash
cd ~/src/md-librarian && cargo fmt --all && git add crates/md-librarian-cli && git commit -m "feat(cli): build --into ROOT installs a slim copy of each book

book.toml, the cover and the rendered output at its relative build-dir
path; a replace, only when the source output is newer; refuses an
escaping build-dir or a destination that is not a book, and skips a
book that already lives in that root.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```

---

### Task 4: CI and documentation

**Files:**
- Modify: `.github/workflows/ci.yml` (test job)
- Modify: `book/src/library.md` (new section after "Where books are found", i.e. before `## Identity is the title`)
- Modify: `book/src/getting-started.md` ("Where books go")
- Modify: `README.md` (Quick start, Command line block and table)
- Modify: `CHANGELOG.md` (`[Unreleased]` → Added)
- Modify: `CLAUDE.md` (Gotchas)

**Interfaces:** consumes the command line from Tasks 2–3 exactly as implemented (`build [DIR]... --root --include --force --into`).

- [ ] **Step 1: CI installs mdbook for the test job**

In `.github/workflows/ci.yml`, in the `test` job, insert after the `Swatinem/rust-cache@v2` step and before `fmt`:
```yaml
      # `md-librarian build` shells out to mdbook; its end-to-end tests skip
      # without it, and CI must not skip them.
      - uses: peaceiris/actions-mdbook@v2
        with:
          mdbook-version: latest
```

- [ ] **Step 2: Library chapter**

Insert into `book/src/library.md` immediately before the line `## Identity is the title`:
````markdown
## Building the library

`md-librarian build` runs `mdbook build` on every book the library would show
that is **stale**: not built, or with `book.toml`, anything under its source
directory (`[book] src`, default `src`), or anything under `theme/` newer than
the rendered `index.html`. The build directory itself is never an input, so a
`build-dir` inside `src` cannot make a book its own newest change. Equal
times count as up to date, so a second run does nothing and is cheap enough
for a rez build or a shell hook.

```sh
md-librarian build                            # every stale book on the roots
md-librarian build --root ~/books --force     # explicit roots; rebuild all
md-librarian build --include "User Guide"     # only these titles
md-librarian build ~/src/foo/docs             # one book, wherever it lives
```

Selection is discovery's: the same roots, the same first-root-wins shadowing,
the same `--include` filter. A shadowed copy in a later root is not built.

`mdbook` is **yours**: the one on `PATH`, with its version and its
preprocessors. The viewer never links mdbook. Without one, `build` stops
before touching anything and says `cargo install mdbook`.

A book that fails to build is logged and the run continues; the summary line
`built N, up to date M, failed K` ends every run, and the exit code is `1`
when `K` is not zero.

### Installing into a root

```sh
md-librarian build ~/src/foo/docs ~/src/bar/docs --into ~/books
```

`--into` copies each built book into `ROOT/<directory name>/` as a **slim
copy**: `book.toml`, the cover if there is one, and the rendered output at the
same relative `build-dir` path, so the copied `book.toml` still points at it.
No sources. The copy happens only when the source's `index.html` is newer
than the destination's, and it is a replace, so chapters deleted at the
source do not linger.

Three things it refuses, each logged for that book while the run continues:

- a `build-dir` that is absolute or climbs out of the book directory, because
  the relative layout cannot be reproduced;
- a destination that exists without a `book.toml`, because it is not something
  this tool made and will not be deleted;
- a destination that *is* the source, which is skipped — building in place
  already did the work.

````

- [ ] **Step 3: Getting started**

In `book/src/getting-started.md`, replace the code block under "Where books go":
```sh
mdbook build ~/src/my-book                      # renders into ~/src/my-book/book
MD_LIBRARIAN_PATH=~/src md-librarian            # ~/src is the root; my-book is found
```
with:
```sh
mdbook build ~/src/my-book                      # renders into ~/src/my-book/book
MD_LIBRARIAN_PATH=~/src md-librarian            # ~/src is the root; my-book is found

md-librarian build ~/src/my-book --into ~/books # or: build it and install a slim copy
MD_LIBRARIAN_PATH=~/books md-librarian          # into a root of your own
```
and add one sentence after the block: "`md-librarian build` with no arguments brings every stale book on the roots up to date; see [Building the library](./library.md#building-the-library)."

- [ ] **Step 4: README**

(a) In the Quick start code block, add a third way after the `cp -r` line:
```sh
md-librarian build ~/src/my-book --into ~/books   # or let it build and install the book
```
(b) In the Command line block, append:
```sh
md-librarian build                            # mdbook-build every stale book on the roots
md-librarian build ~/src/foo/docs --into ~/books   # build one book, install a slim copy
```
(c) After the flag table, add:
```markdown
`build` takes optional book directories, the same `--root` and `--include`,
plus `--force` (rebuild everything) and `--into <ROOT>` (install a slim copy
of each book: `book.toml`, cover, rendered output). It needs `mdbook` on
`PATH`. The [library chapter](book/src/library.md#building-the-library) has
the staleness rule and what `--into` refuses.
```

- [ ] **Step 5: CHANGELOG and CLAUDE.md**

`CHANGELOG.md`, under `## [Unreleased]` → `### Added`, before the existing "Extracted from gpui-yaams" bullet:
```markdown
- **`md-librarian build`.** Runs `mdbook build` on every stale book on the
  search path, or on the book directories given; `--force` rebuilds all,
  `--into <ROOT>` installs a slim copy (`book.toml`, cover, rendered output)
  into a root. Discovery gains `Book::{src_dir, newest_input, is_stale}` and
  `read_book(dir)`. mdbook is found on `PATH`, never linked.
```

`CLAUDE.md`, Gotchas, add a bullet after the "Only … may link wry/tao/GTK" one:
```markdown
- **Only `md-librarian-cli` spawns processes.** `md-librarian build` shells
  out to the user's `mdbook`; discovery answers "is this stale?" as a pure
  filesystem question and must stay that way, so an application can ask it
  without side effects.
```

- [ ] **Step 6: Build the book and check the links**

Run: `cd ~/src/md-librarian && (cd book && mdbook build 2>&1 | grep -i "warn\|error"; true) && grep -n "building-the-library" README.md book/src/getting-started.md && grep -c "^## Building the library" book/src/library.md`
Expected: no warnings; both grep hits; `1`.

- [ ] **Step 7: Commit**

```bash
cd ~/src/md-librarian && git add .github book README.md CHANGELOG.md CLAUDE.md && git commit -m "docs: md-librarian build in the book, README and changelog; CI installs mdbook

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ"
```

---

### Task 5: Final verification and the PR

**Files:** none new.

- [ ] **Step 1: Everything green**

```bash
cd ~/src/md-librarian && cargo fmt --all --check && cargo build --workspace 2>&1 | grep -E "warning|error"; cargo test --workspace --locked 2>&1 | grep "test result" && cargo build --workspace --examples --locked 2>&1 | tail -1 && cargo doc --workspace --no-deps 2>&1 | grep -E "warning|error"; (cd book && mdbook build 2>&1 | tail -1) && git status --short
```
Expected: fmt clean; no build warnings; suites: discovery 22, serve 14, cli unit 8, cli integration 6, doctest 1, everything `0 failed`; no rustdoc warnings; tree clean.

- [ ] **Step 2: Dogfood on this repo**

```bash
cd ~/src/md-librarian && rm -rf book/html && cargo run -q --bin md-librarian -- build --root . 2>&1 | tail -3 && test -f book/html/index.html && cargo run -q --bin md-librarian -- build --root . 2>&1 | tail -1
```
Expected: first run ends `built 1, up to date 0, failed 0` and `book/html/index.html` exists; second run ends `built 0, up to date 1, failed 0`.

- [ ] **Step 3: Push and open the PR**

```bash
cd ~/src/md-librarian && git push -u origin feat/build-subcommand && gh pr create --title "md-librarian build: bring stale books up to date, optionally install into a root" --body "$(cat <<'EOF'
Adds a `build` subcommand: runs the user's `mdbook` (found on PATH, never linked) on every stale book the library would show, or on given book directories; `--force` rebuilds all; `--into <ROOT>` installs a slim copy (book.toml, cover, rendered output at its relative build-dir) into a root, replacing only when the source output is newer. The bare `md-librarian` command and its flags are unchanged.

Discovery gains `Book::{src_dir, newest_input, is_stale}` and `read_book(dir)`, pure filesystem, unit-tested with arranged mtimes. The CLI splits into main / serve / build. End-to-end tests drive the binary against fixture books; CI's test job now installs mdbook so they run there.

Spec: docs/superpowers/specs/2026-09-05-build-subcommand-design.md
Plan: docs/superpowers/plans/2026-09-05-build-subcommand.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)

https://claude.ai/code/session_01V3h4Te7SH9Z8g7UcibQ3uQ
EOF
)"
```

---

## Self-review against the spec

- Command line, optional subcommand, own `--root`/`--include`, positional `DIR...`, `--force`, `--into`: Task 2 (`BuildArgs`, `Cli`). ✔
- Selection without DIRs = `library()` with warnings for `Missing`; with DIRs = `read_book` and an error per non-book: Task 2 `select` + tests. ✔
- Staleness rule incl. `theme/`, never `build_dir`, equal = fresh, `None` on built = fresh: Task 1 methods + 8 tests. ✔
- mdbook on PATH, fatal up front with `cargo install mdbook`; inherited output; failure continues; summary line; exit 1: Task 2 `run`, e2e tests. ✔
- `--into`: exact contents, relative build-dir preserved, copy-only-when-newer, replace, three refusals, `ROOT` created, I/O errors count toward exit 1, refusals do not: Task 3 `install` + `Installed` + 5 unit tests + e2e. ✔
- Code layout main/serve/build: Task 2. ✔
- Tests list in spec: Task 1 (discovery), Task 3 (install unit), Task 2+3 (e2e, mdbook-gated), Task 4 (CI installs mdbook). ✔
- Docs: library.md section, getting-started, README, CHANGELOG, CLAUDE.md: Task 4. ✔
- Type consistency: `select(&BuildArgs) -> Vec<Book>`, `run(BuildArgs) -> Result<i32>`, `install(&Book, &Path) -> Result<Installed>`, `Installed::{Copied, UpToDate, SameDir, Refused(String)}`, `read_book(&Path) -> Option<Book>`, `Book::src_dir() -> PathBuf`, `newest_input() -> Option<SystemTime>`, `is_stale() -> bool` are used with the same names and shapes in every task. ✔
- Placeholder scan: none. Every step has its code or its exact command.
