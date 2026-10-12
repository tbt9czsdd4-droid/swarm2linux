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

from race_hid import RACE_TYPES, Reassembler, json_documents, printable_runs  # noqa: E402


def describe_out(report: bytes) -> str:
    """One line for an output report: the RACE packet, and for cmd 9902 the header bytes and the name or JSON."""
    n = report[1]
    race = report[3:3 + n]
    if len(race) < 6 or race[0] != 0x05:
        return f"raw {report[:3 + n].hex(' ')}"
    rtype, length, cmd = race[1], int.from_bytes(race[2:4], "little"), int.from_bytes(race[4:6], "little")
    payload = race[6:4 + length]
    head = f"{RACE_TYPES.get(rtype, hex(rtype))} cmd={cmd:04x} tgt={report[2]:02x}"
    if cmd != 0x9902:
        return f"{head} {payload.hex(' ')}"
    docs = json_documents(payload)
    tail = " ".join(json.dumps(d, separators=(",", ":")) for d in docs) if docs else " ".join(printable_runs(payload[12:]))
    return f"{head} hdr={payload[:12].hex(' ')} | {payload[12:16].hex(' ')} | {tail}"


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("logs", nargs="+")
    ap.add_argument("--json-only", action="store_true", help="print only JSON documents")
    ap.add_argument("--changes", action="store_true", help="print only KVP changes, one line per key")
    ap.add_argument("--no-out", action="store_true", help="do not print the host's output reports")
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
            if "hex" not in e:
                continue
            if e.get("dir") == "out":
                if not args.no_out:
                    print(f"{ts} [{e.get('src', '')}] >> {describe_out(bytes.fromhex(e['hex']))}")
                continue
            if e.get("dir") != "in":
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
