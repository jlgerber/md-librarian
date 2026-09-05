# md-librarian-webview

A **floating WebKit window** (wry) any Rust app can open and drive — built
for usdlite's help rendering (usdlite #1302): show docs in our own process
instead of punting to an external browser. Extracted from gpui-yaams at
v0.27.0-beta.3; the `[#43]` / `[#47]` links below and the "Changed in 0.2x"
notes refer to that repository's history.

> **If a window never appears and GTK reports
> `Error 71 (Protocol error) dispatching to Wayland display`**, that is a
> known toolkit/driver/compositor interaction, not this crate — see
> [Known issue: the window never appears](./library.md#known-issue-the-window-never-appears-wayland--explicit-sync)
> for the diagnosis and the one-variable fix. It takes down every window this
> crate opens, in any host.

## Why a floating window, not a pane

On Linux wry renders through **WebKitGTK**, which needs GTK widgets and a
GTK-pumping event loop.

Docking into a gpui window is **not impossible** — an earlier version of this
page said wry's child path exists only on macOS/Windows, and that was wrong.
gpui's `Window` implements `HasWindowHandle`, and wry's `build_as_child` accepts
any such handle. wry's own docs say of Linux: *"This will create the webview as a
child window of the parent window. Only X11 is supported. This method won't work
on Wayland."*

Three things sink it anyway:

1. **Wayland.** On a Wayland session gpui returns a `WaylandWindowHandle` and
   `build_as_child` **panics** on an unsupported handle. Wayland is the default
   session on most current Linux desktops, so a docked webview would crash rather
   than degrade for a large share of users.
2. **A second event loop inside gpui's.** wry requires `gtk::init` on the calling
   thread and `gtk::main_iteration_do` interleaved with the host's event loop.
   gpui owns its loop and exposes no per-turn hook, so this would mean
   approximating the pump with a timer.
3. **It is an overlay, not an element.** A child X11 window is a separate native
   surface composited by the X server: it does not clip to a gpui scroll
   container, does not participate in gpui's z-order, and does not move with
   layout. Even on X11 it could not live in a dock or tab that scrolls or gets
   covered — which is most of why one would want it docked.

(3) outlives (1) and (2), so this stays a separate window even if wry and gpui
both improve.

What *is* well-trodden (it is exactly the tauri arrangement) is a **tao window +
wry webview on a GTK-owning thread**, tao pumping GTK. So this crate opens real
OS windows owned by the app process: no browser dependency, closes with the app,
floating above or beside it.

Its live reference is its own example:

```sh
cargo run -p md-librarian-webview --example help              # inline HTML
cargo run -p md-librarian-webview --example help https://…    # any URL
```

Two further examples are regression harnesses rather than demos. Both keep
the host process alive past the close and ask the **X server** what is
still mapped — a harness that ends with the process proves nothing about
teardown, because process exit destroys the client's windows regardless
(see "Why the window used to stay on screen"):

```sh
cargo run -p md-librarian-webview --example close_teardown        # close it yourself
cargo run -p md-librarian-webview --example close_teardown drop   # handle drop closes it

cargo run -p md-librarian-webview --example reopen                # #43: reopen + two at once
cargo run -p md-librarian-webview --example reopen quit           # #43: hard process exit

cargo run -p md-librarian-webview --example first_paint            # #47: does it flash white?
cargo run -p md-librarian-webview --example first_paint stalled    # #47: the fallback bound
cargo run -p md-librarian-webview --example first_paint navigate   # #47: a second load
cargo run -p md-librarian-webview --example first_paint drop-early # #47: drop while hidden
```

## Usage

```rust
use md_librarian_webview::{WebContent, WebWindow, WebWindowOptions};

let help = WebWindow::open(
    WebWindowOptions { title: "usdlite help".into(), width: 1024, height: 768 },
    WebContent::Url("file:///opt/usdlite/docs/index.html".into()),
)?;

// Later — e.g. from a Help menu action:
help.navigate(WebContent::Html("<h1>Context help</h1>".into()));
help.set_title("usdlite help — SDF nodes");
help.focus();                    // raise it if the user buried it

// Keep the handle (e.g. in your app state) or drop it — `is_open()` tells
// you whether the user closed it, and reopening afterwards works.
if help.is_open() { help.focus(); } else { /* open a fresh one */ }

// Dropping the handle closes that window (and only that window).
# anyhow::Ok(())
```

### Reopening, and more than one window

*Changed in 0.23.0 ([#43]).* **Both work.** A handle may be dropped and a
new window opened afterwards, and any number of windows may be live at
once — each with its own handle, independently navigable, retitlable and
closable.

Before 0.23.0, `WebWindow::open` succeeded **once per process**: every call
built a tao event loop on a *fresh* thread, and gtk-rs hard-panics on
`gtk::init` from a second thread (*"Attempted to initialize GTK from two
different threads"*), because GTK3 belongs to whichever thread initialized
it. The panic was contained on the window thread and came back as `Err`, so
it never took the host down — but a host that let the user close Help had no
way to bring it back, and could never show two documents side by side.

0.23.0 removes the second `gtk::init` rather than working around it: see
"Threading contract" below.

Focus-or-reopen is therefore now a **UX choice, not a correctness
requirement**. Reusing one handle with `focus()` + `navigate()` is still
often the nicer behaviour — no window flash, the window keeps its position
and the page keeps its scroll — but dropping the handle and opening a fresh
window is a supported path, not a broken one.

[#43]: https://github.com/jlgerber/gpui-yaams/issues/43

### When the window appears

*Changed in 0.23.2 ([#47]).* A window has **two** moments, and only the
first is a host's business:

1. **Built** — `WebWindow::open` returns. The window and webview exist and
   the page is loading; nothing is on screen. The window is created
   `with_visible(false)`, so GTK has not realized it and there is no X
   window for a window manager, a taskbar or the user to find.
2. **Shown** — the webview reports its first load *finished*, or 1.5 s
   passes, whichever comes first. The window is mapped and focused, once.

Through 0.23.1 there was only moment 1 and the window mapped there: the
user got WebKit's white default for as long as the page took to arrive,
then a beat of unstyled HTML while its stylesheets loaded, and only then
the page.

**`open`'s contract is unchanged** — it returns when the webview is
*built*, and a build failure is still an `Err` from `open` and not a
silently missing window. So is `is_open()`'s: it means built, which is
what a host is asking. Two consequences worth knowing:

- **`focus()` during the hidden phase does nothing**, deliberately. There
  is nothing on screen to raise, and the pending reveal focuses the window
  a moment later. (Queueing it would map an unpainted window, which is the
  defect by another route.)
- **A later `navigate()` cannot re-hide or re-show the window.** The reveal
  happens once. It does not need to happen again either: WebKitGTK keeps
  the old page on screen until the new one commits with styles, so
  navigating a window that is already up does not flash.

The 1.5 s bound is what makes waiting safe. Without it, a page that never
finishes — a stalled stylesheet, an unreachable host — would produce **no
window at all**, which is worse than the flash. A local `file://` doc or an
inline `Html` snippet finishes in tens of milliseconds and never gets near
the bound, so the fallback is only reached where 0.23.1's behaviour was the
best available anyway.

#### Measured

`cargo run -p md-librarian-webview --example first_paint` serves its own page over
a throwaway `TcpListener` and sleeps 800 ms before answering the request
for its one `<link>`ed stylesheet — an inline snippet loads too fast to
reproduce a flash on demand, so the harness manufactures one. It then times
two clocks from outside the crate: when the **X server** first lists the
window, and when its own responder finished writing the stylesheet.

```text
                        0.23.1                    0.23.2
  open() returned       t+ 286ms                  t+ 209ms
  window mapped         t+  73ms                  t+1178ms
  stylesheet served     t+1768ms                  t+1143ms
  verdict               FAIL — 1695ms of          PASS — mapped 35ms
                        visible unstyled page     after the stylesheet
```

The clocks cannot settle the other half of it, though: a fix that merely
*moved* the white would pass them too. So the harness also samples the
client area's mean brightness every 20 ms from the moment the window maps
— 1.0 is white, the styled page is 0.12, the desktop showing through an
unfilled backing store is 0.21:

```text
  0.23.1, mapped t+73ms:
    0.21 0.21 0.21 0.21 0.21 0.14 1.00 1.00 1.00 1.00 1.00 1.00 1.00 1.00 1.00 0.12
                                  └────────── ~860ms of pure white ─────────┘
  0.23.2, mapped t+1178ms:
    0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12 0.12
```

The window is painted the moment it appears, and stays painted. That is
also why **no webview background colour was set** — the obvious companion
tweak, and unnecessary: WebKit renders while unmapped and composites on
map, so there is no residual gap for a colour to cover, and any fixed
choice would have been wrong for half the pages a host shows.

The other modes cover the bound and the interactions: `stalled` (a
stylesheet that never arrives — window at +1512 ms, i.e. the fallback),
`navigate` (a second load starting immediately, and another on an
already-shown window — same X id throughout, never absent), and
`drop-early` (handle dropped 300 ms in, while hidden and with the timer
still armed — `Drop` returns in ~500 µs and no window appears through the
deadline and beyond).

[#47]: https://github.com/jlgerber/gpui-yaams/issues/47

### Host wiring

No assets, fonts, overlay layers, or **environment variables**. The host
machine needs only the WebKitGTK runtime (`webkit2gtk-4.1`, the standard
wry requirement on Linux).

> **Changed in 0.22.2.** 0.22.1 told hosts to set
> `WEBKIT_DISABLE_DMABUF_RENDERER=1` first thing in `main` on NVIDIA
> proprietary-driver setups. That diagnosis was wrong — the blank window
> was a wry/tao attachment bug, not a renderer bug (see below), and the
> variable never fixed anything (verified: blank both with and without
> it, rendering correctly both with and without it after the fix). Hosts
> that set it — usdlite did — should **delete the `set_var`**: it forces
> WebKitGTK off its DMABUF path for no benefit, and it is one fewer
> `unsafe` environment mutation in `main`.

### Why the webview used to render blank

The window had the right title and size, WebKit's network process
fetched the page and its web process stayed alive with no errors — and
the client area showed only the GTK background, for every URL and every
inline document.

wry's `WebViewBuilder::build(&window)` takes a **raw window handle**. Its
X11 implementation wraps that XID in a *foreign* `GdkWindow`, hangs a
**second** `gtk::Window` off it, and packs the webview into that
window's box. Against a non-GTK host (winit) the XID genuinely is
foreign and this works. Against **tao** it cannot: a tao window already
*is* a `gtk::ApplicationWindow`, so GDK already knows the XID and
`gdk_x11_window_foreign_new_for_display` returns the **existing**
`GdkWindow`. Two `GtkWindow`s then share one `GdkWindow`, and only
tao's is wired to receive its configure events — so wry's window is
never size-allocated and the webview widget stays at GTK's unallocated
`1x1+-1+-1` forever. `xwininfo -id <tao window> -tree` showed exactly
that child.

The crate now uses `WebViewBuilderExtUnix::build_gtk` on the tao
window's `default_vbox()` — the tauri arrangement: one `GtkWindow`,
`pack_start(webview, expand, fill, 0)`. The webview gets the client area
and follows window resizes, with no manual `WebView::set_bounds` (which
the raw-handle X11 path would have required).

### Why the window used to stay on screen after closing

Distinct from the blank-content bug, and only visible in a real host:
clicking the window's X ended the event loop and flipped `is_open()` to
`false`, but the window **stayed on screen and stopped responding** — a
zombie. Same for dropping the handle.

`tao::Window::drop` calls `gtk_widget_destroy`, which issues an X
`DestroyWindow`; Xlib only *buffers* that request, and what pushes it to
the server is GDK's flush at the end of a GLib main-context iteration.
`run_return` has just stopped iterating, so nothing ever flushed and the
destroy sat in the output buffer for the life of the process.

Every earlier test missed it because they all ended with the **process**,
and process exit closes the X connection — at which point the server
destroys the client's windows no matter what the client did or did not
send. The bug exists only when the host outlives the window, which is
every real host. 0.22.3 fixed it by draining the GTK main context and
flushing the display by hand after dropping the webview and window.

**0.23.0 deletes that hand-rolled teardown** rather than keeping it. The
flush was only ever needed because the per-window loop had *stopped
iterating* by the time the window was dropped; the shared loop is still
iterating, so GDK flushes the destroy on its own. Verified, not assumed —
`close_teardown` and `reopen` both watch the server's window list with the
process still alive, and both report the window gone from the first tick
with no manual flush in the code. Dropping the hand-rolled path also
dropped the crate's direct `gtk` dependency and its one `unsafe` block
(the GLib default-main-context release, which existed only because the
per-window thread died owning the context — the permanent thread never
dies).

### Threading contract

All GTK/webview work lives on **one process-wide thread**
(`md-librarian-gtk`), started lazily on the first `WebWindow::open` and
living for the rest of the process. It owns `gtk::init`, one tao event
loop, the GLib default main context, and every window the crate has open.
The loop never exits, so GTK is never re-initialized — which is exactly
what makes reopening and multiple windows possible.

A handle is cheap and inert: a window id, a clone of the loop's
`EventLoopProxy`, and a shared open-flag. Every method is a `send_event`
that wakes the loop (no polling, no locks on the hot path, no GTK type
crossing a thread boundary). Commands for a window that is already gone are
dropped on the loop, and `is_open()` reads the flag. Two threads may call
`WebWindow::open` at once.

### Process quit

**Graceful:** a host that drops its handles during shutdown gets the
ordinary close path — a close command to the loop and a *bounded* ack-wait
in `Drop` — while the loop is still pumping. The bound is what keeps a
wedged GTK thread from hanging the host's shutdown (the 0.22.1 `Drop`
joined a thread that could not exit, and hung).

**Hard:** if the process just exits — handles leaked, `exit()` called —
the window still goes away, because the cleanup is the OS's. The GTK
thread is detached and dies with the process; process exit closes the X
connection and the server destroys the client's windows. (That is the same
mechanism as above, read the other way round: it is *because* process exit
destroys them that the zombie only ever appeared while the host outlived
the window.) WebKitGTK's network and web processes are children on an IPC
socket to the UI process and self-terminate when it closes, so nothing
orphans. `cargo run -p md-librarian-webview --example reopen quit` exercises
exactly this and leaves neither an X window nor a `WebKit*Process` behind.

### Thread cost

*Changed in 0.23.1 ([#45]).* Thread count does not return to baseline, but
it **does not grow with use**. Measured, not asserted — `cargo run -p
md-librarian-webview --example reopen churn 8` opens and closes eight windows one
at a time, then opens eight *at once* and closes them:

```text
  before any window:            1 threads
  window  1:  15 threads open ->  14 after close
  window  4:  15 threads open ->  14 after close
  window  8:  14 threads open ->  13 after close
  settled at 13 threads, zero windows open:
    1x ReceiveQueue   2x VBlankMonitor   1x ebsiteDataStore
    4x md-librarian-g   1x gmain   1x gdbus   1x pool-spawner
    1x PressureMonitor   1x reopen
  == peak: 8 windows live at once ==
  8 live at once:            21 threads
  after closing all 8:        13 threads
  net over the sequential settle: 0 threads for 8 more windows opened
```

> **One more than 0.23.1** (13/21/13 against 12/20/12): a second
> `VBlankMonitor`, which 0.23.2's hidden-then-shown lifecycle costs. It is
> a **fixed** cost — it appears with the first window and never again, and
> the net over eight further windows is still zero — so the property this
> section is about survives. It was attributed by bisection, not guessed:
> with 0.23.2's page-load handler and fallback timer in place but the
> window built *visible*, the count is 0.23.1's. The likely mechanism is
> that the webview widget is realized before its toplevel is on any
> monitor, so WebKit binds a vblank monitor for the default one and another
> for the real one at map time — which would make it an artefact of a
> multi-monitor desktop. That part is a hypothesis; the number is not.

Three components, none of which grows with the number of windows a host
has *ever* opened:

- **Fixed, ours (~9).** The GTK thread parks rather than stopping when the
  last window closes, tao's X11 device thread parks beside it, and GLib
  keeps its singletons. This part is *cheaper* than what it replaced: tao
  only stops its device thread when its loop exits, so one loop per window
  leaked one device thread **per window**.
- **Fixed, WebKitGTK's (~3).** One `ebsiteDataStore`, one `ReceiveQueue`,
  one or two `VBlankMonitor` — for the process, because every webview is
  built against **one shared `wry::WebContext`** owned by the GTK thread.
- **Transient, per live window (~1).** Open windows cost about a thread
  each while open, and give it back on close.

Through 0.23.0 the middle bullet grew instead: **+3 per window ever
opened** (33 threads after eight reopens, 53 after sixteen). wry builds a
fresh `wry::WebContext` per webview unless handed one, and a fresh WebKit
context means a fresh `WebsiteDataManager` and a fresh web process, whose
threads WebKitGTK does not return when the webview is destroyed. That was
upstream behaviour rather than anything the shared-thread design
introduced — but it was unreachable while one webview per process was a
hard ceiling, and 0.23.0's reopening made it reachable. 0.23.1 passes
every builder the same context (`WebViewBuilder::new_with_web_context`),
which makes it a constant.

`focus()` + `navigate()` on a kept handle is therefore a **UX**
optimisation only — no window flash, scroll position preserved — and no
longer a way to dodge a thread leak. Reopen as often as the flow wants.

[#45]: https://github.com/jlgerber/gpui-yaams/issues/45
