from urllib.parse import urlsplit, urlunsplit

import feedparser


def _canonical_url(value):
    parts = urlsplit(value.strip())
    if parts.scheme.lower() not in {"http", "https"} or not parts.hostname:
        return ""
    host = parts.hostname.lower()
    if parts.port is not None and not (
        (parts.scheme.lower() == "http" and parts.port == 80)
        or (parts.scheme.lower() == "https" and parts.port == 443)
    ):
        host = f"{host}:{parts.port}"
    if ":" in parts.hostname and not host.startswith("["):
        host = f"[{host}]"
    return urlunsplit((parts.scheme.lower(), host, parts.path or "/", parts.query, ""))


def parse_entries(source):
    parsed = feedparser.parse(source)
    entries = []
    for entry in parsed.entries:
        link = _canonical_url(entry.get("link", ""))
        title = " ".join(str(entry.get("title", "(untitled)")).split())
        published = " ".join(str(entry.get("published", "")).split())
        if link:
            entries.append({"link": link, "title": title, "published": published})
    return entries
