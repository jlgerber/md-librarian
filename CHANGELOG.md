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

- **`md-librarian build`.** Runs `mdbook build` on every stale book on the
  search path, or on the book directories given; `--force` rebuilds all,
  `--into <ROOT>` installs a slim copy (`book.toml`, cover, rendered output)
  into a root. Discovery gains `Book::{src_dir, newest_input, is_stale}` and
  `read_book(dir)`. mdbook is found on `PATH`, never linked.
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
- **`WebWindowOptions::background`** (`Option<(u8, u8, u8)>`, default
  `None`): what the window paints before a page does. **Breaking** for a
  struct literal that names every field: add it, or `..Default::default()`.

### Fixed

- **No white flash opening a book or returning to the library** (#8). The
  window, the webview and the frame all paint the library's background
  before a page does, and moving between books or back to the library hides
  the frame until the new page has loaded, then fades it in. The frame now
  adopts an mdBook's own background between chapters (it read only `<body>`,
  which mdBook leaves transparent).
