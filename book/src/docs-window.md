# Building a documentation window

[`md-librarian-webview`](./webview.md) opens **one page**. Documentation is
usually more than one page. This is the pattern that bridges the gap, extracted
from yaams-tk2 (yaams-tk2#489), where it ships six books in a single window.

Read it before building your own: most of it is traps that cost a review round
each, and every one of them is reachable in a normal deployment.

> **If you want a *library* of books — many of them, discovered at runtime rather
> than compiled in — that already exists: see
> [A library of books](./library.md).** `md-librarian` serves a search path of
> repository roots over a loopback HTTP origin and gives you the card page, the
> bar, and the way back, with nothing to write per app.
>
> This chapter remains the answer for the smaller case: **one app, a fixed
> handful of pages, no server**. Its measured facts still hold and are cited
> throughout the library implementation — with one exception worth knowing.
> Everything below about what `file://` forbids (no reading the frame, no current
> chapter in the bar, no theme sync) is a consequence of the **opaque origin**,
> not of the iframe: serving the same pages over `http://127.0.0.1` lifts all of
> it, which is exactly why the library does.

## The constraint everything follows from

A `WebWindow` is a bare tao window. **No chrome, no back button, no address
bar.** So the moment a click navigates *away* from your list of documents, the
user is stranded with no way back and no way to tell you about it.

That rules out the obvious design — a landing page that links out to each
document — no matter how good the landing page is. It is a one-way door.

## The shape: a generated shell

Put a **persistent bar** of links above an **iframe**, and load the selected
document into the frame:

```
+---------------------------------------+
| Handbook   Reference   Tutorial   …   |  <- never unloads
+---------------------------------------+
|                                       |
|   <iframe name="docframe" src="…">    |
|                                       |
+---------------------------------------+
```

Links carry `target="docframe"`, so a click swaps the frame and leaves the bar
untouched. **The core interaction needs no JavaScript at all** — which is not a
style preference, see the next section.

Two properties fall out of this that a link-out page cannot give you:

- Any document is one click away from any other.
- An **external link inside a document** (to a crates.io page, say) navigates the
  frame but is still not a dead end, because the bar survives it.

## What is measured, and what it forbids

Both halves were verified against a real `WebKit2.WebView`, not assumed:

| | |
|---|---|
| A `file://` page **can** embed another `file://` page in an iframe | it renders normally; `onload` fires |
| It **cannot** script into it | `iframe.contentDocument` is `null` |

WebKit gives every `file://` document an **opaque origin**, so the parent and the
frame are cross-origin to each other. Setting `iframe.src` is a parent-side DOM
operation and is unaffected — that is all the pattern needs.

What this forbids, permanently, unless you introduce a real origin (a server):

- showing the current chapter in the bar;
- syncing a theme toggle between bar and document;
- restoring scroll position;
- anything else that requires *reading* the frame.

Design as if the frame is a black box you can only point at, because it is.

## The traps

### 1. Never link content that is not there

A link to a missing page opens a `file://` 404 **in a window with no back
button**. List the entry, but render it as inert text:

```rust
if doc.rendered_index().is_file() {
    format!("<a href=\"{}\" target=\"docframe\">{}</a>", url_segment(&doc.name), label)
} else {
    format!("<span class=\"unbuilt\">{label}</span>")
}
```

This is not hypothetical if any of your content is generated on demand.

### 2. HTML-escaping is not enough for an attribute

Escaping `& < >` is right for a text node and **wrong** for an `href`. A
directory named `ev" onmouseover=alert(1) x` closes the attribute, and WebKit
parses the remainder as further attributes — a live event handler. The quieter
half of the same bug: a `#` or `?` starts a fragment or query, so the path
silently truncates and the link 404s.

Percent-encode to the unreserved set instead. The result contains only characters
that are safe in an attribute **and** mean themselves in a path, so there is no
second escaping step to forget:

```rust
/// Percent-encode one path segment. Safe in an attribute and in a path at once,
/// which is why this replaces HTML-escaping here rather than joining it.
fn url_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
```

Apply it to the **path you build the URL from** as well, not just the leaf names
— the same hole exists one level up.

### 3. A shell generated once goes stale

If any of your content is built *after* the shell is written, the shell will
list it as unavailable **forever**. In yaams-tk2 exactly one book is generated
from a live daemon on demand, and the deploy-time shell hid it permanently until
this was fixed.

Regenerate the shell wherever content is built, not where it is deployed. Find
the point that changes what exists, and refresh there.

### 4. Only overwrite a page you generated

"Does `index.html` exist?" is not the same question as "is this my shell?". A
directory may hold someone's own landing page. Stamp a marker and require it:

```rust
const SHELL_MARKER: &str = "<!--my-app-docs-shell-->";

let Ok(existing) = std::fs::read_to_string(&dest) else { return };
if !existing.contains(SHELL_MARKER) {
    return;   // not ours to rewrite
}
```

Without this, an unrelated operation elsewhere can silently destroy a user's
file — which is what happened in review before the marker existed.

### 5. Empty labels vanish

If a title can be empty (a missing `title` key, say), rendering it verbatim gives
a **zero-width anchor**: present, clickable in principle, invisible in practice.
Fall back to something that always exists, such as the directory name.

## Host wiring

Hold the handle in a gpui `Global`. `WebWindow`'s `Drop` closes the window, so a
handle owned by a menu handler closes the docs the moment the handler returns:

```rust
/// The single documentation window, or none if it has never been opened.
#[derive(Default)]
struct DocsWindow(Option<md_librarian_webview::WebWindow>);

impl gpui::Global for DocsWindow {}

pub fn show_docs(cx: &mut gpui::App) {
    // Never open a window onto a file that is not there — you get a 404 with no
    // way out. Report the path you looked at instead.
    let Some(shell) = locate_shell() else {
        tracing::warn!("no documentation shell found; deploy the docs first");
        return;
    };

    if !cx.has_global::<DocsWindow>() {
        cx.set_global(DocsWindow::default());
    }
    // Already open: raise it. A handle whose window the user closed is stale and
    // must be replaced, not focused — `is_open()` is what tells the two apart.
    if cx.global::<DocsWindow>().0.as_ref().is_some_and(|w| w.is_open()) {
        cx.global::<DocsWindow>().0.as_ref().unwrap().focus();
        return;
    }

    match md_librarian_webview::WebWindow::open(
        md_librarian_webview::WebWindowOptions {
            title: "documentation".into(),
            width: 1100,
            height: 800,
        },
        md_librarian_webview::WebContent::Url(file_url(&shell)),
    ) {
        Ok(win) => cx.global_mut::<DocsWindow>().0 = Some(win),
        Err(e) => tracing::error!(err = %e, "could not open the documentation window"),
    }
}
```

Reuse **one** window and `navigate` it rather than opening one per document —
see [thread cost](./webview.md#thread-cost) for why that is not merely
tidier.

## Keep the bar to documents

Do not put a title, a brand or any other non-clickable item in the bar. In a
window with no chrome the bar is the *only* way back, so it has to read as
reliable — and a dead entry among live ones means nothing about the bar tells you
which items do anything. The window title already carries your app's name.

## What you cannot test

`WebWindow` needs GTK and a display, so none of the window behaviour is
unit-testable. What *is*: shell rendering as a pure function from your document
list to HTML, and the path-to-URL mapping. Everything else — opening, clicking
between documents, following an external link back, close-then-reopen, and the
no-content-deployed case — is a manual checklist. Write the checklist down rather
than a test that only asserts you called a function.

If you want more confidence than reading HTML gives you, drive the generated page
in a real WebKit view: `python3-gi` with `WebKit2 4.1` and a
`Gtk.OffscreenWindow` will load a `file://` URL, run JavaScript against the
top-level document, and snapshot to PNG so you can count pixels inside the frame
region. That is how the two measured facts above were established, and how the
"a click swaps the frame, not the page" property was confirmed — **0** nav-bar
pixels changed against 87,568 in the frame.
