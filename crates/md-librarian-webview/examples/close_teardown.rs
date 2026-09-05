//! Regression harness for the v0.22.3 teardown bug: does the window
//! actually GO AWAY when the host process keeps running?
//!
//!     cargo run -p md-librarian-webview --example close_teardown        # close it yourself
//!     cargo run -p md-librarian-webview --example close_teardown drop   # handle drop closes it
//!
//! The point is the 30 seconds AFTER the close. Every earlier check ended
//! with the process, and process exit destroys the client's X windows
//! regardless — which masked a `DestroyWindow` that was never flushed.
//! Here the process outlives the window (as a real host does) and the
//! harness prints the surviving X windows every two seconds.
//!
//! PASS: `windows: []` from the first tick after the close.
//! FAIL: the window keeps being listed — and is visibly still on screen,
//! ignoring its own close button.

use md_librarian_webview::{WebContent, WebWindow, WebWindowOptions};

const TITLE: &str = "md-librarian-webview close_teardown";

/// The client's X windows with our title, straight from the server.
fn windows() -> String {
    let out = std::process::Command::new("xwininfo")
        .args(["-root", "-tree"])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| l.contains(TITLE))
            .map(|l| l.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join(", "),
        Err(e) => format!("<xwininfo unavailable: {e}>"),
    }
}

fn main() -> anyhow::Result<()> {
    let drop_mode = std::env::args().nth(1).as_deref() == Some("drop");
    let win = WebWindow::open(
        WebWindowOptions {
            title: TITLE.into(),
            ..Default::default()
        },
        // A loud page: a blank window and a broken window look identical
        // in a screenshot otherwise.
        WebContent::Html("<body style='background:#ff0044;margin:0'></body>".into()),
    )?;
    std::thread::sleep(std::time::Duration::from_secs(3));
    println!("open  -> [{}]", windows());

    let mut win = Some(win);
    if drop_mode {
        println!("dropping the handle...");
        let started = std::time::Instant::now();
        drop(win.take());
        println!(
            "drop returned in {:?} (a hang here is the v0.22.1 bug)",
            started.elapsed()
        );
    } else {
        println!("close the window (its X button) — waiting...");
        while win.as_ref().is_some_and(WebWindow::is_open) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        println!("is_open() == false");
    }

    // The host lives on. THIS is the part that used to fail.
    for tick in 1..=15 {
        std::thread::sleep(std::time::Duration::from_secs(2));
        let live = windows();
        println!(
            "t+{:>2}s -> [{live}]{}",
            tick * 2,
            if live.is_empty() { "  PASS" } else { "  FAIL" }
        );
    }
    Ok(())
}
