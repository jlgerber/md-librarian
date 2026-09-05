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
/// build or an install hit an I/O error, else 0.
pub fn run(args: BuildArgs) -> anyhow::Result<i32> {
    let mdbook = which_mdbook()?;
    let books = select(&args);
    let (mut built, mut up_to_date, mut failed) = (0u32, 0u32, 0u32);
    for book in &books {
        if !args.force && !book.is_stale() {
            tracing::info!(title = %book.title, "up to date");
            up_to_date += 1;
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
                Ok(Installed::Copied) => {
                    tracing::info!(title = %book.title, root = %root.display(), "installed")
                }
                Ok(Installed::UpToDate) => {
                    tracing::debug!(title = %book.title, "install up to date")
                }
                Ok(Installed::SameDir) => {
                    tracing::debug!(title = %book.title, "already in that root")
                }
                Ok(Installed::Refused(why)) => {
                    tracing::error!(title = %book.title, "not installed: {why}")
                }
                Err(e) => {
                    tracing::error!(title = %book.title, error = %e, "install failed");
                    failed += 1;
                }
            }
        }
    }
    eprintln!("built {built}, up to date {up_to_date}, failed {failed}");
    Ok(if failed > 0 { 1 } else { 0 })
}

/// `mdbook` in a PATH-shaped list of directories, if present.
pub fn find_mdbook_in(path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join("mdbook"))
        .find(|p| p.is_file())
}

/// `mdbook` on PATH, or an error naming how to install it.
pub fn which_mdbook() -> anyhow::Result<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    find_mdbook_in(&path).ok_or_else(|| {
        anyhow::anyhow!("`mdbook` not found on PATH — install it with `cargo install mdbook`")
    })
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
    if rel
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
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
        BuildArgs {
            dirs: vec![],
            root: vec![],
            include: vec![],
            force: false,
            into: None,
        }
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
        assert_eq!(
            titles,
            vec!["Beta"],
            "Missing entries are warned about, not built"
        );
    }

    #[test]
    fn select_from_dirs_skips_non_books_and_ignores_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let guide = book(tmp.path(), "guide", "[book]\ntitle = \"Guide\"\n");
        let plain = tmp.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let other_root = tempfile::tempdir().unwrap();
        book(
            other_root.path(),
            "elsewhere",
            "[book]\ntitle = \"Elsewhere\"\n",
        );
        let mut a = args();
        a.dirs = vec![guide.clone(), plain];
        a.root = vec![other_root.path().to_path_buf()];
        let got = select(&a);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].dir, guide);
    }

    #[test]
    fn find_mdbook_in_an_empty_path_is_none() {
        assert_eq!(find_mdbook_in(std::ffi::OsStr::new("")), None);
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(find_mdbook_in(tmp.path().as_os_str()), None);
        std::fs::write(tmp.path().join("mdbook"), "").unwrap();
        assert_eq!(
            find_mdbook_in(tmp.path().as_os_str()),
            Some(tmp.path().join("mdbook"))
        );
    }

    fn built(dir: &Path, sub: &str, index: &str) {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        std::fs::write(dir.join(sub).join("index.html"), index).unwrap();
        std::fs::write(dir.join(sub).join("style.css"), "b{}").unwrap();
    }

    #[test]
    fn install_copies_toml_cover_and_output_at_the_relative_build_dir() {
        let src = tempfile::tempdir().unwrap();
        let dir = book(
            src.path(),
            "guide",
            "[book]\ntitle = \"Guide\"\n\n[build]\nbuild-dir = \"html\"\n",
        );
        std::fs::write(dir.join("cover.png"), b"png").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/intro.md"), "# hi").unwrap();
        built(&dir, "html", "<h1>v1</h1>");
        let b = md_librarian::read_book(&dir).unwrap();
        let root = tempfile::tempdir().unwrap();

        assert_eq!(install(&b, root.path()).unwrap(), Installed::Copied);
        let dest = root.path().join("guide");
        assert_eq!(
            std::fs::read_to_string(dest.join("book.toml")).unwrap(),
            std::fs::read_to_string(dir.join("book.toml")).unwrap()
        );
        assert_eq!(std::fs::read(dest.join("cover.png")).unwrap(), b"png");
        assert_eq!(
            std::fs::read_to_string(dest.join("html/index.html")).unwrap(),
            "<h1>v1</h1>"
        );
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
        assert!(
            !root.path().join("g/book/old-chapter.html").exists(),
            "replace, not merge"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("g/book/index.html")).unwrap(),
            "<h1>v2</h1>"
        );
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
        assert!(
            root.path().join("g/precious").is_dir(),
            "nothing was deleted"
        );
    }

    #[test]
    fn install_refuses_a_build_dir_outside_the_book() {
        let src = tempfile::tempdir().unwrap();
        let dir = book(
            src.path(),
            "g",
            "[book]\ntitle = \"G\"\n\n[build]\nbuild-dir = \"../out\"\n",
        );
        built(src.path(), "out", "<h1/>");
        let b = md_librarian::read_book(&dir).unwrap();
        assert!(
            b.is_built(),
            "the escaping build-dir is honoured for serving"
        );
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
}
