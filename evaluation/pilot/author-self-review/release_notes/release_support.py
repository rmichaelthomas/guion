from __future__ import annotations

from datetime import date
import json
import os
from pathlib import Path
import re
import tempfile


_SEMVER = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?")
_ESCAPE = re.compile(r"([\\`*_{}\[\]()#+.!|>~-])")


def _version_key(text: str):
    match = _SEMVER.fullmatch(text)
    if match is None:
        raise ValueError(f"invalid semantic version: {text}")
    major, minor, patch, prerelease = match.groups()
    if prerelease is None:
        suffix = (1,)
    else:
        parts = []
        for part in prerelease.split("."):
            if part.isdigit():
                parts.append((0, int(part)))
            else:
                parts.append((1, part))
        suffix = (0, tuple(parts))
    return int(major), int(minor), int(patch), suffix


def escape_markdown(value: str) -> str:
    return _ESCAPE.sub(r"\\\1", value)


def prepare_release(changes_dir: str, config_path: str, since: str) -> dict[str, object]:
    try:
        config = json.loads(Path(config_path).read_text(encoding="utf-8"))
        release_date = date.fromisoformat(config["date"]).isoformat()
        categories = config["category_order"]
        if not isinstance(categories, list) or not all(isinstance(item, str) and item for item in categories):
            raise ValueError("category_order must contain non-empty strings")
        since_key = _version_key(since) if since else None
        releases = []
        seen_ids: set[str] = set()
        for path in sorted(Path(changes_dir).glob("*.json")):
            version = path.stem
            version_key = _version_key(version)
            if since_key is not None and version_key <= since_key:
                continue
            data = json.loads(path.read_text(encoding="utf-8"))
            for field in ("id", "category", "text"):
                if field not in data:
                    raise ValueError(f"{path}: missing required field: {field}")
                if not isinstance(data[field], str) or not data[field]:
                    raise ValueError(f"{path}: invalid field type: {field}")
            if data["id"] in seen_ids:
                raise ValueError(f"{path}: duplicate id {data['id']}")
            seen_ids.add(data["id"])
            releases.append((version_key, version, data))
        grouped = []
        by_version: dict[str, list[dict[str, str]]] = {}
        for _, version, entry in releases:
            by_version.setdefault(version, []).append(entry)
        for version in sorted(by_version, key=_version_key, reverse=True):
            entries = sorted(by_version[version], key=lambda item: item["id"])
            grouped.append({"version": version, "entries": entries})
        return {"ok": True, "date": release_date, "categories": categories, "releases": grouped}
    except Exception as exc:
        return {"ok": False, "error": str(exc)}


def _stage(path: Path, content: bytes) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    staged = Path(name)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
    except BaseException:
        staged.unlink(missing_ok=True)
        raise
    return staged


def write_pair_atomic(markdown_path: str, markdown: str, summary_path: str, summary: str) -> dict[str, object]:
    markdown_target = Path(markdown_path)
    summary_target = Path(summary_path)
    staged: list[Path] = []
    replaced: list[tuple[Path, bytes | None]] = []
    try:
        if markdown_target.resolve() == summary_target.resolve():
            raise ValueError("Markdown and summary paths must be different")
        old_values = [path.read_bytes() if path.exists() else None for path in (markdown_target, summary_target)]
        staged = [_stage(markdown_target, markdown.encode("utf-8")), _stage(summary_target, summary.encode("utf-8"))]
        for target, temp_path, old in zip((markdown_target, summary_target), staged, old_values):
            os.replace(temp_path, target)
            replaced.append((target, old))
        return {"ok": True, "error": ""}
    except Exception as exc:
        rollback_errors = []
        for target, old in reversed(replaced):
            try:
                if old is None:
                    target.unlink(missing_ok=True)
                else:
                    backup = _stage(target, old)
                    os.replace(backup, target)
            except Exception as rollback_exc:
                rollback_errors.append(str(rollback_exc))
        message = str(exc)
        if rollback_errors:
            message += "; rollback failed: " + "; ".join(rollback_errors)
        return {"ok": False, "error": message}
    finally:
        for path in staged:
            path.unlink(missing_ok=True)
