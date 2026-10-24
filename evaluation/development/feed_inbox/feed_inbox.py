"""Parser adapter for the feed inbox's managed Python dependency."""

from urllib.parse import urlsplit, urlunsplit

import feedparser


def canonical_url(value):
    parts = urlsplit(value.strip())
    scheme = parts.scheme.lower()
    if scheme not in {"http", "https"} or not parts.hostname:
        return ""
    try:
        port = parts.port
    except ValueError:
        return ""
    host = parts.hostname.lower()
    if port is not None and not ((scheme == "http" and port == 80) or (scheme == "https" and port == 443)):
        host = f"{host}:{port}"
    if ":" in parts.hostname and not host.startswith("["):
        host = f"[{host}]"
    return urlunsplit((scheme, host, parts.path or "/", parts.query, ""))


def parse_entries(source):
    parsed = feedparser.parse(source)
    items = []
    for entry in parsed.entries:
        link = canonical_url(entry.get("link", ""))
        if link:
            items.append({
                "link": link,
                "title": " ".join(str(entry.get("title", "(untitled)")).split()),
                "published": " ".join(str(entry.get("published", "")).split()),
            })
    return items
