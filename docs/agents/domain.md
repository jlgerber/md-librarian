# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root.
- **`docs/adr/`**: read ADRs that touch the area you're about to work in.

If any of these files don't exist, **proceed silently**. Don't flag their absence; don't suggest creating them upfront. The `/domain-modeling` skill (reached via `/grill-with-docs` and `/improve-codebase-architecture`) creates them lazily when terms or decisions actually get resolved. As of this file's creation neither exists here, which is the expected starting state.

Two files in this repo already carry domain knowledge and are worth reading alongside them:

- **`CLAUDE.md`** — the working notes: what each crate is for, the documentation-is-part-of-the-feature rule, and the gotchas.
- **`book/src/`** — the mdbook, one chapter per concern. It is the public documentation, not a substitute for `CONTEXT.md`: the book explains how to *use* a widget, while `CONTEXT.md` would define the vocabulary the code and issues speak in.

## File structure

This is a **single-context** repo — one `CONTEXT.md`, one `docs/adr/`, both at the root:

```
/
├── CONTEXT.md
├── docs/adr/
│   ├── 0001-....md
│   └── 0002-....md
└── crates/
    ├── md-librarian/
    ├── md-librarian-serve/
    ├── md-librarian-webview/
    └── md-librarian-cli/
```

The four crates are one pipeline (discover → serve → show) sharing one domain, not separate bounded contexts, so they take one glossary between them.

Multi-context repos instead put a `CONTEXT-MAP.md` at the root pointing at one `CONTEXT.md` per context, with context-scoped `docs/adr/` beside each. That layout does not apply here; if this repo ever grows genuinely separate contexts, adding `CONTEXT-MAP.md` at the root is what switches the skills over.

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `CONTEXT.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal: either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0007 (…), but worth reopening because…_
