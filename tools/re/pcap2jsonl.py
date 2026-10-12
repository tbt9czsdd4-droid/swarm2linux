#!/usr/bin/env python3
"""Convert a Windows USBPcap capture (pcapng) into the JSONL event log the other RE tools read.

Only the vendor HID reports are kept: SET_REPORT(Output 0x06) and GET_REPORT(Input 0x07) control transfers
on the given interface. Isochronous audio and everything else is dropped.

    pcap2jsonl.py capture.pcapng [-o out.jsonl] [--interface 3]

Each event: {"t": epoch seconds, "dir": "out"|"in", "via": "set_report"|"get_report", "hex": report, "dev": address}
"""
from __future__ import annotations

import argparse
import json
import struct
import sys

LINKTYPE_USBPCAP = 249

# USBPcap transfer types
XFER_CONTROL = 2

# HID class requests
GET_REPORT = 0x01
SET_REPORT = 0x09


def pcapng_packets(path):
    """Yield (linktype, timestamp seconds, data) for every Enhanced Packet Block."""
    with open(path, "rb") as f:
        buf = f.read()
    off = 0
    endian = "<"
    ifaces = []  # (linktype, ts_divisor)
    while off + 12 <= len(buf):
        btype, blen = struct.unpack_from(endian + "II", buf, off)
        if btype == 0x0A0D0D0A:  # section header: learn byte order, reset interfaces
            magic = buf[off + 8:off + 12]
            endian = "<" if magic == b"\x4d\x3c\x2b\x1a" else ">"
            btype, blen = struct.unpack_from(endian + "II", buf, off)
            ifaces = []
        elif btype == 1:  # interface description
            linktype = struct.unpack_from(endian + "H", buf, off + 8)[0]
            divisor = 1_000_000
            opt = off + 16
            end = off + blen - 4
            while opt + 4 <= end:
                code, olen = struct.unpack_from(endian + "HH", buf, opt)
                if code == 0:
                    break
                if code == 9 and olen >= 1:  # if_tsresol
                    v = buf[opt + 4]
                    divisor = 2 ** (v & 0x7F) if v & 0x80 else 10 ** v
                opt += 4 + ((olen + 3) & ~3)
            ifaces.append((linktype, divisor))
        elif btype == 6:  # enhanced packet
            ifid, ts_hi, ts_lo, caplen = struct.unpack_from(endian + "IIII", buf, off + 8)
            linktype, divisor = ifaces[ifid]
            ts = ((ts_hi << 32) | ts_lo) / divisor
            yield linktype, ts, buf[off + 28:off + 28 + caplen]
        if blen < 12:
            raise ValueError(f"corrupt block at {off}")
        off += blen


def usbpcap_header(pkt):
    hlen, irp, status, function, info, bus, dev, ep, xfer, dlen = struct.unpack_from("<HQIHBHHBBI", pkt, 0)
    stage = pkt[27] if hlen >= 28 and xfer == XFER_CONTROL else None
    return {
        "hlen": hlen, "irp": irp, "status": status, "function": function,
        "response": bool(info & 1), "bus": bus, "dev": dev, "ep": ep, "xfer": xfer,
        "dlen": dlen, "stage": stage, "data": pkt[hlen:hlen + dlen],
    }


def convert(path, interface=3):
    """Yield events for the HID report control transfers of every device in the capture."""
    pending = {}  # irp id -> setup info for GET_REPORT requests awaiting their completion
    for linktype, ts, pkt in pcapng_packets(path):
        if linktype != LINKTYPE_USBPCAP or len(pkt) < 27:
            continue
        h = usbpcap_header(pkt)
        if h["xfer"] != XFER_CONTROL:
            continue
        data = h["data"]
        if not h["response"] and h["stage"] == 0 and len(data) >= 8:  # setup stage, host -> device
            bm, req, wvalue, windex, wlength = struct.unpack_from("<BBHHH", data, 0)
            if windex != interface:
                continue
            rtype, rid = wvalue >> 8, wvalue & 0xFF
            if bm == 0x21 and req == SET_REPORT and rtype == 2:
                report = data[8:]
                if report:
                    yield {"t": ts, "dir": "out", "via": "set_report", "hex": report.hex(), "dev": h["dev"]}
                else:
                    pending[h["irp"]] = ("set", rid)
            elif bm == 0xA1 and req == GET_REPORT and rtype == 1:
                pending[h["irp"]] = ("get", rid)
        elif not h["response"] and h["stage"] == 1 and h["irp"] in pending:  # OUT data stage
            kind, rid = pending.pop(h["irp"])
            if kind == "set" and data:
                yield {"t": ts, "dir": "out", "via": "set_report", "hex": data.hex(), "dev": h["dev"]}
        elif h["response"] and h["irp"] in pending:
            kind, rid = pending[h["irp"]]
            if kind == "get" and data:
                pending.pop(h["irp"])
                ev = {"t": ts, "dir": "in", "via": "get_report", "hex": data.hex(), "dev": h["dev"]}
                if h["status"]:
                    ev["error"] = f"status {h['status']:#x}"
                yield ev
            elif h["stage"] in (2, 3) or kind == "set":
                pending.pop(h["irp"], None)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("pcapng")
    ap.add_argument("-o", "--out", help="output JSONL (default: stdout)")
    ap.add_argument("--interface", type=int, default=3, help="HID interface number (default 3)")
    args = ap.parse_args()
    out = open(args.out, "w") if args.out else sys.stdout
    counts = {}
    for ev in convert(args.pcapng, args.interface):
        ev["src"] = f"dev{ev['dev']}"
        out.write(json.dumps(ev) + "\n")
        counts[(ev["dev"], ev["dir"])] = counts.get((ev["dev"], ev["dir"]), 0) + 1
    for (dev, d), n in sorted(counts.items()):
        print(f"device {dev}: {n} {d}", file=sys.stderr)


if __name__ == "__main__":
    main()
