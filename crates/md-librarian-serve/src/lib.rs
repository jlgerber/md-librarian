//! Serves a library of mdbooks over a **loopback HTTP origin**.
//!
//! # Why a server, and not `file://`
//!
//! Two measured facts, not preferences. mdBook's full-text search index and its
//! relative links need an origin that permits `fetch`, which `file://` is
//! refused (usdlite hit this and runs its own `tiny_http` for the same reason).
//! And a `file://` page gets an *opaque* origin, so a shell cannot read the
//! iframe it hosts — `iframe.contentDocument` is `null`. One origin fixes both:
//! search works, and the bar can name the book you are reading.
//!
//! # Server only
//!
//! Nothing here opens a window. That lives in the `md-librarian` binary (the
//! `md-librarian-cli` crate), which is the only crate in the workspace that
//! links `wry`/`tao`/GTK — so the shell can be tested by fetching `/` with no
//! display, and a repository can be served to an ordinary browser or over
//! `ssh -L`.
//!
//! # The URL space
//!
//! ```text
//! /                       the shell: a persistent bar over an iframe
//! /_grid                  the library page (what the frame shows first)
//! /_cover/<root>/<dir>    a book's cover.<ext>
//! /<root>/<dir>/<path>    the book's rendered output
//! ```
//!
//! A book is addressed by **root index + directory name**, never by title:
//! titles are display text, may contain anything, and two books may share one.
//! The `_`-prefixed routes cannot collide with a book, because a root index is
//! always digits.

pub mod shell;

use std::path::{Path, PathBuf};
use std::thread;

use anyhow::{Context, Result};
use md_librarian::{book_at, library};
use tiny_http::{Header, Response, Server as HttpServer};

/// A running viewer server. Serving stops when the process exits.
pub struct Server {
    url: String,
    _thread: thread::JoinHandle<()>,
}

impl Server {
    /// The base URL, e.g. `http://127.0.0.1:41237/`.
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// Bind an ephemeral loopback port and serve `roots` until the process exits.
///
/// Bound to `127.0.0.1` deliberately: this is one user's documentation on one
/// machine, and a documentation server has no business being reachable from the
/// network.
pub fn start(roots: Vec<PathBuf>, include: Option<Vec<String>>) -> Result<Server> {
    let http = HttpServer::http("127.0.0.1:0")
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("could not bind a loopback port for the book server")?;
    let addr = http
        .server_addr()
        .to_ip()
        .context("the book server bound a non-IP address")?;
    let url = format!("http://{}:{}/", addr.ip(), addr.port());

    let root_count = roots.len();
    let thread = thread::Builder::new()
        .name("md-librarian-serve".into())
        .spawn(move || {
            for request in http.incoming_requests() {
                let response = route(&roots, include.as_deref(), request.url());
                if let Err(e) = request.respond(response) {
                    tracing::debug!(error = %e, "book server response failed");
                }
            }
        })
        .context("could not start the book server thread")?;

    tracing::info!(%url, roots = root_count, "book server listening");
    Ok(Server {
        url,
        _thread: thread,
    })
}

/// Resolve one request. Split out from the loop so it is a pure function of the
/// path, which is what makes the whole routing table testable.
fn route(
    roots: &[PathBuf],
    include: Option<&[String]>,
    raw: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    // Split the query off the path; mdbook's search appends one, and the shell
    // itself takes `?book=<title>` (see `--book`).
    let (path, query) = match raw.split_once('?') {
        Some((p, q)) => (p, q.split('#').next().unwrap_or("")),
        None => (raw.split('#').next().unwrap_or(""), ""),
    };
    let Some(segments) = decode_segments(path) else {
        return not_found("malformed path");
    };
    let parts: Vec<&str> = segments.iter().map(String::as_str).collect();

    match parts.as_slice() {
        // The shell and the grid are regenerated on every request: a book built
        // while the viewer is open appears on refresh, read-only roots are fine,
        // and there is no written page to go stale or to overwrite.
        [] => {
            let entries = library(roots, include);
            let initial = requested_book(&entries, query);
            generated(shell::shell_document(&entries, initial), "text/html")
        }
        ["_grid"] => generated(shell::grid_document(&library(roots, include)), "text/html"),

        ["_cover", root, dir] => match resolve(roots, root, dir).and_then(|b| b.cover) {
            Some(cover) => serve_file(&cover),
            None => not_found("no cover"),
        },

        [root, dir, rest @ ..] => match resolve(roots, root, dir) {
            Some(book) => match safe_join(&book.build_dir, rest) {
                Some(file) => serve_file(&file),
                None => not_found("bad path"),
            },
            None => not_found("no such book"),
        },

        _ => not_found("not found"),
    }
}

/// The book named by `?book=<title>`, if it is present *and* built.
///
/// Falls back to `None` — the library — rather than erroring or linking into
/// nothing: a caller passing a title that no root provides (a typo, a book not
/// installed on this machine) should land somewhere useful, and the library
/// page then shows what *is* here. It matches on the title, because that is the
/// book's identity and the only name a caller outside this process knows.
fn requested_book<'a>(
    entries: &'a [md_librarian::Entry],
    query: &str,
) -> Option<&'a md_librarian::Book> {
    let wanted = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("book="))?;
    let wanted = decode_component(wanted)?;
    let found = entries.iter().find_map(|e| match e {
        md_librarian::Entry::Book(b) if b.title == wanted && b.is_built() => Some(b),
        _ => None,
    });
    if found.is_none() {
        tracing::info!(title = %wanted, "no built book with that title; opening the library");
    }
    found
}

/// Percent-decode one query component, treating `+` as a space.
fn decode_component(raw: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(raw.len());
    let mut it = raw.bytes();
    while let Some(b) = it.next() {
        match b {
            b'%' => {
                let hex = |c: u8| (c as char).to_digit(16);
                bytes.push((hex(it.next()?)? * 16 + hex(it.next()?)?) as u8);
            }
            b'+' => bytes.push(b' '),
            b => bytes.push(b),
        }
    }
    String::from_utf8(bytes).ok()
}

/// `root` is an index into the search path; `dir` is one directory name.
fn resolve(roots: &[PathBuf], root: &str, dir: &str) -> Option<md_librarian::Book> {
    let index: usize = root.parse().ok()?;
    book_at(roots, index, dir)
}

/// Join the remaining segments onto a book's output directory.
///
/// Returns `None` for anything that tries to leave it. `..` is rejected outright
/// rather than normalised, and the result is confirmed to still sit under the
/// build directory after symlinks are resolved — belt and braces, because a
/// symlink inside a book's output could otherwise point anywhere.
fn safe_join(build_dir: &Path, rest: &[&str]) -> Option<PathBuf> {
    let mut path = build_dir.to_path_buf();
    for seg in rest {
        // Empty segments come from `//` and from the trailing slash of a
        // directory URL; both are meaningless here.
        if seg.is_empty() || *seg == "." {
            continue;
        }
        if *seg == ".." || seg.contains('/') || seg.contains('\\') || seg.contains('\0') {
            return None;
        }
        path.push(seg);
    }
    if path.is_dir() {
        path.push("index.html");
    }
    let (Ok(real), Ok(root)) = (path.canonicalize(), build_dir.canonicalize()) else {
        return None;
    };
    real.starts_with(root).then_some(real)
}

/// Percent-decode a path into its segments, decoding each one separately so an
/// encoded `%2F` cannot introduce a path separator.
fn decode_segments(path: &str) -> Option<Vec<String>> {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    if trimmed.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for raw in trimmed.split('/') {
        let mut bytes = Vec::with_capacity(raw.len());
        let mut chars = raw.bytes();
        while let Some(b) = chars.next() {
            if b == b'%' {
                let hi = chars.next()?;
                let lo = chars.next()?;
                let hex = |c: u8| (c as char).to_digit(16);
                bytes.push((hex(hi)? * 16 + hex(lo)?) as u8);
            } else {
                bytes.push(b);
            }
        }
        out.push(String::from_utf8(bytes).ok()?);
    }
    // A trailing slash leaves an empty last segment; `/` itself is handled above.
    if out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    Some(out)
}

/// A generated page: never cached, since it is regenerated per request and the
/// whole point is that a newly built book shows up on refresh.
fn generated(body: String, mime: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.into_bytes())
        .with_header(header("Content-Type", &format!("{mime}; charset=utf-8")))
        .with_header(header("Cache-Control", "no-store"))
}

fn serve_file(path: &Path) -> Response<std::io::Cursor<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Response::from_data(bytes).with_header(header("Content-Type", mime_for(path))),
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "book asset not readable");
            not_found("not found")
        }
    }
}

fn not_found(why: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(why.as_bytes().to_vec())
        .with_status_code(404)
        .with_header(header("Content-Type", "text/plain; charset=utf-8"))
}

fn header(name: &str, value: &str) -> Header {
    // Both are ASCII literals or paths we built, so this cannot fail in
    // practice; a bad header is still not worth killing the response over.
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .unwrap_or_else(|_| Header::from_bytes(&b"X-Invalid"[..], &b"1"[..]).unwrap())
}

/// Content types for what an mdbook actually emits.
fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}
