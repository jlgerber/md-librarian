//! Discovery for a **library of mdbooks**: where books live, how one is
//! recognised, and which copy wins when two claim the same title, and whether
//! a book is stale relative to its sources ([`Book::is_stale`]), which is what
//! `md-librarian build` asks.
//!
//! This crate is deliberately pure — no server, no window, no gpui. It answers
//! "what books are installed on this machine, right now?" and nothing else.
//! [`md-librarian-serve`](../md_librarian_serve/index.html) renders and serves the
//! answer; see `book/src/library.md` for the whole feature.
//!
//! # The shape of a repository
//!
//! A **root** is a directory of book *source trees with their built output
//! inside*:
//!
//! ```text
//! <root>/
//! ├── gpui/
//! │   ├── book.toml
//! │   ├── cover.png          <- optional
//! │   ├── src/
//! │   └── book/              <- or wherever [build] build-dir points
//! └── usdlite-user/
//!     ├── book.toml
//!     └── book/
//! ```
//!
//! A directory is a book **iff** it holds a `book.toml`. Roots stack: several
//! are searched in order and the **first one wins**, the same rule the yaams
//! config search path and `USDLITE_ASSET_PATH` already use.
//!
//! # Why identity is the title
//!
//! mdbook 0.5 **rejects unknown configuration**: `[book] id = "…"` fails the
//! build outright (`unknown field 'id'`), and so does a custom top-level table.
//! A book therefore cannot carry an id, a tag, or a cover declaration — anything
//! this crate needs beyond mdbook's six `[book]` keys must come from a
//! *convention* (a file named `cover.png`) or from *outside* the book (the
//! include-list a caller supplies).
//!
//! So a book is identified by its `[book] title`, falling back to the directory
//! name when that is missing or empty. Titles are display text, though — they
//! contain spaces and punctuation — so they are never used to build a URL. That
//! is what [`Book::root_index`] and [`Book::dir_name`] are for.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The search-path environment variable: a stacking, `:`-separated list of roots.
///
/// Note the deliberate contrast with yaams-tk2's `YAAMS_BOOKS_DIR`, which names
/// a **single** directory. `_PATH` is a list, `_DIR` is one — the suffix is the
/// cardinality, so the two can coexist on a box mid-migration without either
/// being mistaken for the other.
pub const BOOK_PATH_VAR: &str = "MD_LIBRARIAN_PATH";

/// Extensions probed for an optional `cover.<ext>` beside `book.toml`.
const COVER_EXTS: [&str; 5] = ["png", "svg", "jpg", "jpeg", "webp"];

/// mdbook's default output directory, used when `book.toml` sets no `build-dir`.
const DEFAULT_BUILD_DIR: &str = "book";

/// mdbook's default source directory, used when `book.toml` sets no `[book] src`.
const DEFAULT_SRC_DIR: &str = "src";

/// One discovered book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Book {
    /// `[book] title`, or the directory name when that is missing or empty.
    ///
    /// This is the book's **identity**: shadowing across roots compares it, and
    /// an include-list names it. It is display text and may contain anything.
    pub title: String,
    /// The directory name — never displayed except to break a tie, but half of
    /// the URL key, because it cannot contain a path separator.
    pub dir_name: String,
    /// The book's source directory (the one holding `book.toml`).
    pub dir: PathBuf,
    /// Where the rendered HTML lives: `dir` joined with `[build] build-dir`.
    ///
    /// Unlike yaams-tk2's equivalent, `build-dir` **is** honoured here. That
    /// crate refuses it because it *rebuilds* books and the output would land
    /// inside its staleness walk, making a book its own newest input; this one
    /// only ever serves, so the hazard does not transfer — and honouring it is
    /// what makes md-librarian's own book (`build-dir = "html"`) discoverable.
    pub build_dir: PathBuf,
    /// `[book] description`, empty when unset. Shown on the card.
    pub description: String,
    /// An optional `cover.<ext>` beside `book.toml`.
    ///
    /// The cover lives *inside the book* rather than in a per-root index, so a
    /// book shared between contexts carries its cover into all of them.
    pub cover: Option<PathBuf>,
    /// Which root this came from — the other half of the URL key, and what
    /// makes two same-titled books in different roots addressable apart.
    pub root_index: usize,
    /// Another book **in the same root** shares this title.
    ///
    /// Cross-root duplicates are shadowed, so they never reach here; within one
    /// root there is no order to break the tie and hiding one would misreport
    /// what is installed, so both are kept and the renderer shows the directory
    /// name to tell them apart.
    pub ambiguous: bool,
    /// The source directory: `dir` joined with `[book] src` (default `src`).
    /// Private because it is derived; read it through [`Book::src_dir`].
    src: PathBuf,
}

impl Book {
    /// Whether the rendered output is actually present.
    ///
    /// A listed-but-unbuilt book must render inert: a link into missing content
    /// opens a 404 in a window with no back button (see `book/src/docs-window.md`).
    pub fn is_built(&self) -> bool {
        self.build_dir.join("index.html").is_file()
    }

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
            Err(e) => {
                tracing::debug!(path = %path.display(), error = %e, "input unreadable; ignored")
            }
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
        let built_at =
            match std::fs::metadata(self.build_dir.join("index.html")).and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(_) => return true,
            };
        match self.newest_input() {
            Some(input) => input > built_at,
            None => false,
        }
    }
}

/// One entry on the library page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A book that was found.
    Book(Book),
    /// A title an include-list asked for that no root provides.
    ///
    /// Rendered as a dead card rather than silently dropped: a typo or an
    /// unmounted root should be *visible*, and the alternative — a library that
    /// quietly omits what you asked for — is indistinguishable from a bug.
    Missing { title: String },
}

impl Entry {
    /// The title this entry sorts and displays under.
    pub fn title(&self) -> &str {
        match self {
            Entry::Book(b) => &b.title,
            Entry::Missing { title } => title,
        }
    }
}

/// The roots to search, in order: an explicit list wins, else [`BOOK_PATH_VAR`],
/// else the XDG default.
///
/// The default is deliberately the same location yaams-tk2's `just install-docs`
/// deploys to, so a bare run finds books that are already on the machine rather
/// than opening empty.
pub fn roots(cli: &[PathBuf]) -> Vec<PathBuf> {
    if !cli.is_empty() {
        return cli.to_vec();
    }
    if let Some(raw) = std::env::var_os(BOOK_PATH_VAR).filter(|s| !s.is_empty()) {
        let from_env: Vec<PathBuf> = std::env::split_paths(&raw)
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
        if !from_env.is_empty() {
            return from_env;
        }
    }
    vec![default_root()]
}

/// `$XDG_DATA_HOME/md-librarian/books`, else `~/.local/share/md-librarian/books`.
pub fn default_root() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    data.join("md-librarian").join("books")
}

/// Build the library: discover every root in order, shadow duplicates, apply an
/// optional include-list, and sort alphabetically by title.
///
/// `include` is a list of **titles**; `None` means everything. A root that does
/// not exist is skipped with a log line — not silently (a typo or an unmounted
/// NFS root should leave a trace) and not fatally (a package that is not
/// resolved on this machine must not stop the viewer).
pub fn library(roots: &[PathBuf], include: Option<&[String]>) -> Vec<Entry> {
    let mut books: Vec<Book> = Vec::new();
    // Titles claimed by an EARLIER root. Filled after each root is processed, so
    // that duplicates *within* one root are both kept.
    let mut claimed: HashSet<String> = HashSet::new();

    for (index, root) in roots.iter().enumerate() {
        if !root.is_dir() {
            tracing::info!(root = %root.display(), "book root does not exist; skipping");
            continue;
        }
        let mut in_root = discover_root(root, index);
        in_root.retain(|b| !claimed.contains(&b.title));
        for b in &in_root {
            claimed.insert(b.title.clone());
        }
        books.append(&mut in_root);
    }

    let mut entries: Vec<Entry> = match include {
        None => books.into_iter().map(Entry::Book).collect(),
        Some(wanted) => {
            let set: HashSet<&str> = wanted.iter().map(String::as_str).collect();
            let mut kept: Vec<Entry> = books
                .into_iter()
                .filter(|b| set.contains(b.title.as_str()))
                .map(Entry::Book)
                .collect();
            for title in wanted {
                if !kept.iter().any(|e| e.title() == title) {
                    kept.push(Entry::Missing {
                        title: title.clone(),
                    });
                }
            }
            kept
        }
    };

    // Alphabetical by title. Case-insensitive first so "gpui" and "GPUI" sit
    // together, with a case-sensitive tiebreak so the order is total (and so the
    // page is stable between requests, which matters because it is regenerated
    // on every one).
    entries.sort_by(|a, b| {
        a.title()
            .to_lowercase()
            .cmp(&b.title().to_lowercase())
            .then_with(|| a.title().cmp(b.title()))
    });
    entries
}

/// Resolve one book by its **URL key** — root index plus directory name.
///
/// The server needs this on every request: one mdbook page pulls dozens of
/// assets, and re-scanning every root for each of them would be silly. This is
/// a single `book.toml` read instead.
///
/// It deliberately does **not** apply shadowing. The grid never links to a
/// shadowed copy, but a URL that names one explicitly should still resolve
/// rather than 404 for reasons the caller cannot see.
///
/// The directory name must be exactly one path component — this is the boundary
/// where `..` and separators are rejected, before anything reaches the
/// filesystem.
pub fn book_at(roots: &[PathBuf], root_index: usize, dir_name: &str) -> Option<Book> {
    if dir_name.is_empty()
        || dir_name == "."
        || dir_name == ".."
        || dir_name.contains('/')
        || dir_name.contains('\\')
        || dir_name.contains('\0')
    {
        return None;
    }
    let root = roots.get(root_index)?;
    let dir = root.join(dir_name);
    if !dir.join("book.toml").is_file() {
        return None;
    }
    Some(read_book_in_root(&dir, root_index))
}

/// Every book directly under one root, in directory order, with `ambiguous` set.
pub fn discover_root(root: &Path, index: usize) -> Vec<Book> {
    let Ok(entries) = std::fs::read_dir(root) else {
        tracing::warn!(root = %root.display(), "book root could not be read; skipping");
        return Vec::new();
    };

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("book.toml").is_file())
        .collect();
    // Sorted so discovery is deterministic; the page re-sorts by title anyway,
    // but two same-titled books in one root must keep a stable relative order.
    dirs.sort();

    let mut books: Vec<Book> = dirs
        .into_iter()
        .map(|dir| read_book_in_root(&dir, index))
        .collect();

    // Mark titles that appear more than once in THIS root.
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for b in &books {
        *counts.entry(b.title.as_str()).or_default() += 1;
    }
    let dupes: HashSet<String> = counts
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(t, _)| t.to_string())
        .collect();
    for b in &mut books {
        b.ambiguous = dupes.contains(&b.title);
    }
    books
}

/// Read one book directory directly, outside any root.
///
/// `None` unless `dir/book.toml` is a file, or unless `dir` cannot be
/// canonicalised.
///
/// The argument is **canonicalised first**, because this is the entry point a
/// user's own path reaches: `.`, `..` and a trailing separator all leave
/// `Path::file_name()` None, and a [`Book`] with an empty [`Book::dir_name`]
/// makes `root.join(&book.dir_name)` collapse onto the root itself. That is how
/// `md-librarian build . --into ROOT` came to replace ROOT. [`discover_root`]
/// needs no such care: its entries come from `read_dir` and already have names.
///
/// The book is otherwise read exactly as a root entry would be (title fallback,
/// `build-dir`, cover, `src`), with `root_index` 0 and `ambiguous` false —
/// there is no root to be ambiguous in.
pub fn read_book(dir: &Path) -> Option<Book> {
    let dir = std::fs::canonicalize(dir).ok()?;
    dir.join("book.toml")
        .is_file()
        .then(|| read_book_in_root(&dir, 0))
}

/// Read one book directory into a [`Book`].
fn read_book_in_root(dir: &Path, root_index: usize) -> Book {
    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let meta = read_meta(&dir.join("book.toml"));

    // An empty title renders as a zero-width, invisible link (docs-window.md's
    // fifth trap), so it falls back to something that always exists.
    let title = match meta.title {
        Some(t) if !t.trim().is_empty() => t,
        _ => dir_name.clone(),
    };
    let build_dir = dir.join(meta.build_dir.as_deref().unwrap_or(DEFAULT_BUILD_DIR));
    let cover = COVER_EXTS
        .iter()
        .map(|ext| dir.join(format!("cover.{ext}")))
        .find(|p| p.is_file());

    Book {
        title,
        dir_name,
        dir: dir.to_path_buf(),
        build_dir,
        description: meta.description.unwrap_or_default(),
        cover,
        root_index,
        ambiguous: false,
        src: dir.join(meta.src.as_deref().unwrap_or(DEFAULT_SRC_DIR)),
    }
}

/// The four keys read out of a `book.toml`.
#[derive(Default)]
struct Meta {
    title: Option<String>,
    description: Option<String>,
    build_dir: Option<String>,
    src: Option<String>,
}

/// Parse `book.toml` for `[book] title`/`description`/`src` and `[build] build-dir`.
///
/// A malformed or minimal file yields defaults rather than dropping the book:
/// the directory holds a `book.toml`, so it *is* a book, and reporting it under
/// its directory name is more useful than pretending it is not there.
fn read_meta(path: &Path) -> Meta {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Meta::default();
    };
    let Ok(value) = text.parse::<toml::Value>() else {
        tracing::warn!(path = %path.display(), "book.toml did not parse; using defaults");
        return Meta::default();
    };
    let str_at = |table: &str, key: &str| -> Option<String> {
        value
            .get(table)
            .and_then(|t| t.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    Meta {
        title: str_at("book", "title"),
        description: str_at("book", "description"),
        build_dir: str_at("build", "build-dir").filter(|s| !s.is_empty()),
        src: str_at("book", "src").filter(|s| !s.is_empty()),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Create `<root>/<dir>/book.toml` with the given `[book]`/`[build]` body.
    fn book(root: &Path, dir: &str, body: &str) -> PathBuf {
        let d = root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("book.toml"), body).unwrap();
        d
    }

    /// Give a book directory a rendered `index.html` in `sub`.
    fn built(dir: &Path, sub: &str) {
        let out = dir.join(sub);
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("index.html"), "<h1>hi</h1>").unwrap();
    }

    fn titles(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(Entry::title).collect()
    }

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
    fn read_book_of_dot_has_the_real_directory_name() {
        // `Path::file_name()` is None for `.`, `..` and a trailing `/`, so an
        // un-canonicalised read leaves `dir_name` empty — and an empty
        // dir_name makes `root.join(&book.dir_name)` resolve to the root
        // itself, which `md-librarian build . --into ROOT` would then replace.
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "mybook", "[book]\ntitle = \"My Book\"\n");
        for path in [
            tmp.path().join("mybook/."),
            tmp.path().join("mybook/../mybook"),
        ] {
            let b = read_book(&path).expect("still a book");
            assert_eq!(b.dir_name, "mybook", "for {}", path.display());
            assert!(b.dir.is_absolute(), "for {}", b.dir.display());
        }
        // `Path::components` normalises an interior `.` away, so the two forms
        // above already name the directory; a path *ending* in `..` is the one
        // that leaves `file_name()` None, exactly as a bare `.` does.
        std::fs::create_dir_all(tmp.path().join("mybook/src")).unwrap();
        let b = read_book(&tmp.path().join("mybook/src/..")).expect("still a book");
        assert_eq!(b.dir_name, "mybook");
        assert!(b.dir.is_absolute(), "for {}", b.dir.display());

        assert!(
            read_book(&tmp.path().join("nope/.")).is_none(),
            "a directory that does not exist is not a book"
        );
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
        write(
            &dir.join("src/intro.md"),
            "# hi",
            t0() + Duration::from_secs(10),
        );
        write(
            &dir.join("theme/x.css"),
            "b{}",
            t0() + Duration::from_secs(20),
        );
        write(
            &dir.join("src/out/index.html"),
            "<h1/>",
            t0() + Duration::from_secs(999),
        );
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
        write(
            &dir.join("book/index.html"),
            "<h1/>",
            t0() + Duration::from_secs(10),
        );
        write(
            &dir.join("src/intro.md"),
            "# hi",
            t0() + Duration::from_secs(20),
        );
        assert!(read_book(&dir).unwrap().is_stale());
    }

    #[test]
    fn fresh_when_index_html_is_newer_than_every_input() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = book(tmp.path(), "g", "[book]\ntitle = \"G\"\n");
        stamp(&dir.join("book.toml"), t0());
        write(
            &dir.join("src/intro.md"),
            "# hi",
            t0() + Duration::from_secs(10),
        );
        write(
            &dir.join("book/index.html"),
            "<h1/>",
            t0() + Duration::from_secs(20),
        );
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

    #[test]
    fn a_directory_without_book_toml_is_not_a_book() {
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "gpui", "[book]\ntitle = \"gpui\"\n");
        std::fs::create_dir_all(tmp.path().join("not-a-book/src")).unwrap();
        let got = library(&[tmp.path().to_path_buf()], None);
        assert_eq!(titles(&got), vec!["gpui"]);
    }

    #[test]
    fn entries_sort_alphabetically_by_title_not_by_directory() {
        let tmp = tempfile::tempdir().unwrap();
        // Directory order is the reverse of title order, so a pass-through of
        // read_dir order would fail this.
        book(tmp.path(), "a-dir", "[book]\ntitle = \"Zebra\"\n");
        book(tmp.path(), "z-dir", "[book]\ntitle = \"Aardvark\"\n");
        let got = library(&[tmp.path().to_path_buf()], None);
        assert_eq!(titles(&got), vec!["Aardvark", "Zebra"]);
    }

    #[test]
    fn a_missing_or_empty_title_falls_back_to_the_directory_name() {
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "no-title", "[book]\nauthors = [\"someone\"]\n");
        book(tmp.path(), "blank-title", "[book]\ntitle = \"   \"\n");
        let got = library(&[tmp.path().to_path_buf()], None);
        // Sorted by the resolved title, i.e. the directory names.
        assert_eq!(titles(&got), vec!["blank-title", "no-title"]);
    }

    #[test]
    fn build_dir_is_honoured_so_a_book_rendering_to_html_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = book(
            tmp.path(),
            "md-librarian",
            "[book]\ntitle = \"md-librarian\"\n\n[build]\nbuild-dir = \"html\"\n",
        );
        built(&dir, "html");
        let got = library(&[tmp.path().to_path_buf()], None);
        let Entry::Book(b) = &got[0] else {
            panic!("expected a book")
        };
        assert_eq!(b.build_dir, dir.join("html"));
        assert!(
            b.is_built(),
            "the rendered index.html must be found under build-dir"
        );
    }

    #[test]
    fn an_unbuilt_book_is_listed_but_not_built() {
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "unbuilt", "[book]\ntitle = \"Unbuilt\"\n");
        let got = library(&[tmp.path().to_path_buf()], None);
        let Entry::Book(b) = &got[0] else {
            panic!("expected a book")
        };
        assert!(
            !b.is_built(),
            "no index.html means it must render inert, not link"
        );
    }

    #[test]
    fn a_cover_beside_book_toml_is_found_and_none_is_fine() {
        let tmp = tempfile::tempdir().unwrap();
        let with = book(tmp.path(), "with", "[book]\ntitle = \"With\"\n");
        std::fs::write(with.join("cover.png"), b"\x89PNG").unwrap();
        book(tmp.path(), "without", "[book]\ntitle = \"Without\"\n");
        let got = library(&[tmp.path().to_path_buf()], None);
        let Entry::Book(a) = &got[0] else { panic!() };
        let Entry::Book(b) = &got[1] else { panic!() };
        assert_eq!(a.cover, Some(with.join("cover.png")));
        assert_eq!(b.cover, None);
    }

    #[test]
    fn the_first_root_wins_and_shadows_later_copies() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        book(
            a.path(),
            "gpui",
            "[book]\ntitle = \"gpui\"\ndescription = \"mine\"\n",
        );
        book(
            b.path(),
            "gpui-shipped",
            "[book]\ntitle = \"gpui\"\ndescription = \"shipped\"\n",
        );
        let got = library(&[a.path().to_path_buf(), b.path().to_path_buf()], None);
        assert_eq!(got.len(), 1, "the later root's copy must be shadowed");
        let Entry::Book(book) = &got[0] else { panic!() };
        assert_eq!(book.description, "mine");
        assert_eq!(book.root_index, 0);
    }

    #[test]
    fn two_same_titled_books_in_one_root_both_show_and_are_marked_ambiguous() {
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "gpui-a", "[book]\ntitle = \"gpui\"\n");
        book(tmp.path(), "gpui-b", "[book]\ntitle = \"gpui\"\n");
        book(tmp.path(), "solo", "[book]\ntitle = \"Solo\"\n");
        let got = library(&[tmp.path().to_path_buf()], None);
        assert_eq!(titles(&got), vec!["gpui", "gpui", "Solo"]);
        for e in &got {
            let Entry::Book(b) = e else { panic!() };
            assert_eq!(
                b.ambiguous,
                b.title == "gpui",
                "only the colliding title is ambiguous: {}",
                b.title
            );
        }
    }

    #[test]
    fn an_include_list_keeps_only_the_titles_it_names() {
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "gpui", "[book]\ntitle = \"gpui\"\n");
        book(tmp.path(), "other", "[book]\ntitle = \"Other\"\n");
        let got = library(&[tmp.path().to_path_buf()], Some(&["gpui".to_string()]));
        assert_eq!(titles(&got), vec!["gpui"]);
    }

    #[test]
    fn a_filtered_title_that_is_nowhere_becomes_a_dead_card() {
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "gpui", "[book]\ntitle = \"gpui\"\n");
        let want = ["gpui".to_string(), "Typo".to_string()];
        let got = library(&[tmp.path().to_path_buf()], Some(&want));
        assert_eq!(titles(&got), vec!["gpui", "Typo"]);
        assert!(
            matches!(got[1], Entry::Missing { .. }),
            "a title no root provides must be visible, not silently dropped"
        );
    }

    #[test]
    fn a_root_that_does_not_exist_is_skipped_rather_than_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "gpui", "[book]\ntitle = \"gpui\"\n");
        let missing = tmp.path().join("nope");
        let got = library(&[missing, tmp.path().to_path_buf()], None);
        assert_eq!(titles(&got), vec!["gpui"]);
    }

    #[test]
    fn a_malformed_book_toml_still_yields_a_book_named_for_its_directory() {
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "odd", "this is not = = toml\n");
        let got = library(&[tmp.path().to_path_buf()], None);
        assert_eq!(titles(&got), vec!["odd"]);
    }

    #[test]
    fn book_at_resolves_by_url_key_and_refuses_to_escape_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "gpui", "[book]\ntitle = \"gpui\"\n");
        let roots = vec![tmp.path().to_path_buf()];
        assert_eq!(book_at(&roots, 0, "gpui").unwrap().title, "gpui");
        assert!(book_at(&roots, 1, "gpui").is_none(), "no such root");
        assert!(book_at(&roots, 0, "nope").is_none(), "no such book");
        for evil in ["..", ".", "../..", "a/b", ""] {
            assert!(book_at(&roots, 0, evil).is_none(), "must refuse {evil:?}");
        }
    }

    #[test]
    fn explicit_roots_win_over_the_environment() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = vec![tmp.path().to_path_buf()];
        assert_eq!(roots(&cli), cli);
    }
}
