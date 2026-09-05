# Mechanical renames for files imported from gpui-yaams (v0.27.0-beta.3).
# Apply with:  sed -i -f docs/superpowers/plans/rename-from-yaams.sed <files>
# ORDER MATTERS: longer / more specific names first, because several of the
# short names are prefixes of longer ones (`yaams-books` is a prefix of
# `yaams-bookserve`; `yaams-webview` of `yaams-webview-gtk`).

# --- thread and scratch names that the generic crate rename must not mangle
s/yaams-webview-gtk/md-librarian-gtk/g
s/yaams-webview-g\b/md-librarian-g/g
s/yaams-first-paint/md-librarian-first-paint/g

# --- crates (underscore = Rust path form, hyphen = package form)
s/yaams_booklib/md_librarian/g
s/yaams-booklib/md-librarian/g
s/yaams_bookserve/md_librarian_serve/g
s/yaams-bookserve/md-librarian-serve/g
s/yaams_webview/md_librarian_webview/g
s/yaams-webview/md-librarian-webview/g

# --- the binary (after bookserve, which it is a prefix of)
s/yaams_books/md_librarian/g
s/yaams-books/md-librarian/g

# --- the environment variable and the default data directory.
# YAAMS_BOOKS_DIR is yaams-tk2's and is deliberately NOT matched.
s/YAAMS_BOOK_PATH/MD_LIBRARIAN_PATH/g
s#yaams/books#md-librarian/books#g

# --- rez
s/GPUI_YAAMS_/MD_LIBRARIAN_/g
s/gpui_yaams/md_librarian/g

# --- the repository, EXCEPT links into gpui-yaams' GitHub issues / PRs / files,
# which are historical references and must keep pointing where they point.
/gpui-yaams\/\(issues\|pull\|blob\)/!s/gpui-yaams/md-librarian/g
