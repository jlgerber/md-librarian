//! `md-librarian` — the standalone book library viewer, and its builder.
//!
//! Extracted from gpui-yaams (`yaams-books`) at v0.27.0-beta.3; the serve
//! path is unchanged apart from the names. The `build` subcommand is new here.
//!
//! ```text
//! md-librarian                                  # MD_LIBRARIAN_PATH, else the XDG default
//! md-librarian --root ~/books --root /opt/books # explicit roots, first wins
//! md-librarian --include gpui --include usdlite # only these titles
//! md-librarian --book gpui                      # open straight onto one book
//! md-librarian --no-window                      # serve only; prints the URL
//! md-librarian build                            # mdbook-build every stale book on the roots
//! md-librarian build ~/src/foo/docs --into ~/books   # build one book, install a copy
//! ```
//!
//! The serve path and its lifetime contract (`--exit-on-stdin-close`,
//! `--parent-pipe`) are in [`serve`]; building is in [`build`].

mod build;
mod serve;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    about = "Browse a library of mdbooks",
    long_about = "Serves every book found on the search path and opens a window on the library.\n\n\
                  Roots come from --root, else MD_LIBRARIAN_PATH (a stacking, colon-separated \
                  list), else $XDG_DATA_HOME/md-librarian/books. Earlier roots shadow later ones.\n\n\
                  `md-librarian build` brings the books on those roots up to date with mdbook."
)]
struct Cli {
    #[command(flatten)]
    serve: serve::ServeArgs,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run `mdbook build` on every stale book on the search path, or on the
    /// given book directories; optionally install slim copies into a root.
    Build(build::BuildArgs),
}

fn main() -> anyhow::Result<()> {
    // Logs go to STDERR so that `--no-window`'s stdout is exactly the URL and
    // nothing else — it is meant to be read by a script.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "md_librarian=info,md_librarian_serve=info,md_librarian_webview=info".into()
            }),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Some(Command::Build(args)) => std::process::exit(build::run(args)?),
        None => serve::run(cli.serve),
    }
}
