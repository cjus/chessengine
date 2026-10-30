# The print edition

The book in `book/src`, bound as one volume: a cover, a contents page, the chapters numbered as
the web book numbers them (6.1, 14.2, ...), seven figures, and an index. The PDF has real page
numbers, a linked contents page and index, and bookmarks; the EPUB reflows on an e-reader.

- [PDF](https://cdn.pnxstudios.com/solrac/book/latest/building-a-chess-engine-in-rust.pdf)
- [EPUB](https://cdn.pnxstudios.com/solrac/book/latest/building-a-chess-engine-in-rust.epub)

Each edition is also kept under the commit it was bound from, at
`https://cdn.pnxstudios.com/solrac/book/<commit>/`, and the cover names that commit. The files
live on the PNX Studios CDN (S3 bucket `pnxstudios-solrac`), not in the repository: a PDF is
compressed, so Git would keep a whole new 5 MB copy of every rebuild. Git ignores them here.

## Rebuilding

After the book changes, commit the change, then run

```sh
scripts/build-print-book.sh
```

It builds the PDF and EPUB here in `book/print/`, then uploads them under the commit and as the
latest edition, and invalidates the latest on CloudFront. It won't upload with uncommitted
changes to `book/src`, `crates` or `book/print`, since the commit's copy has to be what that
commit binds. `scripts/build-print-book.sh --no-upload` builds without publishing, to look at a
change first; makebook options, such as `--type-size 17` for large print, need `--no-upload` too,
so a variant never replaces the published edition.

It uses the makebook skill of the bookcraft Claude Code plugin (marketplace `cjus/cjus-skills`),
unmodified. One-time setup: install bookcraft 1.17.0 or later in Claude Code, run its
`scripts/install.sh` (Python 3.12+, Chromium for Playwright), `brew install poppler epubcheck`,
and have the AWS CLI signed in with write access to the bucket (`AWS_PROFILE` picks a profile).

## What's here

| File | What it is |
|---|---|
| `stage.py` | Turns `book/src` into a folder makebook can bind (see its docstring) |
| `book.json` | The cover text and binding settings; `stage.py` adds the labels, parts and commit |
| `terms.txt` | The index: curated terms, one per line (format in bookcraft's makebook docs) |
| `figures.py` | Draws the figures into `diagrams/` |
| `diagrams/` | The figures, and `README.md`: why each one is there and why other chapters have none |

## When the book changes

The build stops and says so when something no longer fits:

- **A figure's anchor isn't found.** `stage.py` places each figure after a phrase in its
  chapter (`FIGURES`). If the phrase was rewritten, point the anchor at the new text, or drop
  the figure.
- **A transposed table isn't found** (`TRANSPOSE`): the same, for the chapter 19 table that is
  too wide for the page.

Some things need a look by hand, best with `--no-upload` before publishing:

- **The figures' facts.** If a chapter's numbers change (chapter 6's UCI output, the singular
  extension's margins in chapter 14.1), change them in `figures.py` too. `diagrams/README.md`
  says where each figure's facts come from.
- **A new chapter.** It is picked up from `SUMMARY.md` with its label and part. Add its
  techniques to `terms.txt`, and decide whether it needs a figure (and record that in
  `diagrams/README.md` either way).
- **The binder's report.** It lists index terms that matched nothing (fix the term in
  `terms.txt`), tables with a word broken in two (add them to `TRANSPOSE` or shorten the cells),
  and figures whose labels print below 8pt. Code lines wider than the page wrap; nothing is cut.
- **The figure pages.** Look at each one in the PDF: a figure that renders wrong doesn't fail
  the build.
