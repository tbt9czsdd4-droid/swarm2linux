"""Output reports observed from Turtle Beach's Swarm II, replayed byte for byte.

Only frames in this file may be sent to the device by the RE tools. Each entry records where it was observed.

Source of the first frames: a USB capture of Swarm II talking to a Stealth Pro II transmitter, published in
HaloBattery (github.com/HeyOkay/HaloBattery, providers/turtlebeach.py, MIT; capture from issue #173).
"""

# Layout, as far as known:
#   06 18 00 | 05 5a 14 00 02 99 | 48 01 <op> <token: 2 bytes> 00 .. 00 | 61 00 <4-byte ASCII name>
#   report 6, 24 RACE bytes, target 00 | RACE cmd 0x9902 expecting a response | Turtle Beach sub-header | "a\0" + name
# The name starts with an action letter ("S" for the get-style asks seen so far) followed by a record name.

# "SInf": session opener. The device answers with a dump of its whole state. Capture packet 150.
ASK_SINF = bytes.fromhex(
    "061800055a140002994801c04a010000"
    "0000000000610053496e660000000000"
    "00000000000000000000000000000000"
    "0000000000000000000000000000"
)

# "SGSI": ask for the GSI (general status info) record. Capture packet 616 (token 14 04).
ASK_GSI = bytes.fromhex(
    "061800055a1400029948014414040000"
    "00000000006100534753490000000000"
    "00000000000000000000000000000000"
    "0000000000000000000000000000"
)

# The GSI ask with token 11 01. NOT captured as such: HaloBattery inferred it from the token of the
# capture's index ask (packet 280), and it answered on a reporter's unit.
ASK_GSI_TOKEN_1101 = ASK_GSI[:12] + bytes([0x11, 0x01]) + ASK_GSI[14:]

FRAMES = {
    "sinf": ASK_SINF,
    "gsi": ASK_GSI,
    "gsi-1101": ASK_GSI_TOKEN_1101,
}
