#!/usr/bin/env python3
"""Copy target-specific Rust dependency notices into a package staging tree."""

import json
from pathlib import Path
import shutil
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
SUPPORTED_TARGETS = {
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
}
PREFIXES = ("license", "licence", "copying", "notice")
SPDX_LICENSE_TEXTS = {
    "Apache-2.0 WITH LLVM-exception": ROOT / "distribution/licenses/Apache-2.0_WITH_LLVM-exception.txt",
}


def main(destination, target):
    if target not in SUPPORTED_TARGETS:
        raise SystemExit(f"unsupported Rust license target: {target}")
    output = subprocess.run(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            str(ROOT / "Cargo.toml"),
            "--offline",
            "--format-version",
            "1",
            "--filter-platform",
            target,
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    packages = json.loads(output.stdout)["packages"]
    dependencies = sorted(
        (package for package in packages if package["source"]),
        key=lambda package: (package["name"].lower(), package["version"]),
    )
    destination.mkdir(parents=True, exist_ok=False)
    inventory = [
        "# Rust dependency licenses",
        "",
        f"Target: {target}",
        f"Resolved third-party packages: {len(dependencies)}",
        "",
        "License and notice files are copied from the exact Cargo-resolved source packages.",
        "",
    ]
    missing = []
    for package in dependencies:
        root = Path(package["manifest_path"]).parent
        files = [
            path
            for path in root.iterdir()
            if path.is_file() and path.name.lower().startswith(PREFIXES)
        ]
        license_dir = root / "LICENSES"
        if license_dir.is_dir():
            files.extend(path for path in license_dir.rglob("*") if path.is_file())
        files = sorted(set(files), key=lambda path: str(path).lower())
        fallback = None
        if not files:
            fallback = SPDX_LICENSE_TEXTS.get(package.get("license"))
            if fallback is None or not fallback.is_file():
                missing.append(f"{package['name']} {package['version']} ({package.get('license') or 'unknown license'})")
                continue
            files = [fallback]
        folder = f"{package['name']}-{package['version']}"
        target_dir = destination / folder
        target_dir.mkdir()
        copied = []
        for path in files:
            target = target_dir / path.name
            if target.exists():
                target = target_dir / f"{path.parent.name}-{path.name}"
            shutil.copyfile(path, target)
            copied.append(target.relative_to(destination.parent).as_posix())
        license = package.get("license") or "see copied license file"
        inventory.append(f"- {package['name']} {package['version']} — `{license}`")
        if fallback is not None:
            inventory.append(f"  - License text supplied from the bundled canonical SPDX text: `{fallback.relative_to(ROOT).as_posix()}`")
        for path in copied:
            inventory.append(f"  - `{path}`")
    if missing:
        shutil.rmtree(destination)
        raise SystemExit("missing dependency license files: " + ", ".join(missing))
    (destination.parent / "RUST-DEPENDENCIES.md").write_text(
        "\n".join(inventory) + "\n", encoding="utf-8"
    )
    print(f"collected license and notice files for {len(dependencies)} Rust dependencies")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: collect-rust-licenses.py DESTINATION RUST_TARGET")
    main(Path(sys.argv[1]), sys.argv[2])
