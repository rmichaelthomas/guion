"""Standard-library adapters used by the Veritas documentation index CLI."""

from __future__ import annotations

import os
import json
import re
import sqlite3
import tempfile
from pathlib import Path
from typing import Any


_LINK = re.compile(r"\[\[([^\]]+)\]\]")
_H1_TITLE = re.compile(r"^ {0,3}#[ \t]+(.+?)\s*#*\s*$")


def _title_key(value: str) -> str:
    return " ".join(value.split()).casefold()


def _path_key(value: str) -> tuple[str, str]:
    normalized = " ".join(value.split()).casefold()
    return (normalized, value)


def _target_key(value: str) -> tuple[str, str]:
    normalized = _title_key(value)
    return (normalized, value)


def _page_title(text: str, fallback: str) -> str:
    in_fence = False
    fence_char = ""
    fence_size = 0
    for line in text.splitlines():
        stripped = line.lstrip(" ")
        if stripped.startswith("```") or stripped.startswith("~~~"):
            marker = stripped[0]
            run = len(stripped) - len(stripped.lstrip(marker))
            if not in_fence:
                in_fence, fence_char, fence_size = True, marker, run
            elif marker == fence_char and run >= fence_size:
                in_fence = False
            continue
        if in_fence:
            continue
        match = _H1_TITLE.match(line)
        if match:
            title = match.group(1).strip()
            if title:
                return title
    return fallback


def _front_matter(text: str) -> tuple[str, list[str]]:
    """Return the Markdown body and aliases from the supported front-matter subset."""
    lines = text.splitlines()
    if not lines or lines[0] != "---":
        return text, []
    closing = next((index for index in range(1, len(lines)) if lines[index] == "---"), None)
    if closing is None:
        raise ValueError("front matter opener has no matching closing --- line")

    aliases: list[str] = []
    aliases_seen = False
    for line in lines[1:closing]:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        key, separator, value = line.partition(":")
        if key.strip() != "aliases":
            continue
        if key != "aliases" or not separator:
            raise ValueError("aliases field must use the form aliases: JSON array")
        if aliases_seen:
            raise ValueError("duplicate aliases field")
        aliases_seen = True
        try:
            parsed = json.loads(value.strip())
        except json.JSONDecodeError as error:
            raise ValueError(f"malformed JSON in aliases field: {error.msg}") from error
        if not isinstance(parsed, list) or any(not isinstance(alias, str) for alias in parsed):
            raise ValueError("aliases must be a JSON array of strings")
        aliases = parsed

    return "\n".join(lines[closing + 1 :]), aliases


def _wiki_links(text: str) -> list[str]:
    links: list[str] = []
    in_fence = False
    fence_char = ""
    fence_size = 0
    for line in text.splitlines():
        stripped = line.lstrip(" ")
        if stripped.startswith("```") or stripped.startswith("~~~"):
            marker = stripped[0]
            run = len(stripped) - len(stripped.lstrip(marker))
            if not in_fence:
                in_fence, fence_char, fence_size = True, marker, run
            elif marker == fence_char and run >= fence_size:
                in_fence = False
            continue
        if in_fence:
            continue
        links.extend(
            match.group(1).strip()
            for match in _LINK.finditer(line)
            if match.group(1).strip()
        )
    return links


def scan_markdown(root: str) -> dict[str, Any]:
    """Read every regular Markdown file before the caller replaces any index."""
    document_root = Path(root)
    if document_root.is_symlink():
        raise ValueError("documentation root must not be a symlink")
    if not document_root.is_dir():
        raise NotADirectoryError(f"documentation root is not a directory: {root}")

    pages: list[dict[str, Any]] = []
    for directory, child_dirs, filenames in os.walk(document_root, topdown=True, followlinks=False):
        base = Path(directory)
        child_dirs[:] = sorted(
            (name for name in child_dirs if not (base / name).is_symlink()),
            key=lambda name: _path_key(name),
        )
        for filename in sorted(filenames, key=_path_key):
            path = base / filename
            if path.is_symlink() or not path.is_file() or path.suffix.casefold() != ".md":
                continue
            raw = path.read_bytes()
            text = raw.decode("utf-8", errors="strict")
            relative = path.relative_to(document_root).as_posix()
            body, aliases = _front_matter(text)
            pages.append(
                {
                    "path": relative,
                    "title": _page_title(body, path.stem),
                    "aliases": aliases,
                    "links": _wiki_links(body),
                }
            )
    pages.sort(key=lambda page: _path_key(page["path"]))
    return {"pages": pages}


def build_index(database: str, document: dict[str, Any]) -> None:
    """Build a complete SQLite index beside the destination, then atomically swap it in."""
    destination = Path(database)
    parent = destination.parent if str(destination.parent) else Path(".")
    parent.mkdir(parents=True, exist_ok=True)
    fd, temporary_name = tempfile.mkstemp(prefix=f".{destination.name}.", suffix=".tmp", dir=parent)
    os.close(fd)
    temporary = Path(temporary_name)
    try:
        connection = sqlite3.connect(temporary)
        try:
            connection.execute("PRAGMA journal_mode=DELETE")
            connection.execute("PRAGMA synchronous=FULL")
            with connection:
                connection.execute(
                    "CREATE TABLE pages (path TEXT PRIMARY KEY, title TEXT NOT NULL, title_key TEXT NOT NULL)"
                )
                connection.execute(
                    "CREATE TABLE links (source TEXT NOT NULL, target TEXT NOT NULL, target_key TEXT NOT NULL)"
                )
                connection.execute(
                    "CREATE TABLE aliases (page_path TEXT NOT NULL, alias TEXT NOT NULL, alias_key TEXT NOT NULL)"
                )
                connection.execute("CREATE INDEX pages_title_key_idx ON pages(title_key)")
                connection.execute("CREATE INDEX links_target_key_idx ON links(target_key)")
                connection.execute("CREATE INDEX aliases_alias_key_idx ON aliases(alias_key)")
                pages = document["pages"]
                connection.executemany(
                    "INSERT INTO pages(path, title, title_key) VALUES (?, ?, ?)",
                    [(page["path"], page["title"], _title_key(page["title"])) for page in pages],
                )
                connection.executemany(
                    "INSERT INTO links(source, target, target_key) VALUES (?, ?, ?)",
                    [
                        (page["path"], target, _title_key(target))
                        for page in pages
                        for target in page["links"]
                    ],
                )
                connection.executemany(
                    "INSERT INTO aliases(page_path, alias, alias_key) VALUES (?, ?, ?)",
                    [
                        (page["path"], alias, _title_key(alias))
                        for page in pages
                        for alias in page["aliases"]
                    ],
                )
            connection.close()
        except BaseException:
            connection.close()
            raise
        os.replace(temporary, destination)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


def _open_index(database: str) -> sqlite3.Connection:
    connection = sqlite3.connect(Path(database).absolute().as_uri() + "?mode=ro", uri=True)
    connection.row_factory = sqlite3.Row
    return connection


def check_index(database: str) -> dict[str, Any]:
    connection = _open_index(database)
    try:
        rows = connection.execute(
            "SELECT source, target, "
            "((SELECT COUNT(*) FROM pages WHERE title_key = links.target_key) + "
            "(SELECT COUNT(DISTINCT page_path) FROM aliases "
            "WHERE alias_key = links.target_key "
            "AND page_path NOT IN (SELECT path FROM pages WHERE title_key = links.target_key))) "
            "AS match_count "
            "FROM links"
        ).fetchall()
    finally:
        connection.close()
    missing = [
        {"source": row["source"], "target": row["target"]}
        for row in rows
        if row["match_count"] == 0
    ]
    ambiguous = [
        {"source": row["source"], "target": row["target"]}
        for row in rows
        if row["match_count"] > 1
    ]
    sort_issues = lambda issue: (_path_key(issue["source"]), _target_key(issue["target"]))
    missing.sort(key=sort_issues)
    ambiguous.sort(key=sort_issues)
    return {"missing": missing, "ambiguous": ambiguous}


def backlinks_index(database: str, title: str) -> dict[str, Any]:
    key = _title_key(title)
    connection = _open_index(database)
    try:
        sources = {
            row[0]
            for row in connection.execute(
                "SELECT DISTINCT source FROM links WHERE target_key = ?",
                (key,),
            ).fetchall()
        }
    finally:
        connection.close()
    return {"backlinks": sorted(sources, key=_path_key)}


def write_report(path: str, report: dict[str, Any]) -> None:
    """Write deterministic UTF-8 JSON with atomic replacement."""
    destination = Path(path)
    parent = destination.parent if str(destination.parent) else Path(".")
    parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(report, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n"
    fd, temporary_name = tempfile.mkstemp(prefix=f".{destination.name}.", suffix=".tmp", dir=parent)
    os.close(fd)
    temporary = Path(temporary_name)
    try:
        temporary.write_text(encoded, encoding="utf-8", newline="\n")
        os.replace(temporary, destination)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass
