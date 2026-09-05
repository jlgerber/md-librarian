# Releasing

md-librarian releases as a **single unit**: all four crates inherit
`version.workspace = true`, and each release is a git tag `vX.Y.Z` that
consumers pin.

1. Verify: `just test`, `just docs-build`.
2. Bump `workspace.package.version` in the root `Cargo.toml` per semver.
3. Move the `[Unreleased]` entries in `CHANGELOG.md` under `## [X.Y.Z] - YYYY-MM-DD`.
4. Commit and push `main`.
5. Tag and push:
   ```sh
   git tag -a vX.Y.Z -m "vX.Y.Z — <summary>"
   git push origin vX.Y.Z
   ```
6. In each consumer, bump the `tag`:
   ```toml
   md-librarian = { git = "https://github.com/jlgerber/md-librarian", tag = "vX.Y.Z" }
   ```

Versioning: **patch** for internal fixes, **minor** for additive API or new CLI
flags, **major** for breaking API, a renamed flag, or a change to the
`MD_LIBRARIAN_PATH` / root-layout contract.
