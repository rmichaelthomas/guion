"""Independent standard-library reference for the Veritas API synchronization app."""

from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import tempfile
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


def request(method: str, url: str, token: str, body: str = "") -> tuple[int, str]:
    req = Request(
        url,
        data=body.encode() if method == "POST" else None,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/json",
            "Content-Type": "application/json",
        },
        method=method,
    )
    try:
        with urlopen(req, timeout=5) as response:
            return response.status, response.read().decode("utf-8")
    except HTTPError as error:
        return error.code, error.read().decode("utf-8")


def atomic_write(path: Path, value: dict) -> None:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=path.parent, delete=False) as handle:
        handle.write(encoded)
        staged = Path(handle.name)
    staged.replace(path)


def main(args: list[str]) -> int:
    if len(args) not in (1, 2):
        print("Usage: python3 reference.py BASE_URL [START_PAGE]")
        return 2
    base_url = args[0]
    initial_page = args[1] if len(args) == 2 else "1"
    token = os.environ.get("VERITAS_API_TOKEN")
    if token is None:
        print("Missing VERITAS_API_TOKEN")
        return 2
    state_path = Path("sync-state.json")
    if state_path.exists():
        state = json.loads(state_path.read_text(encoding="utf-8"))
        records = state["records"]
        page = state["next_page"]
    else:
        records = {}
        page = initial_page

    while page is not None:
        for attempt in range(3):
            try:
                status, body = request("GET", f"{base_url}/records?page={page}", token)
            except (URLError, TimeoutError):
                continue
            if 200 <= status < 300:
                document = json.loads(body)
                records.update({item["id"]: item for item in document["items"]})
                page = document["next"]
                atomic_write(state_path, {"records": records, "next_page": page})
                break
            if status < 500 or attempt == 2:
                print("API page failed with status:")
                print(status)
                return 1
        else:
            print("API page request failed after retries")
            return 1

    acknowledgement = json.dumps({"records_seen": len(records)}, separators=(",", ":"))
    for attempt in range(3):
        try:
            status, _ = request("POST", f"{base_url}/ack", token, acknowledgement)
        except (URLError, TimeoutError):
            continue
        if 200 <= status < 300:
            print("Records:")
            print(len(records))
            return 0
        if status < 500 or attempt == 2:
            print("Acknowledgement failed with status:")
            print(status)
            return 1
    print("Acknowledgement failed after retries")
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
