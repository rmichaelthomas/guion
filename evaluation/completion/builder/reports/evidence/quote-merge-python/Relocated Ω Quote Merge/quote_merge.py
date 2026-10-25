#!/usr/bin/env python3
"""Merge independent quote feeds using only the Python standard library."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import math
from pathlib import Path
import socket
import sys
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import urlopen


def positive_float(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("must be finite and greater than zero")
    return number


def worker_count(value):
    number = int(value)
    if number < 2:
        raise argparse.ArgumentTypeError("must be at least 2")
    return number


def validate_feed(payload):
    if not isinstance(payload, dict) or not isinstance(payload.get("quotes"), list):
        raise ValueError("expected quotes array")
    quotes = []
    for item in payload["quotes"]:
        if not isinstance(item, dict):
            raise ValueError("expected quote object")
        if any(not isinstance(item.get(key), str) or not item[key].strip()
               for key in ("carrier", "service")):
            raise ValueError("expected nonempty carrier and service")
        if any(type(item.get(key)) is not int or item[key] < 0
               for key in ("price_cents", "days")):
            raise ValueError("expected nonnegative integer price and days")
        quotes.append({key: item[key] for key in
                       ("carrier", "service", "price_cents", "days")})
    return quotes


def fetch_feed(url, timeout):
    try:
        if urlsplit(url).scheme not in ("http", "https"):
            return None, "invalid_url"
        with urlopen(url, timeout=timeout) as response:
            body = response.read()
        try:
            return validate_feed(json.loads(body)), None
        except (ValueError, UnicodeError, RecursionError):
            return None, "malformed"
    except HTTPError as exc:
        return None, exc.code
    except (TimeoutError, socket.timeout):
        return None, "timeout"
    except URLError as exc:
        return None, "timeout" if isinstance(exc.reason, TimeoutError) else "network_error"
    except (OSError, ValueError):
        return None, "network_error"


def merge(urls, workers, timeout, max_days=None):
    winners = {}
    failed = []
    successful = 0
    with ThreadPoolExecutor(max_workers=workers) as pool:
        results = pool.map(lambda url: fetch_feed(url, timeout), urls)
        for url, (quotes, error) in zip(urls, results):
            if error is not None:
                failed.append({"url": url, "status": error})
                continue
            successful += 1
            for quote in quotes:
                if max_days is not None and quote["days"] > max_days:
                    continue
                carrier = quote["carrier"]
                key = lambda q: (q["price_cents"], q["service"], q["days"])
                if carrier not in winners or key(quote) < key(winners[carrier]):
                    winners[carrier] = quote
    return {"quotes": sorted(winners.values(), key=lambda q:
                            (q["price_cents"], q["carrier"], q["service"], q["days"])),
            "successful_feeds": successful, "failed_feeds": failed}


def nonnegative_int(value):
    number = int(value)
    if number < 0:
        raise argparse.ArgumentTypeError("must be nonnegative")
    return number


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("feeds", type=Path, help="UTF-8 text file, one URL per nonblank line")
    parser.add_argument("--workers", type=worker_count, default=4)
    parser.add_argument("--timeout", type=positive_float, default=2.0, help="request timeout in seconds")
    parser.add_argument("--output", type=Path, help="write JSON to this path instead of stdout")
    parser.add_argument("--max-days", type=nonnegative_int, help="exclude quotes above this delivery estimate")
    args = parser.parse_args()
    try:
        urls = [line.strip() for line in args.feeds.read_text(encoding="utf-8").splitlines()
                if line.strip()]
        report = merge(urls, args.workers, args.timeout, args.max_days)
        rendered = json.dumps(report, ensure_ascii=False, sort_keys=True, indent=2) + "\n"
        if args.output:
            args.output.write_text(rendered, encoding="utf-8")
        else:
            sys.stdout.write(rendered)
    except (OSError, UnicodeError) as exc:
        parser.exit(2, f"quote_merge: {exc}\n")


if __name__ == "__main__":
    main()
