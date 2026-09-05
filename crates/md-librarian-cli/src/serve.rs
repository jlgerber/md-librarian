//! The default path: serve the library and open a window on it.
//!
//! # Lifetime
//!
//! An application launches this **out of process** — no crate dependency, no
//! server inside the app, and the same code path standalone and embedded. But a
//! child is not killed when its parent dies on Linux; it is reparented to init.
//! So the host keeps the write end of a pipe and lets this process watch the
//! read end: when the host dies *for any reason*, including `SIGKILL`, the write
//! end closes, this process reads EOF, and it exits. Killing the child from the
//! host's shutdown path would cover only the graceful case.
//!
//! The pipe every host already has is **stdin**, which is why
//! `--exit-on-stdin-close` is the recommended wiring — `Stdio::piped()` plus
//! holding the returned `ChildStdin` is the whole host side, with no `unsafe`
//! and no libc. `--parent-pipe <FD>` exists for a host that needs stdin for
//! something else, but note that Rust sets `CLOEXEC` on descriptors it creates,
//! so such a host must arrange inheritance itself (`pre_exec` + `dup2`).

use std::path::PathBuf;

use clap::Args;

#[derive(Args, Debug)]
pub struct ServeArgs {
    /// A repository root; repeatable. Overrides MD_LIBRARIAN_PATH. Earlier wins.
    #[arg(long, value_name = "DIR")]
    root: Vec<PathBuf>,

    /// Show only these book titles; repeatable. A title no root provides is
    /// shown as a dead card rather than silently dropped.
    #[arg(long, value_name = "TITLE")]
    include: Vec<String>,

    /// Open straight onto this book instead of the library.
    ///
    /// Still inside the shell, so the bar and the way back to the library are
    /// there from the first frame. A title no root provides (or one that is not
    /// built) opens the library instead — a caller cannot know what is
    /// installed on this machine, so this is a preference, not a demand.
    #[arg(long, value_name = "TITLE")]
    book: Option<String>,

    /// Exit when stdin reaches EOF — i.e. when the process that spawned this
    /// one dies, however it dies. The recommended way for an application to
    /// tie the viewer's lifetime to its own.
    #[arg(long)]
    exit_on_stdin_close: bool,

    /// Read end of an inherited pipe to watch instead of stdin: exit on EOF.
    ///
    /// For a host that needs stdin for something else. That host must make the
    /// descriptor survive `exec` itself — Rust sets `CLOEXEC` on what it creates.
    #[arg(long, value_name = "FD")]
    parent_pipe: Option<std::os::fd::RawFd>,

    /// Serve without opening a window, printing the URL. For headless use and
    /// for driving the page from a real browser.
    #[arg(long)]
    no_window: bool,
}

pub fn run(cli: ServeArgs) -> anyhow::Result<()> {
    if cli.exit_on_stdin_close {
        watch_eof(std::io::stdin(), "stdin");
    }
    if let Some(fd) = cli.parent_pipe {
        // SAFETY: the descriptor was arranged by the parent for us to inherit,
        // and nothing else in this process touches it.
        let pipe = unsafe { <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(fd) };
        watch_eof(pipe, "pipe");
    }

    let roots = md_librarian::roots(&cli.root);
    let include = (!cli.include.is_empty()).then_some(cli.include.clone());
    let server = md_librarian_serve::start(roots, include)?;
    let url = match &cli.book {
        // `?book=` rather than an argument threaded into the server: the server
        // stays stateless, the choice is visible in the URL, and `--no-window`
        // prints something a browser can use directly.
        Some(title) => format!(
            "{}?book={}",
            server.url(),
            md_librarian_serve::shell::url_segment(title)
        ),
        None => server.url().to_string(),
    };

    if cli.no_window {
        println!("{url}");
        park();
    }

    match md_librarian_webview::WebWindow::open(
        md_librarian_webview::WebWindowOptions {
            title: "Books".into(),
            width: 1100,
            height: 800,
        },
        md_librarian_webview::WebContent::Url(url.clone()),
    ) {
        Ok(window) => {
            while window.is_open() {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
        Err(e) => {
            // No WebKitGTK on this machine. The server is already up and the
            // library is perfectly usable in any browser, so say where it is
            // rather than exiting on a failure the user can trivially work
            // around.
            tracing::error!(error = %e, "could not open a window; serving only");
            println!("{url}");
            park();
        }
    }
    Ok(())
}

/// Exit when the parent's end of `reader` closes.
fn watch_eof(mut reader: impl std::io::Read + Send + 'static, what: &'static str) {
    std::thread::Builder::new()
        .name("parent-watch".into())
        .spawn(move || {
            let mut buf = [0u8; 64];
            loop {
                match reader.read(&mut buf) {
                    // EOF: the parent is gone, whatever the reason.
                    Ok(0) => break,
                    // The parent wrote something. Nothing is defined to be sent
                    // over this pipe, so ignore it and keep watching.
                    Ok(_) => continue,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        tracing::warn!(error = %e, what, "parent pipe unreadable; exiting");
                        break;
                    }
                }
            }
            tracing::info!(what, "the parent closed its end; exiting");
            std::process::exit(0);
        })
        .expect("could not start the parent-watch thread");
}

/// Serve until killed (or until the parent-watch thread exits the process).
fn park() -> ! {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
