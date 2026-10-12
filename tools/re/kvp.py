#!/usr/bin/env python3
"""Talk to the Stealth Pro II's key/value records (RACE cmd 0x9902), the way Swarm II does.

    kvp.py index                 list the records
    kvp.py dump [--save]         read every record (with --save, write a snapshot to captures/snapshots/)
    kvp.py get GSI               subscribe to and read one record
    kvp.py set 0x1210 2          write one key (only keys and values Swarm II was seen writing)
    kvp.py restore FILE          write back the writable keys of a snapshot
    kvp.py watch [SECONDS]       subscribe to every record and print pushed changes

Frame layouts, all observed from Swarm II (captures/windows, 2026-10-12):

    payload of RACE cmd 0x9902 = 48 <op> <session> <counter> <seq> 00*7 <body>
      op 01, body 61 00 'S' <record>          subscribe and read a record
      op 01, body 61 00 'R' <record>          read a record once
      op 01, body b5 'index'                  list the records
      op 03, body b7 'set_kvp' ff <json>      write keys, json like {"0x1210":"2"}
    response: 68 <op'> <session> <counter> <seq> 00*7 <body>; op' 45 carries a document, 43 acks a set
"""
from __future__ import annotations

import argparse
import json
import os
import random
import struct
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from race_hid import (  # noqa: E402
    MAX_STREAM, RACE_CMD, RACE_RSP, JsonlLog, RaceHid, Reassembler, default_device, json_documents, race_packet,
)

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
CMD_KVP = 0x9902

# Keys Swarm II was seen writing, with the values it wrote (inclusive ranges). Anything else is refused.
WRITABLE = {
    0x1210: (1, 4),                                       # AQG game EQ preset: 1 Signature, 2 Bass, 3 Bass&Treble, 4 Vocal
    **{k: (-90, 90) for k in range(0x1220, 0x12B1, 0x10)},  # AQG game EQ bands 1..10, steps of 5
    0x700: (0, 1), 0x710: (0, 100),                       # SAF: mic monitoring on/off, volume
    0x720: (0, 1), 0x730: (0, 2), 0x740: (0, 100),        # SAF: Superhuman Hearing on/off, level, volume
    0x750: (0, 1), 0x760: (0, 100),                       # SAF: ANC on/off, level
    0x610: (0, 100), 0x620: (0, 100), 0x630: (0, 1),      # Mic: volume, noise gate level, A.I. noise reduction
}


# Records Swarm II reads with R (once) instead of S (subscribe); BT does not answer an S ask.
READ_ONCE = {"BT"}


class KvpClient:
    def __init__(self, dev: RaceHid):
        self.dev = dev
        self.reasm = Reassembler()
        self.session = random.randrange(0x10, 0xF0)
        self.counter = random.randrange(0x10, 0xF0)
        self.seq = 0
        self.pushed = []  # UP documents received while waiting for responses

    def _send(self, op: int, body: bytes) -> bytes:
        self.counter = (self.counter + 1) & 0xFF
        self.seq = (self.seq + 1) & 0xFF
        token = bytes([self.session, self.counter, self.seq])
        payload = bytes([0x48, op]) + token + bytes(7) + body
        race = race_packet(RACE_CMD, CMD_KVP, payload)
        if len(race) > MAX_STREAM:
            raise ValueError(f"request of {len(race)} bytes does not fit one report")
        self.dev.send_race(race)
        return token

    def _wait(self, token: bytes, timeout: float = 2.0):
        """The response packet for token, or None. Pushed documents seen meanwhile are collected."""
        for report in self.dev.reports(timeout):
            for p in self.reasm.feed(report):
                if p.cmd != CMD_KVP:
                    continue
                if p.type == RACE_RSP and p.payload[2:5] == token:
                    self.pushed += [d for d in json_documents(p.payload) if "UP" in d]
                    return p
                self.pushed += [d for d in json_documents(p.payload) if "UP" in d]
        return None

    def index(self) -> list:
        p = self._wait(self._send(0x01, b"\xb5index"))
        if p is None:
            raise TimeoutError("no answer to index")
        start = p.payload.find(b"[")
        return json.loads(p.payload[start:p.payload.rfind(b"]") + 1])

    def get(self, record: str, subscribe: bool = True) -> dict:
        action = b"S" if subscribe and record not in READ_ONCE else b"R"
        p = self._wait(self._send(0x01, b"\x61\x00" + action + record.encode()))
        if p is None:
            raise TimeoutError(f"no answer for {record}")
        docs = [d for d in json_documents(p.payload) if d.get("OR") == record]
        return docs[0]["KVP"] if docs else {}

    def set(self, key: int, value) -> bool:
        lo, hi = WRITABLE.get(key, (None, None))
        if lo is None:
            raise ValueError(f"key {key:#x} was never seen written by Swarm II; refusing")
        if not lo <= int(value) <= hi:
            raise ValueError(f"value {value} for {key:#x} outside the range Swarm II used ({lo}..{hi})")
        body = b"\xb7set_kvp\xff" + json.dumps({f"{key:#x}": str(int(value))}, separators=(",", ":")).encode()
        p = self._wait(self._send(0x03, body))
        return p is not None and p.payload[1] == 0x43

    def drain(self, seconds: float):
        for report in self.dev.reports(seconds):
            for p in self.reasm.feed(report):
                self.pushed += [d for d in json_documents(p.payload) if "UP" in d]


def _open(args):
    path = args.device or default_device().path
    log = JsonlLog(os.path.join(ROOT, "captures", "linux", f"kvp-{time.strftime('%Y%m%d-%H%M%S')}.jsonl"))
    return RaceHid(path, log=log), log


def _print_pushed(client):
    for d in client.pushed:
        print("  push:", json.dumps(d, separators=(",", ":")))
    client.pushed.clear()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--device", help="/dev/hidrawN (default: first Turtle Beach 0xFF13 collection)")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("index")
    d = sub.add_parser("dump")
    d.add_argument("--save", action="store_true")
    g = sub.add_parser("get")
    g.add_argument("record")
    g.add_argument("--once", action="store_true", help="read without subscribing (R instead of S)")
    s = sub.add_parser("set")
    s.add_argument("key", type=lambda x: int(x, 0))
    s.add_argument("value", type=int)
    r = sub.add_parser("restore")
    r.add_argument("snapshot")
    w = sub.add_parser("watch")
    w.add_argument("seconds", type=float, nargs="?", default=60.0)
    args = ap.parse_args()

    dev, log = _open(args)
    with dev:
        c = KvpClient(dev)
        c.drain(0.3)  # stale queued bytes
        c.pushed.clear()
        if args.cmd == "index":
            print(c.index())
        elif args.cmd == "dump":
            state = {}
            for rec in c.index():
                try:
                    state[rec] = c.get(rec)
                except TimeoutError as e:
                    print(f"{rec:4} -- {e}")
                    continue
                print(f"{rec:4} {json.dumps(state[rec], separators=(',', ':'))}")
            if args.save:
                out = os.path.join(ROOT, "captures", "snapshots", f"snapshot-{time.strftime('%Y%m%d-%H%M%S')}.json")
                os.makedirs(os.path.dirname(out), exist_ok=True)
                with open(out, "w") as f:
                    json.dump(state, f, indent=1)
                print("saved", out)
        elif args.cmd == "get":
            print(json.dumps(c.get(args.record, subscribe=not args.once), indent=1))
        elif args.cmd == "set":
            ok = c.set(args.key, args.value)
            print("ack" if ok else "NO ACK")
            c.drain(0.5)
            _print_pushed(c)
        elif args.cmd == "restore":
            snap = json.load(open(args.snapshot))
            for kvp in snap.values():
                for k, v in kvp.items():
                    key = int(k, 16)
                    if key in WRITABLE and isinstance(v, str) and v.lstrip("-").isdigit():
                        ok = c.set(key, int(v))
                        print(f"{key:#x} = {v}: {'ack' if ok else 'NO ACK'}")
            c.drain(0.5)
            _print_pushed(c)
        elif args.cmd == "watch":
            for rec in c.index():
                try:
                    c.get(rec)
                except TimeoutError as e:
                    print(f"{rec}: {e}")
            c.pushed.clear()
            end = time.monotonic() + args.seconds
            try:
                while time.monotonic() < end:
                    c.drain(0.5)
                    _print_pushed(c)
            except KeyboardInterrupt:
                pass
    log.close()


if __name__ == "__main__":
    main()
