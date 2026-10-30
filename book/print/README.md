# The print edition

`building-a-chess-engine-in-rust.pdf` and `.epub` are the book in `book/src`, bound as one
volume: a cover, a contents page, the chapters numbered as the web book numbers them (6.1,
14.2, ...), seven figures, and an index. The PDF has real page numbers, a linked contents page
and index, and bookmarks; the EPUB reflows on an e-reader. Both come from
`scripts/build-print-book.sh`, which uses the makebook skill of the bookcraft Claude Code plugin
(marketplace `cjus/cjus-skills`).

## Rebuilding

After the book changes, run

```sh
scripts/build-print-book.sh
```

and commit the PDF and EPUB with the change. The cover names the commit the text and code
excerpts come from, so build after committing the chapter changes (it says "with uncommitted
changes" otherwise). The PDF changes on every build, if only its creation stamp, and each
version adds about 5 MB to the repository's history, so rebuild when the book has changed, not
on every commit.

One-time setup: install bookcraft from the `cjus-skills` marketplace in Claude Code, run its
`scripts/install.sh` (Python 3.12+, Chromium for Playwright), and `brew install poppler
epubcheck`.

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

Some things need a look by hand:

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
