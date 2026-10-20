//! The library grouped **by root** (issue #9): one [`Library`] per root on the
//! search path, named for its directory or for its `library.toml`.
//!
//! [`library`](crate::library) is this flattened; both share the shadowing rule
//! and the title order defined here.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::{Book, discover_root};

/// The optional file in a root that names its library: `name = "…"`.
///
/// Display only. A book's identity is still its title, and nothing about
/// discovery or URLs changes with it.
pub const LIBRARY_FILE: &str = "library.toml";

/// One root's books — a **library** on the page.
///
/// Each root on the search path is its own library, named for the directory
/// it is (or by its [`LIBRARY_FILE`]). Shadowing still runs *across*
/// libraries: a book whose title an earlier library already has is dropped
/// from the later one, which is what keeps a user's root in front of the one a
/// rez package appends.
///
/// `#[non_exhaustive]` so a field added later does not break a struct literal
/// elsewhere; build one with [`Library::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Library {
    /// The display name: `name` from the root's [`LIBRARY_FILE`], else the
    /// root's directory name — lengthened with parent directories only as far
    /// as it takes to tell two same-named libraries apart (`/a/docs` and
    /// `/b/docs` show as `a/docs` and `b/docs`).
    pub name: String,
    /// The root directory, made absolute **lexically**: `.` dropped and `..`
    /// folded, symlinks left as they are. So `--root .`, `--root ../books` and
    /// a trailing separator read as the directory meant, a symlinked root keeps
    /// the link's name, and every [`Book::dir`] in [`Library::books`] starts
    /// with it.
    pub root: PathBuf,
    /// The root's position on the search path — the same index every
    /// [`Book::root_index`] in [`Library::books`] carries, and the first
    /// segment of their URLs. Not renumbered when a library is hidden.
    pub root_index: usize,
    /// This library's books, after shadowing and the include-list, sorted by
    /// title the same way [`library`](crate::library) sorts. Never empty when
    /// returned by [`libraries`].
    pub books: Vec<Book>,
}

impl Library {
    /// A library from its parts, for callers that render one themselves.
    pub fn new(name: String, root: PathBuf, root_index: usize, books: Vec<Book>) -> Self {
        Library {
            name,
            root,
            root_index,
            books,
        }
    }
}

/// Build the library grouped **by root**: one [`Library`] per root that still
/// has a book to show, in search-path order.
///
/// - A book is dropped from a later library when an **earlier** library has a
///   book with the same title (first wins, as in [`library`](crate::library)).
///   Two same-titled books in *one* root are both kept, marked
///   [`Book::ambiguous`].
/// - `include` filters by title after shadowing, exactly as in
///   [`library`](crate::library).
/// - A root that does not exist, holds no books, or is left with none after
///   shadowing and filtering is **not returned**. A missing root is logged. The
///   same directory spelled twice is therefore one library: the second
///   spelling's books are all shadowed by the first's.
///
/// Titles an include-list names that no library provides are not attached to
/// any library — there is none they belong to. Ask [`missing_titles`] for them.
pub fn libraries(roots: &[PathBuf], include: Option<&[String]>) -> Vec<Library> {
    let wanted: Option<HashSet<&str>> = include.map(|w| w.iter().map(String::as_str).collect());
    // Titles claimed by an EARLIER root. Filled after each root is processed, so
    // that duplicates *within* one root are both kept. Claimed before the
    // include-list filters, so a filtered-out book still shadows later copies.
    let mut claimed: HashSet<String> = HashSet::new();
    let mut found: Vec<(usize, PathBuf, Vec<Book>)> = Vec::new();

    for (index, given) in roots.iter().enumerate() {
        let root = lexical_absolute(given);
        if !root.is_dir() {
            tracing::info!(root = %given.display(), "book root does not exist; skipping");
            continue;
        }
        let mut books = discover_root(&root, index);
        books.retain(|b| !claimed.contains(&b.title));
        for b in &books {
            claimed.insert(b.title.clone());
        }
        if let Some(set) = &wanted {
            books.retain(|b| set.contains(b.title.as_str()));
        }
        if books.is_empty() {
            continue;
        }
        books.sort_by(|a, b| by_title(&a.title, &b.title));
        found.push((index, root, books));
    }

    let declared: Vec<Option<String>> = found.iter().map(|(_, r, _)| declared_name(r)).collect();
    let named: Vec<(&Path, Option<&str>)> = found
        .iter()
        .zip(&declared)
        .map(|((_, r, _), d)| (r.as_path(), d.as_deref()))
        .collect();
    let names = library_names(&named);
    found
        .into_iter()
        .zip(names)
        .map(|((root_index, root, books), name)| Library::new(name, root, root_index, books))
        .collect()
}

/// The titles an include-list names that none of `libraries` provides, in
/// include-list order and without repeats. Empty when there is no list.
///
/// Pure over its arguments, so it agrees with whatever [`libraries`] returned.
pub fn missing_titles(libraries: &[Library], include: Option<&[String]>) -> Vec<String> {
    let Some(wanted) = include else {
        return Vec::new();
    };
    let present: HashSet<&str> = libraries
        .iter()
        .flat_map(|l| l.books.iter().map(|b| b.title.as_str()))
        .collect();
    let mut seen: HashSet<&str> = HashSet::new();
    wanted
        .iter()
        .filter(|t| !present.contains(t.as_str()) && seen.insert(t.as_str()))
        .cloned()
        .collect()
}

/// Alphabetical by title. Case-insensitive first so "gpui" and "GPUI" sit
/// together, with a case-sensitive tiebreak so the order is total (and so the
/// page is stable between requests, which matters because it is regenerated
/// on every one).
pub(crate) fn by_title(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// `path` made absolute against the current directory, with `.` dropped and
/// `..` folded into its parent — without touching the filesystem.
///
/// Lexical on purpose: canonicalising would follow a symlinked root to its
/// target, renaming the library after a directory the user never named. The
/// price is the usual one for lexical `..`: `link/..` folds to the link's
/// parent, not the target's.
fn lexical_absolute(path: &Path) -> PathBuf {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `name` from `<root>/library.toml`, if the file is there and says one.
///
/// A file that cannot be read or parsed, or whose `name` is missing, not a
/// string, or blank, is logged and ignored: the library falls back to its
/// directory name rather than disappearing or losing its heading.
fn declared_name(root: &Path) -> Option<String> {
    let path = root.join(LIBRARY_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "library.toml unreadable; using the directory name");
            return None;
        }
    };
    let name = text
        .parse::<toml::Value>()
        .ok()
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if name.is_none() {
        tracing::warn!(
            path = %path.display(),
            "library.toml has no usable `name = \"…\"`; using the directory name"
        );
    }
    name
}

/// A display name per root: the declared name, else the directory name —
/// lengthened by one parent directory at a time for exactly the roots that
/// still collide, until every name is unique or a root runs out of parents
/// (then its full path).
///
/// A declared name stands in for the root's last component, so two roots that
/// both declare `Docs` grow to `a/Docs` and `b/Docs`.
pub(crate) fn library_names(roots: &[(&Path, Option<&str>)]) -> Vec<String> {
    let parts: Vec<Vec<String>> = roots
        .iter()
        .map(|(r, _)| {
            r.components()
                .filter_map(|c| match c {
                    Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect()
        })
        .collect();
    let parents = |i: usize| parts[i].len().saturating_sub(1);
    // depth 1 is the leaf alone; each step adds one parent; past the last
    // parent it is the full path, which is unique unless the path repeats.
    let tail = |i: usize, depth: usize| -> String {
        let (root, declared) = roots[i];
        let p = &parts[i];
        let leaf = match (declared, p.last()) {
            (Some(d), _) => d.to_string(),
            (None, Some(last)) => last.clone(),
            (None, None) => return root.display().to_string(),
        };
        let up = depth - 1;
        if up > parents(i) {
            return root.display().to_string();
        }
        let mut segs: Vec<&str> = p[parents(i) - up..parents(i)]
            .iter()
            .map(String::as_str)
            .collect();
        segs.push(&leaf);
        segs.join("/")
    };
    let mut depth = vec![1; roots.len()];
    loop {
        let names: Vec<String> = (0..roots.len()).map(|i| tail(i, depth[i])).collect();
        let mut grew = false;
        for i in 0..roots.len() {
            let collides = (0..roots.len()).any(|j| j != i && names[j] == names[i]);
            if collides && depth[i] <= parents(i) + 1 {
                depth[i] += 1;
                grew = true;
            }
        }
        if !grew {
            return names;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Entry, library};

    /// Create `<root>/<dir>/book.toml` with the given body.
    fn book(root: &Path, dir: &str, body: &str) -> PathBuf {
        let d = root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("book.toml"), body).unwrap();
        d
    }

    /// `<tmp>/<rel>` as a root directory, created.
    fn root(tmp: &Path, rel: &str) -> PathBuf {
        let r = tmp.join(rel);
        std::fs::create_dir_all(&r).unwrap();
        r
    }

    fn names(libs: &[Library]) -> Vec<&str> {
        libs.iter().map(|l| l.name.as_str()).collect()
    }

    fn lib_titles(lib: &Library) -> Vec<&str> {
        lib.books.iter().map(|b| b.title.as_str()).collect()
    }

    fn titled(t: &str) -> String {
        format!("[book]\ntitle = \"{t}\"\n")
    }

    #[test]
    fn each_root_is_its_own_library_in_search_path_order_with_books_sorted_by_title() {
        let tmp = tempfile::tempdir().unwrap();
        let mine = root(tmp.path(), "mine");
        let shipped = root(tmp.path(), "shipped");
        book(&mine, "z", &titled("zeta"));
        book(&mine, "a", &titled("Alpha"));
        book(&shipped, "m", &titled("middle"));
        let libs = libraries(&[shipped.clone(), mine.clone()], None);
        assert_eq!(
            names(&libs),
            vec!["shipped", "mine"],
            "path order, not alphabetical"
        );
        assert_eq!(lib_titles(&libs[1]), vec!["Alpha", "zeta"]);
        assert_eq!((libs[0].root_index, libs[1].root_index), (0, 1));
        assert!(libs[1].books.iter().all(|b| b.root_index == 1));
        assert_eq!(libs[1].root, mine);
        assert!(
            libs[1]
                .books
                .iter()
                .all(|b| b.dir.starts_with(&libs[1].root))
        );
    }

    #[test]
    fn a_later_library_loses_only_the_titles_an_earlier_one_already_has() {
        let tmp = tempfile::tempdir().unwrap();
        let user = root(tmp.path(), "user");
        let pkg = root(tmp.path(), "pkg");
        book(
            &user,
            "gpui",
            "[book]\ntitle = \"gpui\"\ndescription = \"mine\"\n",
        );
        book(
            &pkg,
            "gpui",
            "[book]\ntitle = \"gpui\"\ndescription = \"shipped\"\n",
        );
        book(&pkg, "guide", &titled("Guide"));
        let libs = libraries(&[user, pkg], None);
        assert_eq!(lib_titles(&libs[0]), vec!["gpui"]);
        assert_eq!(libs[0].books[0].description, "mine");
        assert_eq!(
            lib_titles(&libs[1]),
            vec!["Guide"],
            "the shadowed copy is gone"
        );
    }

    #[test]
    fn a_library_left_empty_by_shadowing_is_hidden_and_keeps_later_indices() {
        let tmp = tempfile::tempdir().unwrap();
        let a = root(tmp.path(), "a");
        let b = root(tmp.path(), "b");
        let c = root(tmp.path(), "c");
        book(&a, "gpui", &titled("gpui"));
        book(&b, "gpui", &titled("gpui"));
        book(&c, "other", &titled("other"));
        let libs = libraries(&[a, b, c], None);
        assert_eq!(names(&libs), vec!["a", "c"]);
        assert_eq!(
            libs[1].root_index, 2,
            "an index is a URL segment and never renumbers"
        );
    }

    #[test]
    fn missing_and_bookless_roots_are_not_libraries() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = root(tmp.path(), "empty");
        std::fs::create_dir_all(empty.join("not-a-book")).unwrap();
        let real = root(tmp.path(), "real");
        book(&real, "gpui", &titled("gpui"));
        let libs = libraries(&[tmp.path().join("nope"), empty, real], None);
        assert_eq!(names(&libs), vec!["real"]);
        assert_eq!(libs[0].root_index, 2);
        assert!(libraries(&[tmp.path().join("nope")], None).is_empty());
    }

    #[test]
    fn two_same_titled_books_in_one_library_are_both_kept() {
        let tmp = tempfile::tempdir().unwrap();
        book(tmp.path(), "gpui-a", &titled("gpui"));
        book(tmp.path(), "gpui-b", &titled("gpui"));
        let libs = libraries(&[tmp.path().to_path_buf()], None);
        let dirs: Vec<&str> = libs[0].books.iter().map(|b| b.dir_name.as_str()).collect();
        assert_eq!(dirs, vec!["gpui-a", "gpui-b"]);
        assert!(libs[0].books.iter().all(|b| b.ambiguous));
    }

    #[test]
    fn the_include_list_filters_each_library_and_hides_one_it_empties() {
        let tmp = tempfile::tempdir().unwrap();
        let a = root(tmp.path(), "a");
        let b = root(tmp.path(), "b");
        book(&a, "x", &titled("keep"));
        book(&a, "y", &titled("drop"));
        book(&b, "z", &titled("drop too"));
        let include = vec!["keep".to_string(), "Typo".to_string(), "Typo".to_string()];
        let libs = libraries(&[a, b], Some(&include));
        assert_eq!(names(&libs), vec!["a"]);
        assert_eq!(lib_titles(&libs[0]), vec!["keep"]);
        assert_eq!(
            missing_titles(&libs, Some(&include)),
            vec!["Typo"],
            "once, not twice"
        );
        assert!(missing_titles(&libs, None).is_empty());
    }

    #[test]
    fn a_filtered_out_book_still_shadows_a_later_copy() {
        // Shadowing precedes the include-list, as it always has: the flat list
        // and the grouped one must agree on which copy is "the" book.
        let tmp = tempfile::tempdir().unwrap();
        let a = root(tmp.path(), "a");
        let b = root(tmp.path(), "b");
        book(&a, "g", &titled("gpui"));
        book(&b, "g", &titled("gpui"));
        let include = vec!["gpui".to_string()];
        assert_eq!(names(&libraries(&[a, b], Some(&include))), vec!["a"]);
    }

    #[test]
    fn the_flat_library_is_the_grouped_one_flattened_and_resorted() {
        let tmp = tempfile::tempdir().unwrap();
        let a = root(tmp.path(), "a");
        let b = root(tmp.path(), "b");
        book(&a, "z", &titled("zeta"));
        book(&b, "a", &titled("alpha"));
        book(&b, "z", &titled("zeta"));
        let include = vec!["zeta".into(), "alpha".into(), "Typo".into()];
        let flat = library(&[a, b], Some(&include));
        let titles: Vec<&str> = flat.iter().map(Entry::title).collect();
        assert_eq!(titles, vec!["alpha", "Typo", "zeta"]);
        let Entry::Book(zeta) = &flat[2] else {
            panic!()
        };
        assert_eq!(zeta.root_index, 0, "first root still wins");
    }

    #[test]
    fn a_library_is_named_for_its_directory_however_the_root_was_spelled() {
        let tmp = tempfile::tempdir().unwrap();
        let docs = root(tmp.path(), "docs");
        book(&docs, "g", &titled("gpui"));
        let slash = PathBuf::from(format!("{}/", docs.display()));
        let dot = docs.join(".");
        let dotdot = docs.join("g").join("..");
        for spelling in [slash, dot, dotdot] {
            let libs = libraries(std::slice::from_ref(&spelling), None);
            assert_eq!(names(&libs), vec!["docs"], "{}", spelling.display());
            assert_eq!(libs[0].root, docs);
        }
    }

    #[test]
    fn a_relative_root_is_made_absolute_against_the_working_directory() {
        let got = lexical_absolute(Path::new("./a/../b/."));
        assert_eq!(got, std::env::current_dir().unwrap().join("b"));
    }

    #[test]
    fn the_same_directory_spelled_twice_is_one_library() {
        let tmp = tempfile::tempdir().unwrap();
        let d = root(tmp.path(), "d");
        book(&d, "g", &titled("gpui"));
        let libs = libraries(&[d.clone(), d.join(".")], None);
        assert_eq!(names(&libs), vec!["d"], "not d and a disambiguated twin");
    }

    #[test]
    fn nested_roots_are_separate_libraries() {
        let tmp = tempfile::tempdir().unwrap();
        let outer = root(tmp.path(), "outer");
        let inner = root(&outer, "inner");
        book(&outer, "o", &titled("outer book"));
        book(&inner, "i", &titled("inner book"));
        let libs = libraries(&[outer, inner], None);
        assert_eq!(names(&libs), vec!["outer", "inner"]);
        assert_eq!(
            lib_titles(&libs[0]),
            vec!["outer book"],
            "a root lists only its children"
        );
        assert_eq!(lib_titles(&libs[1]), vec!["inner book"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_root_keeps_the_links_name() {
        let tmp = tempfile::tempdir().unwrap();
        let target = root(tmp.path(), "real-books");
        book(&target, "g", &titled("gpui"));
        let link = tmp.path().join("my-books");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let libs = libraries(std::slice::from_ref(&link), None);
        assert_eq!(names(&libs), vec!["my-books"]);
        assert_eq!(libs[0].root, link);
        assert!(
            libs[0].books[0].dir.starts_with(&link),
            "Book::dir agrees with root"
        );
    }

    #[test]
    fn same_named_roots_are_told_apart_by_their_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let a = root(tmp.path(), "a/docs");
        let b = root(tmp.path(), "b/docs");
        book(&a, "x", &titled("x"));
        book(&b, "y", &titled("y"));
        assert_eq!(names(&libraries(&[a, b], None)), vec!["a/docs", "b/docs"]);
    }

    #[test]
    fn library_toml_names_the_library() {
        let tmp = tempfile::tempdir().unwrap();
        let r = root(tmp.path(), "books");
        book(&r, "g", &titled("gpui"));
        std::fs::write(r.join(LIBRARY_FILE), "name = \"  md-librarian  \"\n").unwrap();
        assert_eq!(names(&libraries(&[r], None)), vec!["md-librarian"]);
    }

    #[test]
    fn colliding_declared_names_grow_parents_like_directory_names() {
        let tmp = tempfile::tempdir().unwrap();
        let a = root(tmp.path(), "a/books");
        let b = root(tmp.path(), "b/docs");
        book(&a, "x", &titled("x"));
        book(&b, "y", &titled("y"));
        std::fs::write(a.join(LIBRARY_FILE), "name = \"Docs\"\n").unwrap();
        std::fs::write(b.join(LIBRARY_FILE), "name = \"Docs\"\n").unwrap();
        assert_eq!(names(&libraries(&[a, b], None)), vec!["a/Docs", "b/Docs"]);
    }

    #[test]
    fn a_malformed_or_nameless_library_toml_falls_back_to_the_directory_name() {
        for body in [
            "name = ",
            "name = 3\n",
            "title = \"x\"\n",
            "name = \"   \"\n",
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let r = root(tmp.path(), "books");
            book(&r, "g", &titled("gpui"));
            std::fs::write(r.join(LIBRARY_FILE), body).unwrap();
            assert_eq!(names(&libraries(&[r], None)), vec!["books"], "{body:?}");
        }
    }

    #[test]
    fn library_names_grow_only_as_far_as_a_collision_needs() {
        let p = |s: &'static str| (Path::new(s), None);
        assert_eq!(
            library_names(&[p("/x/a/docs"), p("/y/a/docs"), p("/z/books"), p("/docs")]),
            vec!["x/a/docs", "y/a/docs", "books", "/docs"],
            "a root out of parents shows its full path"
        );
        assert_eq!(library_names(&[p("/")]), vec!["/"]);
        assert_eq!(library_names(&[p("/a/docs/")]), vec!["docs"]);
        assert_eq!(
            library_names(&[(Path::new("/a/books"), Some("docs")), p("/b/docs")]),
            vec!["a/docs", "b/docs"],
            "a declared name and a directory name collide like any two"
        );
    }
}
