//! Eyeball example: open a floating help window with inline HTML, retitle
//! and navigate it after a beat, and exit when the user closes it.
//!
//!     cargo run -p md-librarian-webview --example help [url]

use md_librarian_webview::{WebContent, WebWindow, WebWindowOptions};

fn main() -> anyhow::Result<()> {
    let content = match std::env::args().nth(1) {
        Some(url) => WebContent::Url(url),
        None => WebContent::Html(
            "<html><body style='font-family:sans-serif;background:#222;color:#ddd'>\
             <h1>md-librarian-webview</h1><p>A floating WebKit window on the \
             crate's shared GTK thread. Close me to end the example.</p>\
             </body></html>"
                .into(),
        ),
    };
    let win = WebWindow::open(
        WebWindowOptions {
            title: "md-librarian-webview example".into(),
            ..Default::default()
        },
        content,
    )?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    win.set_title("md-librarian-webview example — retitled from the app thread");
    // Wait for the user to close the window.
    while win.is_open() {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Ok(())
}
