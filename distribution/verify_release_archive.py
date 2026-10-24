#!/usr/bin/env python3
"""Validate release tar members before the archive is published."""

from __future__ import annotations

import argparse
import tarfile


class ArchiveValidationError(ValueError):
    pass


def _parts(name: str) -> tuple[str, ...]:
    if not name or name.startswith("/") or "\\" in name or "\x00" in name:
        raise ArchiveValidationError(f"unsafe archive path: {name!r}")
    value = name[:-1] if name.endswith("/") else name
    parts = tuple(value.split("/"))
    if not value or any(part in ("", ".", "..") for part in parts):
        raise ArchiveValidationError(f"unsafe archive path: {name!r}")
    return parts


def _resolve_symlink(member_parts: tuple[str, ...], linkname: str) -> tuple[str, ...]:
    if not linkname or linkname.startswith("/") or "\\" in linkname or "\x00" in linkname:
        raise ArchiveValidationError(
            f"unsafe symlink target for {'/'.join(member_parts)}: {linkname!r}"
        )
    resolved = list(member_parts[:-1])
    for part in linkname.split("/"):
        if part in ("", "."):
            continue
        if part == "..":
            if len(resolved) <= 1:
                raise ArchiveValidationError(
                    f"symlink escapes package root: {'/'.join(member_parts)} -> {linkname}"
                )
            resolved.pop()
        else:
            resolved.append(part)
    return tuple(resolved)


def verify_release_archive(archive_path: str, expected_root: str) -> int:
    expected = _parts(expected_root)
    if len(expected) != 1:
        raise ArchiveValidationError("expected package root must be one path component")
    members: dict[tuple[str, ...], tarfile.TarInfo] = {}
    links: list[tuple[tuple[str, ...], tuple[str, ...], bool]] = []
    with tarfile.open(archive_path, mode="r:gz") as archive:
        for member in archive:
            parts = _parts(member.name)
            if parts[0] != expected[0]:
                raise ArchiveValidationError(
                    f"archive member is outside package root {expected[0]!r}: {member.name!r}"
                )
            if parts in members:
                raise ArchiveValidationError(f"duplicate archive member: {member.name!r}")
            if not (member.isfile() or member.isdir() or member.issym() or member.islnk()):
                raise ArchiveValidationError(f"unsupported archive member type: {member.name!r}")
            members[parts] = member
            if member.issym():
                links.append((parts, _resolve_symlink(parts, member.linkname), False))
            elif member.islnk():
                target = _parts(member.linkname)
                if target[0] != expected[0]:
                    raise ArchiveValidationError(
                        f"hard link escapes package root: {member.name!r} -> {member.linkname!r}"
                    )
                links.append((parts, target, True))

    if not members:
        raise ArchiveValidationError("release archive is empty")
    root_member = members.get(expected)
    if root_member is None or not root_member.isdir():
        raise ArchiveValidationError("release archive must contain its package root directory")

    for parts, member in members.items():
        for index in range(1, len(parts)):
            parent = parts[:index]
            parent_member = members.get(parent)
            if parent_member is not None and not parent_member.isdir():
                raise ArchiveValidationError(
                    f"archive member traverses a non-directory entry: {'/'.join(parts)!r}"
                )

    for source, target, hard_link in links:
        target_member = members.get(target)
        if target_member is None:
            raise ArchiveValidationError(
                f"archive link target is missing: {'/'.join(source)} -> {'/'.join(target)}"
            )
        if hard_link and not target_member.isfile():
            raise ArchiveValidationError(
                f"hard link target is not a regular file: {'/'.join(source)} -> {'/'.join(target)}"
            )
    return len(members)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive")
    parser.add_argument("package_root")
    args = parser.parse_args()
    try:
        count = verify_release_archive(args.archive, args.package_root)
    except (ArchiveValidationError, OSError, tarfile.TarError) as error:
        parser.exit(1, f"release archive validation failed: {error}\n")
    print(f"validated {count} archive entries under {args.package_root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
