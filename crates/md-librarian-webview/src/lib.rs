//! A floating **WebKit window** (wry) a gpui app can open and drive — the
//!
//! Extracted from gpui-yaams at v0.27.0-beta.3. Issue numbers in this crate
//! (`#43`, `#47`) and "Changed in 0.2x" notes refer to
//! <https://github.com/jlgerber/gpui-yaams>, where it was developed.
//! usdlite #1302 seam: render help/docs in our own process instead of
//! punting to an external browser.
//!
//! ## Why a floating window, not a pane
//!
//! On Linux wry renders through WebKitGTK, which needs GTK widgets and a
//! GTK-pumping event loop.
//!
//! Docking into a gpui window is **not impossible** — an earlier version of
//! this comment said wry's child path exists only on macOS/Windows, and that
//! was wrong. gpui's `Window` implements `HasWindowHandle`, and wry's
//! `build_as_child` accepts any such handle; its own docs say of Linux
//! *"This will create the webview as a child window of the parent window.
//! Only X11 is supported. This method won't work on Wayland."*
//!
//! Three things sink it anyway:
//!
//! 1. **Wayland.** On a Wayland session gpui hands back a
//!    `WaylandWindowHandle` and `build_as_child` **panics** on an
//!    unsupported handle. Wayland is the default session on most current
//!    Linux desktops, so this would be a feature that crashes rather than
//!    degrades for a large share of users.
//! 2. **A second event loop inside gpui's.** wry requires `gtk::init` on the
//!    calling thread and `gtk::main_iteration_do` interleaved with the host's
//!    event loop; gpui owns its loop and exposes no per-turn hook.
//! 3. **It is an overlay, not an element.** A child X11 window is a separate
//!    native surface composited by the X server: it does not clip to a gpui
//!    scroll container, does not participate in gpui's z-order, and does not
//!    move with layout. Even on X11 it could not live in a dock or tab that
//!    scrolls or gets covered — which is most of why one would want it docked.
//!
//! (3) outlives (1) and (2), so this stays a separate window even if wry and
//! gpui both improve. What IS well-trodden — it is exactly the tauri
//! arrangement — is a **tao window + wry webview on a GTK-owning thread**,
//! with tao pumping GTK. So this crate opens real OS windows owned by the app
//! process: "our own application" in the sense that matters (no browser
//! dependency, dies with the app), floating in the sense the issue allows.
//!
//! ## GTK attachment
//!
//! The webview is packed into the tao window's own `gtk::Box` with wry's
//! [`wry::WebViewBuilderExtUnix::build_gtk`]. The raw-handle
//! [`wry::WebViewBuilder::build`] path must NOT be used here: on X11 it
//! builds a second `gtk::Window` over the tao window's GdkWindow, which
//! never gets size-allocated, leaving the webview at 1x1 — a window that
//! loads its page perfectly and displays nothing. See the comment at the
//! call site.
//!
//! ## Visibility: built, then shown on first paint
//!
//! A window has **two** moments, not one, and only the first is a host's
//! business:
//!
//! 1. **Built** — [`WebWindow::open`] returns. The tao window and the
//!    webview exist and the page is loading, but nothing is on screen: the
//!    window is created with `with_visible(false)`, so GTK has not even
//!    realized it and there is no X window for a window manager, a taskbar
//!    or the user to find.
//! 2. **Shown** — the webview reports its first `load-changed → Finished`
//!    (wry's `PageLoadEvent::Finished`, WebKitGTK's own "document parsed,
//!    subresources fetched, stylesheets applied"), or **1.5 s** elapses,
//!    whichever comes first. The window is mapped and focused, once.
//!
//! Through v0.23.1 there was only moment 1, and the window mapped there:
//! the user got WebKit's white default for as long as the page took to
//! arrive, then a beat of unstyled HTML while its stylesheets loaded, then
//! the page (issue #47 — reported from usdlite's Help → Documentation).
//! Measured with `examples/first_paint`, whose responder delays a
//! `<link>`ed stylesheet by 800 ms, that was a **1.7 s** window with
//! nothing usable in it; with the wait, there is no window at all until
//! 35 ms *after* the stylesheet.
//!
//! And the window is *painted* when it appears — which is the half of the
//! claim a timing number cannot make, since a fix that only moved the white
//! would still pass on the clocks. Sampling the client area's mean
//! brightness every 20 ms from the moment it maps (1.0 is white; the styled
//! page is 0.12):
//!
//! ```text
//!   0.23.1, mapped t+73ms:
//!     0.21 0.21 0.21 0.21 0.21 0.14 1.00 1.00 1.00 1.00 1.00 1.00 1.00 1.00 1.00 0.12
//!                                   └────────── ~860ms of pure white ─────────┘
//!   0.23.2, mapped t+1178ms:
//!     0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12
//! ```
//!
//! That is also why **no webview background colour is set** — the obvious
//! companion tweak, and unnecessary here: WebKit renders while unmapped and
//! composites on map, so there is no residual gap for a colour to cover,
//! and any fixed choice would have been wrong for half the pages a host
//! shows.
//!
//! What this does and does not change:
//!
//! - **[`WebWindow::open`]'s contract is unchanged.** It still returns when
//!   the webview is *built*, and a build failure is still an `Err` from
//!   `open` rather than a silently missing window. It has never promised
//!   the window is on screen when it returns; now it is not.
//! - **[`is_open`](WebWindow::is_open) means built, not visible**, for the
//!   same reason: it answers "is there a window behind this handle", which
//!   is what a host asks it.
//! - **[`focus`](WebWindow::focus) during the hidden phase is dropped, not
//!   queued.** The pending reveal shows *and* focuses, so the intent is
//!   served either way; honouring it early would map an unpainted window,
//!   which is the defect by another route.
//! - **A later [`navigate`](WebWindow::navigate) cannot re-hide or
//!   re-show.** The reveal happens once per window. (It also does not need
//!   to: WebKitGTK keeps the old page on screen until the new one commits
//!   with styles, so navigating a window that is already up does not
//!   flash.)
//!
//! One measured cost, on the table because #45 published these numbers: the
//! hidden-then-shown lifecycle adds **one** thread to the process, a second
//! WebKitGTK `VBlankMonitor`, and only one — it appears with the first
//! window and never again, so the "flat in windows ever opened" property is
//! intact (`reopen churn 8`: 13 settled / 21 peak / 13 after, against
//! 12 / 20 / 12). Attributed by bisection rather than guessed: with the
//! page-load handler and the fallback timer in place but the window built
//! *visible*, the count is the old one. The likely mechanism is that the
//! webview widget is realized before its toplevel is on any monitor, so
//! WebKit binds a vblank monitor for the default one and another for the
//! real one when the window maps — which would make it a two-monitor
//! artefact, but that part is a hypothesis and the number is not.
//!
//! ## Threading contract: ONE process-wide GTK thread
//!
//! All GTK/webview work happens on a single thread — `md-librarian-gtk` —
//! started lazily on the first [`WebWindow::open`] and living for the rest
//! of the process. It owns `gtk::init`, one tao event loop
//! (`with_any_thread`), the GLib default main context, and **every** window
//! this crate has open. The loop never exits, so GTK is never
//! re-initialized.
//!
//! That is the whole reason [`WebWindow::open`] can be called more than
//! once (issue #43). GTK3 belongs to whichever thread called `gtk::init`,
//! and gtk-rs hard-panics on a second `init` from a second thread; the
//! previous design built a fresh event loop on a fresh thread per window,
//! so the second window of a process — reopened *or* simultaneous — always
//! tripped that guard. With one permanent thread there is no second
//! `gtk::init` to trip.
//!
//! A [`WebWindow`] handle is therefore cheap and inert: a window id, a
//! clone of the loop's [`tao::event_loop::EventLoopProxy`], and a shared
//! open-flag. Every method is a `send_event` that wakes the loop — no
//! polling, no locks on the hot path, no GTK types crossing a thread
//! boundary. Commands for a window that is already gone are dropped on the
//! loop (the window being gone IS the closed state).
//!
//! Two threads may call [`WebWindow::open`] at once: the lazily-started
//! thread lives behind a mutex, and the proxy is clonable.
//!
//! One consequence worth naming: tao spawns an X11 *device* thread per
//! event loop (`device::spawn`, parked in `XNextEvent` on its own X
//! connection) which it only stops when the loop exits. Since this crate
//! now builds exactly one loop that never exits, that is **one** such
//! thread for the life of the process rather than one per window — a fixed
//! cost instead of a per-window leak.
//!
//! ## Teardown
//!
//! Closing a window — the user's X (`WindowEvent::CloseRequested`) or
//! dropping the handle — destroys just that window: the webview is dropped
//! before the tao window (the order WebKitGTK wants, enforced by field
//! declaration order in `LiveWindow`), the entry leaves the loop's maps,
//! and the shared open-flag goes false. The loop keeps running and keeps
//! iterating GTK, which is what pushes the X `DestroyWindow` to the server.
//!
//! v0.22.3 had to drain the GTK main context and flush the display **by
//! hand** here, because `tao::Window::drop` only issues a *buffered* Xlib
//! `DestroyWindow` and the thing that normally flushes it — GDK's flush
//! inside a GLib main-context iteration — had just stopped when the
//! per-window loop returned. A host that outlived the window therefore saw
//! a zombie: mapped, unresponsive, `is_open()` already `false`. That hazard
//! is designed out rather than patched: the shared loop is still iterating
//! after the close, so the flush happens on its own (verified with the
//! `close_teardown` example, which keeps the process alive for 30 s past
//! the close and watches the server's window list).
//!
//! ## Process quit
//!
//! A permanent thread invites the question "what happens when the host
//! exits?", so: nothing a host has to arrange.
//!
//! **Graceful.** A host that drops its [`WebWindow`] handles during
//! shutdown gets the ordinary close path — a `Cmd::Close` to the loop, the
//! bounded ack-wait in [`WebWindow`]'s `Drop` — while the loop is still
//! pumping GTK. That is the only shape in which the crate's own teardown
//! runs, and it is the shape a host should prefer, because it is the one
//! that is *observable*: `Drop` returns after the window is really gone.
//!
//! **Hard.** If the process simply exits — handles leaked, `exit()` called,
//! the GTK thread killed mid-park — the window still goes away, because the
//! cleanup is the operating system's, not ours. The GTK thread is
//! **detached** (nothing joins it) and dies with the process; process exit
//! closes the X connection, and the server destroys every window that
//! client owned. This is the same mechanism the #42 teardown comments call
//! out, read the other way round: it is precisely *because* process exit
//! destroys the client's windows that the zombie-window hazard existed only
//! while the host **outlived** the window. WebKitGTK's separate network and
//! web processes are children over an IPC socket to the UI process, and
//! self-terminate when it closes, so they do not orphan either. (Verified:
//! `cargo run -p md-librarian-webview --example reopen quit` exits with a live
//! window and leaves neither an X window nor a `WebKit*Process` behind.)
//!
//! ## Thread cost
//!
//! **Thread count does not return to baseline, but it does not grow with
//! use** (issue #45, fixed in 0.23.1). Measured with `cargo run -p
//! md-librarian-webview --example reopen churn 8`, which opens and closes eight
//! windows one at a time and then opens eight *at once* and closes them:
//!
//! ```text
//!                               0.23.0   0.23.1   0.23.2
//!   before any window                1        1        1
//!   after 8 opened + closed         33       12       13
//!   8 live at once                  57       20       21
//!   after those 8 closed            53       12       13
//! ```
//!
//! 0.23.2's extra one is the second `VBlankMonitor` the hidden-then-shown
//! lifecycle costs — fixed, not per-window; see "Visibility" above.
//!
//! - **Fixed, ours.** The GTK thread does not stop when the last window
//!   closes — it parks in `gtk::main_iteration_do` waiting for the next
//!   `send_event` — and tao's X11 device thread parks beside it in
//!   `XNextEvent`. With GLib's own singletons (`gmain`, `gdbus`,
//!   `pool-spawner`, `PressureMonitor`) that is ~9 threads, whatever the
//!   host does. This part is *cheaper* than what it replaced: tao only
//!   stops its device thread when its loop exits, so one loop per window
//!   leaked one device thread **per window**.
//! - **Fixed, WebKitGTK's.** One `ebsiteDataStore`, one `ReceiveQueue`,
//!   one or two `VBlankMonitor` — for the *process*, not per window,
//!   because every webview shares one `wry::WebContext` (built once by the
//!   GTK thread; see `create_window`).
//! - **Transient, per live window.** An *open* window costs roughly one
//!   more thread while it is open — eight at once measured 20, and gave
//!   them all back on close. So the cost tracks how many windows a host
//!   has open, and nothing tracks how many it has ever opened.
//!
//! Through 0.23.0 that third bullet was the second one. wry builds a fresh
//! `wry::WebContext` per webview unless told otherwise, and a fresh WebKit
//! context means a fresh `WebsiteDataManager` and a fresh web process,
//! whose three threads (`ebsiteDataStore`, `ReceiveQueue`,
//! `VBlankMonitor`) WebKitGTK does **not** give back when the webview is
//! destroyed. That cost **+3 per window ever opened** — 33 threads after
//! eight reopens, 53 after sixteen. It was upstream behaviour rather than
//! something the shared-thread design introduced, but it only became
//! reachable in 0.23.0, when reopening stopped being impossible; sharing
//! one context makes it a constant.
//!
//! [`focus`](WebWindow::focus) + [`navigate`](WebWindow::navigate) on a
//! kept handle is therefore no longer a thread-cost workaround, only a UX
//! one — no window flash, scroll position preserved. Hosts may reopen as
//! freely as their flow wants.
//!
//! ```no_run
//! use md_librarian_webview::{WebContent, WebWindow, WebWindowOptions};
//!
//! let help = WebWindow::open(
//!     WebWindowOptions { title: "usdlite help".into(), width: 1024, height: 768 },
//!     WebContent::Url("https://example.com/docs/index.html".into()),
//! )?;
//! // Later, e.g. from a menu action:
//! help.navigate(WebContent::Html("<h1>Context help</h1>".into()));
//! // Dropping `help` closes that window; opening another one afterwards
//! // (or beside it) works.
//! # anyhow::Ok(())
//! ```

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::platform::unix::{EventLoopBuilderExtUnix, WindowExtUnix};
use tao::window::WindowBuilder;
use wry::{WebViewBuilder, WebViewBuilderExtUnix};

/// How long [`WebWindow::drop`] waits for the GTK thread to acknowledge the
/// close before giving up. The loop only has to wake and drop two objects,
/// so this is orders of magnitude of slack — its job is to bound a wedged
/// GTK thread, which must never hang the host's shutdown (the v0.22.1 bug,
/// where `Drop` joined a thread that could not exit).
const CLOSE_ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a freshly built window stays hidden waiting for its first page
/// load to finish before being shown regardless (issue #47).
///
/// The bound is the whole reason this is safe: "show it when the page is
/// ready" alone would mean a page that never becomes ready — a stalled
/// stylesheet, an unreachable host, a URL that 404s into a hanging
/// keep-alive — produces **no window at all**, which is a worse bug than
/// the flash. 1.5 s is chosen against what it is racing: a `file://` doc
/// or an inline `Html` snippet finishes in tens of milliseconds and never
/// gets near it, while a genuinely slow page reaches the bound and shows
/// itself mid-load, exactly as v0.23.1 always did. So the fallback is only
/// ever reached in the cases where the old behaviour was the best
/// available anyway.
const FIRST_LOAD_TIMEOUT: Duration = Duration::from_millis(1500);

/// What the window shows. `Url` is anything WebKit can load (`https://…`,
/// `file:///…`); `Html` is an inline document (help snippets, generated
/// pages).
#[derive(Clone, Debug)]
pub enum WebContent {
    Url(String),
    Html(String),
}

/// Initial window chrome/size. The window is a normal, resizable,
/// user-closable OS window.
#[derive(Clone, Debug)]
pub struct WebWindowOptions {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl Default for WebWindowOptions {
    fn default() -> Self {
        Self {
            title: "Help".into(),
            width: 1024,
            height: 768,
        }
    }
}

/// Our own window id, handed out before the window exists.
///
/// It cannot be `tao::window::WindowId`: the handle needs an id the moment
/// [`WebWindow::open`] is called, and tao's is only knowable once the loop
/// has built the window. The loop keeps a second map from tao's id to this
/// one so `WindowEvent`s can be routed back.
type WindowKey = u64;

static NEXT_WINDOW_KEY: AtomicU64 = AtomicU64::new(1);

/// A window living on the GTK thread.
///
/// **Field order is load-bearing**: struct fields drop in declaration
/// order, so the webview is torn down before the tao window that hosts it —
/// the order WebKitGTK wants, and what the old per-window thread spelled
/// out with two explicit `drop`s.
struct LiveWindow {
    webview: wry::WebView,
    window: tao::window::Window,
    open: Arc<AtomicBool>,
    /// `false` between "built" and "on screen" — the hidden pre-paint phase
    /// (issue #47). [`reveal_window`] flips it once and only once; every
    /// later page load leaves it alone, so a `navigate` cannot re-run the
    /// show/focus.
    shown: bool,
    /// The fallback timer's off switch, shared with the closure
    /// [`create_window`] hands to `glib::timeout_add_local_once`.
    ///
    /// A one-shot GLib source cannot be un-scheduled without risking
    /// `g_source_remove` on a source that has already fired, so the timer
    /// is *neutered* instead of cancelled: reveal and close both clear this
    /// flag, and the closure checks it before sending anything. The
    /// [`reveal_window`] key lookup would make a stray fire harmless
    /// anyway; the flag is what makes it not even happen.
    reveal_armed: Rc<Cell<bool>>,
}

/// Everything the shared loop owns across events — GTK-thread state, and
/// therefore `!Send` by construction, which is exactly right: it never
/// leaves [`gtk_thread_main`].
struct LoopState {
    windows: HashMap<WindowKey, LiveWindow>,
    /// tao's `WindowId` is only knowable once the window is built, so
    /// routing a `WindowEvent` back to a [`WindowKey`] needs this second map.
    keys_by_tao_id: HashMap<tao::window::WindowId, WindowKey>,
    /// ONE WebKit context for every webview this process builds — the
    /// process-wide thread cost of the crate depends on it (issue #45; see
    /// the module docs and [`create_window`]).
    ///
    /// Built lazily on the first create rather than at loop start, because
    /// `wry::WebContext::new` constructs GObjects and wants GTK already
    /// initialized. It is, by the time the run callback executes — but only
    /// building it *there* makes that ordering the code's rather than a
    /// comment's.
    web_context: Option<wry::WebContext>,
    /// A proxy back to this very loop, so GTK-thread callbacks that cannot
    /// borrow this state — the webview's page-load handler and the
    /// first-paint fallback timer — can still reach it.
    ///
    /// Both fire *inside* a GTK main iteration, i.e. underneath the
    /// `run_return` callback that owns `&mut LoopState`, so neither can
    /// touch a window directly. Posting a [`LoopEvent::Reveal`] hands the
    /// work back to the one place that legitimately holds the state.
    /// (`EventLoopWindowTarget` has no `create_proxy`; only the
    /// `EventLoop` does, which is why this is captured at loop start
    /// rather than made on demand in `create_window`.)
    proxy: EventLoopProxy<LoopEvent>,
}

impl LoopState {
    fn new(proxy: EventLoopProxy<LoopEvent>) -> Self {
        Self {
            windows: HashMap::new(),
            keys_by_tao_id: HashMap::new(),
            web_context: None,
            proxy,
        }
    }
}

/// Commands the handle sends to one window on the shared loop.
enum Cmd {
    Navigate(WebContent),
    SetTitle(String),
    /// Bring the (possibly buried) window to the front.
    Focus,
    /// Destroy this window and acknowledge, so `Drop` knows when the window
    /// is really gone without joining anything.
    Close {
        ack: mpsc::Sender<()>,
    },
}

/// Everything a [`WebWindow`] asks the shared loop to do — plus the one
/// thing the loop asks *itself*.
enum LoopEvent {
    Create(CreateRequest),
    Command {
        key: WindowKey,
        cmd: Cmd,
    },
    /// Take this window out of its hidden pre-paint phase (issue #47).
    ///
    /// Not a [`Cmd`]: no handle can send it and no host can ask for it. It
    /// is posted by the window's own page-load handler when the first load
    /// finishes, and by the [`FIRST_LOAD_TIMEOUT`] fallback if that never
    /// happens — two racing senders, whichever arrives first wins, and
    /// [`reveal_window`] makes the loser a no-op.
    Reveal {
        key: WindowKey,
    },
}

/// A build request, carrying its own reply channel: [`WebWindow::open`]
/// blocks on it so a build failure surfaces *there* rather than as a
/// silently missing window.
struct CreateRequest {
    key: WindowKey,
    opts: WebWindowOptions,
    content: WebContent,
    open: Arc<AtomicBool>,
    reply: mpsc::Sender<anyhow::Result<()>>,
}

/// The process-wide GTK thread, as seen from any other thread.
enum GtkThread {
    Running(EventLoopProxy<LoopEvent>),
    /// It could not be started (no display, no GTK, …). Remembering *why*
    /// is what keeps a later `open` from blocking forever on a reply that
    /// nobody will ever send.
    Failed(String),
}

/// Started on first use, never restarted. The mutex is what makes two
/// simultaneous first calls to [`WebWindow::open`] safe: the loser blocks
/// until the winner has recorded the verdict, then reads it.
static GTK_THREAD: Mutex<Option<GtkThread>> = Mutex::new(None);

/// A proxy to the process-wide GTK thread's event loop, starting the thread
/// if this is the first call.
fn gtk_proxy() -> anyhow::Result<EventLoopProxy<LoopEvent>> {
    let mut slot = match GTK_THREAD.lock() {
        Ok(slot) => slot,
        // Only reachable if a panic unwound while this lock was held, which
        // means the verdict below never got recorded. Report rather than
        // propagate the panic.
        Err(_) => anyhow::bail!("the md-librarian-webview GTK thread registry is poisoned"),
    };
    match &*slot {
        Some(GtkThread::Running(proxy)) => return Ok(proxy.clone()),
        Some(GtkThread::Failed(why)) => anyhow::bail!("{why}"),
        None => {}
    }

    // The proxy can only be made from the event loop, which can only be made
    // on the thread that will own GTK — so it comes back over a channel.
    let (ready_tx, ready_rx) = mpsc::channel::<Result<EventLoopProxy<LoopEvent>, String>>();
    std::thread::Builder::new()
        .name("md-librarian-gtk".into())
        .spawn(move || {
            gtk_thread_main(&ready_tx);
        })?;
    // Held across the recv on purpose: the lock IS the "only one thread ever
    // starts GTK" guarantee, and the wait is bounded by the loop's startup.
    let verdict = match ready_rx.recv() {
        Ok(v) => v,
        // The sender was dropped without sending: the thread unwound during
        // `gtk::init`/loop construction (gtk-rs panics rather than erroring,
        // e.g. with no `$DISPLAY`). The panic stays on that thread; the host
        // gets an `Err`, now and for every later call.
        Err(_) => Err("the md-librarian-webview GTK thread died during initialization".to_string()),
    };
    match verdict {
        Ok(proxy) => {
            *slot = Some(GtkThread::Running(proxy.clone()));
            Ok(proxy)
        }
        Err(why) => {
            *slot = Some(GtkThread::Failed(why.clone()));
            Err(anyhow::anyhow!(why))
        }
    }
}

/// Record that the GTK thread is unusable, so subsequent [`WebWindow::open`]
/// calls fail fast instead of waiting on a reply channel with no sender.
///
/// Reached when the loop stops answering *after* a successful start — i.e.
/// it panicked inside the run callback. Nothing restarts it: GTK cannot be
/// re-initialized on a fresh thread in this process.
fn mark_gtk_thread_failed(why: &str) {
    if let Ok(mut slot) = GTK_THREAD.lock() {
        *slot = Some(GtkThread::Failed(why.to_string()));
    }
}

/// The body of the process-wide GTK thread. Returns only if the loop could
/// not be built (by unwinding) — the loop itself never exits.
fn gtk_thread_main(ready: &mpsc::Sender<Result<EventLoopProxy<LoopEvent>, String>>) {
    // `with_any_thread`: tao asserts main-thread by default; this loop lives
    // here for the life of the process, GTK included.
    let mut event_loop = EventLoopBuilder::<LoopEvent>::with_user_event()
        .with_any_thread(true)
        .build();
    // Ignore a send failure rather than returning: a thread that has already
    // run `gtk::init` must never die, because it would die owning the GLib
    // default main context and break `g_main_context_acquire` for the whole
    // process. (Unreachable in practice — `gtk_proxy` always waits.)
    let _ = ready.send(Ok(event_loop.create_proxy()));

    let mut state = LoopState::new(event_loop.create_proxy());

    // `run_return`, not `run`: `run` diverges and exits the PROCESS on loop
    // end on some platforms. This loop is permanent either way, but keeping
    // `run_return` means an accidental exit surfaces as a returned thread
    // rather than as the host process vanishing.
    event_loop.run_return(|event, target, control_flow| {
        // Unconditional, and safe to be so: nothing in this crate ever sets
        // `Exit` any more, so there is no armed exit to clobber. (v0.22.1
        // needed `if !matches!(*control_flow, ControlFlow::Exit)` here,
        // because a per-window loop DID exit and this reset ate the request:
        // the window's X did nothing and `Drop`'s join hung the host.)
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(LoopEvent::Create(req)) => {
                let CreateRequest {
                    key,
                    opts,
                    content,
                    open,
                    reply,
                } = req;
                let verdict = create_window(target, key, &opts, &content, &open, &mut state);
                let _ = reply.send(verdict);
            }
            Event::UserEvent(LoopEvent::Command { key, cmd }) => match cmd {
                Cmd::Navigate(content) => {
                    if let Some(live) = state.windows.get(&key) {
                        match content {
                            WebContent::Url(u) => {
                                let _ = live.webview.load_url(&u);
                            }
                            WebContent::Html(h) => {
                                let _ = live.webview.load_html(&h);
                            }
                        }
                    }
                }
                Cmd::SetTitle(t) => {
                    if let Some(live) = state.windows.get(&key) {
                        live.window.set_title(&t);
                    }
                }
                Cmd::Focus => {
                    if let Some(live) = state.windows.get(&key) {
                        // Only for a window the user can already see.
                        //
                        // During the hidden pre-paint phase this is
                        // deliberately dropped rather than queued: the
                        // pending reveal shows AND focuses in a moment, so
                        // the user's intent is served either way, and
                        // honouring it here would map an unpainted window
                        // early — which is the entire defect (#47), reached
                        // by a different door. #43 fixed focusing a window
                        // that is not there; this is the variant where the
                        // window is there but has nothing on it yet.
                        if live.shown {
                            live.window.set_visible(true);
                            live.window.set_focus();
                        }
                    }
                }
                Cmd::Close { ack } => {
                    close_window(key, &mut state);
                    // Acknowledge even when there was nothing to close (the
                    // user got there first): `Drop` must not wait out its
                    // timeout on an already-closed window.
                    let _ = ack.send(());
                }
            },
            Event::UserEvent(LoopEvent::Reveal { key }) => reveal_window(key, &mut state),
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if let Some(key) = state.keys_by_tao_id.get(&window_id).copied() {
                    close_window(key, &mut state);
                }
            }
            _ => {}
        }
    });
}

/// Build one window + webview on the GTK thread and register it —
/// **hidden**, with two racing senders armed to show it.
fn create_window(
    target: &EventLoopWindowTarget<LoopEvent>,
    key: WindowKey,
    opts: &WebWindowOptions,
    content: &WebContent,
    open: &Arc<AtomicBool>,
    state: &mut LoopState,
) -> anyhow::Result<()> {
    // `WindowBuilder::build` wants an `&EventLoopWindowTarget`, which the
    // run callback is handed — which is why windows can be created from
    // inside a loop that is already running, and therefore why one loop can
    // serve every window in the process.
    let window = WindowBuilder::new()
        .with_title(&opts.title)
        .with_inner_size(tao::dpi::LogicalSize::new(
            f64::from(opts.width),
            f64::from(opts.height),
        ))
        // Built HIDDEN — issue #47. A tao window otherwise maps the instant
        // it is built, which is long before WebKit has painted: the user
        // gets WebKit's white default, then a beat of unstyled HTML as the
        // document arrives ahead of its stylesheets, and only then the
        // page. Nothing downstream can fix that, because by then the window
        // is already on screen. `reveal_window` shows it (see below).
        //
        // `with_visible(false)` is a tao-level hide, not a GTK one: it
        // means the `GtkWindow` is never `show_all`n, so GTK does not even
        // realize it and no X window exists yet. wry still `show_all`s the
        // *webview widget* inside it (its own `visible` attribute defaults
        // to true), so the child is ready to be displayed the moment the
        // toplevel is.
        .with_visible(false)
        .build(target)
        .map_err(|e| anyhow::anyhow!("window build: {e}"))?;
    // `new_with_web_context`, never `WebViewBuilder::new()` — issue #45.
    //
    // With `new()` the attributes carry no context, so wry's `new_gtk` builds
    // one for itself (`default_context = Default::default()`) — a fresh
    // `webkit2gtk::WebContext` per webview, each with its own
    // `WebsiteDataManager` and its own web process. WebKitGTK never gives the
    // resulting threads back when the webview is destroyed, so through
    // 0.23.0 a host carried three of them (`ebsiteDataStore`, `ReceiveQueue`,
    // `VBlankMonitor`) per window it had **ever** opened. One shared context
    // turns that into a fixed cost: WebKit reuses the pool's web process, and
    // the churn line goes flat. Measured, not assumed — see the thread
    // accounting in the module docs and `examples/reopen.rs`'s `churn` mode.
    let builder = WebViewBuilder::new_with_web_context(
        state
            .web_context
            .get_or_insert_with(|| wry::WebContext::new(None)),
    );
    let builder = match content {
        WebContent::Url(u) => builder.with_url(u),
        WebContent::Html(h) => builder.with_html(h),
    };
    // The first of the two senders that can end the hidden phase (#47).
    //
    // On the GTK path wry wires this straight to webkit2gtk's
    // `load-changed` signal, so `Finished` is WebKitGTK's own
    // `WEBKIT_LOAD_FINISHED` — the document parsed, its subresources
    // fetched, its stylesheets applied. That is the earliest instant at
    // which showing the window cannot show unstyled content, which is
    // exactly what we want to wait for and no longer.
    //
    // The handler outlives the first load and fires again on every
    // `navigate`; `reveal_window` is idempotent, so those are no-ops rather
    // than a second show. It cannot touch the window directly — it runs
    // inside a GTK main iteration, underneath the callback that owns
    // `LoopState` — hence the round trip through the proxy.
    let builder = {
        let proxy = state.proxy.clone();
        builder.with_on_page_load_handler(move |event, _url| {
            if matches!(event, wry::PageLoadEvent::Finished) {
                let _ = proxy.send_event(LoopEvent::Reveal { key });
            }
        })
    };
    // Attach into tao's OWN gtk container — never `build(&window)`.
    //
    // `WebViewBuilder::build` takes a raw window handle, and its X11
    // implementation wraps that XID in a *foreign* GdkWindow, hangs a SECOND
    // `gtk::Window` off it, and packs the webview into that window's vbox.
    // For a non-GTK host (winit) the XID really is foreign and the trick
    // works. For tao it does not: a tao window IS a `gtk::ApplicationWindow`,
    // so GDK already knows the XID and
    // `gdk_x11_window_foreign_new_for_display` hands back the EXISTING
    // GdkWindow — leaving two GtkWindows over one GdkWindow, of which only
    // tao's is wired to receive configure events. wry's parasite window is
    // therefore never size-allocated, so the webview widget sits at GTK's
    // unallocated 1x1 at (-1,-1) forever: the window has the right title and
    // size, the page LOADS (network process fetches, web process stays up,
    // no errors anywhere), and the client area shows nothing but the GTK
    // background. That is the whole of the "blank webview" defect — it was
    // never the NVIDIA DMABUF renderer, which is why disabling that did not
    // help.
    //
    // `build_gtk` into `default_vbox` is the tauri arrangement: one
    // GtkWindow, `pack_start(webview, expand, fill, 0)`, so the webview is
    // allocated the client area and follows window resizes without the
    // manual `set_bounds` the X11 path needs.
    let vbox = window
        .default_vbox()
        // Only reachable via `WindowBuilderExtUnix::with_default_vbox(false)`,
        // which this crate never does.
        .ok_or_else(|| anyhow::anyhow!("tao window has no default gtk vbox"))?;
    let webview = builder
        .build_gtk(vbox)
        .map_err(|e| anyhow::anyhow!("webview build: {e}"))?;

    // The second sender: the bound on the hidden phase.
    //
    // On the GTK thread's own main loop, because that is where the show has
    // to happen and where the flag it reads lives — a `std::thread::sleep`
    // would work too, but it would cost a thread per open for 1.5 s, which
    // is precisely the accounting #45 spent a release flattening.
    //
    // One-shot and self-neutering: `reveal_window` and `close_window` both
    // clear `reveal_armed`, so a timer whose window was shown early (the
    // common case) or closed during the hidden phase sends nothing at all.
    // Even if it did, `reveal_window` looks the key up and finds nothing —
    // a closed window cannot be resurrected by a timer that outlived it.
    let reveal_armed = Rc::new(Cell::new(true));
    {
        let armed = Rc::clone(&reveal_armed);
        let proxy = state.proxy.clone();
        glib::timeout_add_local_once(FIRST_LOAD_TIMEOUT, move || {
            if armed.get() {
                let _ = proxy.send_event(LoopEvent::Reveal { key });
            }
        });
    }

    open.store(true, Ordering::SeqCst);
    state.keys_by_tao_id.insert(window.id(), key);
    state.windows.insert(
        key,
        LiveWindow {
            webview,
            window,
            open: Arc::clone(open),
            shown: false,
            reveal_armed,
        },
    );
    Ok(())
}

/// End a window's hidden pre-paint phase: show it, focus it, and disarm its
/// fallback timer (issue #47).
///
/// Idempotent and total, because it is called from three directions that
/// cannot see each other: the page-load handler on **every** load, the
/// fallback timer once, and both for a key that may already have closed.
/// Only the first call for a live, still-hidden window does anything.
///
/// That is what keeps the obvious hazards out:
///
/// - a `navigate` that finishes later cannot re-show a window the user has
///   since minimised or buried, nor steal focus back;
/// - the fallback cannot map a window whose handle was dropped a second
///   ago (the entry is gone, so the lookup fails);
/// - a page-load `Finished` after the fallback already showed the window is
///   simply nothing.
///
/// Showing implies focusing, deliberately: v0.23.1 mapped the window at
/// build time and it took focus then, so revealing without focus would be
/// the behaviour change, not revealing with it.
fn reveal_window(key: WindowKey, state: &mut LoopState) {
    let Some(live) = state.windows.get_mut(&key) else {
        return;
    };
    if live.shown {
        return;
    }
    live.shown = true;
    live.reveal_armed.set(false);
    live.window.set_visible(true);
    live.window.set_focus();
}

/// Destroy one window, leaving the loop and every other window untouched.
///
/// A no-op for a key that is already gone, which is the normal race: the
/// user closes the window and the host drops the handle afterwards.
fn close_window(key: WindowKey, state: &mut LoopState) {
    let Some(live) = state.windows.remove(&key) else {
        return;
    };
    state.keys_by_tao_id.remove(&live.window.id());
    live.open.store(false, Ordering::SeqCst);
    // A window closed during its hidden pre-paint phase still has a
    // fallback timer armed. Removing the entry above already makes a late
    // `Reveal` a no-op; clearing the flag means the timer does not even
    // post one. (#47 — a `Drop` two hundred milliseconds after `open` is
    // an ordinary host shutdown, not an exotic race.)
    live.reveal_armed.set(false);
    // Webview then window (field order), and then nothing else — in
    // particular NOT the shared `WebContext`, which outlives every window on
    // purpose (#45). The loop is still iterating GTK, so GDK flushes the
    // `DestroyWindow` that `tao::Window::drop` buffers. The hand-rolled drain
    // + `Display::flush` v0.22.3 needed here existed only because the
    // per-window loop had stopped iterating by this point.
    drop(live);
}

/// A live floating webview window. Cheap handle: commands go through the
/// shared GTK thread's event-loop proxy; nothing here touches GTK. Drop
/// closes this window (and only this window).
pub struct WebWindow {
    key: WindowKey,
    proxy: EventLoopProxy<LoopEvent>,
    open: Arc<AtomicBool>,
}

impl WebWindow {
    /// Open a window on the process-wide GTK thread and load `content`,
    /// starting that thread if this is the first call. Returns once the
    /// window + webview have been built (or failed to), so a build error
    /// surfaces here rather than as a silently missing window.
    ///
    /// # When the window appears
    ///
    /// *Changed in 0.23.2 ([#47]).* Not here. The window is built
    /// **hidden** and becomes visible — and focused — when its first page
    /// load finishes, or after a bounded wait (1.5 s) if that never
    /// happens. A `file://` page or an inline `Html` snippet is up in a few
    /// tens of milliseconds; a slow remote page waits, rather than showing
    /// the user a white rectangle for a second and a half. See "Visibility"
    /// in the module docs for what that does and does not change.
    ///
    /// [#47]: https://github.com/jlgerber/gpui-yaams/issues/47
    ///
    /// # Reopening and multiple windows
    ///
    /// Both work, as of 0.23.0 (issue #43). A handle may be dropped and a
    /// new window opened afterwards, and any number of windows may be live
    /// at once — each with its own handle, independently navigable and
    /// closable. Before 0.23.0 this returned `Err` on every call after the
    /// first, because each one needed a `gtk::init` on a fresh thread and
    /// GTK3 belongs to the thread that initialized it; the shared thread
    /// removes the second `init` entirely.
    ///
    /// A focus-or-reopen Help flow is therefore a UX choice now, not a
    /// correctness requirement — reusing one handle with
    /// [`focus`](Self::focus) + [`navigate`](Self::navigate) still avoids a
    /// window flash and keeps scroll position, but dropping and reopening
    /// is no longer broken.
    ///
    /// # Errors
    ///
    /// The window or webview failing to build, or the GTK thread being
    /// unavailable — it could not start (no display, no WebKitGTK runtime)
    /// or it died. In the unavailable case every later call fails the same
    /// way immediately, rather than blocking: GTK cannot be re-initialized
    /// on another thread in this process, so there is nothing to retry.
    pub fn open(opts: WebWindowOptions, content: WebContent) -> anyhow::Result<WebWindow> {
        let proxy = gtk_proxy()?;
        let key = NEXT_WINDOW_KEY.fetch_add(1, Ordering::Relaxed);
        let open = Arc::new(AtomicBool::new(false));
        let (reply_tx, reply_rx) = mpsc::channel::<anyhow::Result<()>>();
        let request = CreateRequest {
            key,
            opts,
            content,
            open: Arc::clone(&open),
            reply: reply_tx,
        };
        if proxy.send_event(LoopEvent::Create(request)).is_err() {
            mark_gtk_thread_failed("the md-librarian-webview GTK event loop is gone");
            anyhow::bail!("the md-librarian-webview GTK event loop is gone");
        }
        match reply_rx.recv() {
            Ok(verdict) => verdict.map(|()| WebWindow { key, proxy, open }),
            Err(_) => {
                let why = "the md-librarian-webview GTK thread died while building the window";
                mark_gtk_thread_failed(why);
                anyhow::bail!("{why}")
            }
        }
    }

    /// Load a new page. A no-op after this window closed.
    pub fn navigate(&self, content: WebContent) {
        self.send(Cmd::Navigate(content));
    }

    /// Retitle the window. A no-op after this window closed.
    pub fn set_title(&self, title: impl Into<String>) {
        self.send(Cmd::SetTitle(title.into()));
    }

    /// Raise + focus the window (e.g. Help chosen again while it is already
    /// open). A no-op after this window closed.
    ///
    /// Also a no-op *before* the window is first shown: between `open` and
    /// first paint there is deliberately nothing on screen to raise, and
    /// the pending reveal focuses it a moment later anyway (#47).
    pub fn focus(&self) {
        self.send(Cmd::Focus);
    }

    /// Is this window still up? `false` once the user closed it, or once
    /// the handle was dropped. A closed window's handle is inert, not
    /// dangerous — commands are dropped on the loop.
    ///
    /// "Up" is *built*, not *visible*: it is `true` during the brief hidden
    /// phase between `open` and first paint (#47).
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }

    /// Post a command for this window. Send errors are swallowed: a loop
    /// that is gone means every window is gone, which is the closed state.
    fn send(&self, cmd: Cmd) {
        let _ = self
            .proxy
            .send_event(LoopEvent::Command { key: self.key, cmd });
    }
}

impl Drop for WebWindow {
    /// Close this window and wait — briefly — for the GTK thread to confirm
    /// it is gone.
    ///
    /// The wait is what makes "drop the handle, then open another window"
    /// deterministic for a caller, and it is *bounded* so a wedged GTK
    /// thread can never hang the host's shutdown the way the old
    /// join-the-thread `Drop` could.
    fn drop(&mut self) {
        let (ack_tx, ack_rx) = mpsc::channel::<()>();
        if self
            .proxy
            .send_event(LoopEvent::Command {
                key: self.key,
                cmd: Cmd::Close { ack: ack_tx },
            })
            .is_err()
        {
            self.open.store(false, Ordering::SeqCst);
            return;
        }
        let _ = ack_rx.recv_timeout(CLOSE_ACK_TIMEOUT);
    }
}
