#!/usr/bin/env python3
"""Log every packet the device sends, until Ctrl-C or --duration.

Use it while working the headset's physical controls to see which keys change. Survives unplugging: it waits for
a Turtle Beach device with the vendor collection (transmitter, dock or headset cable) and reopens it, sending the
SInf session opener on every (re)open when --session is given. Every report is logged to
captures/linux/monitor-<time>.jsonl.
"""
from __future__ import annotations

import argparse
import errno
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from frames import ASK_SINF  # noqa: E402
from race_hid import KNOWN_PIDS, JsonlLog, RaceHid, Reassembler, first_device  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--session", action="store_true", help="send the SInf session opener on every open")
    ap.add_argument("--poll", type=float, default=0.06, help="GET_REPORT polling interval in seconds")
    ap.add_argument("--duration", type=float, default=0.0, help="stop after N seconds (0 = until Ctrl-C)")
    args = ap.parse_args()

    stamp = time.strftime("%Y%m%d-%H%M%S")
    log = JsonlLog(os.path.join(ROOT, "captures", "linux", f"monitor-{stamp}.jsonl"))
    start = time.monotonic()
    state = {}

    def elapsed():
        return time.monotonic() - start

    def out(msg):
        print(f"{elapsed():8.2f} {time.strftime('%H:%M:%S')} {msg}")

    def remaining():
        return float("inf") if not args.duration else args.duration - elapsed()

    print(f"log -> {log.f.name}")
    waiting_said = False
    try:
        while remaining() > 0:
            info = first_device()
            if info is None:
                if not waiting_said:
                    out("-- no device, waiting")
                    waiting_said = True
                time.sleep(0.5)
                continue
            waiting_said = False
            label = KNOWN_PIDS.get(info.pid, f"{info.pid:04x}")
            out(f"-- open {info.path} {info.pid:04x} {label}")
            log({"t": time.time(), "note": f"open {info.path} {info.pid:04x}"})
            try:
                with RaceHid(info.path, log=log) as dev:
                    reasm = Reassembler()
                    if args.session:
                        dev.write_report(ASK_SINF)
                        log({"t": time.time(), "note": "ask sinf"})
                    while remaining() > 0:
                        for report in dev.reports(min(remaining(), 1.0), poll_get_report=args.poll):
                            for p in reasm.feed(report):
                                _show(p, state, out, f"{info.pid:04x}")
            except OSError as e:
                if e.errno not in (errno.ENODEV, errno.ENOENT, errno.EIO, errno.EPROTO, errno.ESHUTDOWN):
                    raise
                out(f"-- {info.path} gone ({e.strerror})")
                log({"t": time.time(), "note": f"gone {info.path}: {e.strerror}"})
                time.sleep(0.5)
    except KeyboardInterrupt:
        pass
    log.close()


def _show(p, state, out, src):
    d = p.describe()
    for doc in d.get("json", []):
        out(f"[{src}] {json.dumps(doc, separators=(',', ':'))}")
        changes = _diff(state, src, doc)
        if changes:
            out(f"[{src}]   changed: {changes}")
    if "json" not in d:
        out(f"[{src}] {d['type']} cmd={d['cmd']} {d['payload']}")


def _diff(state: dict, src: str, doc: dict) -> dict:
    """Track KVP values per device and record; return {record.key: (old, new)} for keys this document changed."""
    record = doc.get("UP") or doc.get("OR")
    kvp = doc.get("KVP")
    if not record or not isinstance(kvp, dict):
        return {}
    known = state.setdefault((src, record), {})
    changes = {}
    for k, v in kvp.items():
        if known.get(k) != v:
            if k in known:
                changes[f"{record}.{k}"] = (known[k], v)
            known[k] = v
    return changes


if __name__ == "__main__":
    main()
