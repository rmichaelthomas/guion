#!/usr/bin/env python3
"""Runner-side check for T1.

Not given to the subject. The subject writes its own check; this one decides the
score, so that "it is concurrent" is a measurement rather than a claim.

Passes only if a second client is answered while a first client holds a
connection half-sent. Run it against the starter first: it must fail.
"""
import socket
import sys
import time

HOST = "127.0.0.1"
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8090
HOLD_SECONDS = float(sys.argv[2]) if len(sys.argv) > 2 else 3.0


def connect(port):
    s = socket.create_connection((HOST, port), timeout=2.0)
    return s


def main() -> int:
    errors = []

    # Client A: opens a connection and sends a request that is deliberately
    # incomplete. A serial server blocks in read() waiting for the rest.
    try:
        holder = connect(PORT)
        holder.sendall(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n")
    except OSError as exc:
        print(f"FAIL setup: could not open the holding connection: {exc}")
        return 2

    time.sleep(0.25)  # let the server reach its blocking read

    # Client B: a complete request with a short deadline.
    started = time.perf_counter()
    try:
        probe = connect(PORT)
        probe.sendall(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        probe.settimeout(1.0)
        body = b""
        while True:
            try:
                chunk = probe.recv(4096)
            except socket.timeout:
                errors.append("second client got no response within 1.0 s while the first held")
                break
            if not chunk:
                break
            body += chunk
        probe.close()
        if not body:
            errors.append("second client returned nothing")
    except OSError as exc:
        errors.append(f"second client failed: {exc}")
    elapsed = (time.perf_counter() - started) * 1000

    # Finish client A so the server can exit cleanly.
    try:
        holder.sendall(b"Connection: close\r\n\r\n")
        holder.settimeout(1.0)
        holder.recv(64)
    except OSError:
        pass
    finally:
        holder.close()

    if errors:
        print(f"T1 CONCURRENT: FAIL in {elapsed:.0f} ms — {errors[0]}")
        return 1
    print(f"T1 CONCURRENT: PASS — second client answered in {elapsed:.0f} ms "
          f"while the first held for {HOLD_SECONDS:.1f} s")
    return 0


if __name__ == "__main__":
    sys.exit(main())
