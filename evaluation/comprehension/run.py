#!/usr/bin/env python3
"""Runner for the comprehension harness (H-1.4).

    run.py setup  t1..t5 RUN [--dist DIR] [--dest DIR]   make an isolated subject root, print the prompt
    run.py setup  t6 RUN [--site DIR] [--archive FILE] [--dest DIR]   website track: site text + the download, no docs folder
    run.py score  t1|t2|t3 ROOT                            re-run everything against the subject's project
    run.py check  TRANSCRIPT [--root ROOT]                 contamination scan (wraps check.sh)
    run.py receipt t1|t2|t3 RUN OF --root ROOT [--subject S] [--budget B] [--wrong W1:turn:"quote" ...]
                   [--first-correct N] [--notes TEXT] [--paths N]   append a receipt to receipts/H-1.1.md

The subject's root is `ROOT/dist` (a copy of the Guion distribution) and `ROOT/project` (one starter).
Scoring never trusts the subject's own check: it runs the program itself.
"""
import argparse
import datetime
import json
import os
import pathlib
import re
import shutil
import signal
import subprocess
import sys
import time
import tomllib
from html.parser import HTMLParser

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parent.parent
DEFAULT_DIST = REPO / "target/completion/releases/macos-aarch64/current"
DEFAULT_ARCHIVE = REPO / "target/completion/releases/macos-aarch64/guion-macos-aarch64.tar.gz"
SITE_PAGES = {"index.html": "index.txt", "developer/index.html": "developer.txt", "llms.txt": "llms.txt", "agents.json": "agents.json"}
STARTERS = {"t6": None, "t1": "t1-server", "t2": "t2-tags", "t3": "t3-grant", "t4": "t4-effects", "t5": "t5-csv"}
T2_EXPECTED = ["rust: 3", "cli: 2", "web: 2", "docs: 1"]
VERSION = "H-1.4"


def guion(root):
    return pathlib.Path(root) / "dist/bin/guion"


def setup(args):
    if args.task == "t6":
        return setup_site(args)
    dest = pathlib.Path(args.dest or (pathlib.Path(os.environ.get("TMPDIR", "/tmp")) / f"hx-{args.task}-run{args.run}"))
    shutil.rmtree(dest, ignore_errors=True)
    dest.mkdir(parents=True)
    shutil.copytree(args.dist, dest / "dist", symlinks=True)
    shutil.copytree(HERE / "starters" / STARTERS[args.task], dest / "project")
    version = subprocess.run([str(guion(dest)), "--version"], capture_output=True, text=True).stdout.strip()
    prompt = (HERE / "PROMPT.md").read_text().split("\n---\n", 1)[1].lstrip("\n")
    task = (HERE / "tasks" / (args.task.upper() + ".md")).read_text()
    prompt = prompt.replace("{{ROOT}}", str(dest)).replace("{{TASK}}", task.rstrip("\n"))
    (dest / "SUBJECT-PROMPT.txt").write_text(prompt)
    # The prompt file sits beside, not inside, the subject's two directories.
    print(f"root: {dest}\nartifact: {version}\nprompt: {dest / 'SUBJECT-PROMPT.txt'}")


class _Text(HTMLParser):
    """The page as a visitor's reader (or an agent's fetch tool) would give it: text, with each link's target kept."""
    SKIP = {"script", "style", "noscript", "svg"}
    BLOCK = {"p", "div", "li", "h1", "h2", "h3", "h4", "section", "pre", "tr", "br", "header", "footer", "nav", "ul", "ol", "table"}

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.out, self.skip, self.href, self.pre = [], 0, [], 0

    def handle_starttag(self, tag, attrs):
        if tag in self.SKIP:
            self.skip += 1
        if tag == "pre":
            self.pre += 1
        if tag in self.BLOCK:
            self.out.append("\n")
        if tag == "a":
            self.href.append(dict(attrs).get("href"))

    def handle_endtag(self, tag):
        if tag in self.SKIP and self.skip:
            self.skip -= 1
        if tag == "pre" and self.pre:
            self.pre -= 1
        if tag == "a" and self.href:
            target = self.href.pop()
            if target and not self.skip:
                self.out.append(f" ({target})")
        if tag in self.BLOCK:
            self.out.append("\n")

    def handle_data(self, data):
        if not self.skip:
            self.out.append(data if self.pre else re.sub(r"[ \t]+", " ", data))


def page_text(html):
    parser = _Text()
    parser.feed(html)
    text = "".join(parser.out)
    return re.sub(r"\n\s*\n+", "\n\n", text).strip() + "\n"


def setup_site(args):
    dest = pathlib.Path(args.dest or (pathlib.Path(os.environ.get("TMPDIR", "/tmp")) / f"hx-t6-run{args.run}"))
    shutil.rmtree(dest, ignore_errors=True)
    for sub in ("site", "download", "work", "home"):
        (dest / sub).mkdir(parents=True)
    public = pathlib.Path(args.site)
    for page, name in SITE_PAGES.items():
        raw = (public / page).read_text()
        (dest / "site" / name).write_text(page_text(raw) if page.endswith(".html") else raw)
    shutil.copy(args.archive, dest / "download" / pathlib.Path(args.archive).name)
    prompt = (HERE / "PROMPT-SITE.md").read_text().split("\n---\n", 1)[1].lstrip("\n")
    task = (HERE / "tasks" / "T6.md").read_text().replace("{{ROOT}}", str(dest))
    prompt = prompt.replace("{{ROOT}}", str(dest)).replace("{{TASK}}", task.rstrip("\n"))
    (dest / "SUBJECT-PROMPT.txt").write_text(prompt)
    print(f"root: {dest}\nsite pages: {', '.join(SITE_PAGES.values())}\narchive: {pathlib.Path(args.archive).name}\nprompt: {dest / 'SUBJECT-PROMPT.txt'}")


def score_site(args):
    """The runner runs the subject's installed binary on the subject's program. It trusts nothing the subject says."""
    root = pathlib.Path(args.root)
    home = root / "home"
    binaries = [p for p in home.rglob("guion") if p.is_file() and os.access(p, os.X_OK)]
    print(f"  installed binaries under home/: {[str(b.relative_to(root)) for b in binaries] or 'none'}")
    sources = sorted(p for p in (root / "work").rglob("*.gu") if "/share/guion/" not in str(p))  # not the archive's own stdlib
    print(f"  programs under work/: {[str(s.relative_to(root)) for s in sources] or 'none'}")
    closed = False
    for binary in binaries[:1]:
        env = dict(os.environ, HOME=str(home))
        for source in sources:
            ran = subprocess.run([str(binary), "run", str(source)], capture_output=True, text=True, env=env, timeout=60)
            hit = ran.stdout.strip() == "Hello, Ada!"
            print(f"  {source.relative_to(root)}: exit {ran.returncode}, stdout {ran.stdout.strip()[:60]!r}" + ("  <- exact" if hit else ""))
            closed = closed or hit
    print("OUTPUT " + ("correct: Hello, Ada!" if closed else "NOT correct: no installed binary ran a program that prints exactly 'Hello, Ada!'"))
    print("REPORT: read it. Count separately, never blended:")
    print("  wrong turns: W1 says it cannot be installed/run; W2/W3 names a command, flag or install path that does not exist;")
    print("               W7 attributes a step to the site that the site does not contain (check each 'where I learned it' against site/).")
    print("  site gaps:   each place the subject needed something no site page states (the number the website track exists to produce).")
    return 0


def run_program(root, project, extra=(), timeout=60):
    return subprocess.run([str(guion(root)), "run", *extra, "."], cwd=project, capture_output=True, text=True, timeout=timeout)


def t1_check(root, project, label):
    proc = subprocess.Popen([str(guion(root)), "run", "."], cwd=project, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    try:
        time.sleep(2.5)
        out = subprocess.run([sys.executable, str(HERE / "checks/t1_two_clients.py"), "8090"], capture_output=True, text=True).stdout.strip()
    finally:
        os.killpg(proc.pid, signal.SIGTERM)
        proc.wait()
        time.sleep(0.5)
    print(f"  {label}: {out}")
    return "PASS" in out.split(":", 1)[-1][:8] or out.startswith("T1 CONCURRENT: PASS")


def score(args):
    if args.task == "t6":
        return score_site(args)
    root = pathlib.Path(args.root)
    project = root / "project"
    if args.task == "t1":
        starter = HERE / "starters/t1-server"
        on_starter = t1_check(root, starter, "starter (must fail)")
        mine = t1_check(root, project, "subject (must pass)")
        ok = mine and not on_starter
        print(("CLOSED-CANDIDATE" if ok else "NOT CLOSED") + ": subject passes and the untouched starter fails" if ok else "NOT CLOSED: " + ("the starter passed the runner check" if on_starter else "the subject's program did not serve a second client"))
    elif args.task == "t2":
        ran = run_program(root, project)
        lines = [l.rstrip() for l in ran.stdout.splitlines() if l.strip()]
        print(f"  exit {ran.returncode}; output: {lines}")
        ok = lines == T2_EXPECTED
        print("OUTPUT " + ("correct" if ok else f"wrong, expected {T2_EXPECTED}"))
        print("REPORT: read it. It must say there is no builtin that removes repeats or counts by key (E_NAME_UNKNOWN for unique/group_by/count_by), and the loop-based way that works.")
        print("  a report that says such a builtin exists, or that it cannot be done, is a wrong turn (W2 / W1).")
    elif args.task == "t5":
        ran = run_program(root, project)
        lines = [l.strip() for l in ran.stdout.splitlines() if l.strip()]
        print(f"  exit {ran.returncode}; output: {lines}")
        print("OUTPUT " + ("correct" if lines == ["total: 1660"] else "wrong, expected ['total: 1660']"))
        # The runner's own edge case: a quoted field with a comma, and a bad amount.
        data = project / "data.csv"
        keep = data.read_text()
        data.write_text('name,amount\nrent,1200\n"a, b",oops\nx,5\n')
        edge = run_program(root, project)
        data.write_text(keep)
        print(f"  with a non-numeric amount: exit {edge.returncode}; output {edge.stdout.strip()[:60]!r}; stderr {edge.stderr.strip()[:80]!r}")
        print("REPORT: read it. It must say that a non-numeric amount is a problem and what its program does then (skips, stops, or crashes).")
    elif args.task == "t4":
        print("  T4 is scored by reading the report. The answer key for src/main.gu is:")
        print("    save   -> filesystem.read, filesystem.write (SQLite opens notes.db: a file, not a separate effect)")
        print("    fetch  -> network.http (example.com)")
        print("    main   -> both, because it calls them. No clock, no process, no socket.")
        print("  closed = names save and fetch with those reaches, and says the answer is static reachability, not a sandbox.")
        print("  wrong turns: W1 'it cannot touch the machine / SQLite is not file access'; W2 an effect the program does not have (process, clock); W3 a command that does not exist.")
    else:
        manifest = tomllib.loads((project / "guion.toml").read_text()) if (project / "guion.toml").exists() else {}
        hosts = manifest.get("network", {}).get("hosts", [])
        out = project / "out.txt"
        if out.exists():
            out.unlink()
        extra = ["--allow-http-host", args.host] if getattr(args, "host", None) else []
        ran = run_program(root, project, extra)
        wrote = out.exists() and re.search(r"wrote \d+ bytes", ran.stdout or "")
        print(f"  with the manifest alone: exit {ran.returncode}: {ran.stdout.strip()[:80]}")
        print(f"  grant in guion.toml: network.hosts = {hosts}")
        wildcard = any("*" in h for h in hosts)
        if getattr(args, "host", None):
            hosts = hosts or [args.host]
            print(f"  re-run with the subject's command-line grant --allow-http-host {args.host}: exit {ran.returncode}")
        ok = bool(wrote) and len(hosts) == 1 and not wildcard and "*" not in "".join(hosts)
        if not wrote:
            print("  the program did not run from the manifest alone; if the subject used a command-line flag, read the report and re-run with it, e.g. --allow-http-host HOST")
        print("T3 " + ("closed: one host, no wildcard" if ok else "NOT closed (check the report for a command-line grant before scoring)"))
        print("  also confirm the report quotes the exact grant and no wider grant (--file-root, --allow-command ...) was added.")
    return 0


def check(args):
    env = dict(os.environ)
    if args.root:
        env["HX_ROOT"] = args.root
    return subprocess.run(["sh", str(HERE / "check.sh"), args.transcript], env=env).returncode


def receipt(args):
    today = datetime.date.today().isoformat()
    fmt = "unknown"
    root = pathlib.Path(args.root)
    ver = subprocess.run([str(guion(root)), "--version"], capture_output=True, text=True).stdout.strip()
    m = re.search(r"format (\d+)", ver)
    fmt = m.group(1) if m else fmt
    turns = [w.split(":", 2) for w in (args.wrong or [])]
    lines = [
        f"{VERSION} / {args.task.upper()} / run {args.run} of {args.of} / {today} / artifact format {fmt}" + (" / PILOT n=1" if args.of == 1 else ""),
        f"subject: {args.subject}",
        f"budget hit: {args.budget}",
        f"contamination: {args.contamination}",
        f"success: {args.success}",
        f"wrong turns: {len(turns)}",
    ]
    if args.site_gaps is not None:
        lines.insert(len(lines) - 1, f"site gaps: {args.site_gaps}")
    for code, turn, quote in turns:
        lines.append(f'  {code} "{quote}" — turn {turn}')
    lines += [f"first correct action: {args.first_correct}", f"notes: {args.notes}", f"paths read: {args.paths}", ""]
    out = HERE / "receipts" / f"{VERSION}.md"
    out.parent.mkdir(exist_ok=True)
    with out.open("a") as f:
        f.write("\n".join(lines) + "\n")
    print(f"appended to {out}")


def main():
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("setup"); s.add_argument("task", choices=STARTERS); s.add_argument("run", type=int); s.add_argument("--dist", default=str(DEFAULT_DIST)); s.add_argument("--dest"); s.add_argument("--site", default=str(REPO / "site/public"), help="t6: the built website"); s.add_argument("--archive", default=str(DEFAULT_ARCHIVE), help="t6: the download the Download button gives")
    s = sub.add_parser("score"); s.add_argument("task", choices=STARTERS); s.add_argument("root"); s.add_argument("--host", help="T3: the host the subject granted on the command line instead of in guion.toml")
    s = sub.add_parser("check"); s.add_argument("transcript"); s.add_argument("--root")
    s = sub.add_parser("receipt"); s.add_argument("task", choices=STARTERS); s.add_argument("run", type=int); s.add_argument("of", type=int); s.add_argument("--root", required=True)
    s.add_argument("--subject", default="unknown"); s.add_argument("--budget", default="not hit"); s.add_argument("--contamination", default="clean"); s.add_argument("--success", default="unscored")
    s.add_argument("--wrong", action="append"); s.add_argument("--site-gaps", default=None, help="t6: count of places the website left the subject not knowing what to do"); s.add_argument("--first-correct", default="n/a"); s.add_argument("--notes", default=""); s.add_argument("--paths", default="not recorded")
    args = p.parse_args()
    return {"setup": setup, "score": score, "check": check, "receipt": receipt}[args.cmd](args) or 0


if __name__ == "__main__":
    sys.exit(main())
