//! Regression harness for #47: does the window appear **before** its page
//! has styles?
//!
//!     cargo run -p md-librarian-webview --example first_paint           # slow stylesheet
//!     cargo run -p md-librarian-webview --example first_paint stalled   # stylesheet never arrives
//!     cargo run -p md-librarian-webview --example first_paint navigate  # open, then navigate at once
//!     cargo run -p md-librarian-webview --example first_paint drop-early # drop while still hidden
//!
//! ## Why it serves its own page
//!
//! "It flashes white" is not a property a `WebContent::Html` snippet can
//! reproduce on demand: an inline document loads in a few milliseconds, so
//! the gap between *window mapped* and *page painted* is a coin flip on
//! machine speed. Here a throwaway `TcpListener` serves an HTML page whose
//! only styling comes from a `<link>`ed stylesheet, and **sleeps
//! [`CSS_DELAY`] before answering the stylesheet request**. The unstyled
//! document is deliberately loud (white page, black serif text); the styled
//! one is dark. The FOUC becomes deterministic and, more to the point, wide
//! enough to *time*.
//!
//! ## What it measures
//!
//! Two clocks, both taken from outside the crate:
//!
//! - **mapped** — when the X server first lists a window with our title,
//!   polled from a thread started *before* `open`. That is the moment a
//!   user can see it, which is the only definition of "flash" that counts.
//! - **styles served** — when this process's own responder finished writing
//!   the stylesheet. The page cannot be styled before that instant,
//!   whatever WebKit does after it.
//!
//! PASS is `mapped >= styles served`: the window was never on screen while
//! the page was *guaranteed* unstyled. FAIL prints how long a user spent
//! looking at white.
//!
//! ## And why it takes pictures
//!
//! A timing number and a screenshot fail in different ways, and #47 is a
//! visual defect. The shots are **event-driven, not on a fixed schedule**,
//! because each one is aimed at an interval the *page* defines and a
//! timetable would only approximate: document served (unstyled content
//! exists), stylesheet about to go out (the last guaranteed-unstyled
//! instant), the window's first appearance, and settled.
//!
//! Each grabs the client window by X id — `import -window <id>`, ~60 ms —
//! rather than grabbing the root and cropping, which costs 1.3 s on a large
//! desktop and would land after the interval it was aimed at. When there is
//! no window to grab, that is not a failed screenshot: "the X server has no
//! such window" is the strongest form the passing evidence can take, and it
//! is recorded as such.
//!
//! The window's first second on screen gets a *burst* rather than a shot,
//! summarised as mean brightness. Deferring the map only fixes anything if
//! the window is painted when it appears, and a single grab cannot show
//! that: X hands back whatever is behind a window the compositor has not
//! filled yet, so consecutive shots land on the page and on the desktop
//! with equal ease. Sixteen frames 20 ms apart can carry the weaker but
//! honest claim the fix actually needs — none of them was white.
//!
//! PNGs land in `/tmp/md-librarian-first-paint/`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use md_librarian_webview::{WebContent, WebWindow, WebWindowOptions};

const TITLE: &str = "md-librarian-webview first_paint";
const SHOT_DIR: &str = "/tmp/md-librarian-first-paint";

/// How long the responder sits on the stylesheet request. Long enough that
/// a window mapped immediately is unmistakably white for a beat, and long
/// enough that neither the poller's resolution nor a screen grab's cost can
/// blur the verdict.
const CSS_DELAY: Duration = Duration::from_millis(800);

/// Milliseconds since the run started — every number this harness prints.
fn ms(t0: Instant) -> u64 {
    t0.elapsed().as_millis() as u64
}

// ---------------------------------------------------------------------------
// The page under test
// ---------------------------------------------------------------------------

/// Unstyled, this is a white page with black serif text — WebKit's
/// defaults, which is precisely the "unstyled content" half of the bug.
/// Styled, it is dark. A screenshot tells the two apart at a glance, which
/// is the whole reason the page looks like this.
const PAGE: &str = "<!doctype html><html><head><meta charset=utf-8>\
<title>md-librarian-webview first_paint</title>\
<link rel=stylesheet href=\"/slow.css\">\
</head><body><h1>STYLED</h1>\
<p>White page, black serif text = the stylesheet has not arrived and the \
user is looking at the flash this harness exists to catch.</p></body></html>";

const CSS: &str = "html,body{height:100%}\
body{background:#101828;color:#e6e8ee;margin:0;font-family:sans-serif;\
display:flex;flex-direction:column;align-items:center;justify-content:center}\
h1{font-size:72px;margin:0 0 8px;letter-spacing:.1em}\
p{max-width:30em;text-align:center;opacity:.7;font-size:20px}";

/// Serve `PAGE` immediately and `CSS` only after [`CSS_DELAY`] — or, in
/// `stalled` mode, never.
///
/// One thread per connection, because the whole trick is that the
/// stylesheet response blocks while the document response does not; a
/// single-threaded responder would serialise them and hide the FOUC.
/// Returns the port and a clock the responder stamps when the stylesheet
/// goes out.
fn serve(stalled: bool, t0: Instant) -> anyhow::Result<(u16, Arc<AtomicU64>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    // 0 means "not yet".
    let css_served = Arc::new(AtomicU64::new(0));
    let stamp = Arc::clone(&css_served);

    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { continue };
            let stamp = Arc::clone(&stamp);
            std::thread::spawn(move || {
                let _ = handle(conn, stalled, t0, &stamp);
            });
        }
    });
    Ok((port, css_served))
}

fn handle(
    mut conn: TcpStream,
    stalled: bool,
    t0: Instant,
    stamp: &AtomicU64,
) -> anyhow::Result<()> {
    let mut buf = [0u8; 2048];
    let n = conn.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    // Query string dropped: the `navigate` mode's second load asks for
    // `/?second` precisely so it is a *different* URL, and it still has to
    // reach the document handler.
    let target = req.split_whitespace().nth(1).unwrap_or("/");
    let path = target.split('?').next().unwrap_or("/").to_string();

    if path.starts_with("/slow.css") {
        if stalled {
            // Hold the connection and answer nothing. WebKit blocks
            // rendering on a pending stylesheet, so `load-changed:Finished`
            // never arrives and only the bounded fallback can produce a
            // window at all.
            println!(
                "  [server] t+{:>5}ms  GET {path} — stalling, no reply ever",
                ms(t0)
            );
            snap(t0, "stalled-css-requested");
            std::thread::sleep(Duration::from_secs(600));
            return Ok(());
        }
        println!(
            "  [server] t+{:>5}ms  GET {path} — sleeping {CSS_DELAY:?}",
            ms(t0)
        );
        std::thread::sleep(CSS_DELAY);
        // THE decisive shot: the last instant at which the page is
        // *guaranteed* to have no styles. A window visible here is the bug.
        snap(t0, "css-about-to-be-served");
        write_response(&mut conn, "text/css", CSS)?;
        // First serve only. The `navigate` mode loads the page a second
        // time, and the verdict is about the *first* display — letting the
        // second load restamp this would compare the window's map time
        // against a stylesheet served five seconds later and call a passing
        // run a failure.
        let _ = stamp.compare_exchange(0, ms(t0).max(1), Ordering::SeqCst, Ordering::SeqCst);
        println!("  [server] t+{:>5}ms  stylesheet served", ms(t0));
        return Ok(());
    }
    if path == "/" || path.starts_with("/index") {
        println!("  [server] t+{:>5}ms  GET {path}", ms(t0));
        write_response(&mut conn, "text/html", PAGE)?;
        // The document is out and the stylesheet is not; WebKit is about to
        // have something unstyled it *could* show.
        snap(t0, "document-served");
        return Ok(());
    }
    // favicon and friends
    conn.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
    Ok(())
}

fn write_response(conn: &mut TcpStream, mime: &str, body: &str) -> anyhow::Result<()> {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {mime}; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    conn.write_all(head.as_bytes())?;
    conn.write_all(body.as_bytes())?;
    conn.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The X server's opinion of when the window appeared
// ---------------------------------------------------------------------------

/// The X id and size of our window's **client** area, or `None` while it
/// does not exist on the server at all.
///
/// A hidden tao window is not merely unmapped: `with_visible(false)` means
/// the `GtkWindow` is never shown, GTK never realizes it, and there is no X
/// window for `xwininfo` to list. So mere presence in the tree is a sound
/// "the user can see it" test here, and no `Map State` probe is needed.
///
/// A reparenting WM (mutter) wraps the client in a same-titled **frame**,
/// and around a map it also throws up one or two same-titled scraps (a 1x1,
/// a ~120x160) that live for a few hundred milliseconds. So "the window"
/// has to be picked out of three to four candidates, and the two obvious
/// rules both pick wrong: smallest lands on a scrap and produces a
/// one-pixel screenshot that looks like a successful grab, largest lands on
/// the frame and puts a titlebar in the evidence.
///
/// Excluding the frame **by class** and taking the largest of the rest is
/// what survives both: the scraps are smaller than the client, and on a
/// non-reparenting WM there is no frame line to exclude in the first place.
fn client_window() -> Option<(String, u32, u32)> {
    let out = std::process::Command::new("xwininfo")
        .args(["-root", "-tree"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains(TITLE) && !l.contains("mutter-x11-frames"))
        .filter_map(|l| {
            let id = l.split_whitespace().next()?.to_string();
            let size = l.split_whitespace().nth_back(1)?;
            let (w, h) = size.split_once('x')?;
            let (h, _) = h.split_once('+')?;
            Some((id, w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .max_by_key(|(_, w, h)| u64::from(*w) * u64::from(*h))
}

/// Poll the server until our window exists, stamp when — and then sample it
/// hard for a second.
///
/// Started **before** `open`, so the stamp is not an artefact of when we
/// looked. The burst that follows answers the question the two clocks
/// cannot: deferring the map is only a fix if the window is *painted* when
/// it finally appears. If WebKit did no work while unmapped, the white
/// would simply have moved, and the timing verdict would still say PASS.
fn watch_for_map(t0: Instant, stop: Arc<AtomicBool>) -> Arc<AtomicU64> {
    let mapped = Arc::new(AtomicU64::new(0));
    let stamp = Arc::clone(&mapped);
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            if client_window().is_some() {
                stamp.store(ms(t0).max(1), Ordering::SeqCst);
                burst(t0);
                return;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    });
    mapped
}

/// A single grab's mean channel value, 0.0 (black) to 1.0 (white).
///
/// Brightness is a crude summary of an image and exactly the right one
/// here: the defect is *white*, and nothing else this page can show is
/// anywhere near 1.0 — the styled page means ~0.12, the desktop behind it
/// ~0.21, a bare GTK window on a dark theme ~0.14.
fn mean_brightness(path: &str) -> Option<f64> {
    let out = std::process::Command::new("identify")
        .args(["-format", "%[fx:mean]", path])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Sample the window as fast as `import` allows for the first second it
/// exists, and report whether any frame was white.
///
/// A single grab at the instant of map is not evidence either way: X will
/// happily hand back the content *behind* a window whose backing store the
/// compositor has not filled yet, so one shot lands on the page, the next
/// on the desktop, and neither is what the user saw. A run of them is
/// different — a white period long enough for a human to notice cannot hide
/// between samples 60 ms apart, so "no frame was white" is a claim the
/// series can actually support.
fn burst(t0: Instant) {
    let _ = std::fs::create_dir_all(SHOT_DIR);
    let Some((id, _, _)) = client_window() else {
        return;
    };
    let mut samples: Vec<(u64, f64)> = Vec::new();
    for i in 0..16 {
        let at = ms(t0);
        let path = format!("{SHOT_DIR}/mapped-{i:02}-t{at:05}ms.png");
        let ok = std::process::Command::new("import")
            .args(["-window", &id, "-silent", &path])
            .output()
            .is_ok_and(|o| o.status.success());
        if ok && let Some(m) = mean_brightness(&path) {
            samples.push((at, m));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if samples.is_empty() {
        println!("  [burst]  no readable frame in the first second after the map");
        return;
    }
    let series: Vec<String> = samples
        .iter()
        .map(|(at, m)| format!("{at}:{m:.2}"))
        .collect();
    println!("  [burst]  t+ms:brightness  {}", series.join("  "));
    let whitest = samples
        .iter()
        .cloned()
        .fold((0u64, 0.0f64), |a, b| if b.1 > a.1 { b } else { a });
    println!(
        "  [burst]  brightest frame {:.2} at t+{}ms  {}",
        whitest.1,
        whitest.0,
        if whitest.1 > 0.85 {
            "FAIL — a white frame reached the screen after the map"
        } else {
            "PASS — no white frame after the map"
        }
    );
}

/// Grab our window's client area — or record that there is no window to
/// grab, which in this harness is the *good* outcome and not a failure.
///
/// `import -window <id>` rather than a root grab plus a crop: 60 ms against
/// 1.3 s on a 7680x2160 desktop, which is the difference between a shot
/// that samples the interval it was aimed at and one that lands after it.
fn snap(t0: Instant, label: &str) {
    let at = ms(t0);
    let Some((id, w, h)) = client_window() else {
        println!("  [shot]   t+{at:>5}ms  {label}: NO WINDOW on the X server");
        return;
    };
    let _ = std::fs::create_dir_all(SHOT_DIR);
    let path = format!("{SHOT_DIR}/{label}-t{at:05}ms.png");
    // Retry briefly. An X window exists from the moment GTK realizes it,
    // which is a hair before the server makes it *viewable*, and `XGetImage`
    // on an unviewable window fails with EAGAIN. The `first-mapped` shot
    // aims at exactly that hair, so the first attempt losing the race is
    // the normal case and not an error — and the shot that eventually
    // lands, taken as soon as the content is readable at all, is the one
    // that answers "is the window painted when it appears?".
    let mut last = String::new();
    for attempt in 0..40 {
        let out = std::process::Command::new("import")
            .args(["-window", &id, "-silent", &path])
            .output();
        match out {
            Ok(o) if o.status.success() => {
                println!(
                    "  [shot]   t+{:>5}ms  {label}: {id} {w}x{h} -> {path}{}",
                    ms(t0),
                    if attempt > 0 {
                        format!("  (readable after {attempt} retries)")
                    } else {
                        String::new()
                    }
                );
                return;
            }
            Ok(o) => last = String::from_utf8_lossy(&o.stderr).trim().to_string(),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    println!("  [shot]   t+{at:>5}ms  {label}: import never succeeded: {last}");
}

// ---------------------------------------------------------------------------
// The runs
// ---------------------------------------------------------------------------

/// `first_paint drop-early`: drop the handle **during** the hidden
/// pre-paint phase, with the fallback timer still armed.
///
/// The hidden phase gave `Drop` a shape it never had before 0.23.2 — a
/// window that has been built, has never been on screen, and has a GLib
/// timeout scheduled to put it there. Three things must hold:
///
/// 1. the ack path still resolves, so `Drop` returns in microseconds rather
///    than sitting out its two-second bound;
/// 2. no window ever appears — in particular not when the timer fires, a
///    second after the handle is gone. A timer that resurrects a closed
///    window is the exact failure the disarm exists to prevent, and it is
///    invisible in every other mode because they all outlive the bound;
/// 3. nothing is left on the server afterwards.
///
/// It runs against the stalled responder so the load *cannot* finish on its
/// own: any window this produces came from the timer.
fn drop_early(t0: Instant, url: &str) -> anyhow::Result<()> {
    let win = WebWindow::open(
        WebWindowOptions {
            title: TITLE.into(),
            width: 900,
            height: 600,
        },
        WebContent::Url(url.to_string()),
    )?;
    println!(
        "  [host]   t+{:>5}ms  open() returned Ok, is_open={}",
        ms(t0),
        win.is_open()
    );
    // Comfortably inside the hidden phase and comfortably before the 1.5 s
    // fallback: the window exists, has never been shown, and its timer is
    // still pending.
    std::thread::sleep(Duration::from_millis(300));
    let hidden = client_window().is_none();
    println!(
        "  [host]   t+{:>5}ms  still hidden before the drop: {hidden}  {}",
        ms(t0),
        if hidden { "PASS" } else { "FAIL" }
    );

    let started = Instant::now();
    drop(win);
    let took = started.elapsed();
    println!(
        "  [host]   t+{:>5}ms  drop returned in {took:?}  {}",
        ms(t0),
        if took < Duration::from_millis(500) {
            "PASS"
        } else {
            "FAIL — the ack path stalled"
        }
    );

    // Watch straight through the fallback deadline and well past it.
    let mut appeared: Option<u64> = None;
    for _ in 0..150 {
        if appeared.is_none() && client_window().is_some() {
            appeared = Some(ms(t0));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    match appeared {
        None => println!(
            "  no window appeared through t+{}ms — the fallback did not resurrect \
             the closed window  PASS",
            ms(t0)
        ),
        Some(at) => println!("  a window appeared at t+{at}ms AFTER the handle was dropped  FAIL"),
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let drop_early_mode = mode == "drop-early";
    // `drop-early` uses the stalled responder so that any window it sees can
    // only have come from the fallback timer.
    let stalled = mode == "stalled" || drop_early_mode;
    let navigate_race = mode == "navigate";
    let label = if mode.is_empty() {
        "slow-css"
    } else {
        mode.as_str()
    };

    let t0 = Instant::now();
    let (port, css_served) = serve(stalled, t0)?;
    let url = format!("http://127.0.0.1:{port}/");
    println!("== first_paint [{label}] — {url} ==");

    if drop_early_mode {
        println!("  [host]   t+{:>5}ms  open()", ms(t0));
        return drop_early(t0, &url);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let mapped = watch_for_map(t0, Arc::clone(&stop));

    println!("  [host]   t+{:>5}ms  open()", ms(t0));
    let win = WebWindow::open(
        WebWindowOptions {
            title: TITLE.into(),
            width: 900,
            height: 600,
        },
        WebContent::Url(url.clone()),
    )?;
    let opened_at = ms(t0);
    println!(
        "  [host]   t+{opened_at:>5}ms  open() returned Ok, is_open={}",
        win.is_open()
    );

    if navigate_race {
        // The usdlite Help pattern at its worst: a second load starts while
        // the first is still in flight. It must not re-hide the window and
        // it must not show one twice.
        win.navigate(WebContent::Url(url.clone()));
        println!(
            "  [host]   t+{:>5}ms  navigate() immediately after open",
            ms(t0)
        );
    }
    // A `Focus` during the hidden phase. Focusing an invisible window is the
    // bug #43 fixed; forcing an *unpainted* one on screen early is #47's
    // variant of it. Neither may happen here.
    win.focus();
    println!(
        "  [host]   t+{:>5}ms  focus() during the hidden phase",
        ms(t0)
    );

    // Long enough for a stalled run to reach the fallback bound and for a
    // slow one to finish loading and settle.
    std::thread::sleep(Duration::from_secs(5));
    snap(t0, "settled");
    stop.store(true, Ordering::SeqCst);

    if navigate_race {
        // The other half of the navigate question. The page-load handler
        // stays connected for the life of the webview and fires on every
        // load, so a second navigation re-enters the reveal path on a
        // window that is already up. It must be a no-op: no re-hide, no
        // second map, no stolen focus. Watch the server's window list
        // across the whole of the next load and fail on any gap.
        let before = client_window().map(|(id, _, _)| id);
        win.navigate(WebContent::Url(format!("{url}?second")));
        println!(
            "  [host]   t+{:>5}ms  navigate() again, on a window already shown",
            ms(t0)
        );
        let mut vanished = false;
        for _ in 0..100 {
            if client_window().is_none() {
                vanished = true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let after = client_window().map(|(id, _, _)| id);
        snap(t0, "after-second-navigate");
        println!(
            "  second navigate: window {} {}",
            if vanished {
                "DISAPPEARED at some point".to_string()
            } else if before == after {
                format!("stayed up, same X id {}", after.clone().unwrap_or_default())
            } else {
                format!("was replaced: {before:?} -> {after:?}")
            },
            if !vanished && before == after && after.is_some() {
                "PASS"
            } else {
                "FAIL"
            }
        );
    }

    let mapped_at = mapped.load(Ordering::SeqCst);
    let css_at = css_served.load(Ordering::SeqCst);
    println!("  ---");
    println!("  open() returned at : t+{opened_at}ms");
    println!(
        "  window mapped at   : {}",
        if mapped_at == 0 {
            "never".into()
        } else {
            format!("t+{mapped_at}ms")
        }
    );
    println!(
        "  stylesheet served  : {}",
        if css_at == 0 {
            "never (stalled)".into()
        } else {
            format!("t+{css_at}ms")
        }
    );

    let verdict = if stalled {
        // No load can ever finish, so only the bounded fallback can produce
        // a window. Nothing on screen at all would be the worse bug.
        match mapped_at {
            0 => "FAIL — a stalled load left the user with no window at all".to_string(),
            t => format!(
                "PASS — the fallback produced a window at t+{t}ms ({}ms after open returned) \
                 despite a load that never finishes",
                t.saturating_sub(opened_at)
            ),
        }
    } else {
        match (mapped_at, css_at) {
            (0, _) => "FAIL — the window never appeared".to_string(),
            (_, 0) => "FAIL — the stylesheet was never served; rerun".to_string(),
            (m, c) if m >= c => {
                format!(
                    "PASS — mapped {}ms AFTER the stylesheet; no unstyled window",
                    m - c
                )
            }
            (m, c) => format!(
                "FAIL — mapped {}ms BEFORE the stylesheet: {}ms of visible unstyled page",
                c - m,
                c - m
            ),
        }
    };
    println!("  {verdict}");

    drop(win);
    std::thread::sleep(Duration::from_millis(500));
    let left = client_window();
    println!(
        "  windows left after drop: {}  {}",
        if left.is_some() { "one" } else { "none" },
        if left.is_some() { "FAIL" } else { "PASS" }
    );
    Ok(())
}
