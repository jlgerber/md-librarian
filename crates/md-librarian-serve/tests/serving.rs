//! The contract of the served library, asserted over real HTTP.
//!
//! This is the half of the feature that *can* be tested: `book/src/docs-window.md`
//! records that anything touching a `WebWindow` needs GTK and a display and can
//! only be a manual checklist, but the server and the pages it generates are a
//! pure function of what is on disk. Splitting the window into a separate binary
//! is what buys these tests.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

/// Minimal HTTP/1.1 GET, so the tests exercise the real server rather than the
/// routing function. Returns (status, body).
fn get(base: &str, path: &str) -> (u16, String) {
    let authority = base
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string();
    let mut stream = TcpStream::connect(&authority).expect("connect");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
    )
    .expect("write");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (status, body.to_string())
}

/// A book directory, optionally built.
fn book(root: &Path, dir: &str, toml: &str, build: Option<(&str, &str)>) -> PathBuf {
    let d = root.join(dir);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("book.toml"), toml).unwrap();
    if let Some((build_dir, html)) = build {
        let out = d.join(build_dir);
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("index.html"), html).unwrap();
    }
    d
}

fn serve(roots: Vec<PathBuf>, include: Option<Vec<String>>) -> String {
    md_librarian_serve::start(roots, include)
        .expect("server starts")
        .url()
        .to_string()
    // The Server is dropped here on purpose: the serving thread is detached
    // and keeps running for the life of the test process, which is exactly
    // how the viewer uses it.
}

#[test]
fn the_shell_is_a_bar_over_a_frame_showing_the_grid() {
    let tmp = tempfile::tempdir().unwrap();
    book(
        tmp.path(),
        "gpui",
        "[book]\ntitle = \"gpui\"\n",
        Some(("book", "<h1>gpui</h1>")),
    );
    let base = serve(vec![tmp.path().to_path_buf()], None);

    let (status, body) = get(&base, "/");
    assert_eq!(status, 200);
    assert!(
        body.contains("src=\"/_grid\""),
        "the frame starts on the library"
    );
    assert!(
        body.contains("href=\"/_grid\""),
        "the bar links back to the library"
    );
    assert!(body.contains("Library"), "the bar names the way back");
}

#[test]
fn the_frame_is_not_painted_white_between_pages() {
    // Every chapter is a separate document, so the browser blanks the frame on
    // each navigation and the frame's own background is what shows through. A
    // white flash against a dark book is the worst case, so the default is grey
    // — and the shell adopts the book's real background after each load, which
    // is only possible because the two share an origin.
    let tmp = tempfile::tempdir().unwrap();
    book(
        tmp.path(),
        "gpui",
        "[book]\ntitle = \"gpui\"\n",
        Some(("book", "<h1>x</h1>")),
    );
    let base = serve(vec![tmp.path().to_path_buf()], None);

    let (_, shell) = get(&base, "/");
    assert!(
        !shell.contains("border:0;background:#fff"),
        "the frame must not paint white between pages"
    );
    assert!(
        shell.contains("getComputedStyle(b).backgroundColor"),
        "the shell must adopt the book's own background after a load"
    );
}

#[test]
fn a_built_book_gets_a_card_that_links_to_it() {
    let tmp = tempfile::tempdir().unwrap();
    book(
        tmp.path(),
        "gpui",
        "[book]\ntitle = \"gpui\"\ndescription = \"Developing with gpui.\"\n",
        Some(("book", "<h1>gpui</h1>")),
    );
    let base = serve(vec![tmp.path().to_path_buf()], None);

    let (_, grid) = get(&base, "/_grid");
    assert!(
        grid.contains("href=\"/0/gpui/\""),
        "linked by root index + directory: {grid}"
    );
    assert!(grid.contains("gpui"));
    assert!(
        grid.contains("Developing with gpui."),
        "the description is on the card"
    );

    let (status, page) = get(&base, "/0/gpui/");
    assert_eq!(status, 200);
    assert!(
        page.contains("<h1>gpui</h1>"),
        "the book's own index.html is served"
    );
}

#[test]
fn build_dir_is_honoured_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    book(
        tmp.path(),
        "md-librarian",
        "[book]\ntitle = \"md-librarian\"\n\n[build]\nbuild-dir = \"html\"\n",
        Some(("html", "<h1>rendered to html/</h1>")),
    );
    let base = serve(vec![tmp.path().to_path_buf()], None);
    let (status, page) = get(&base, "/0/md-librarian/");
    assert_eq!(status, 200, "a book rendering to html/ must serve");
    assert!(page.contains("rendered to html/"));
}

#[test]
fn an_unbuilt_book_is_listed_but_never_linked() {
    let tmp = tempfile::tempdir().unwrap();
    book(tmp.path(), "unbuilt", "[book]\ntitle = \"Unbuilt\"\n", None);
    let base = serve(vec![tmp.path().to_path_buf()], None);

    let (_, grid) = get(&base, "/_grid");
    assert!(grid.contains("Unbuilt"), "it is still listed");
    assert!(grid.contains("not built yet"), "and says why");
    assert!(
        !grid.contains("href=\"/0/unbuilt/\""),
        "a link into missing content is a 404 in a window with no back button"
    );
}

#[test]
fn a_filtered_title_that_is_nowhere_is_a_dead_card() {
    let tmp = tempfile::tempdir().unwrap();
    book(
        tmp.path(),
        "gpui",
        "[book]\ntitle = \"gpui\"\n",
        Some(("book", "<h1>x</h1>")),
    );
    let base = serve(
        vec![tmp.path().to_path_buf()],
        Some(vec!["gpui".into(), "Typo".into()]),
    );

    let (_, grid) = get(&base, "/_grid");
    assert!(
        grid.contains("Typo"),
        "the missing title is visible, not swallowed"
    );
    assert!(grid.contains("not found in any root"));
}

#[test]
fn two_same_titled_books_in_one_root_are_told_apart_by_directory() {
    let tmp = tempfile::tempdir().unwrap();
    book(
        tmp.path(),
        "gpui-a",
        "[book]\ntitle = \"gpui\"\n",
        Some(("book", "<h1>a</h1>")),
    );
    book(
        tmp.path(),
        "gpui-b",
        "[book]\ntitle = \"gpui\"\n",
        Some(("book", "<h1>b</h1>")),
    );
    book(
        tmp.path(),
        "solo",
        "[book]\ntitle = \"Solo\"\n",
        Some(("book", "<h1>s</h1>")),
    );
    let base = serve(vec![tmp.path().to_path_buf()], None);

    let (_, grid) = get(&base, "/_grid");
    assert!(grid.contains("href=\"/0/gpui-a/\"") && grid.contains("href=\"/0/gpui-b/\""));
    assert!(
        grid.contains(">gpui-a<") && grid.contains(">gpui-b<"),
        "colliding titles show their directory names"
    );
    assert!(!grid.contains(">solo<"), "an unambiguous book does not");
}

#[test]
fn a_directory_name_needing_encoding_still_resolves() {
    let tmp = tempfile::tempdir().unwrap();
    book(
        tmp.path(),
        "my book",
        "[book]\ntitle = \"Spaced\"\n",
        Some(("book", "<h1>spaced</h1>")),
    );
    let base = serve(vec![tmp.path().to_path_buf()], None);

    let (_, grid) = get(&base, "/_grid");
    assert!(
        grid.contains("href=\"/0/my%20book/\""),
        "percent-encoded, not raw: {grid}"
    );
    let (status, page) = get(&base, "/0/my%20book/");
    assert_eq!(status, 200);
    assert!(page.contains("spaced"));
}

#[test]
fn the_server_refuses_to_serve_outside_a_book() {
    let tmp = tempfile::tempdir().unwrap();
    book(
        tmp.path(),
        "gpui",
        "[book]\ntitle = \"gpui\"\n",
        Some(("book", "<h1>x</h1>")),
    );
    std::fs::write(tmp.path().join("secret.txt"), "nope").unwrap();
    let base = serve(vec![tmp.path().to_path_buf()], None);

    for path in [
        "/0/gpui/../../secret.txt",
        "/0/gpui/%2e%2e/%2e%2e/secret.txt",
        "/0/../secret.txt",
        "/9/gpui/",
        "/0/nosuchbook/",
    ] {
        let (status, _) = get(&base, path);
        assert_eq!(status, 404, "{path} must not be served");
    }
}

#[test]
fn a_cover_is_served_and_a_book_without_one_gets_a_generated_card() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = book(
        tmp.path(),
        "covered",
        "[book]\ntitle = \"Covered\"\n",
        Some(("book", "<h1>c</h1>")),
    );
    std::fs::write(dir.join("cover.svg"), "<svg/>").unwrap();
    book(
        tmp.path(),
        "bare",
        "[book]\ntitle = \"Bare\"\n",
        Some(("book", "<h1>b</h1>")),
    );
    let base = serve(vec![tmp.path().to_path_buf()], None);

    let (_, grid) = get(&base, "/_grid");
    assert!(
        grid.contains("src=\"/_cover/0/covered\""),
        "the real cover is used"
    );
    let (status, cover) = get(&base, "/_cover/0/covered");
    assert_eq!(status, 200);
    assert!(cover.contains("<svg/>"));

    assert!(
        grid.contains("class=\"cover\" viewBox"),
        "a book with no cover.<ext> gets a generated one"
    );
}

#[test]
fn the_first_root_wins_across_roots() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    book(
        a.path(),
        "mine",
        "[book]\ntitle = \"gpui\"\n",
        Some(("book", "<h1>mine</h1>")),
    );
    book(
        b.path(),
        "shipped",
        "[book]\ntitle = \"gpui\"\n",
        Some(("book", "<h1>shipped</h1>")),
    );
    let base = serve(vec![a.path().to_path_buf(), b.path().to_path_buf()], None);

    let (_, grid) = get(&base, "/_grid");
    assert!(grid.contains("href=\"/0/mine/\""));
    assert!(
        !grid.contains("href=\"/1/shipped/\""),
        "the later copy is shadowed"
    );
}

#[test]
fn book_query_opens_the_shell_straight_onto_that_book() {
    let tmp = tempfile::tempdir().unwrap();
    book(
        tmp.path(),
        "guide",
        "[book]\ntitle = \"User Guide\"\n",
        Some(("book", "<h1>g</h1>")),
    );
    book(
        tmp.path(),
        "other",
        "[book]\ntitle = \"Other\"\n",
        Some(("book", "<h1>o</h1>")),
    );
    let base = serve(vec![tmp.path().to_path_buf()], None);

    // Percent-encoded, because a title is display text and may contain spaces.
    let (status, shell) = get(&base, "/?book=User%20Guide");
    assert_eq!(status, 200);
    assert!(
        shell.contains("name=\"docframe\" src=\"/0/guide/\""),
        "the frame must start on the requested book: {shell}"
    );
    assert!(
        shell.contains("href=\"/_grid\""),
        "and the way back to the library is still in the bar"
    );
}

#[test]
fn an_unknown_or_unbuilt_book_query_falls_back_to_the_library() {
    let tmp = tempfile::tempdir().unwrap();
    book(
        tmp.path(),
        "guide",
        "[book]\ntitle = \"User Guide\"\n",
        Some(("book", "<h1>g</h1>")),
    );
    book(tmp.path(), "draft", "[book]\ntitle = \"Draft\"\n", None);
    let base = serve(vec![tmp.path().to_path_buf()], None);

    for query in ["/?book=Nonexistent", "/?book=Draft", "/?book="] {
        let (status, shell) = get(&base, query);
        assert_eq!(status, 200, "{query} must still serve the shell");
        assert!(
            shell.contains("src=\"/_grid\""),
            "{query} must land on the library, never on nothing"
        );
    }
}

#[test]
fn an_empty_library_says_how_to_point_it_at_books() {
    let tmp = tempfile::tempdir().unwrap();
    let base = serve(vec![tmp.path().to_path_buf()], None);
    let (_, grid) = get(&base, "/_grid");
    assert!(
        grid.contains("MD_LIBRARIAN_PATH"),
        "an empty page must be actionable"
    );
}
