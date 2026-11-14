"""The reference renderer for the publish-one-page slice.

It renders the block model of a fixture page to HTML with Python's standard library only, so the
Guion output can be compared with it byte for byte. It implements no server, session or upload.

    python3 twin/render.py FIXTURE.json            the whole page (what a publish writes)
    python3 twin/render.py FIXTURE.json --body     the article only
"""
import json
import sys
import urllib.parse

TEXT = (("&", "&amp;"), ("<", "&lt;"), (">", "&gt;"), ('"', "&quot;"), ("'", "&#39;"))


def text(value):
    """Element text and quoted attribute values."""
    for raw, entity in TEXT:
        value = value.replace(raw, entity)
    return value


def attr(value):
    """A quoted attribute value: the same, plus the backtick."""
    return text(value).replace("`", "&#96;")


def url(value):
    """One URL component: everything but A-Z a-z 0-9 - . _ ~ is percent-encoded as UTF-8."""
    return urllib.parse.quote(value, safe="")


def block(entry):
    if entry["type"] == "image":
        return '<figure><img src="/static/%s" alt="%s"></figure>\n' % (url(entry["name"]), attr(entry["alt"]))
    if entry["type"] == "paragraph":
        return "<p>%s</p>\n" % text(entry["text"])
    raise ValueError("unknown block type %r" % entry["type"])


def body(page):
    return "<article>\n<h1>%s</h1>\n%s</article>\n" % (text(page["title"]), "".join(block(b) for b in page["blocks"]))


def document(page):
    return (
        '<!doctype html>\n<html lang="en">\n'
        '<head><meta charset="utf-8"><title>%s</title></head>\n<body>\n%s</body>\n</html>\n'
        % (text(page["title"]), body(page))
    )


def main(argv):
    with open(argv[1], encoding="utf-8") as handle:
        page = json.load(handle)
    out = body(page) if "--body" in argv[2:] else document(page)
    sys.stdout.buffer.write(out.encode("utf-8"))


if __name__ == "__main__":
    main(sys.argv)
