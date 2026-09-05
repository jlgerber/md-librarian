# md-librarian — working notes

`md-librarian` is the mdbook **library viewer**: `md-librarian` (discovery, pure),
`md-librarian-serve` (loopback server + generated shell), `md-librarian-webview`
(floating wry/WebKit window) and `md-librarian-cli` (the `md-librarian` binary).
Extracted from gpui-yaams at v0.27.0-beta.3; usdlite and yaams-tk2 consume it.

## Documentation is part of the feature

**Every public change MUST be documented in the mdbook in the same change**:

- Extend the relevant chapter under `book/src/` (`library.md` for discovery and
  serving, `webview.md` for the window, `getting-started.md` for host wiring),
  and keep `book/src/SUMMARY.md` current.
- Every new public function or CLI flag gets a **usage snippet** with the real
  signature.
- Run `just docs-build` before committing. The book is also the rez package's
  shipped library root, so a broken book ships.

## Gotchas

- **Only `md-librarian-cli` may link wry/tao/GTK.** `md-librarian` and
  `md-librarian-serve` are display-free by construction; that is what lets
  their tests run headless and lets an application depend on discovery without
  paying for WebKit. Do not add the webview crate as a dependency of either.
- **Wayland explicit-sync.** On some driver/compositor combinations every
  WebKitGTK window dies with `Error 71 (Protocol error)`. `just run` and the
  rez package default `WEBKIT_DISABLE_DMABUF_RENDERER=1` (guarded, so an
  explicit value wins). It is a workaround for other people's bug; see
  `book/src/library.md` → "Known issue".
- **Roots stack, first wins.** `MD_LIBRARIAN_PATH` is a list; the rez package
  *appends* its own book so a user's roots shadow it. Keep that direction.
- **Identity is the title.** mdbook rejects unknown `[book]` keys, so a book
  cannot carry an id. Do not design around one.
- **Issue numbers in the webview crate** (`#43`, `#47`) refer to gpui-yaams.

## Agent skills

### Issue tracker

GitHub issues in `jlgerber/md-librarian`, via `gh`. See `docs/agents/issue-tracker.md`.

### Triage labels

See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root (created lazily).
See `docs/agents/domain.md`.
