# md-librarian

A library of mdbooks: discovery on a search path, a loopback server with a card
page and a persistent bar, and a floating WebKit viewer.

```sh
cargo install --path crates/md-librarian-cli
MD_LIBRARIAN_PATH=~/books md-librarian          # every book under ~/books
md-librarian --book "usdlite user guide"        # straight onto one book
md-librarian --no-window                        # serve only; prints the URL
```

A **root** is a directory of books, each a subdirectory holding `book.toml`
beside its built output. Roots stack; the first one wins.

- `md-librarian` — discovery (what an application depends on)
- `md-librarian-serve` — the server and the generated shell
- `md-librarian-webview` — the floating WebKit window
- `md-librarian-cli` — the `md-librarian` binary

Docs: `just docs` (mdbook). Packaging: `REZ.md`. Extracted from
[gpui-yaams](https://github.com/jlgerber/gpui-yaams) at v0.27.0-beta.3.
