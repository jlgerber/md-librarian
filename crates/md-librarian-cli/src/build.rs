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
}
