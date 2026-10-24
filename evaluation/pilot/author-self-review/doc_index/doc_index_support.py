from __future__ import annotations

import json
import os
from pathlib import Path
import re
import sqlite3


_ALIASES = re.compile(r"^aliases:\s*\[(.*?)\]\s*$", re.MULTILINE)
_LINKS = re.compile(r"\[\[([^\]]+)\]\]")


def normalize_title(value: str) -> str:
    return " ".join(value.casefold().split())


def scan_docs(root_text: str) -> dict[str, object]:
    root = Path(root_text).resolve()
    documents = []
    try:
        for base, directories, filenames in os.walk(root, followlinks=False):
            base_path = Path(base)
            directories[:] = [name for name in directories if not (base_path / name).is_symlink()]
            for filename in filenames:
                path = base_path / filename
                if path.suffix.lower() != ".md" or path.is_symlink():
                    continue
                text = path.read_text(encoding="utf-8")
                relative = path.relative_to(root).as_posix()
                title = next((line[2:].strip() for line in text.splitlines() if line.startswith("# ")), path.stem)
                aliases: list[str] = []
                if text.startswith("---\n"):
                    end = text.find("\n---", 4)
                    if end >= 0:
                        match = _ALIASES.search(text[4:end])
                        if match:
                            aliases = [item.strip().strip("\"'") for item in match.group(1).split(",") if item.strip()]
                links = _LINKS.findall(text)
                documents.append({"path": relative, "title": title, "aliases": aliases, "links": links})
        documents.sort(key=lambda item: item["path"])
        return {"ok": True, "documents": documents}
    except (OSError, UnicodeError, ValueError) as exc:
        return {"ok": False, "error": str(exc), "documents": []}


def store_index(db_path: str, documents: list[dict[str, object]]) -> bool:
    path = Path(db_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(path) as db:
        db.executescript(
            "CREATE TABLE IF NOT EXISTS documents(path TEXT PRIMARY KEY,title TEXT NOT NULL,aliases TEXT NOT NULL);"
            "CREATE TABLE IF NOT EXISTS links(source TEXT NOT NULL,target TEXT NOT NULL);"
        )
        db.execute("DELETE FROM links")
        db.execute("DELETE FROM documents")
        for document in documents:
            db.execute(
                "INSERT INTO documents(path,title,aliases) VALUES(?,?,?)",
                (document["path"], document["title"], json.dumps(document["aliases"], ensure_ascii=False)),
            )
            for target in document["links"]:
                db.execute("INSERT INTO links(source,target) VALUES(?,?)", (document["path"], target))
    return True


def read_index(db_path: str) -> dict[str, object]:
    with sqlite3.connect(db_path) as db:
        db.row_factory = sqlite3.Row
        documents = [
            {"path": row["path"], "title": row["title"], "aliases": json.loads(row["aliases"])}
            for row in db.execute("SELECT path,title,aliases FROM documents ORDER BY path")
        ]
        links = [
            {"source": row["source"], "target": row["target"]}
            for row in db.execute("SELECT source,target FROM links ORDER BY source,target")
        ]
    return {"documents": documents, "links": links}
