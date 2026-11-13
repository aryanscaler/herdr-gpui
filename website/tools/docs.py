#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["Markdown==3.8.2"]
# ///
"""Build sheprd.org/docs from Markdown.

Run from anywhere:   uv run website/tools/docs.py           (writes website/site/docs)
Check only:          uv run website/tools/docs.py --check   (writes nothing)

Pages come from website/docs/*.md, plus the user-facing guides in docs/ that
REPO_DOCS names, so those keep one copy. The output is generated at deploy
time and is not tracked: parallel pull requests edit docs/cli.md constantly,
and committed HTML would make each of them conflict.
"""

from __future__ import annotations

import argparse
import html
import json
import re
import shutil
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCES = ROOT / "website/docs"
THEME = Path(__file__).resolve().parent / "docs-theme"
OUT = ROOT / "website/site/docs"
SITE = "https://sheprd.org"
REPO = "https://github.com/sheprd-labs/sheprd"
STATUSES = {
    "shipped": ("Shipped", "st-shipped"),
    "optin": ("Opt-in", "st-optin"),
    "experimental": ("Experimental", "st-experimental"),
    "testflight": ("TestFlight beta", "st-testflight"),
}
FRONT_MATTER_KEYS = {"title", "blurb", "status", "platforms"}

# The order of groups and pages is the order readers see, so it is not sorted.
NAV: list[tuple[str, list[str]]] = [
    ("Start", ["introduction", "install", "quick-start", "updates"]),
    ("Workspaces", ["worktrees", "terminals", "review", "browser", "editors"]),
    ("Agents", ["chat", "orchestrator", "usage", "notifications"]),
    ("Machines", ["ssh-hosts", "cloud", "teleport", "windows", "remote-control", "relays"]),
    ("Customize", ["appearance", "keyboard", "configuration"]),
    ("Extend", ["automations", "mods", "cli"]),
    ("Reference", ["platforms", "troubleshooting"]),
]

# Guides in docs/ written for users. Their front matter lives here so GitHub
# keeps rendering the files as plain Markdown.
REPO_DOCS: dict[str, dict[str, str]] = {
    # keep-sorted start
    "automations": {"path": "docs/automations.md", "blurb": "Scheduled Luau jobs you approve first.", "status": "shipped", "platforms": "macOS, Linux, Windows"},
    "cli": {"path": "docs/cli.md", "blurb": "Drive the running app from scripts and agents."},
    "mods": {"path": "docs/mods.md", "blurb": "Sandboxed Luau that adds to the interface.", "status": "shipped", "platforms": "macOS, Linux, Windows"},
    # keep-sorted end
}

HOME = "introduction"


class DocsError(Exception):
    """A problem in the sources; the message names the file."""


@dataclass
class Page:
    slug: str
    source: Path
    group: str
    title: str
    blurb: str
    status: str | None = None
    platforms: str | None = None
    lead_html: str = ""
    body_html: str = ""
    ids: set[str] = field(default_factory=set)
    toc: list[tuple[str, str]] = field(default_factory=list)
    sections: list[dict[str, str]] = field(default_factory=list)

    @property
    def url(self) -> str:
        return "/docs/" if self.slug == HOME else f"/docs/{self.slug}/"


def github_slug(value: str, separator: str = "-") -> str:
    """Heading ids as GitHub makes them, so links into docs/ files keep working."""
    value = re.sub(r"<[^>]+>", "", value)
    value = html.unescape(value).strip().lower()
    value = re.sub(r"[^\w\- ]", "", value)
    return value.replace(" ", separator)


def parse_front_matter(text: str, where: str) -> tuple[dict[str, str], str]:
    if not text.startswith("---\n"):
        return {}, text
    end = text.find("\n---\n", 4)
    if end < 0:
        raise DocsError(f"{where}: front matter has no closing ---")
    meta: dict[str, str] = {}
    for line in text[4:end].splitlines():
        if not line.strip():
            continue
        key, sep, value = line.partition(":")
        key = key.strip()
        if not sep or key not in FRONT_MATTER_KEYS:
            raise DocsError(f"{where}: unknown front matter line {line!r}")
        meta[key] = value.strip()
    return meta, text[end + 5 :]


_LIST_ITEM = re.compile(r"^(\s*)([-*+]|\d+\.)\s")
_FENCE = re.compile(r"^\s*(```|~~~)")


def normalize(text: str) -> str:
    """Bridge GitHub Markdown to Python-Markdown outside code fences.

    GitHub starts a list or a table right after a paragraph line and nests
    lists with two spaces; Python-Markdown needs a blank line first and four.
    """
    out: list[str] = []
    fenced = False
    for line in text.splitlines():
        if _FENCE.match(line):
            fenced = not fenced
            out.append(line)
            continue
        if fenced:
            out.append(line)
            continue
        item = _LIST_ITEM.match(line)
        prev = out[-1] if out else ""
        starts_block = (item and not item.group(1)) or line.startswith("|")
        prev_is_text = prev.strip() and not _LIST_ITEM.match(prev) and not prev.startswith(("|", ">", " ", "#"))
        if starts_block and prev_is_text:
            out.append("")
        if item and item.group(1) and len(item.group(1)) % 4:
            line = " " * (len(item.group(1)) * 2) + line.lstrip()
        out.append(line)
    return "\n".join(out) + "\n"


def split_title_and_lead(text: str, where: str) -> tuple[str, str, str]:
    lines = text.strip("\n").splitlines()
    if not lines or not lines[0].startswith("# "):
        raise DocsError(f"{where}: the first line must be the page's # title")
    title = lines[0][2:].strip()
    rest = lines[1:]
    while rest and not rest[0].strip():
        rest.pop(0)
    lead: list[str] = []
    while rest and rest[0].strip() and not rest[0].startswith(("#", "```", "|", ">", "-", "*")):
        lead.append(rest.pop(0))
    if not lead:
        raise DocsError(f"{where}: the title needs a lead paragraph under it")
    if any(re.match(r"^# \S", line) for line in _outside_fences(rest)):
        raise DocsError(f"{where}: only the title may be a level-one heading")
    return title, " ".join(lead), "\n".join(rest)


def _outside_fences(lines: list[str]) -> list[str]:
    out, fenced = [], False
    for line in lines:
        if _FENCE.match(line):
            fenced = not fenced
        elif not fenced:
            out.append(line)
    return out


def _markdown():
    import markdown  # imported late so --help works without it

    return markdown.Markdown(
        extensions=["fenced_code", "tables", "toc", "sane_lists"],
        extension_configs={"toc": {"slugify": github_slug, "toc_depth": "2-3"}},
    )


_CALLOUT = re.compile(r"<blockquote>\s*<p><strong>([^<]+?)</strong>\s*(.*?)</blockquote>", re.S)


def decorate(body: str) -> str:
    body = _CALLOUT.sub(lambda m: f'<aside class="note"><b>{m.group(1).rstrip(".:")}</b><p>{m.group(2)}</aside>', body)
    body = body.replace("<table>", '<div class="table-wrap"><table>').replace("</table>", "</table></div>")
    # GitHub reads `\|` in a table cell as a literal pipe, even inside code;
    # Python-Markdown keeps the backslash in code spans.
    return re.sub(r"<table>.*?</table>", lambda m: m.group(0).replace("\\|", "|"), body, flags=re.S)


def load_page(slug: str, group: str, source: Path, meta: dict[str, str], repo_root: Path) -> Page:
    where = source.relative_to(repo_root).as_posix()
    text = source.read_text(encoding="utf-8")
    front, text = parse_front_matter(text, where)
    meta = {**front, **{k: v for k, v in meta.items() if k != "path"}}
    title, lead, rest = split_title_and_lead(text, where)
    if meta.get("title", title) != title:
        raise DocsError(f"{where}: front matter title {meta['title']!r} differs from the # title {title!r}")
    if not meta.get("blurb"):
        raise DocsError(f"{where}: front matter needs a blurb")
    status = meta.get("status") or None
    if status and status not in STATUSES:
        raise DocsError(f"{where}: status must be one of {', '.join(STATUSES)}, not {status!r}")
    md = _markdown()
    body = decorate(md.convert(normalize(rest)))
    lead_html = _markdown().convert(lead).removeprefix("<p>").removesuffix("</p>")
    page = Page(slug, source, group, title, meta["blurb"], status, meta.get("platforms") or None, lead_html, body)
    page.ids = set(re.findall(r'\bid="([^"]+)"', body))
    page.toc = [(html.unescape(t["name"]), t["id"]) for t in md.toc_tokens if t["level"] == 2]
    page.sections = sections_for_search(body)
    return page


def sections_for_search(body: str) -> list[dict[str, str]]:
    parts = re.split(r'<h2 id="([^"]+)">(.*?)</h2>', body)
    out = []
    for i in range(1, len(parts) - 1, 3):
        text = re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", parts[i + 2]))).strip()
        out.append({"id": parts[i], "title": html.unescape(re.sub(r"<[^>]+>", "", parts[i + 1])), "text": text})
    return out


def rewrite_links(page: Page, pages: dict[str, Page], by_path: dict[Path, Page], sources: Path, repo_root: Path) -> list[str]:
    errors: list[str] = []
    where = page.source.relative_to(repo_root).as_posix()

    def fix(match: re.Match[str]) -> str:
        href = html.unescape(match.group(1))
        if re.match(r"^(https?:|mailto:)", href):
            return match.group(0)
        path, _, frag = href.partition("#")
        if not path:
            if frag not in page.ids:
                errors.append(f"{where}: no heading #{frag} on this page")
            return match.group(0)
        target = (page.source.parent / path).resolve()
        # website/docs pages link every page by its slug, wherever its source lives.
        sibling = pages.get(target.stem) if target.parent == sources and target.suffix == ".md" else None
        if target in by_path or sibling:
            other = by_path.get(target) or sibling
            if frag and frag not in other.ids:
                errors.append(f"{where}: {path} has no heading #{frag}")
            return f'href="{html.escape(other.url + ("#" + frag if frag else ""))}"'
        if not target.exists() or repo_root not in target.parents:
            errors.append(f"{where}: broken link {href}")
            return match.group(0)
        kind = "tree" if target.is_dir() else "blob"
        url = f"{REPO}/{kind}/main/{target.relative_to(repo_root).as_posix()}" + (f"#{frag}" if frag else "")
        return f'href="{html.escape(url)}"'

    page.body_html = re.sub(r'href="([^"]*)"', fix, page.body_html)
    page.lead_html = re.sub(r'href="([^"]*)"', fix, page.lead_html)
    return errors


def load_all(sources: Path, repo_root: Path, nav=NAV, repo_docs=REPO_DOCS) -> tuple[list[Page], list[str]]:
    errors: list[str] = []
    pages: list[Page] = []
    sources, repo_root = sources.resolve(), repo_root.resolve()
    listed = [slug for _, slugs in nav for slug in slugs]
    if len(set(listed)) != len(listed):
        errors.append("NAV lists a page twice")
    on_disk = {p.stem for p in sources.glob("*.md")}
    for stem in sorted(on_disk - set(listed)):
        errors.append(f"{(sources / stem).relative_to(repo_root)}.md is not in NAV in website/tools/docs.py")
    for group, slugs in nav:
        for slug in slugs:
            meta = repo_docs.get(slug, {})
            source = repo_root / meta["path"] if "path" in meta else sources / f"{slug}.md"
            if not source.exists():
                errors.append(f"NAV names {slug}, but {source.relative_to(repo_root)} does not exist")
                continue
            try:
                pages.append(load_page(slug, group, source, meta, repo_root))
            except DocsError as e:
                errors.append(str(e))
    by_slug = {p.slug: p for p in pages}
    by_path = {p.source.resolve(): p for p in pages}
    for page in pages:
        errors.extend(rewrite_links(page, by_slug, by_path, sources, repo_root))
    return pages, errors


def nav_html(pages: list[Page], current: Page) -> str:
    out = []
    for group in dict.fromkeys(p.group for p in pages):
        out.append(f"<h4>{html.escape(group)}</h4>")
        for p in pages:
            if p.group != group:
                continue
            on = ' class="on" aria-current="page"' if p is current else ""
            out.append(f'<a href="{p.url}"{on}>{html.escape(p.title)}</a>')
    return "\n".join(out)


def meta_html(page: Page) -> str:
    bits = []
    if page.status:
        label, cls = STATUSES[page.status]
        bits.append(f'<span class="st {cls}">{label}</span>')
    if page.platforms:
        bits.append(f"<span>{html.escape(page.platforms)}</span>")
    return f'<p class="meta">{"".join(bits)}</p>' if bits else ""


def cards_html(pages: list[Page]) -> str:
    out = ['<section class="browse" aria-labelledby="browse"><h2 id="browse">Browse the docs</h2>']
    for group in dict.fromkeys(p.group for p in pages):
        items = [p for p in pages if p.group == group and p.slug != HOME]
        if not items:
            continue
        out.append(f'<h3>{html.escape(group)}</h3><div class="cards">')
        out += [f'<a href="{p.url}"><b>{html.escape(p.title)} →</b><span>{html.escape(p.blurb)}</span></a>' for p in items]
        out.append("</div>")
    out.append("</section>")
    return "".join(out)


def render(page: Page, pages: list[Page], template: str, repo_root: Path) -> str:
    i = pages.index(page)
    prev_page, next_page = (pages[i - 1] if i else None), (pages[i + 1] if i + 1 < len(pages) else None)
    pager = ""
    if prev_page:
        pager += f'<a href="{prev_page.url}"><small>Previous</small>{html.escape(prev_page.title)}</a>'
    if next_page:
        pager += f'<a class="next" href="{next_page.url}"><small>Next</small>{html.escape(next_page.title)}</a>'
    toc = "".join(f'<a href="#{html.escape(i)}" data-toc="{html.escape(i)}">{html.escape(t)}</a>' for t, i in page.toc)
    source = page.source.relative_to(repo_root).as_posix()
    values = {
        "title": "Sheprd docs" if page.slug == HOME else f"{page.title} · Sheprd docs",
        "description": html.escape(page.blurb if page.slug != HOME else re.sub(r"<[^>]+>", "", page.lead_html), quote=True),
        "canonical": SITE + page.url,
        "nav": nav_html(pages, page),
        "crumb": html.escape(f"{page.group} · {page.title}"),
        "h1": html.escape(page.title),
        "lead": page.lead_html,
        "meta": meta_html(page),
        "body": page.body_html + (cards_html(pages) if page.slug == HOME else ""),
        "pager": pager,
        "toc": (f"<h4>On this page</h4>{toc}" if toc else "") + f'<a class="edit" href="{REPO}/edit/main/{source}">Edit on GitHub</a>',
    }
    return re.sub(r"\{\{(\w+)\}\}", lambda m: values[m.group(1)], template)


def search_index(pages: list[Page]) -> str:
    entries = [
        {"t": p.title, "u": p.url, "g": p.group, "b": p.blurb, "s": p.sections}
        for p in pages
    ]
    return "window.SHEPRD_DOCS_INDEX = " + json.dumps(entries, ensure_ascii=False, separators=(",", ":")) + ";\n"


def build(sources: Path = SOURCES, repo_root: Path = ROOT, theme: Path = THEME, nav=NAV, repo_docs=REPO_DOCS) -> tuple[dict[str, str], list[str]]:
    """Every output file (path relative to the docs folder -> text), and the errors."""
    repo_root = repo_root.resolve()
    pages, errors = load_all(sources, repo_root, nav, repo_docs)
    if errors:
        return {}, errors
    template = (theme / "page.html").read_text(encoding="utf-8")
    files = {("index.html" if p.slug == HOME else f"{p.slug}/index.html"): render(p, pages, template, repo_root) for p in pages}
    files["search.js"] = search_index(pages)
    for asset in ("docs.css", "docs.js"):
        files[asset] = (theme / asset).read_text(encoding="utf-8")
    return files, []


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true", help="check the sources and write nothing")
    parser.add_argument("--out", type=Path, default=OUT, help="where to write the site (default: website/site/docs)")
    args = parser.parse_args(argv)
    files, errors = build()
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    if args.check:
        print(f"docs: {len(files) - 3} pages check out")
        return 0
    out = args.out.resolve()
    if out.exists():
        if out.name != "docs":
            print(f"error: refusing to replace {out}; its name is not docs", file=sys.stderr)
            return 1
        shutil.rmtree(out)
    for rel, text in files.items():
        path = out / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    print(f"docs: wrote {len(files) - 3} pages to {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
