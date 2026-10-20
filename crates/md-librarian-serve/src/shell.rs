//! The generated pages: the **shell** (a persistent bar over an iframe), the
//! **grid** of cards, and the escaping rules both depend on.
//!
//! Everything here is a pure function from a book list to HTML, which is the
//! point: `book/src/docs-window.md` records that anything touching a real window
//! needs GTK and a display and can only be a manual checklist, while *this* half
//! is testable — and is where all the decisions live.

use md_librarian::{Book, Entry, Library};

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

/// The library's background, as opaque RGB — the one colour every surface
/// that can paint before a page does should agree on.
///
/// The library page's CSS, the shell's own body, the frame's default and the
/// native window and webview behind all of them (`md-librarian-cli` passes this
/// to `WebWindowOptions::background`) are all derived from it, so a surface
/// cannot drift to a different dark — or back to white — on its own. Issue #8.
pub const LIBRARY_BACKGROUND: (u8, u8, u8) = (0x0f, 0x14, 0x19);

/// [`LIBRARY_BACKGROUND`] as a CSS colour, `#rrggbb`.
pub fn library_background_css() -> String {
    let (r, g, b) = LIBRARY_BACKGROUND;
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Shared CSS for both generated documents.
fn style() -> String {
    format!(
        "*{{box-sizing:border-box}}\
         body{{margin:0;background:{bg};color:#c9d1d9;\
          font:14px/1.5 system-ui,-apple-system,Segoe UI,sans-serif}}\
         a{{color:inherit;text-decoration:none}}",
        bg = library_background_css()
    )
}

/// How long the frame stays hidden waiting for a book to load before it is
/// shown regardless, in milliseconds.
///
/// The same bound, for the same reason, as the webview's first-paint wait: a
/// page that never finishes loading must not leave an empty window. A book
/// served from loopback loads in tens of milliseconds and never reaches it.
pub const FRAME_REVEAL_TIMEOUT_MS: u32 = 1500;

/// The shell's script. Plain JavaScript, not a `format!` template, so it reads
/// as the language it is; [`shell_document`] defines `MAP`, `LIB` and `WAIT`
/// ahead of it.
///
/// - **Adopt.** After each load, read the frame's real background and paint
///   the frame element with it, so the blank between two chapters of one book
///   is the book's own colour. "Real background" follows the CSS canvas rule —
///   the root element's, else the body's. mdBook sets it on `<html>` and leaves
///   `<body>` transparent, so reading only the body (as through #7) never
///   adopted a book's colour.
/// - **Veil.** When a click *leaves* the book on screen — a card, the Library
///   button, a link into another book — hide the frame at once and fade the
///   new page in once it has loaded. A veiled frame is transparent, so what
///   shows is the shell's body: always the library colour, never a half-loaded
///   page or WebKit's white. That includes a light book opened a second time:
///   it goes dark, then fades in light. Navigations inside one book are not
///   veiled: the adopted colour already hides those, and veiling every chapter
///   would trade a flash of the book's colour for a dark one.
/// - **Unrecognised navigations fall back to the library colour.** A page
///   that unloads without a click this script saw — WebKit's context-menu
///   Back/Forward, a mouse's back button, a script setting `location` — may be
///   heading anywhere, including from a light book to the dark library. So
///   `pagehide` repaints the frame with `LIB` (not veiled). mdBook's
///   arrow-key chapter keys are recognised as staying in the book. The cost:
///   going back between two chapters of a light book shows a dark blank
///   instead of the book's colour.
/// - **Name.** Show the current book in the bar and hide the Library button
///   on the library itself.
///
/// A click whose default was prevented, a modified or non-primary click, a
/// `download` link, a link with another target and a link to another origin
/// are all left alone, and the [`FRAME_REVEAL_TIMEOUT_MS`] bound reveals the
/// frame even if a navigation that was veiled never loads.
///
/// The frame's listeners are attached on its `load`. A click on a page that
/// has not finished loading (a slow image, say) is therefore not seen; it
/// falls to the `pagehide` fallback above. There is no event the parent can
/// hear earlier for a document that does not exist yet.
const SHELL_SCRIPT: &str = r#"
var f=document.getElementById('docframe'),c=document.getElementById('current'),
  k=document.getElementById('back');
var cur=null,veil=0,known=false;
function owner(p){
  if(p==='/_grid')return '/_grid';
  for(var i=0;i<MAP.length;i++){if(p.indexOf(MAP[i][0])===0)return MAP[i][0]}
  return null;
}
function plain(e){
  return !e.defaultPrevented&&e.button===0&&!e.metaKey&&!e.ctrlKey&&!e.shiftKey&&!e.altKey;
}
function leave(dest){
  if(!dest||dest===cur)return;
  f.style.background=LIB;
  f.className='veiled';
  clearTimeout(veil);veil=setTimeout(function(){f.className=''},WAIT);
}
function follow(e){
  if(!plain(e))return;
  var a=e.target&&e.target.closest&&e.target.closest('a[href]');
  if(!a||a.hasAttribute('download'))return;
  var t=a.getAttribute('target');if(t&&t!=='_self')return;
  var u;try{u=new URL(a.href)}catch(x){return}
  if(u.origin!==location.origin)return;
  var h=f.contentWindow.location;
  if(u.hash&&u.pathname===h.pathname&&u.search===h.search)return;
  known=true;leave(owner(u.pathname));
}
function chapterKey(e){
  if(!e.defaultPrevented&&!e.metaKey&&!e.ctrlKey&&!e.shiftKey&&!e.altKey&&
    (e.key==='ArrowLeft'||e.key==='ArrowRight'))known=true;
}
function unknownExit(){if(!known)f.style.background=LIB}
k.addEventListener('click',function(e){if(plain(e)){known=true;leave('/_grid')}});
f.addEventListener('load',function(){
  clearTimeout(veil);f.className='';known=false;
  var bg='';
  try{
    var d=f.contentDocument,els=d?[d.documentElement,d.body]:[];
    for(var j=0;j<els.length&&!bg;j++){
      if(!els[j])continue;
      var v=getComputedStyle(els[j]).backgroundColor;
      if(v&&v!=='transparent'&&v!=='rgba(0, 0, 0, 0)')bg=v;
    }
    if(bg)f.style.background=bg;
  }catch(e){}
  var p;try{p=f.contentWindow.location.pathname}catch(e){cur=null;k.className='';return}
  cur=owner(p);
  try{
    var w=f.contentWindow;
    w.addEventListener('click',follow);
    w.addEventListener('keydown',chapterKey);
    w.addEventListener('pagehide',unknownExit);
  }catch(e){}
  k.className=p==='/_grid'?'home':'';
  var t='';for(var i=0;i<MAP.length;i++){if(p.indexOf(MAP[i][0])===0){t=MAP[i][1];break}}
  c.textContent=t;document.title=t?t+' \u2014 Books':'Books';
});
"#;

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
/// defaults to the library's background ([`LIBRARY_BACKGROUND`]) — never
/// white, the worst case against a dark book — and after each load the shell
/// reads the book's real background and adopts it, so later navigations
/// within that book flash the book's own colour and are effectively
/// invisible.
///
/// Moving *between* books, or back to the library, is the case adoption alone
/// gets wrong: the frame still holds the colour of the book being left, which
/// is white after a light book. So those navigations are veiled instead — the
/// frame is hidden, so the shell's library-coloured body shows, and the new
/// page fades in on load. See [`SHELL_SCRIPT`] for the exact rules.
///
/// The blank itself cannot be eliminated from here: it belongs to the frame's
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
         <title>Books</title><style>{style}\
         html,body{{height:100%}}\
         body{{display:flex;flex-direction:column}}\
         header{{display:flex;align-items:center;gap:.9rem;padding:.5rem .9rem;\
           background:#161c24;border-bottom:1px solid #28313b;flex:0 0 auto}}\
         header a{{padding:.25rem .6rem;border-radius:.35rem;background:#1e2731;\
           border:1px solid #28313b}}\
         header a:hover{{border-color:#4b9fff}}\
         #current{{color:#8b949e}}\
         #back.home{{visibility:hidden}}\
         iframe{{flex:1 1 auto;width:100%;border:0;background:{bg};\
           transition:opacity .15s ease-out}}\
         iframe.veiled{{opacity:0;transition:none}}\
         </style></head><body>\
         <header><a id=\"back\" href=\"/_grid\" target=\"docframe\">&#8592; Library</a>\
         <span id=\"current\"></span></header>\
         <iframe id=\"docframe\" name=\"docframe\" src=\"{initial_src}\" title=\"book\"></iframe>\
         <script>var MAP=[{map}];var LIB=\"{bg}\";var WAIT={FRAME_REVEAL_TIMEOUT_MS};\
         {SHELL_SCRIPT}</script></body></html>",
        style = style(),
        bg = library_background_css(),
    )
}

/// The library page: one section per [`Library`], in search-path order, each a
/// heading (the library's name, its root path beneath it and as a tooltip), a
/// rule, and that library's cards. `missing` — titles an include-list names
/// that no root provides — follows as a last section of dead cards.
///
/// A heading is shown even when there is only one library, so the page reads
/// the same however many roots are on the path. The page scrolls as a whole;
/// libraries are never collapsed or put in their own scroll regions.
pub fn grid_document(libraries: &[Library], missing: &[String]) -> String {
    let sections = if libraries.is_empty() && missing.is_empty() {
        format!(
            "<p class=\"empty\">No books found. Point <code>{}</code> at a directory of \
             mdbooks — one subdirectory per book, each with its own <code>book.toml</code>.</p>",
            md_librarian::BOOK_PATH_VAR
        )
    } else {
        let mut out: Vec<String> = libraries.iter().map(library_section).collect();
        if !missing.is_empty() {
            let cards: String = missing.iter().map(|t| missing_card(t)).collect();
            out.push(format!(
                "<section class=\"library missing\"><h2 class=\"name\">Not found</h2>\
                 <p class=\"root\">named by the include-list, in no root</p>\
                 <hr class=\"rule\"><div class=\"grid\">{cards}</div></section>"
            ));
        }
        out.join("")
    };

    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Library</title>\
         <style>{style}\
         main{{padding:1.6rem;max-width:1100px;margin:0 auto}}\
         .library+.library{{margin-top:2.2rem}}\
         .library .name{{font-size:1.2rem;margin:0;font-weight:600;color:#e6edf3}}\
         .library .root{{color:#6e7681;font-size:.82em;margin:.15rem 0 0;\
           font-family:ui-monospace,monospace;overflow-wrap:anywhere}}\
         .rule{{border:0;border-top:1px solid #28313b;margin:.7rem 0 1rem}}\
         .grid{{display:grid;gap:1rem;\
           grid-template-columns:repeat(auto-fill,minmax(240px,1fr))}}\
         .card{{display:block;border:1px solid #28313b;border-radius:10px;\
           background:#161c24;overflow:hidden;transition:border-color .15s,transform .15s}}\
         a.card:hover{{border-color:#4b9fff;transform:translateY(-2px)}}\
         .card .body{{padding:.9rem 1rem 1.1rem}}\
         .cover,img.cover{{display:block;width:100%;height:150px;background:{bg}}}\
         img.cover{{object-fit:contain;padding:.5rem;box-sizing:border-box}}\
         .title{{font-weight:600;margin:0 0 .25rem}}\
         .desc{{color:#8b949e;margin:0}}\
         .dir{{color:#6e7681;font-size:.85em;margin:.35rem 0 0;font-family:ui-monospace,monospace}}\
         .dead{{opacity:.55}}\
         .why{{color:#d29922;font-size:.85em;margin:.35rem 0 0}}\
         .empty{{color:#8b949e}}\
         </style></head><body><main>{sections}</main></body></html>",
        style = style(),
        bg = library_background_css(),
    )
}

/// One library: heading, root path, rule, cards.
fn library_section(lib: &Library) -> String {
    let root = html_escape(&lib.root.display().to_string());
    let cards: String = lib.books.iter().map(book_card).collect();
    format!(
        "<section class=\"library\" data-root=\"{index}\">\
         <h2 class=\"name\" title=\"{root}\">{name}</h2>\
         <p class=\"root\">{root}</p><hr class=\"rule\">\
         <div class=\"grid\">{cards}</div></section>",
        index = lib.root_index,
        name = html_escape(&lib.name),
    )
}

/// One card.
///
/// **Never link content that is not there.** A link to a missing page opens a
/// 404 in a window with no back button, so an unbuilt book and a title no root
/// provides are both rendered as inert `<div>`s that say why — listed, because
/// hiding them makes a typo or an unmounted root indistinguishable from a book
/// that was never asked for.
fn book_card(b: &Book) -> String {
    if b.is_built() {
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
    } else {
        format!(
            "<div class=\"card dead\">{cover}<div class=\"body\">\
             <p class=\"title\">{title}</p>{desc}{dir}\
             <p class=\"why\">not built yet</p></div></div>",
            cover = generated_cover(&b.title),
            title = html_escape(&b.title),
            desc = description(&b.description),
            dir = dir_line(b),
        )
    }
}

/// The dead card for a title an include-list names that no root provides.
fn missing_card(title: &str) -> String {
    format!(
        "<div class=\"card dead\">{cover}<div class=\"body\">\
         <p class=\"title\">{t}</p>\
         <p class=\"why\">not found in any root</p></div></div>",
        cover = generated_cover(title),
        t = html_escape(title),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_background_css_is_the_rgb_constant() {
        assert_eq!(library_background_css(), "#0f1419");
    }

    #[test]
    fn the_library_page_and_the_shell_paint_their_body_with_the_library_background() {
        let body = format!("body{{margin:0;background:{}", library_background_css());
        assert!(grid_document(&[], &[]).contains(&body));
        assert!(shell_document(&[], None).contains(&body));
    }

    #[test]
    fn the_frame_defaults_to_the_library_background_not_grey_or_white() {
        let shell = shell_document(&[], None);
        assert!(shell.contains(&format!(
            "iframe{{flex:1 1 auto;width:100%;border:0;background:{}",
            library_background_css()
        )));
        assert!(!shell.contains("#6e7681"), "the old grey default is gone");
        assert!(!shell.contains("background:#fff"));
    }

    #[test]
    fn the_script_is_handed_the_library_colour_and_the_reveal_bound() {
        let shell = shell_document(&[], None);
        assert!(shell.contains(&format!("var LIB=\"{}\";", library_background_css())));
        assert!(shell.contains(&format!("var WAIT={FRAME_REVEAL_TIMEOUT_MS};")));
    }

    #[test]
    fn the_shell_styles_a_veiled_frame_as_hidden() {
        assert!(shell_document(&[], None).contains("iframe.veiled{opacity:0"));
    }

    #[test]
    fn the_script_is_injected_verbatim() {
        assert!(
            !SHELL_SCRIPT.contains("</script"),
            "it cannot end its own element"
        );
        assert!(
            shell_document(&[], None).contains(SHELL_SCRIPT),
            "inserted as-is, not through a format template that would halve its braces"
        );
    }

    #[test]
    fn a_library_name_and_root_cannot_break_out_of_the_heading() {
        let lib = Library {
            name: "a\"<b>".into(),
            root: std::path::PathBuf::from("/x/\" onmouseover=alert(1) \"/a\"<b>"),
            root_index: 3,
            books: Vec::new(),
        };
        let page = grid_document(&[lib], &[]);
        assert!(page.contains(">a&quot;&lt;b&gt;</h2>"));
        assert!(
            !page.contains("\" onmouseover"),
            "the tooltip stays one attribute"
        );
        assert!(page.contains("data-root=\"3\""));
    }

    #[test]
    fn no_libraries_and_nothing_missing_is_the_actionable_empty_page() {
        assert!(grid_document(&[], &[]).contains("class=\"empty\""));
        let only_missing = grid_document(&[], &["Typo".to_string()]);
        assert!(!only_missing.contains("class=\"empty\""));
        assert!(only_missing.contains("not found in any root"));
    }
}
