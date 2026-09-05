# md-librarian

A **library of mdbooks**: put built books under one or more directories, and
`md-librarian` finds them, serves them over a loopback origin, and opens a window
with a card per book, a persistent bar, and a way back.

```text
md-librarian                                  # MD_LIBRARIAN_PATH, else the XDG default
md-librarian --root ~/books --root /opt/books # explicit roots; the first wins
md-librarian --book "usdlite user guide"      # open straight onto one book
md-librarian --no-window                      # serve only; prints the URL
```

Four crates, one binary:

| Crate | What it is | Links GTK? |
|---|---|---|
| `md-librarian` | discovery: what counts as a book, which roots, which copy wins | no |
| `md-librarian-serve` | the loopback server and the generated shell page | no |
| `md-librarian-webview` | a floating WebKit (wry) window | yes |
| `md-librarian-cli` | the `md-librarian` binary, tying the three together | yes |

An application depends on `md-librarian` alone to ask "is that book installed?",
and launches the binary out of process to show it — see
[Getting started](./getting-started.md). The design and its measured
constraints are in [A library of books](./library.md).

This project was extracted from
[gpui-yaams](https://github.com/jlgerber/gpui-yaams) at v0.27.0-beta.3, where
it was `yaams-booklib`, `yaams-bookserve`, `yaams-webview` and `yaams-books`.
