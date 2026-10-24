"""Independent Python implementation of the feed-digest acceptance workload."""

from concurrent.futures import ThreadPoolExecutor
import http.client
import socket
import ssl
import time
from urllib.parse import urljoin, urlsplit, urlunsplit
import sys

import feedparser


ALLOWED_HOSTS = {"127.0.0.1"}
MAX_RESPONSE_BYTES = 16 * 1024 * 1024
TIMEOUT_SECONDS = 15
MAX_REDIRECTS = 5


class FeedFailure(Exception):
    def __init__(self, code, message):
        self.code = code
        self.message = message


def _check_host(url):
    parts = urlsplit(url)
    host = (parts.hostname or "").rstrip(".").lower()
    if parts.scheme not in {"http", "https"} or not host:
        raise FeedFailure("R_HTTP_URL", "only HTTP and HTTPS URLs without embedded credentials are allowed")
    if parts.username is not None or parts.password is not None:
        raise FeedFailure("R_HTTP_URL", "only HTTP and HTTPS URLs without embedded credentials are allowed")
    if host not in ALLOWED_HOSTS:
        raise FeedFailure("R_NETWORK_DENIED", "network access to this host is not granted")


def _fetch(url):
    deadline = time.monotonic() + TIMEOUT_SECONDS
    redirects = 0
    while True:
        _check_host(url)
        parts = urlsplit(url)
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise FeedFailure("R_HTTP_TIMEOUT", "HTTP request timed out")
        port = parts.port or (443 if parts.scheme.lower() == "https" else 80)
        timeout = min(5, remaining)
        if parts.scheme.lower() == "https":
            connection = http.client.HTTPSConnection(
                parts.hostname, port, timeout=timeout, context=ssl.create_default_context()
            )
        else:
            connection = http.client.HTTPConnection(parts.hostname, port, timeout=timeout)
        target = parts.path or "/"
        if parts.query:
            target += "?" + parts.query
        try:
            connection.request("GET", target)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise FeedFailure("R_HTTP_TIMEOUT", "HTTP request timed out")
            if connection.sock is not None:
                connection.sock.settimeout(remaining)
            response = connection.getresponse()
            if response.status in (301, 302, 303, 307, 308):
                location = response.getheader("Location")
                if not location or redirects >= MAX_REDIRECTS:
                    raise FeedFailure("R_HTTP_REDIRECT", "HTTP redirect exceeded the limit or targeted a host without a grant")
                url = urljoin(url, location)
                _check_host(url)
                redirects += 1
                continue
            if response.status < 200 or response.status >= 300:
                raise FeedFailure("R_HTTP_STATUS", f"HTTP server returned status {response.status}")
            length = response.getheader("Content-Length")
            if length is not None and int(length) > MAX_RESPONSE_BYTES:
                raise FeedFailure("R_HTTP_LIMIT", "HTTP response exceeds the configured byte limit")
            body = bytearray()
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise FeedFailure("R_HTTP_TIMEOUT", "HTTP request timed out")
                if connection.sock is not None:
                    connection.sock.settimeout(remaining)
                chunk = response.read(min(64 * 1024, MAX_RESPONSE_BYTES + 1 - len(body)))
                if not chunk:
                    break
                body.extend(chunk)
                if len(body) > MAX_RESPONSE_BYTES:
                    raise FeedFailure("R_HTTP_LIMIT", "HTTP response exceeds the configured byte limit")
            return bytes(body).decode("utf-8")
        except FeedFailure:
            raise
        except (socket.timeout, TimeoutError) as error:
            raise FeedFailure("R_HTTP_TIMEOUT", "HTTP request timed out") from error
        except (http.client.HTTPException, OSError, ValueError) as error:
            if time.monotonic() >= deadline:
                raise FeedFailure("R_HTTP_TIMEOUT", "HTTP request timed out") from error
            raise FeedFailure("R_HTTP_REQUEST", "HTTP request failed") from error
        finally:
            connection.close()


def _canonical_url(value):
    parts = urlsplit(value.strip())
    if parts.scheme.lower() not in {"http", "https"} or not parts.hostname:
        return ""
    host = parts.hostname.lower()
    try:
        port = parts.port
    except ValueError:
        return ""
    if port is not None and not (
        (parts.scheme.lower() == "http" and port == 80)
        or (parts.scheme.lower() == "https" and port == 443)
    ):
        host = f"{host}:{port}"
    if ":" in parts.hostname and not host.startswith("["):
        host = f"[{host}]"
    return urlunsplit((parts.scheme.lower(), host, parts.path or "/", parts.query, ""))


def _parse(source):
    parsed = feedparser.parse(source)
    entries = []
    for entry in parsed.entries:
        link = _canonical_url(entry.get("link", ""))
        if link:
            title = " ".join(str(entry.get("title", "(untitled)")).split())
            published = " ".join(str(entry.get("published", "")).split())
            entries.append((link, title, published))
    return entries


def render(urls):
    with ThreadPoolExecutor(max_workers=min(4, len(urls))) as pool:
        futures = [pool.submit(_fetch, url) for url in urls]
        responses = []
        for future in futures:
            try:
                responses.append((future.result(), None))
            except FeedFailure as error:
                responses.append((None, error))

    entries_by_link = {}
    failures = []
    for url, (source, error) in zip(urls, responses):
        if error:
            failures.append(f"- {url} ({error.code}: {error.message})\n")
            continue
        for link, title, published in _parse(source):
            current = entries_by_link.get(link)
            if current is None or (published and not current[1]) or ((bool(published) == bool(current[1])) and title < current[0]):
                entries_by_link[link] = (title, published)

    output = "# Feed digest\n\n"
    if failures:
        output += "## Feed errors\n\n" + "".join(failures) + "\n"
    output += "## Entries\n\n"
    for link in sorted(entries_by_link):
        title, published = entries_by_link[link]
        output += f"- [{title}]({link})"
        if published:
            output += f" — {published}"
        output += "\n"
    return output


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit("usage: python_baseline.py OUTPUT_PATH URL URL [URL ...]")
    with open(sys.argv[1], "w", encoding="utf-8", newline="") as output_file:
        output_file.write(render(sys.argv[2:]))
