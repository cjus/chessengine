"""Stage the mdBook sources as a folder bookcraft's makebook can bind.

    python3 book/print/stage.py <book/src> <staging dir> <commit>

`scripts/build-print-book.sh` runs this; see `book/print/README.md`. It never writes to
`book/src`. In the staging folder it:

- fills in every `{{#include file:anchor}}` the way mdBook does, so the print edition shows the
  same code as the web book;
- names the files `NN-<name>.md` in SUMMARY.md order, because makebook binds in filename order;
- places the figures in `book/print/diagrams/` after the paragraphs listed in `FIGURES`;
- turns the tables in `TRANSPOSE` on their side, because they are too wide for a 6in page;
- writes `book.json`: `book/print/book.json` plus the chapter labels (6.1, 14.2, ...), the parts
  and the commit, all derived from SUMMARY.md and git.

Every anchor here must still match the text. When a chapter is rewritten and one doesn't, the
script stops and names it: move the anchor, or drop the figure.
"""

import json
import re
import shutil
import sys
import textwrap
from pathlib import Path

HERE = Path(__file__).resolve().parent

# Figures: (chapter file, anchor regex, where, figure). "after" puts the figure after the
# paragraph or list the anchor ends; "before" puts it before the anchor's line. Why each figure
# is here, and why other chapters have none: diagrams/README.md.
FIGURES = [
    ("ch02-movegen.md", r"may only move along\s+that line\.", "after", "check-mask-and-pin"),
    ("ch03-magics.md", r"for every one of the 1024 patterns\.", "after", "magic-index"),
    ("ch05-alphabeta.md", r"a \*\*beta cutoff\*\*\.", "after", "alpha-beta-cutoff"),
    ("ch06-iterative.md", r"Stage 7's quiescence search fixes it\.", "after", "time-limits"),
    ("ch08-1-tt-buckets.md", r"looks at\s+one of them\.", "after", "tt-bucket"),
    ("ch14-1-extensions.md", r"That is a \*\*double extension\*\*\.", "after", "singular-outcomes"),
    ("ch21-nnue.md", r"^## Whole numbers instead of floats", "before", "nnue-accumulator"),
]

# Tables too wide for the 6in measure, by the start of their header row: rows become columns.
TRANSPOSE = [("ch19-levels.md", "| Level | 0 | 2 | 4 |")]

INCLUDE = re.compile(r"\{\{#include\s+([^}:\s]+)(?::([A-Za-z0-9_]+))?\s*\}\}")
ANCHOR_LINE = re.compile(r"ANCHOR(_END)?:\s*[A-Za-z0-9_]+")


def resolve(md_path: Path, m: re.Match) -> str:
    """The text an mdBook include stands for, dedented so it fits the page better."""
    target = (md_path.parent / m.group(1)).resolve()
    lines = target.read_text(encoding="utf-8").splitlines()
    name = m.group(2)
    if name:
        start = re.compile(rf"ANCHOR:\s*{name}\b")
        end = re.compile(rf"ANCHOR_END:\s*{name}\b")
        try:
            i = next(k for k, ln in enumerate(lines) if start.search(ln))
            j = next(k for k in range(i + 1, len(lines)) if end.search(lines[k]))
        except StopIteration:
            sys.exit(f"error: anchor {name!r} not found in {target}")
        lines = lines[i + 1:j]
    # mdBook drops every other anchor marker inside the included range.
    lines = [ln for ln in lines if not ANCHOR_LINE.search(ln)]
    return textwrap.dedent("\n".join(lines)).strip("\n")


def place(text: str, f: str) -> str:
    for name, pat, where, fig in FIGURES:
        if name != f:
            continue
        m = re.search(pat, text, flags=re.M)
        if not m:
            sys.exit(f"error: figure anchor {pat!r} not found in {f}; see book/print/README.md")
        img = f"![](diagrams/{fig}.svg)"
        if where == "after":
            end = text.find("\n\n", m.end())
            end = len(text) if end < 0 else end
            text = text[:end] + f"\n\n{img}" + text[end:]
        else:
            text = text[:m.start()] + f"{img}\n\n" + text[m.start():]
    return text


def transpose(text: str, f: str) -> str:
    for name, head in TRANSPOSE:
        if name != f:
            continue
        lines = text.split("\n")
        i = next((k for k, ln in enumerate(lines) if ln.startswith(head)), None)
        if i is None:
            sys.exit(f"error: table {head!r} not found in {f}; see book/print/README.md")
        j = i
        while j < len(lines) and lines[j].startswith("|"):
            j += 1
        rows = [[c.strip() for c in ln.strip().strip("|").split("|")]
                for k, ln in enumerate(lines[i:j]) if k != 1]
        cols = list(zip(*rows))
        new = ["| " + " | ".join(cols[0]) + " |",
               "| " + " | ".join(["---"] + ["---:"] * (len(cols[0]) - 1)) + " |"]
        new += ["| " + " | ".join(c) + " |" for c in cols[1:]]
        lines[i:j] = new
        text = "\n".join(lines)
    return text


def main() -> None:
    src, out, commit = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), sys.argv[3]
    out.mkdir(parents=True, exist_ok=True)

    # Reading order, parts and labels, from SUMMARY.md. A chapter's label is its number in the
    # file name (ch06-1-... is 6.1); the introduction and how-to-use pages have none.
    order, labels, sections = [], [], []
    part = None
    for ln in (src / "SUMMARY.md").read_text(encoding="utf-8").splitlines():
        if ln.startswith("# Part"):
            part = ln[2:].strip()
            sections.append({"title": part, "chapters": []})
            continue
        m = re.search(r"\]\(([^)]+\.md)\)", ln)
        if not m:
            continue
        f = m.group(1)
        order.append(f)
        cm = re.match(r"ch(\d+)(?:-(\d+))?-", f)
        labels.append("" if not cm else
                      str(int(cm.group(1))) + (f".{cm.group(2)}" if cm.group(2) else ""))
        if part and cm:
            sections[-1]["chapters"].append(len(order))

    for n, f in enumerate(order):
        p = src / f
        text = p.read_text(encoding="utf-8")
        text = INCLUDE.sub(lambda m: resolve(p, m), text)
        # mdBook-only fence attribute; the binder wants a plain language name.
        text = re.sub(r"^```rust,ignore\s*$", "```rust", text, flags=re.M)
        text = place(text, f)
        text = transpose(text, f)
        (out / f"{n:02d}-{Path(f).stem}.md").write_text(text, encoding="utf-8")

    cfg = json.loads((HERE / "book.json").read_text(encoding="utf-8"))
    cfg["footnote"] = cfg["footnote"].replace("{commit}", commit)
    cfg["labels"] = labels
    cfg["sections"] = sections
    (out / "book.json").write_text(json.dumps(cfg, indent=2, ensure_ascii=False) + "\n",
                                   encoding="utf-8")
    shutil.copy(HERE / "terms.txt", out / "terms.txt")
    shutil.copytree(HERE / "diagrams", out / "diagrams", dirs_exist_ok=True)
    print(f"staged {len(order)} chapters in {out}")


if __name__ == "__main__":
    main()
