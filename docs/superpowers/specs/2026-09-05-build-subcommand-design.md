# `md-librarian build`: building and installing a library of mdbooks

- **Date:** 2026-09-05
- **Status:** Approved (design) — pending implementation plan
- **Owner:** this repo (`github.com/jlgerber/md-librarian`)
- **Builds on:** [`2026-09-05-extract-from-gpui-yaams-design.md`](./2026-09-05-extract-from-gpui-yaams-design.md)

## Background

md-librarian discovers and serves mdbooks that are already built. It never
runs `mdbook`. That was deliberate: a viewer that only serves can honour a
book's `[build] build-dir` safely, which is what makes this repo's own book
(`build-dir = "html"`) discoverable.

The cost is that "building a library" is manual: run `mdbook build` in every
book, then arrange the source trees under a root. The rez package does that for
one book in `rez_build.sh`; usdlite does it in a `build.rs`. Every consumer
re-invents the loop.

This adds a `build` subcommand that owns the loop, in two modes: build the
books discovery already knows about, in place; and build a book from anywhere
and install a slim copy into a root.

## Goals

- `md-librarian build` brings every stale book on the search path up to date,
  and is cheap and idempotent when nothing changed, so it can run from a rez
  build, a shell hook, or a CI step.
- `md-librarian build <DIR>... --into <ROOT>` turns scattered book sources into
  a library root without copying their sources.
- The bare `md-librarian` command, its flags, and the discovery crate's public
  API shapes are unchanged. usdlite's launch line keeps working.
- `mdbook` stays the user's: whatever version and preprocessors are on PATH.

## Non-goals

- A dry-run or "what is stale" listing. The per-book log lines say what was
  built and what was skipped.
- Watching for changes, serving while building, or a `--build` flag on the
  serve path. Building is a separate invocation.
- Linking the `mdbook` crate. It pins one mdbook version, brings a large
  dependency tree, and would not load the user's preprocessors.
- Copying sources with `--into`. The installed copy is not rebuildable in
  place; rebuild from the source and install again.

## Decisions

| Question | Decision |
|---|---|
| Meaning of `build` | both: in place on the roots by default; `--into <ROOT>` installs a copy |
| When to rebuild | when stale (inputs newer than output) or unbuilt; `--force` rebuilds all |
| How mdbook runs | subprocess `mdbook build <dir>`, found on PATH |
| What `--into` copies | `book.toml`, the cover, the built output at its relative `build-dir` path |
| Where the logic lives | staleness in the discovery crate; subprocess and copy in the CLI |

## Command line

```text
md-librarian                                        # unchanged: serve + window
md-librarian build [DIR]... [--root DIR]... [--include TITLE]... [--force] [--into ROOT]
```

`build` is an optional clap subcommand. With no subcommand the existing serve
path runs with the existing flags. The subcommand carries its own `--root` and
`--include` so `md-librarian build --root x` reads naturally; they mean the
same as on the serve path.

| Argument | Meaning |
|---|---|
| `DIR...` | Book directories (each holds a `book.toml`) to build directly. With none given, the set is what discovery would show. |
| `--root <DIR>` | Repeatable; overrides `MD_LIBRARIAN_PATH`; earlier wins. Ignored when `DIR...` is given. |
| `--include <TITLE>` | Repeatable; only these titles. A title no root provides is a warning, not a failure. |
| `--force` | Rebuild every selected book, stale or not. |
| `--into <ROOT>` | After building, install a slim copy of each book at `ROOT/<dir_name>/`. |

**Selection without `DIR...`** is `md_librarian::library(&roots, include)`:
the same roots resolution, the same first-root-wins shadowing, the same filter.
A shadowed duplicate in a later root is not built; the book that would be shown
is. `Entry::Missing { title }` produces one `warn!` and is otherwise ignored.

**Selection with `DIR...`**: each path must be a directory holding
`book.toml`; otherwise that argument is an error naming the path, and the run
continues with the rest. Each is read with the same code discovery uses for a
root entry, so `title`, `build_dir`, `cover` and the new `src_dir` come out the
same way.

## Staleness (discovery crate)

`Meta` gains `[book] src` (mdbook's default is `src`). `Book` gains:

```rust
impl Book {
    /// `dir` joined with `[book] src`, default `src`.
    pub fn src_dir(&self) -> PathBuf;

    /// The newest modification time among the build inputs: `book.toml`,
    /// every file under `src_dir()`, and every file under `dir/theme` if it
    /// exists. Never descends into `build_dir`, so a build-dir placed inside
    /// `src` cannot make a book its own newest input. `None` when nothing
    /// readable exists.
    pub fn newest_input(&self) -> Option<SystemTime>;

    /// Not built, or `newest_input()` is newer than `build_dir/index.html`.
    pub fn is_stale(&self) -> bool;
}
```

`is_stale` compares against `index.html` because mdbook rewrites every output
file on each build, so that one file's mtime is the build time. Equal mtimes
count as up to date. An unreadable input is skipped with a `debug!`, not an
error: a permission problem should not turn a whole library into "stale".

The existing `is_built()` and every other public item are unchanged.

## Building (CLI)

Before touching any book, `build` resolves `mdbook` on PATH. If it is absent
the run fails immediately with one message naming `cargo install mdbook`.

For each selected book, in library order:

1. If `--force` is not set and `!book.is_stale()`, log `up to date` at info and
   skip to the install step.
2. Run `mdbook build <book.dir>` with stdout and stderr inherited, so mdbook's
   own output reaches the terminal unchanged, and the current directory left
   alone (mdbook resolves `build-dir` relative to the book root it is given).
3. A non-zero exit is logged at error with the title and continues to the next
   book. A book that failed to build is not installed.

The run ends with one summary line on stderr:
`built N, up to date M, failed K` and exits `1` if `K > 0`, else `0`. Logs stay
on stderr, as they do for the serve path.

## Installing with `--into ROOT` (CLI)

For each book that built successfully or was up to date, the destination is
`ROOT/<book.dir_name>/`. The copy contains exactly:

- `book.toml`, byte for byte;
- `cover.<ext>` if the book has one;
- the built output, at the same path relative to the book directory that
  `build_dir` has, so the copied `book.toml`'s `build-dir` still resolves.

The copy runs only when the destination's `build_dir/index.html` is missing or
older than the source's, so a re-run with nothing changed copies nothing. It is
a replace: the existing destination directory is removed first, so chapters
deleted from the source do not linger in the install.

Three refusals, each logged at error for that book, none fatal to the run:

- `build_dir` is absolute or resolves outside `book.dir` (a `build-dir` of
  `../out`, say): the relative layout cannot be preserved, so the book is not
  installed. Building still happened.
- The destination exists but holds no `book.toml`: it is not a book this tool
  made, so it is not deleted. Logged, skipped.
- The destination is the source itself (the book was discovered from `ROOT`):
  skipped silently at debug level. Building in place already did the work.

`ROOT` is created if it does not exist. Nothing else in `ROOT` is touched.

## Code layout

The CLI crate splits `src/main.rs` into three files:

- `main.rs`: the clap types (`Cli` with an `Option<Command>` subcommand,
  `BuildArgs`), tracing setup, the stdin/pipe watchers, and dispatch.
- `serve.rs`: the existing serve-and-window path, moved verbatim.
- `build.rs`: selection, the mdbook subprocess, the install copy, the summary.
  Its pure functions take paths and return `Result`, so they are testable on
  temp directories without mdbook.

The discovery crate's `lib.rs` gains the three methods and the `src` field of
`Meta`, beside `is_built`.

## Error handling

- Missing `mdbook`: fatal, before any work.
- A `DIR` argument that is not a book: error for that argument, run continues.
- mdbook failure on one book: error, run continues, exit 1 at the end.
- Install refusals: error per book, run continues, do **not** set exit 1
  (the build itself succeeded; the message says what to fix).
- Copy I/O errors (disk full, permissions): error per book, run continues,
  exit 1 at the end.

## Testing

Discovery crate, unit tests on `tempfile` dirs using `File::set_modified` to
arrange mtimes:

- `newest_input` sees `book.toml`, `src/**`, and `theme/**`, and ignores
  `build_dir` even when `build-dir` points inside `src`.
- `is_stale` is true when unbuilt, true when a source file is newer than
  `index.html`, false when `index.html` is newer, false on equal mtimes.
- `src_dir` honours `[book] src`.

CLI crate:

- Unit tests for the install copy: the three files land at the right paths;
  `build-dir = "html"` is preserved; a stale destination is replaced and a
  removed file is gone; a destination without `book.toml` is refused; an
  escaping `build-dir` is refused; an up-to-date destination is left alone.
- One end-to-end test that runs the built binary's `build` on a fixture book
  in a temp root (`mdbook init`-style minimal book written by the test), then
  asserts `index.html` exists, a second run reports `up to date`, and
  `--into` produces the slim copy. Skipped with a message when `mdbook` is not
  on PATH; the CI `test` job installs mdbook with the same
  `peaceiris/actions-mdbook` step the `book` job uses, so CI does not skip it.

Existing 29 tests keep passing; `cargo run --bin md-librarian` with no
subcommand behaves exactly as before.

## Documentation

- `book/src/library.md`: a "Building the library" section after "Where books
  are found": the command lines, what stale means, `--into`, the three
  refusals.
- `book/src/getting-started.md`: "Where books go" shows `md-librarian build`
  as the second way to fill a root.
- `README.md`: the two `build` lines in the command-line block, `build`'s
  flags in the table, and a short "Building a library" paragraph under
  Quick start.
- `CHANGELOG.md`: an Added entry.
- `CLAUDE.md` gotchas: "the CLI is the only crate that spawns processes;
  discovery stays pure".

## Sequencing

Independent of the gpui-yaams removal and the consumer migrations. Lands as
one PR, then usdlite's `build.rs` and the rez package's `rez_build.sh` can
switch to `md-librarian build --into` in their own time.
