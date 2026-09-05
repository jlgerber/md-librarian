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
    std::fs::write(
        dir.join("book.toml"),
        format!("[book]\ntitle = \"{title}\"\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("src/SUMMARY.md"),
        "# Summary\n\n- [Intro](intro.md)\n",
    )
    .unwrap();
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
    assert!(
        !tmp.path().join("guide/book/index.html").exists(),
        "nothing must be built"
    );
}

#[test]
fn build_renders_stale_books_and_then_reports_up_to_date() {
    if !mdbook_on_path() {
        eprintln!("skipping: mdbook not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let guide = fixture(tmp.path(), "guide", "Guide");

    let out = bin()
        .args(["build", "--root"])
        .arg(tmp.path())
        .output()
        .unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "{err}");
    assert!(guide.join("book/index.html").is_file(), "{err}");
    assert!(err.contains("built 1, up to date 0, failed 0"), "{err}");

    let out = bin()
        .args(["build", "--root"])
        .arg(tmp.path())
        .output()
        .unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "{err}");
    assert!(err.contains("built 0, up to date 1, failed 0"), "{err}");

    let out = bin()
        .args(["build", "--force", "--root"])
        .arg(tmp.path())
        .output()
        .unwrap();
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
    assert!(
        out.status.success(),
        "a non-book argument is an error line, not a failure: {err}"
    );
    assert!(guide.join("book/index.html").is_file());
    assert!(
        err.contains("plain"),
        "the non-book path must be named: {err}"
    );
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
    // mdbook 0.5 rejects unknown configuration keys (measured in the
    // 2026-09-01 spec: `unknown field`), so this book cannot build. Discovery
    // still lists it: a book.toml with extra keys is still a book.
    let bad = fixture(tmp.path(), "bad", "Bad");
    std::fs::write(
        bad.join("book.toml"),
        "[book]\ntitle = \"Bad\"\nbogus = \"x\"\n",
    )
    .unwrap();

    let out = bin()
        .args(["build", "--root"])
        .arg(tmp.path())
        .output()
        .unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(good.join("book/index.html").is_file(), "{err}");
    assert!(err.contains("built 1, up to date 0, failed 1"), "{err}");
}

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

    let out = bin()
        .arg("build")
        .arg(&guide)
        .arg("--into")
        .arg(&into)
        .output()
        .unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "{err}");
    let dest = into.join("guide");
    assert!(dest.join("book.toml").is_file(), "{err}");
    assert!(dest.join("cover.svg").is_file());
    assert!(dest.join("book/index.html").is_file());
    assert!(!dest.join("src").exists());
    assert!(err.contains("installed"), "{err}");

    // Second run: built book is fresh, install is fresh, nothing copied.
    let out = bin()
        .arg("build")
        .arg(&guide)
        .arg("--into")
        .arg(&into)
        .output()
        .unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("built 0, up to date 1, failed 0"), "{err}");
    assert!(!err.contains("installed"), "{err}");
}
