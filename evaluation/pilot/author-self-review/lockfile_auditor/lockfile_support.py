from __future__ import annotations

from datetime import date
import re


_SEMVER = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?")


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


def compare_semver(left: str, right: str) -> int:
    a = _version_key(left)
    b = _version_key(right)
    return (a > b) - (a < b)


def is_expired(as_of: str, expires: str) -> bool:
    return date.fromisoformat(as_of) >= date.fromisoformat(expires)
