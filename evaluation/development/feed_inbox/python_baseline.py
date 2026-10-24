"""Independent standard-library CLI baseline for the persistent feed inbox."""

from concurrent.futures import ThreadPoolExecutor
import http.client
import socket
import sys
from pathlib import Path
from urllib.parse import urlsplit

from feed_inbox import parse_entries


def read_state(path):
    try:
        import json
        document = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return []
    except (OSError, ValueError) as error:
        raise ValueError(f"Invalid inbox.json: {error}") from error
    if not isinstance(document, dict) or not isinstance(document.get("entries"), list):
        raise ValueError("Invalid inbox.json: missing entries")
    entries = document["entries"]
    required = {"link": str, "title": str, "published": str, "read": bool}
    for entry in entries:
        if not isinstance(entry, dict) or any(not isinstance(entry.get(key), kind) for key, kind in required.items()):
            raise ValueError("Invalid inbox.json: malformed entry")
    return entries


def write_state(path, entries):
    import json
    path.write_text(json.dumps({"entries": entries}, ensure_ascii=False, sort_keys=True, separators=(",", ":")), encoding="utf-8")


def fetch(url):
    parts = urlsplit(url)
    host = (parts.hostname or "").lower()
    if parts.scheme not in {"http", "https"} or host != "127.0.0.1" or parts.username or parts.password:
        raise OSError("R_NETWORK_DENIED: network access to this host is not granted")
    port = parts.port or (443 if parts.scheme == "https" else 80)
    connection_type = http.client.HTTPSConnection if parts.scheme == "https" else http.client.HTTPConnection
    connection = connection_type(parts.hostname, port, timeout=15)
    target = parts.path or "/"
    if parts.query:
        target += "?" + parts.query
    try:
        connection.request("GET", target)
        response = connection.getresponse()
        if response.status < 200 or response.status >= 300:
            raise OSError(f"R_HTTP_STATUS: HTTP server returned status {response.status}")
        body = response.read(16 * 1024 * 1024 + 1)
        if len(body) > 16 * 1024 * 1024:
            raise OSError("R_HTTP_LIMIT: HTTP response exceeds the configured byte limit")
        return body.decode("utf-8")
    except (socket.timeout, TimeoutError) as error:
        raise OSError("R_HTTP_TIMEOUT: HTTP request timed out") from error
    finally:
        connection.close()


def fetch_result(url):
    try:
        return fetch(url), None
    except OSError as error:
        code, separator, message = str(error).partition(": ")
        if not separator:
            code, message = "R_HTTP_REQUEST", "HTTP request failed"
        return None, (code, message)


def merge(entries, incoming):
    by_link = {item["link"]: item for item in entries}
    for candidate in incoming:
        previous = by_link.get(candidate["link"])
        if previous is None:
            by_link[candidate["link"]] = {**candidate, "read": False}
        else:
            choose_candidate = bool(candidate["published"]) and not previous["published"]
            if bool(candidate["published"]) == bool(previous["published"]) and candidate["title"] < previous["title"]:
                choose_candidate = True
            chosen = candidate if choose_candidate else previous
            by_link[candidate["link"]] = {**chosen, "read": previous["read"]}
    # Keep the first-seen ordering stable, even when duplicate metadata improves.
    ordered = []
    seen = set()
    for item in entries:
        link = item["link"]
        if link not in seen:
            ordered.append(by_link[link])
            seen.add(link)
    for item in incoming:
        link = item["link"]
        if link not in seen:
            ordered.append(by_link[link])
            seen.add(link)
    return ordered


def main(argv):
    if not argv:
        print("Usage: veritas run PROJECT <refresh URL [URL ...]|import-feed PATH|list [unread]|summary|read LINK|unread LINK>")
        return 2
    state_path = Path("inbox.json")
    try:
        entries = read_state(state_path)
    except ValueError as error:
        print(str(error))
        return 1
    command = argv[0]
    if command == "list":
        if len(argv) not in (1, 2) or (len(argv) == 2 and argv[1] != "unread"):
            print("Usage: list [unread]")
            return 2
        unread_only = len(argv) == 2
        shown = 0
        for entry in entries:
            if not unread_only or not entry["read"]:
                marker = "read" if entry["read"] else "new"
                print(f"[{marker}] {entry['title']} — {entry['published']} — {entry['link']}")
                shown += 1
        if shown == 0:
            print("No unread items." if unread_only else "No feed items.")
        return 0
    if command == "summary":
        if len(argv) != 1:
            print("Usage: summary")
            return 2
        read_count = sum(entry["read"] for entry in entries)
        print(f"Items:\n{len(entries)}\nUnread:\n{len(entries) - read_count}\nRead:\n{read_count}")
        return 0
    if command in {"read", "unread"}:
        if len(argv) != 2:
            print(f"Usage: {command} LINK")
            return 2
        found = False
        for entry in entries:
            if entry["link"] == argv[1]:
                entry["read"] = command == "read"
                found = True
        if not found:
            print(f"No inbox item: {argv[1]}")
            return 1
        write_state(state_path, entries)
        print(f"Marked {command}: {argv[1]}")
        return 0
    if command == "import-feed":
        if len(argv) != 2:
            print("Usage: import-feed PATH")
            return 2
        try:
            incoming = parse_entries(Path(argv[1]).read_text(encoding="utf-8"))
        except OSError as error:
            print(f"Cannot read feed: R_FILE_READ — {error}")
            return 1
        new_count = sum(not any(item["link"] == entry["link"] for entry in entries) for item in incoming)
        entries = merge(entries, incoming)
        write_state(state_path, entries)
        print(f"New items:\n{new_count}\nTotal items:\n{len(entries)}")
        return 0
    if command == "refresh":
        if len(argv) < 2:
            print("Usage: refresh FEED_URL [FEED_URL ...]")
            return 2
        urls = argv[1:]
        with ThreadPoolExecutor(max_workers=min(4, len(urls))) as pool:
            results = list(pool.map(fetch_result, urls))
        new_count = 0
        failures = ""
        for url, (source, error) in zip(urls, results):
            if error:
                code, message = error
                failures += f"{url} — {code}: {message}\n"
                continue
            incoming = parse_entries(source)
            new_count += sum(not any(item["link"] == entry["link"] for entry in entries) for item in incoming)
            entries = merge(entries, incoming)
        write_state(state_path, entries)
        print(f"New items:\n{new_count}\nTotal items:\n{len(entries)}")
        if failures:
            print("Feed errors:")
            print(failures)
            return 1
        return 0
    print(f"Unknown command: {command}")
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
