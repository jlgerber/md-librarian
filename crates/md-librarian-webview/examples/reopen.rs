//! Regression harness for #43: `WebWindow::open` used to succeed **once per
//! process**. This exercises both shapes the issue reproduces in, in one
//! long-lived host process.
//!
//!     cargo run -p md-librarian-webview --example reopen
//!     cargo run -p md-librarian-webview --example reopen user   # you close it, then reopen
//!     cargo run -p md-librarian-webview --example reopen quit   # hard process exit
//!     env -u DISPLAY cargo run -p md-librarian-webview --example reopen degraded
//!     cargo run -p md-librarian-webview --example reopen churn 8   # thread cost
//!
//! Written like `close_teardown`: the process outlives every window and asks
//! the X server what is actually mapped, because a harness that exits masks
//! teardown failures (process exit destroys the client's windows regardless
//! of what the client sent).
//!
//! - **sequential** — open, drop the handle, open again. Before 0.23.0 the
//!   second `open` returned `Err("webview thread died before reporting
//!   ready")`, because it needed a `gtk::init` on a fresh thread and gtk-rs
//!   hard-panics on that.
//! - **simultaneous** — two live windows at once, independently navigable,
//!   independently closable.
//! - **user-closed** (`user` mode) — the actual Help flow from the issue: the
//!   *user* closes the window (`WindowEvent::CloseRequested`, not a handle
//!   drop) and the host opens it again. Needs a human to click the X, which
//!   is why it is a separate mode.
//!
//! PASS: every `open -> Ok`, the X window counts match `want`, and the last
//! ticks show none left.

use std::collections::BTreeSet;

use md_librarian_webview::{WebContent, WebWindow, WebWindowOptions};

const TAG: &str = "md-librarian-webview reopen";

/// The mapped X windows whose title carries our tag, straight from the
/// server — not from anything this process believes.
///
/// Keyed by **title**, not by window id: a reparenting WM (mutter here)
/// wraps each client window in a frame that carries the same title, so
/// `xwininfo -tree` lists two ids per window and an id count would double.
/// `close_teardown` never noticed because it only asks whether the list is
/// empty; this harness has to count.
fn windows() -> BTreeSet<String> {
    let out = std::process::Command::new("xwininfo")
        .args(["-root", "-tree"])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| l.contains(TAG))
            .filter_map(|l| {
                let (_, rest) = l.split_once('"')?;
                let (title, _) = rest.split_once('"')?;
                Some(title.to_string())
            })
            .collect(),
        Err(e) => BTreeSet::from([format!("<xwininfo unavailable: {e}>")]),
    }
}

/// Report the server's window list and whether it matches what we expect.
fn check(label: &str, want: usize) {
    let live = windows();
    let verdict = if live.len() == want { "PASS" } else { "FAIL" };
    let titles: Vec<&str> = live
        .iter()
        .map(|t| t.trim_start_matches(TAG).trim_start_matches(" — "))
        .collect();
    println!(
        "  {label:<34} want {want}, got {} {titles:?}  {verdict}",
        live.len()
    );
}

fn page(color: &str, heading: &str) -> WebContent {
    WebContent::Html(format!(
        "<body style='background:{color};color:#fff;font-family:sans-serif;margin:0;\
         display:flex;align-items:center;justify-content:center'><h1>{heading}</h1></body>"
    ))
}

fn open(title: &str, color: &str, heading: &str) -> anyhow::Result<WebWindow> {
    let win = WebWindow::open(
        WebWindowOptions {
            title: format!("{TAG} — {title}"),
            width: 640,
            height: 400,
        },
        page(color, heading),
    )?;
    println!("  open({title:?}) -> Ok, is_open={}", win.is_open());
    Ok(win)
}

fn settle() {
    std::thread::sleep(std::time::Duration::from_secs(2));
}

/// `reopen quit`: the **hard** shutdown path — quit the process with a
/// window still live and the GTK thread still pumping, dropping nothing.
///
/// The crate's own teardown never runs here. What must clean up is the
/// operating system: process exit closes the X connection, and the server
/// destroys every window the client owned; WebKitGTK's network/web child
/// processes lose the IPC socket to their UI process and self-terminate.
/// Check both from OUTSIDE, after this returns:
///
///     cargo run -p md-librarian-webview --example reopen quit
///     xwininfo -root -tree | grep 'md-librarian-webview reopen'   # expect nothing
///     pgrep -af WebKit                                     # expect nothing
fn hard_quit() -> anyhow::Result<()> {
    let win = open("hard-quit", "#333366", "quitting with this window live")?;
    settle();
    check("live at quit", 1);
    // Not a drop: the point is that NOTHING in this crate gets to run.
    std::mem::forget(win);
    println!("  exiting the process with the window still up...");
    std::process::exit(0);
}

/// `reopen user`: the Help flow the issue is actually about — the **user**
/// closes the window, and the host opens it again afterwards.
///
/// Distinct from the `drop` path in the main run: this goes through tao's
/// `connect_delete_event` -> `WindowEvent::CloseRequested`, not through
/// `Drop`. Before 0.23.0 the reopen at the end was the `Err` a host had to
/// fall back from (usdlite opened the external browser).
fn user_close_then_reopen() -> anyhow::Result<()> {
    let first = open("close-me", "#0055ff", "close me with the X button")?;
    settle();
    check("before the user closes it", 1);

    println!("  close the window (its X button) — waiting...");
    while first.is_open() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    println!("  is_open() == false without dropping the handle");
    settle();
    check("after the user's close", 0);
    // The handle outlives its window, as a host's would. Commands to it are
    // inert, not fatal.
    first.focus();
    first.navigate(page("#000000", "nobody sees this"));
    println!("  commands to the closed handle were ignored, not fatal");
    drop(first);

    let again = open("reopened", "#009933", "reopened after a user close")?;
    settle();
    check("after reopening", 1);
    drop(again);
    settle();
    check("after the final drop", 0);
    Ok(())
}

/// `reopen degraded`: no usable GTK. Run it as
/// `env -u DISPLAY cargo run -p md-librarian-webview --example reopen degraded`.
///
/// The GTK thread cannot start (gtk-rs panics rather than erroring), and the
/// panic stays on that thread. What must NOT happen is the host blocking on
/// a reply channel with no sender: every call has to come back with a clear
/// `Err`, and the calls after the first have to come back *immediately* —
/// GTK cannot be re-initialized on another thread in this process, so there
/// is nothing to retry and nothing to wait for.
fn degraded() -> anyhow::Result<()> {
    for attempt in 1..=3 {
        let started = std::time::Instant::now();
        let verdict = WebWindow::open(
            WebWindowOptions {
                title: format!("{TAG} — degraded"),
                width: 320,
                height: 200,
            },
            page("#000000", "unreachable"),
        );
        match verdict {
            Ok(win) => {
                println!(
                    "  attempt {attempt}: Ok in {:?} — a display IS available; \
                     rerun under `env -u DISPLAY`",
                    started.elapsed()
                );
                drop(win);
            }
            Err(e) => println!("  attempt {attempt}: Err in {:?} — {e}", started.elapsed()),
        }
    }
    Ok(())
}

/// This process's live thread count, from the kernel.
fn threads() -> usize {
    std::fs::read_dir("/proc/self/task")
        .map(|d| d.count())
        .unwrap_or(0)
}

/// The names the kernel has for them, tallied. Linux gives a thread its
/// creator's `comm` unless it sets its own, so everything the GTK thread
/// spawns without naming itself also shows up as `md-librarian-g…`.
fn thread_names() -> Vec<String> {
    let mut tally: std::collections::BTreeMap<String, usize> = Default::default();
    if let Ok(dir) = std::fs::read_dir("/proc/self/task") {
        for entry in dir.flatten() {
            let name = std::fs::read_to_string(entry.path().join("comm"))
                .unwrap_or_default()
                .trim()
                .to_string();
            *tally.entry(name).or_default() += 1;
        }
    }
    tally
        .into_iter()
        .map(|(n, c)| format!("{c}x {n}"))
        .collect()
}

/// `reopen churn [n]`: the thread-cost oracle — issue #45.
///
/// Two phases, because "how many threads does a host carry?" has two
/// answers and only one of them was ever the bug:
///
/// - **sequential** — open and close `n` windows one at a time. This is the
///   #45 measurement. Through 0.23.0 it climbed by exactly three per window
///   (`ebsiteDataStore`, `ReceiveQueue`, `VBlankMonitor`), retained after
///   the webview was destroyed, so the cost tracked the number of windows a
///   host had *ever* opened. 0.23.1 shares one `wry::WebContext` across
///   every webview and the line is flat.
/// - **peak** — `n` windows live at once, then all closed. What is left
///   after that is the honest answer to "does anything at all accumulate",
///   with the sequential phase's process-reuse taken out of the picture.
///
/// Measured rather than asserted in a doc comment, because the thing being
/// claimed is upstream behaviour we do not control.
fn churn() -> anyhow::Result<()> {
    let n: usize = std::env::args()
        .nth(2)
        .and_then(|a| a.parse().ok())
        .unwrap_or(6);
    let baseline = threads();
    println!("  before any window:            {baseline} threads");
    for i in 1..=n {
        let win = open(&format!("churn-{i}"), "#204060", &format!("churn {i}"))?;
        std::thread::sleep(std::time::Duration::from_millis(1200));
        let live = threads();
        drop(win);
        std::thread::sleep(std::time::Duration::from_millis(1200));
        println!(
            "  window {i:>2}: {live:>3} threads open -> {:>3} after close",
            threads()
        );
    }
    let settled = threads();
    println!("  settled at {settled} threads, zero windows open:");
    for line in thread_names() {
        println!("    {line}");
    }

    println!("  == peak: {n} windows live at once ==");
    let mut all = Vec::new();
    for i in 1..=n {
        all.push(open(&format!("peak-{i}"), "#402060", &format!("peak {i}"))?);
    }
    std::thread::sleep(std::time::Duration::from_millis(1200));
    println!("  {n} live at once:            {} threads", threads());
    all.clear();
    std::thread::sleep(std::time::Duration::from_millis(2400));
    let after_peak = threads();
    println!("  after closing all {n}:        {after_peak} threads");
    for line in thread_names() {
        println!("    {line}");
    }
    println!(
        "  net over the sequential settle: {} threads for {n} more windows opened",
        after_peak as i64 - settled as i64
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("quit") => return hard_quit(),
        Some("user") => return user_close_then_reopen(),
        Some("degraded") => return degraded(),
        Some("churn") => return churn(),
        _ => {}
    }

    println!("== sequential: open, drop, open again ==");
    let first = open("first", "#0055ff", "first")?;
    settle();
    check("after first open", 1);

    println!("  dropping the first handle...");
    let started = std::time::Instant::now();
    drop(first);
    println!("  drop returned in {:?}", started.elapsed());
    settle();
    // The teardown check from #42, now with the loop still running.
    check("after drop", 0);

    // THE #43 CASE. Before 0.23.0 this line was an `Err`.
    let second = open("second", "#009933", "reopened")?;
    settle();
    check("after reopen", 1);

    println!("== simultaneous: a second live window beside the first ==");
    // ALSO the #43 case: an open beside a live window, not after a close.
    let third = open("third", "#cc3300", "third")?;
    settle();
    check("two live windows", 2);

    println!("  driving them independently...");
    second.set_title(format!("{TAG} — second (retitled)"));
    second.navigate(page("#663399", "second, navigated"));
    third.navigate(page("#996600", "third, navigated"));
    third.focus();
    settle();
    check("after independent drives", 2);

    println!("  dropping ONLY the second handle...");
    drop(second);
    settle();
    check("third must survive", 1);
    println!("  third.is_open() = {}", third.is_open());

    println!("== teardown: the host outlives every window ==");
    drop(third);
    for tick in 1..=5 {
        settle();
        check(&format!("t+{}s after last drop", tick * 2), 0);
    }

    println!("== a fourth window, long after the loop went idle ==");
    let fourth = open("fourth", "#111111", "fourth")?;
    settle();
    check("after fourth open", 1);
    drop(fourth);
    settle();
    check("after fourth drop", 0);
    Ok(())
}
