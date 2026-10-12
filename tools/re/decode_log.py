#!/usr/bin/env python3
"""Replay a probe/monitor JSONL log through the reassembler and print the decoded packets with wall-clock times.

    decode_log.py captures/linux/monitor-*.jsonl [--json-only] [--changes]
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from race_hid import Reassembler  # noqa: E402


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("logs", nargs="+")
    ap.add_argument("--json-only", action="store_true", help="print only JSON documents")
    ap.add_argument("--changes", action="store_true", help="print only KVP changes, one line per key")
    args = ap.parse_args()

    state = {}
    for path in args.logs:
        reasms = {}  # one reassembler per device: a log can interleave several
        for line in open(path):
            e = json.loads(line)
            ts = time.strftime("%H:%M:%S", time.localtime(e["t"])) + f".{int(e['t'] * 100) % 100:02d}"
            if "note" in e:
                if not args.changes:
                    print(f"{ts} -- {e['note']}")
                if e["note"].startswith("open"):
                    reasms.pop(e["note"].split()[-1] if len(e["note"].split()) > 2 else "", None)
                continue
            if e.get("dir") != "in" or "hex" not in e:
                continue
            src = e.get("src", "")
            reasm = reasms.setdefault(src, Reassembler())
            for p in reasm.feed(bytes.fromhex(e["hex"])):
                d = p.describe()
                for doc in d.get("json", []):
                    record = doc.get("UP") or doc.get("OR")
                    kvp = doc.get("KVP") if isinstance(doc.get("KVP"), dict) else {}
                    if args.changes:
                        for k, v in kvp.items():
                            old = state.get((src, record, k))
                            if old != v:
                                print(f"{ts} [{src}] {record}.{k}: {old} -> {v}")
                            state[(src, record, k)] = v
                    else:
                        print(f"{ts} [{src}] {json.dumps(doc, separators=(',', ':'))}")
                if "json" not in d and not args.json_only and not args.changes:
                    print(f"{ts} [{src}] {d['type']} cmd={d['cmd']} {d['payload']}")


if __name__ == "__main__":
    main()
