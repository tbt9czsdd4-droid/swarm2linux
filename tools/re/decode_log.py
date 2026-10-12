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
        reasm = Reassembler()
        for line in open(path):
            e = json.loads(line)
            ts = time.strftime("%H:%M:%S", time.localtime(e["t"])) + f".{int(e['t'] * 100) % 100:02d}"
            if "note" in e:
                if not args.changes:
                    print(f"{ts} -- {e['note']}")
                if e["note"].startswith("open"):
                    reasm = Reassembler()
                continue
            if e.get("dir") != "in" or "hex" not in e:
                continue
            for p in reasm.feed(bytes.fromhex(e["hex"])):
                d = p.describe()
                for doc in d.get("json", []):
                    record = doc.get("UP") or doc.get("OR")
                    kvp = doc.get("KVP") if isinstance(doc.get("KVP"), dict) else {}
                    if args.changes:
                        for k, v in kvp.items():
                            old = state.get((record, k))
                            if old != v:
                                print(f"{ts} {record}.{k}: {old} -> {v}")
                            state[(record, k)] = v
                    else:
                        print(f"{ts} {json.dumps(doc, separators=(',', ':'))}")
                if "json" not in d and not args.json_only and not args.changes:
                    print(f"{ts} {d['type']} cmd={d['cmd']} {d['payload']}")


if __name__ == "__main__":
    main()
