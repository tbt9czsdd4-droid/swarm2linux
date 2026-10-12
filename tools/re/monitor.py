#!/usr/bin/env python3
"""Log every packet the device sends, until Ctrl-C.

Use it while working the headset's physical controls to see which keys change. With --session the
session opener (SInf) is sent first, in case the device only pushes updates inside a session.
Every report is logged to captures/linux/monitor-<time>.jsonl.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from frames import ASK_SINF  # noqa: E402
from race_hid import JsonlLog, RaceHid, Reassembler, default_device  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--device", help="/dev/hidrawN (default: first Turtle Beach 0xFF13 collection)")
    ap.add_argument("--session", action="store_true", help="send the SInf session opener first")
    ap.add_argument("--poll", type=float, default=0.06, help="GET_REPORT polling interval in seconds")
    ap.add_argument("--duration", type=float, default=0.0, help="stop after N seconds (0 = until Ctrl-C)")
    ap.add_argument("--quiet-dump", action="store_true",
                    help="do not print the session dump, only what arrives after it")
    args = ap.parse_args()

    path = args.device or default_device().path
    stamp = time.strftime("%Y%m%d-%H%M%S")
    log = JsonlLog(os.path.join(ROOT, "captures", "linux", f"monitor-{stamp}.jsonl"))
    start = time.monotonic()
    state = {}
    print(f"monitoring {path}, log -> {log.f.name}")
    with RaceHid(path, log=log) as dev:
        reasm = Reassembler()
        quiet_until = 0.0
        if args.session:
            dev.write_report(ASK_SINF)
            log({"t": time.time(), "note": "ask sinf"})
            if args.quiet_dump:
                quiet_until = time.monotonic() + 1.5
        try:
            while True:
                chunk = 3600.0 if not args.duration else args.duration - (time.monotonic() - start)
                if chunk <= 0:
                    break
                for report in dev.reports(min(chunk, 1.0), poll_get_report=args.poll):
                    for p in reasm.feed(report):
                        d = p.describe()
                        quiet = time.monotonic() < quiet_until
                        for doc in d.get("json", []):
                            changes = _diff(state, doc)
                            if not quiet:
                                ts = f"{time.monotonic() - start:8.2f}"
                                print(f"{ts} {json.dumps(doc, separators=(',', ':'))}")
                                if changes:
                                    print(f"{'':8}  changed: {changes}")
                        if not quiet and "json" not in d:
                            print(f"{time.monotonic() - start:8.2f} {d['type']} cmd={d['cmd']} "
                                  f"src={d['src']} {d['payload']}")
        except KeyboardInterrupt:
            pass
    log.close()


def _diff(state: dict, doc: dict) -> dict:
    """Track KVP values per record; return {record.key: (old, new)} for the keys this document changed."""
    record = doc.get("UP") or doc.get("OR")
    kvp = doc.get("KVP")
    if not record or not isinstance(kvp, dict):
        return {}
    known = state.setdefault(record, {})
    changes = {}
    for k, v in kvp.items():
        if known.get(k) != v:
            if k in known:
                changes[f"{record}.{k}"] = (known[k], v)
            known[k] = v
    return changes


if __name__ == "__main__":
    main()
