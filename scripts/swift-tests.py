#!/usr/bin/env python3
"""Run Swift tests in a strict cooperative pool with a bounded execution time."""

import argparse
import collections
import os
import re
import selectors
import signal
import subprocess
import sys
import time
from pathlib import Path


def terminate_tree(process):
    # SwiftPM's test helpers create their own process groups. Snapshot PPIDs before
    # terminating parents so every descendant remains reachable for TERM and KILL.
    children = collections.defaultdict(list)
    for line in subprocess.check_output(["ps", "-axo", "pid=,ppid="], text=True).splitlines():
        pid, parent = map(int, line.split())
        children[parent].append(pid)
    descendants = []

    def visit(parent):
        for child in children[parent]:
            visit(child)
            descendants.append(child)

    visit(process.pid)
    for sig in (signal.SIGTERM, signal.SIGKILL):
        targets = descendants + ([process.pid] if process.poll() is None else [])
        for pid in targets:
            try:
                os.kill(pid, sig)
            except ProcessLookupError:
                pass
        if sig == signal.SIGTERM:
            # Give the runners a bounded grace period; still kill every snapshotted
            # descendant even if its parent exits during this interval.
            try:
                process.wait(timeout=1)
            except subprocess.TimeoutExpired:
                pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", default="build/swift-tests.log")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    arguments = parser.parse_args()
    timeout = float(os.environ.get("WIREBOLT_SWIFT_TEST_TIMEOUT_SECONDS", "180"))
    if timeout <= 0:
        raise ValueError("Swift test timeout must be positive")
    environment = dict(os.environ, LIBDISPATCH_COOPERATIVE_POOL_STRICT="1")
    command = arguments.command or ["swift", "test", "--package-path", "apple", "--skip-build"]
    active = collections.Counter()
    pending = b""
    log_path = Path(arguments.log)
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("wb") as log, subprocess.Popen(
        command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        env=environment, start_new_session=True,
    ) as process:
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        deadline = time.monotonic() + timeout
        try:
            while selector.get_map() or process.poll() is None:
                if time.monotonic() >= deadline:
                    print(f"\nSwift tests exceeded {timeout:g}s; log: {log_path}", file=sys.stderr)
                    print("Tests started but not finished:", file=sys.stderr)
                    for name, count in sorted(active.items()):
                        if count > 0:
                            print(f"  {name} ({count} running)", file=sys.stderr)
                    if not any(count > 0 for count in active.values()):
                        print("  No unfinished tests recorded; see the log for runner startup/shutdown.", file=sys.stderr)
                    return 124
                for key, _ in selector.select(timeout=0.2):
                    chunk = os.read(key.fileobj.fileno(), 65536)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    log.write(chunk)
                    log.flush()
                    sys.stdout.buffer.write(chunk)
                    sys.stdout.buffer.flush()
                    pending += chunk
                    while b"\n" in pending:
                        line, pending = pending.split(b"\n", 1)
                        text = re.sub(r"\x1b\[[0-9;]*m", "", line.decode("utf-8", errors="replace"))
                        started = re.match(r".*?Test (.+) started\.$", text)
                        finished = re.match(r".*?Test (.+?) (?:passed|failed|skipped)\b", text)
                        if started:
                            active[started[1]] += 1
                        elif finished:
                            active[finished[1]] -= 1
            return process.wait()
        finally:
            selector.close()
            if process.poll() is None:
                terminate_tree(process)


if __name__ == "__main__":
    sys.exit(main())
