//! The generated pages: the **shell** (a persistent bar over an iframe), the
//! **grid** of cards, and the escaping rules both depend on.
//!
//! Everything here is a pure function from a book list to HTML, which is the
//! point: `book/src/docs-window.md` records that anything touching a real window
//! needs GTK and a display and can only be a manual checklist, while *this* half
//! is testable — and is where all the decisions live.

use md_librarian::{Book, Entry};

/// Percent-encode one path segment to the unreserved set.
///
/// This replaces HTML-escaping in an `href` rather than joining it. Escaping
/// `& < >` is right for a text node and **wrong** for an attribute: a directory
/// named `ev" onmouseover=alert(1) x` closes the attribute and WebKit parses the
/// rest as further attributes — a live event handler. The quieter half of the
/// same bug is that a `#` or `?` starts a fragment or query, so the path
/// silently truncates and the link 404s. The result of this function contains
/// only characters that are safe in an attribute *and* mean themselves in a
/// path, so there is no second escaping step to forget.
pub fn url_segment(s: &str) -> String {
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

/// Escape text for an HTML text node.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape a string for a JavaScript double-quoted literal.
fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            // `</script` inside a string still ends the element, so break it up.
            '<' => out.push_str("\\u003C"),
            '>' => out.push_str("\\u003E"),
            '&' => out.push_str("\\u0026"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// The URL prefix a book's pages are served under: root index + directory name.
///
/// Titles are never used here. They are display text — they contain spaces and
/// punctuation, and two books may share one — while this has to be unique across
/// the whole search path and safe in a URL.
pub fn book_prefix(book: &Book) -> String {
    format!("/{}/{}/", book.root_index, url_segment(&book.dir_name))
}

/// A deterministic cover for a book with no `cover.<ext>`: the title's initial
/// on a colour derived from the whole title.
///
/// Deterministic so a book looks the same on every machine and every refresh,
/// and derived from the title rather than assigned in order so adding a book
/// never recolours the others.
pub fn generated_cover(title: &str) -> String {
    // FNV-1a. Any stable hash would do; std's is explicitly not stable across
    // releases, and this value ends up baked into how a book *looks*.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in title.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let hue = hash % 360;
    let initial = title
        .chars()
        .find(|c| !c.is_whitespace())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into());

    format!(
        "<svg class=\"cover\" viewBox=\"0 0 320 180\" role=\"img\" aria-hidden=\"true\">\
           <rect width=\"320\" height=\"180\" fill=\"hsl({hue} 42% 26%)\"/>\
           <text x=\"160\" y=\"104\" text-anchor=\"middle\" \
             font-family=\"system-ui, sans-serif\" font-size=\"72\" font-weight=\"600\" \
             fill=\"hsl({hue} 70% 82%)\">{initial}</text>\
         </svg>",
        initial = html_escape(&initial)
    )
}

/// Shared CSS for both generated documents.
const STYLE: &str = "\
*{box-sizing:border-box}\
body{margin:0;background:#0f1419;color:#c9d1d9;\
 font:14px/1.5 system-ui,-apple-system,Segoe UI,sans-serif}\
a{color:inherit;text-decoration:none}";

/// The shell: a bar that never unloads, over the frame everything else renders in.
///
/// The bar is the only chrome the window has — a `WebWindow` is a bare tao
/// window with no back button and no address bar — so it must survive anything
/// the frame does, including a link out to an external site. That is exactly why
/// the frame is an iframe and not the window itself.
///
/// The core interaction needs **no JavaScript**: the cards and the "Library"
/// link are ordinary anchors targeting the frame. The script only adds the
/// current book's name to the bar and keeps the frame's paint colour in step,
/// both of which are possible because everything here is served from one
/// origin — under `file://` the parent cannot read the frame at all
/// (`iframe.contentDocument` is `null`, measured).
///
/// # The flash between pages
///
/// Every chapter is a separate document, so the browser blanks the frame on
/// each navigation and the frame's own background is what shows through. It
/// defaults to grey rather than white — a white flash against a dark book is
/// the worst case — and after each load the shell reads the book's real
/// background and adopts it, so subsequent navigations flash the book's own
/// colour and are effectively invisible.
///
/// It cannot be eliminated from here: the blank paint belongs to the frame's
/// navigation, not to anything this page controls. Following a link to an
/// external site keeps whatever colour was last adopted, because reading that
/// document is not permitted.
pub fn shell_document(entries: &[Entry], initial: Option<&Book>) -> String {
    let map = entries
        .iter()
        .filter_map(|e| match e {
            Entry::Book(b) if b.is_built() => Some(format!(
                "[\"{}\",\"{}\"]",
                js_string(&book_prefix(b)),
                js_string(&b.title)
            )),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(",");

    // A caller can open straight onto one book (`--book`, i.e. `/?book=<title>`).
    // It still lands inside the shell rather than on the book alone, so the bar
    // — and the way back to the library — is there from the first frame.
    let initial_src = initial
        .map(book_prefix)
        .unwrap_or_else(|| "/_grid".to_string());

    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>Books</title><style>{STYLE}\
         html,body{{height:100%}}\
         body{{display:flex;flex-direction:column}}\
         header{{display:flex;align-items:center;gap:.9rem;padding:.5rem .9rem;\
           background:#161c24;border-bottom:1px solid #28313b;flex:0 0 auto}}\
         header a{{padding:.25rem .6rem;border-radius:.35rem;background:#1e2731;\
           border:1px solid #28313b}}\
         header a:hover{{border-color:#4b9fff}}\
         #current{{color:#8b949e}}\
         #back.home{{visibility:hidden}}\
         iframe{{flex:1 1 auto;width:100%;border:0;background:#6e7681}}\
         </style></head><body>\
         <header><a id=\"back\" href=\"/_grid\" target=\"docframe\">&#8592; Library</a>\
         <span id=\"current\"></span></header>\
         <iframe id=\"docframe\" name=\"docframe\" src=\"{initial_src}\" title=\"book\"></iframe>\
         <script>\
         var MAP=[{map}];\
         var f=document.getElementById('docframe'),c=document.getElementById('current'),\
           k=document.getElementById('back');\
         f.addEventListener('load',function(){{\
           try{{\
             var b=f.contentDocument&&f.contentDocument.body;\
             if(b){{var bg=getComputedStyle(b).backgroundColor;\
               if(bg&&bg!=='transparent'&&bg!=='rgba(0, 0, 0, 0)'){{f.style.background=bg}}}}\
           }}catch(e){{}}\
           var p;try{{p=f.contentWindow.location.pathname}}catch(e){{k.className='';return}}\
           k.className=p==='/_grid'?'home':'';\
           var t='';for(var i=0;i<MAP.length;i++){{if(p.indexOf(MAP[i][0])===0){{t=MAP[i][1];break}}}}\
           c.textContent=t;document.title=t?t+' \\u2014 Books':'Books';\
         }});\
         </script></body></html>"
    )
}

/// The library page: one card per entry, already sorted by title.
pub fn grid_document(entries: &[Entry]) -> String {
    let cards = if entries.is_empty() {
        format!(
            "<p class=\"empty\">No books found. Point <code>{}</code> at a directory of \
             mdbooks — one subdirectory per book, each with its own <code>book.toml</code>.</p>",
            md_librarian::BOOK_PATH_VAR
        )
    } else {
        entries.iter().map(card).collect::<Vec<_>>().join("")
    };

    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Library</title>\
         <style>{STYLE}\
         main{{padding:1.6rem;max-width:1100px;margin:0 auto}}\
         h1{{font-size:1.35rem;margin:0 0 1.2rem;font-weight:600}}\
         .grid{{display:grid;gap:1rem;\
           grid-template-columns:repeat(auto-fill,minmax(240px,1fr))}}\
         .card{{display:block;border:1px solid #28313b;border-radius:10px;\
           background:#161c24;overflow:hidden;transition:border-color .15s,transform .15s}}\
         a.card:hover{{border-color:#4b9fff;transform:translateY(-2px)}}\
         .card .body{{padding:.9rem 1rem 1.1rem}}\
         .cover,img.cover{{display:block;width:100%;height:150px;background:#0f1419}}\
         img.cover{{object-fit:contain;padding:.5rem;box-sizing:border-box}}\
         .title{{font-weight:600;margin:0 0 .25rem}}\
         .desc{{color:#8b949e;margin:0}}\
         .dir{{color:#6e7681;font-size:.85em;margin:.35rem 0 0;font-family:ui-monospace,monospace}}\
         .dead{{opacity:.55}}\
         .why{{color:#d29922;font-size:.85em;margin:.35rem 0 0}}\
         .empty{{color:#8b949e}}\
         </style></head><body><main><h1>Library</h1>\
         <div class=\"grid\">{cards}</div></main></body></html>"
    )
}

/// One card.
///
/// **Never link content that is not there.** A link to a missing page opens a
/// 404 in a window with no back button, so an unbuilt book and a title no root
/// provides are both rendered as inert `<div>`s that say why — listed, because
/// hiding them makes a typo or an unmounted root indistinguishable from a book
/// that was never asked for.
fn card(entry: &Entry) -> String {
    match entry {
        Entry::Book(b) if b.is_built() => {
            let cover = match &b.cover {
                Some(_) => format!(
                    "<img class=\"cover\" src=\"/_cover/{}/{}\" alt=\"\">",
                    b.root_index,
                    url_segment(&b.dir_name)
                ),
                None => generated_cover(&b.title),
            };
            format!(
                "<a class=\"card\" href=\"{href}\">{cover}<div class=\"body\">\
                 <p class=\"title\">{title}</p>{desc}{dir}</div></a>",
                href = book_prefix(b),
                title = html_escape(&b.title),
                desc = description(&b.description),
                dir = dir_line(b),
            )
        }
        Entry::Book(b) => format!(
            "<div class=\"card dead\">{cover}<div class=\"body\">\
             <p class=\"title\">{title}</p>{desc}{dir}\
             <p class=\"why\">not built yet</p></div></div>",
            cover = generated_cover(&b.title),
            title = html_escape(&b.title),
            desc = description(&b.description),
            dir = dir_line(b),
        ),
        Entry::Missing { title } => format!(
            "<div class=\"card dead\">{cover}<div class=\"body\">\
             <p class=\"title\">{t}</p>\
             <p class=\"why\">not found in any root</p></div></div>",
            cover = generated_cover(title),
            t = html_escape(title),
        ),
    }
}

fn description(desc: &str) -> String {
    if desc.trim().is_empty() {
        String::new()
    } else {
        format!("<p class=\"desc\">{}</p>", html_escape(desc))
    }
}

/// The directory name, shown **only** when another book in the same root shares
/// this title — it is the one thing that tells two otherwise identical cards
/// apart, and noise on every other card.
fn dir_line(b: &Book) -> String {
    if b.ambiguous {
        format!("<p class=\"dir\">{}</p>", html_escape(&b.dir_name))
    } else {
        String::new()
    }
}
