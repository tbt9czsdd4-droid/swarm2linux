#!/usr/bin/env python3
"""Read-only first contact: listen, open a session, ask for the status record, and dump everything.

Every report in and out is logged to captures/linux/probe-<time>.jsonl; the decoded RACE packets are printed.
Only frames from frames.py are sent.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from frames import FRAMES  # noqa: E402
from race_hid import JsonlLog, RaceHid, Reassembler, default_device, find_devices  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


def show(packets, docs):
    for p in packets:
        d = p.describe()
        print(f"  {d['type']:9} cmd={d['cmd']} src={d['src']} len={d['len']:3}  {d['payload']}")
        for doc in d.get("json", []):
            print("    json:", json.dumps(doc, separators=(",", ":")))
            docs.append(doc)
        if "ascii" in d:
            print("    ascii:", d["ascii"])


def collect(dev, reasm, seconds, poll, docs):
    n = 0
    for report in dev.reports(seconds, poll_get_report=poll):
        n += 1
        show(reasm.feed(report), docs)
    return n


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--device", help="/dev/hidrawN (default: first Turtle Beach 0xFF13 collection)")
    ap.add_argument("--listen", type=float, default=2.0, help="seconds to listen before sending anything")
    ap.add_argument("--wait", type=float, default=2.0, help="seconds to collect after each ask")
    ap.add_argument("--poll", type=float, default=0.06, help="GET_REPORT polling interval in seconds")
    ap.add_argument("--asks", default="sinf,gsi", help=f"comma-separated frames to send, from {sorted(FRAMES)}")
    args = ap.parse_args()

    for d in find_devices():
        print(f"{d.path}: {d.pid:04x} {d.name} vendor-collection={d.vendor_collection}")
    path = args.device or default_device().path

    stamp = time.strftime("%Y%m%d-%H%M%S")
    log = JsonlLog(os.path.join(ROOT, "captures", "linux", f"probe-{stamp}.jsonl"))
    docs = []
    with RaceHid(path, log=log) as dev:
        reasm = Reassembler()
        print(f"\n== listening {args.listen}s on {path} before sending anything")
        n = collect(dev, reasm, args.listen, args.poll, docs)
        print(f"   {n} reports")
        for name in filter(None, args.asks.split(",")):
            frame = FRAMES[name]
            log({"t": time.time(), "note": f"ask {name}"})
            print(f"\n== ask {name}: {frame.rstrip(bytes(1)).hex(' ')}")
            dev.write_report(frame)
            n = collect(dev, reasm, args.wait, args.poll, docs)
            print(f"   {n} reports")
        if reasm.buf:
            print(f"\n   unfinished packet in buffer: {bytes(reasm.buf).hex(' ')}")
        if reasm.skipped:
            print(f"   bytes skipped while resyncing: {bytes(reasm.skipped).hex(' ')}")
    log.close()

    out = os.path.join(ROOT, "captures", "linux", f"probe-{stamp}-docs.json")
    with open(out, "w") as f:
        json.dump(docs, f, indent=1)
    print(f"\n{len(docs)} JSON documents -> {out}")
    print(f"raw log -> {log.f.name}")


if __name__ == "__main__":
    main()
