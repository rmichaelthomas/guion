#!/usr/bin/env python3
"""Checks for the publish-one-page slice that need no server.

    python3 run_checks.py [--guion PATH_TO_GUION] [--only size|caps|twin|effects]

Run it in a copy of this directory in which `guion add web --path .../packages/web` was run (the
manifest here names the package by a relative path; `guion add` records the absolute one). It checks:

  size      application source excluding tests: printed, near 600 lines; over 700 fails
  caps      the capability block of guion.toml fits one screen (40 lines) and asks for at most 8 grants
  twin      the three fixture pages render to the same bytes in Guion and in twin/render.py
  effects   `guion doc --json` attributes filesystem.write, process.spawn and the clock per function
  handles   no public function of the application names a connection, listener or database handle

The socket tests (concurrency, SQLite readers, byte comparison through the server) are in
crates/cli/tests/f10_publish_page.rs.
"""
import json
import pathlib
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent
LINE_TARGET = 600
LINE_LIMIT = 700
SCREEN_LINES = 40
GRANT_LIMIT = 8
HANDLES = ("TcpConn", "TcpListener", "UdpSock", "FileReader", "Db", "Transaction", "TomlDocument", "HttpServerRequest")
FAILED = []


def check(ok, message):
    print(("ok    " if ok else "FAIL  ") + message)
    if not ok:
        FAILED.append(message)


def source_files():
    src = ROOT / "src"
    files = [p for p in src.rglob("*.gu") if not p.name.endswith(".gut.gu")]
    files += src.rglob("*.gut")
    return sorted(files)


def size():
    counts = {str(p.relative_to(ROOT)): len(p.read_text().splitlines()) for p in source_files()}
    total = sum(counts.values())
    print("source lines (src/, tests excluded): %d (target near %d, limit %d)" % (total, LINE_TARGET, LINE_LIMIT))
    for name, count in counts.items():
        print("      %4d  %s" % (count, name))
    check(total <= LINE_LIMIT, "application source is %d lines, at most %d" % (total, LINE_LIMIT))
    return total


def capabilities():
    text = (ROOT / "guion.toml").read_text()
    manifest = tomllib.loads(text)
    capability_tables = ("clock", "sockets", "process", "environment", "network", "resize", "runtime", "log")
    block = []
    pending = []
    keep = False
    for line in text.splitlines():
        if line.startswith("["):
            keep = line.strip("[]").split(".")[0] in capability_tables
            if keep:
                block.extend(pending)
            pending = []
        elif line.startswith("#") or not line.strip():
            pending.append(line)
            continue
        else:
            pending = []
        if keep:
            block.append(line)
    grants = 0
    grants += 1 if manifest.get("clock", {}).get("enabled") else 0
    for table, key in (("sockets", "bind"), ("sockets", "connect"), ("process", "commands"), ("environment", "variables"), ("network", "hosts")):
        grants += len(manifest.get(table, {}).get(key, []))
    print("capability block: %d lines, %d grants" % (len(block), grants))
    check(len(block) <= SCREEN_LINES, "the capability block is %d lines, at most %d" % (len(block), SCREEN_LINES))
    check(grants <= GRANT_LIMIT, "the application asks for %d grants, at most %d" % (grants, GRANT_LIMIT))
    declared = manifest.get("resize", {}).get("command")
    check(declared in manifest.get("process", {}).get("commands", []), "the resize command %r is the one granted under [process]" % declared)


def guion(binary, *args):
    return subprocess.run([binary, *args], cwd=ROOT, capture_output=True, text=True)


def twin(binary):
    for fixture in sorted((ROOT / "fixtures").glob("page*.json")):
        name = fixture.stem
        ran = guion(binary, "run", "render.gu", str(fixture.relative_to(ROOT)), name)
        check(ran.returncode == 0, "%s renders in Guion %s" % (name, ran.stderr.strip()))
        for suffix, flags in ((".html", []), (".body.html", ["--body"])):
            expected = subprocess.run(["python3", str(ROOT / "twin/render.py"), str(fixture), *flags], capture_output=True).stdout
            got = (ROOT / "out" / (name + suffix)).read_bytes() if (ROOT / "out" / (name + suffix)).exists() else b""
            check(got == expected and len(expected) > 0, "%s%s: Guion and the Python twin agree byte for byte (%d bytes)" % (name, suffix, len(expected)))


def effects(binary):
    ran = guion(binary, "doc", "--json", "src/main.gu")
    check(ran.returncode == 0, "guion doc --json src/main.gu " + ran.stderr.strip())
    if ran.returncode != 0:
        return
    report = json.loads(ran.stdout)
    by_name = {f["name"]: f for f in report["functions"] if f.get("source", "").startswith("src/")}
    found = lambda name: set(by_name[name]["effects"])
    check("filesystem.write" in found("publish_page"), "publish_page reports filesystem.write: %s" % sorted(found("publish_page")))
    check({"filesystem.write", "process.spawn"} <= found("upload_cover"), "upload_cover reports filesystem.write and process.spawn: %s" % sorted(found("upload_cover")))
    check("filesystem.write" in found("cover_receive"), "cover_receive reports filesystem.write (from std:http_body): %s" % sorted(found("cover_receive")))
    check("process.spawn" in found("resize_image"), "resize_image, the one function that names the tool, reports process.spawn")
    check("filesystem.write" not in found("view_page") and "filesystem.write" not in found("create_page"), "view_page and create_page report no filesystem.write")
    check("clock.read" in found("view_page"), "view_page reports clock.read (session expiry)")
    check("network.socket" in found("start"), "start reports network.socket and concurrency.parallel through the web package")
    check("filesystem.write" in report["package_effects"]["web"]["effects"], "package_effects attributes filesystem.write to the web package")
    store = {name: sorted(found(name)) for name in by_name if name.startswith("store_")}
    check(all("filesystem.write" not in effects_ for effects_ in store.values()), "store_* functions: %s" % store)
    print("FINDING the effect analysis has no SQLite effect: every store_* function above reports no effect of its own.")
    print("        The writes it does see are filesystem.write (bundle, upload) and process.spawn (resize).")
    public = [line.split("(")[0][len("pub fn "):] for p in source_files() if p.suffix == ".gu" for line in p.read_text().splitlines() if line.startswith("pub fn ")]
    handles = [name for name in public if any(h in json.dumps(by_name[name]) for h in HANDLES)]
    check(len(public) >= 8 and not handles, "no public function of the application names a handle type (%d checked): %s" % (len(public), handles))


def main(argv):
    binary = "guion"
    if "--guion" in argv:
        binary = argv[argv.index("--guion") + 1]
    only = argv[argv.index("--only") + 1] if "--only" in argv else None
    for name, run in (("size", lambda: size()), ("caps", lambda: capabilities()), ("twin", lambda: twin(binary)), ("effects", lambda: effects(binary))):
        if only in (None, name):
            run()
    print("%d checks failed" % len(FAILED) if FAILED else "all checks passed")
    return 1 if FAILED else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
