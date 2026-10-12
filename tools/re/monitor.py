#!/usr/bin/env python3
"""Log every packet the device sends, until Ctrl-C or --duration.

Use it while working the headset's physical controls to see which keys change. Watches every Turtle Beach device
with the vendor collection at once (transmitter, dock, headset cable) and follows plugging and unplugging, sending
the SInf session opener on every open when --session is given. Every report is logged to
captures/linux/monitor-<time>.jsonl.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from frames import ASK_SINF  # noqa: E402
from race_hid import KNOWN_PIDS, JsonlLog, RaceHid, Reassembler, find_devices  # noqa: E402

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
    open_devs = {}  # path -> (RaceHid, Reassembler, label)
    next_scan = 0.0
    try:
        while remaining() > 0:
            if time.monotonic() >= next_scan:
                next_scan = time.monotonic() + 1.0
                for info in find_devices():
                    if not info.vendor_collection or info.path in open_devs:
                        continue
                    src = f"{info.pid:04x}"
                    try:
                        dev = RaceHid(info.path, log=_tagged(log, src))
                    except OSError:
                        continue
                    out(f"-- open {info.path} {src} {KNOWN_PIDS.get(info.pid, '?')}")
                    log({"t": time.time(), "note": f"open {info.path} {src}"})
                    if args.session:
                        dev.write_report(ASK_SINF)
                        log({"t": time.time(), "note": f"ask sinf {src}"})
                    open_devs[info.path] = (dev, Reassembler(), src)
                if not open_devs:
                    if not waiting_said:
                        out("-- no device, waiting")
                        waiting_said = True
                else:
                    waiting_said = False
            for path, (dev, reasm, src) in list(open_devs.items()):
                try:
                    report = dev.get_input_report()
                except OSError as e:
                    out(f"-- {path} [{src}] gone ({e.strerror})")
                    log({"t": time.time(), "note": f"gone {path} {src}: {e.strerror}"})
                    dev.close()
                    del open_devs[path]
                    continue
                for p in reasm.feed(report):
                    _show(p, state, out, src)
            time.sleep(args.poll)
    except KeyboardInterrupt:
        pass
    for dev, _, _ in open_devs.values():
        dev.close()
    log.close()


def _tagged(log, src):
    """A logger that stamps every event with the device it came from."""
    def emit(event):
        event["src"] = src
        log(event)
    return emit


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
