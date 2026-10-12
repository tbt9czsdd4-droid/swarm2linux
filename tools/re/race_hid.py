"""Airoha RACE over USB HID, as spoken by the Turtle Beach Stealth Pro II transmitter.

Dependency-free: talks to /dev/hidrawN directly.

HID framing (vendor collection, usage page 0xFF13):
    out: 06 <len> <target> <len bytes of RACE stream>   padded to 62 bytes
    in:  07 <len> <source> <len bytes of RACE stream>   62 bytes

RACE packet:
    05 <type> <length u16 LE> <cmd id u16 LE> <payload: length-2 bytes>
    type: 5A command expecting a response, 5B response, 5C command without response, 5D indication

Long packets span several input reports, so the input payloads are concatenated and cut into
RACE packets by their own length field (Reassembler).
"""
from __future__ import annotations

import errno
import fcntl
import glob
import json
import os
import select
import struct
import time
from dataclasses import dataclass, field
from typing import Iterator, List, Optional

VID = 0x10F5
KNOWN_PIDS = {
    0x2287: "Stealth Pro II PC (USB-A transmitter)",
    0x2289: "Stealth Pro II PC headset (USB-C cable)",
    0x229B: "Stealth Pro II transmitter dock",
    0x229E: "Stealth Pro II headset (USB-C cable)",
}

REPORT_OUT = 0x06
REPORT_IN = 0x07
REPORT_SIZE = 62  # report id + 61 data bytes
MAX_STREAM = REPORT_SIZE - 3  # RACE bytes that fit in one report after id, len, target

RACE_HEAD = 0x05
RACE_CMD = 0x5A
RACE_RSP = 0x5B
RACE_CMD_NORSP = 0x5C
RACE_IND = 0x5D
RACE_TYPES = {RACE_CMD: "cmd", RACE_RSP: "rsp", RACE_CMD_NORSP: "cmd-norsp", RACE_IND: "ind"}

# GET_REPORT polling cadence; the vendor app paces its asks at about 60 ms
POLL_INTERVAL = 0.060

# RACE command id ranges we must never send (flash storage, firmware update)
FORBIDDEN_CMD_RANGES = [(0x0400, 0x04FF), (0x1C00, 0x1CFF)]


def _ioc(direction: int, type_: str, nr: int, size: int) -> int:
    return (direction << 30) | (size << 16) | (ord(type_) << 8) | nr


def HIDIOCGINPUT(size: int) -> int:
    return _ioc(3, "H", 0x0A, size)


# ---------------------------------------------------------------------------
# device discovery


@dataclass
class HidrawInfo:
    path: str
    pid: int
    name: str
    vendor_collection: bool


def find_devices() -> List[HidrawInfo]:
    """Every hidraw node of a Turtle Beach device, flagging the ones with the 0xFF13 collection."""
    found = []
    for node in sorted(glob.glob("/sys/class/hidraw/hidraw*")):
        try:
            with open(os.path.join(node, "device", "uevent")) as f:
                uevent = dict(line.strip().split("=", 1) for line in f if "=" in line)
            with open(os.path.join(node, "device", "report_descriptor"), "rb") as f:
                rdesc = f.read()
        except OSError:
            continue
        _bus, vid, pid = uevent.get("HID_ID", "0:0:0").split(":")
        if int(vid, 16) != VID:
            continue
        found.append(HidrawInfo(
            path="/dev/" + os.path.basename(node),
            pid=int(pid, 16),
            name=uevent.get("HID_NAME", "?"),
            vendor_collection=b"\x06\x13\xff" in rdesc,  # Usage Page (0xFF13)
        ))
    return found


def first_device() -> Optional[HidrawInfo]:
    devs = [d for d in find_devices() if d.vendor_collection]
    known = [d for d in devs if d.pid in KNOWN_PIDS]
    return (known or devs or [None])[0]


def default_device() -> HidrawInfo:
    dev = first_device()
    if dev is None:
        raise SystemExit("no Turtle Beach device with a 0xFF13 vendor collection found")
    return dev


# ---------------------------------------------------------------------------
# RACE packets


def race_packet(type_: int, cmd: int, payload: bytes) -> bytes:
    return struct.pack("<BBHH", RACE_HEAD, type_, len(payload) + 2, cmd) + payload


@dataclass
class RacePacket:
    type: int
    cmd: int
    payload: bytes
    source: int = 0  # byte 2 of the HID report the packet started in

    @property
    def raw(self) -> bytes:
        return race_packet(self.type, self.cmd, self.payload)

    def describe(self) -> dict:
        d = {
            "type": RACE_TYPES.get(self.type, f"{self.type:02x}"),
            "cmd": f"{self.cmd:04x}",
            "src": f"{self.source:02x}",
            "len": len(self.payload),
            "payload": self.payload.hex(" "),
        }
        docs = json_documents(self.payload)
        if docs:
            d["json"] = docs
        else:
            text = printable_runs(self.payload)
            if text:
                d["ascii"] = text
        return d


class Reassembler:
    """Cuts the concatenated input-report payloads into RACE packets."""

    def __init__(self):
        self.buf = bytearray()
        self.source = 0
        self.skipped = bytearray()

    def feed(self, report: bytes) -> List[RacePacket]:
        if len(report) < 3 or report[0] != REPORT_IN:
            return []
        n = min(report[1], len(report) - 3)
        if n == 0:
            return []
        if not self.buf:
            self.source = report[2]
        self.buf += report[3:3 + n]
        out = []
        while self.buf:
            # resync: a packet starts with 05, a known type and a length of at least the cmd id
            if self.buf[0] != RACE_HEAD or (len(self.buf) > 1 and self.buf[1] not in RACE_TYPES):
                self.skipped.append(self.buf.pop(0))
                continue
            if len(self.buf) < 4:
                break
            length = struct.unpack_from("<H", self.buf, 2)[0]
            if length < 2 or length > 0x1000:
                self.skipped.append(self.buf.pop(0))
                continue
            total = 4 + length
            if len(self.buf) < total:
                break
            raw = bytes(self.buf[:total])
            del self.buf[:total]
            cmd = struct.unpack_from("<H", raw, 4)[0] if total >= 6 else 0
            out.append(RacePacket(raw[1], cmd, raw[6:], self.source))
            self.source = report[2]
        return out


# ---------------------------------------------------------------------------
# helpers for the Turtle Beach JSON layer


def json_documents(blob: bytes) -> List[dict]:
    """Every complete top-level JSON object embedded in a byte string, in order."""
    docs = []
    i = 0
    while True:
        start = blob.find(b"{", i)
        if start < 0:
            return docs
        end = _object_end(blob, start)
        if end is None:
            i = start + 1
            continue
        try:
            doc = json.loads(blob[start:end].decode("utf-8"))
            if isinstance(doc, dict):
                docs.append(doc)
                i = end
                continue
        except (ValueError, UnicodeDecodeError):
            pass
        i = start + 1


def _object_end(blob: bytes, start: int) -> Optional[int]:
    depth = 0
    in_str = esc = False
    for j in range(start, len(blob)):
        c = blob[j]
        if in_str:
            if esc:
                esc = False
            elif c == 0x5C:
                esc = True
            elif c == 0x22:
                in_str = False
        elif c == 0x22:
            in_str = True
        elif c == 0x7B:
            depth += 1
        elif c == 0x7D:
            depth -= 1
            if depth == 0:
                return j + 1
        elif c < 0x20 and c not in (0x09, 0x0A, 0x0D):
            return None
    return None


def printable_runs(blob: bytes, minimum: int = 3) -> List[str]:
    runs, cur = [], bytearray()
    for c in blob + b"\0":
        if 0x20 <= c < 0x7F:
            cur.append(c)
        else:
            if len(cur) >= minimum:
                runs.append(cur.decode())
            cur = bytearray()
    return runs


# ---------------------------------------------------------------------------
# the hidraw device


class RaceHid:
    def __init__(self, path: str, log=None):
        self.path = path
        self.fd = os.open(path, os.O_RDWR | os.O_NONBLOCK)
        self.log = log  # optional callable(event: dict)

    def close(self):
        os.close(self.fd)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def _emit(self, direction: str, data: bytes, how: str):
        if self.log:
            self.log({"t": time.time(), "dir": direction, "via": how, "hex": data.hex()})

    def write_report(self, report: bytes):
        """Send one output report exactly as given (report id first), padded to 62 bytes."""
        if report[0] != REPORT_OUT or len(report) > REPORT_SIZE:
            raise ValueError("not an output report 0x06 of at most 62 bytes")
        if len(report) >= 9 and report[3] == RACE_HEAD:
            cmd = struct.unpack_from("<H", report, 7)[0]
            for lo, hi in FORBIDDEN_CMD_RANGES:
                if lo <= cmd <= hi:
                    raise ValueError(f"refusing to send RACE command {cmd:04x}")
        report = report.ljust(REPORT_SIZE, b"\0")
        os.write(self.fd, report)
        self._emit("out", report, "write")

    def send_race(self, race: bytes, target: int = 0x00):
        if len(race) > MAX_STREAM:
            raise ValueError("RACE packet longer than one report; splitting is not implemented")
        self.write_report(bytes([REPORT_OUT, len(race), target]) + race)

    def read_interrupt(self, timeout: float) -> Optional[bytes]:
        """One input report from the interrupt endpoint, or None after timeout seconds."""
        r, _, _ = select.select([self.fd], [], [], timeout)
        if not r:
            return None
        try:
            data = os.read(self.fd, 256)
        except OSError as e:
            if e.errno in (errno.EAGAIN, errno.EWOULDBLOCK):
                return None
            raise
        self._emit("in", data, "interrupt")
        return data

    def get_input_report(self) -> bytes:
        """GET_REPORT(Input, 0x07) over the control endpoint."""
        buf = bytearray(REPORT_SIZE)
        buf[0] = REPORT_IN
        try:
            n = fcntl.ioctl(self.fd, HIDIOCGINPUT(REPORT_SIZE), buf, True)
        except OSError as e:
            # the transmitter occasionally fails a control transfer (EPROTO/EPIPE/ETIMEDOUT) while
            # it is busy; treat it as an idle poll. A gone device (ENODEV) still raises.
            if e.errno not in (errno.EPROTO, errno.EPIPE, errno.ETIMEDOUT, errno.EAGAIN, errno.EIO):
                raise
            if self.log:
                self.log({"t": time.time(), "dir": "in", "via": "get_report", "error": e.strerror})
            return bytes([REPORT_IN, 0, 0])
        data = bytes(buf[:n])
        self._emit("in", data, "get_report")
        return data

    def reports(self, duration: float, poll_get_report: float = POLL_INTERVAL) -> Iterator[bytes]:
        """Input reports for `duration` seconds.

        The transmitter queues its replies as a byte stream and serves them only on GET_REPORT, so
        it is polled every poll_get_report seconds; a report with a zero length byte is idle and
        skipped. The interrupt endpoint is read as well in case anything ever arrives there.
        """
        end = time.monotonic() + duration
        next_poll = time.monotonic()
        while True:
            now = time.monotonic()
            if now >= end:
                return
            wait = end - now
            if poll_get_report:
                wait = max(0.0, min(wait, next_poll - now))
            data = self.read_interrupt(wait)
            if data is not None:
                yield data
                continue
            if poll_get_report and time.monotonic() >= next_poll:
                next_poll = time.monotonic() + poll_get_report
                data = self.get_input_report()
                if len(data) > 1 and data[1]:
                    yield data


class JsonlLog:
    """Appends every event to a JSONL file."""

    def __init__(self, path: str):
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        self.f = open(path, "a")

    def __call__(self, event: dict):
        self.f.write(json.dumps(event) + "\n")
        self.f.flush()

    def close(self):
        self.f.close()
