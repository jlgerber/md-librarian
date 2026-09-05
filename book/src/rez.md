# rez packaging

`package.py` ships this repo as the rez package **`md_librarian`** (rez reads
`-` as the name/version separator, so the hyphenated name is not legal). The
package holds the built tools and the rendered book, not the crates: another
Rust workspace depends on the crates as cargo git dependencies pinned to a tag.

| Tool | Crate | What it is |
|---|---|---|
| `md-librarian` | `md-librarian-cli` | the library viewer |
| `md-librarian-webview-demo` | `md-librarian-webview` | the `help` example, under a name safe for a shared PATH |

The package also installs this book as a discoverable root at
`{root}/books/md-librarian/` and **appends** `{root}/books` to
`MD_LIBRARIAN_PATH`, so a resolve opens with something in it and a user's own
roots still win.

```sh
just rez-build                 # rez build -i into ~/packages
just rez-build-isolated        # into /tmp/rez-md-librarian, touching nothing
just rez-test                  # the default tests: tools on PATH, the book root
rez test md_librarian books_serve   # binds a port; explicit
rez test md_librarian cargo_test    # the workspace suite; needs a source checkout
```

`REZ.md` at the repository root has the full description: the knobs
(`MD_LIBRARIAN_RUN_TESTS`, `MD_LIBRARIAN_SKIP_BOOK`), the pre-release
versioning trap, and the `WEBKIT_DISABLE_DMABUF_RENDERER` workaround.
