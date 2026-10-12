use crate::REPORT_IN;

const HEAD: u8 = 0x05;
/// Longest RACE length field accepted while resyncing; anything larger is treated as noise.
const MAX_LENGTH: usize = 0x1000;

/// The type byte of a RACE packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaceType {
    /// Command expecting a response (`5A`).
    Command,
    /// Response (`5B`). The Stealth Pro II also uses it for pushed documents.
    Response,
    /// Command without response (`5C`); the transmitter sends these unprompted too.
    CommandNoResponse,
    /// Indication (`5D`).
    Indication,
}

impl RaceType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x5A => Some(Self::Command),
            0x5B => Some(Self::Response),
            0x5C => Some(Self::CommandNoResponse),
            0x5D => Some(Self::Indication),
            _ => None,
        }
    }

    pub fn byte(self) -> u8 {
        match self {
            Self::Command => 0x5A,
            Self::Response => 0x5B,
            Self::CommandNoResponse => 0x5C,
            Self::Indication => 0x5D,
        }
    }
}

/// One RACE packet: `05 <type> <length u16 LE> <cmd u16 LE> <payload>`, length = payload + 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RacePacket {
    pub kind: RaceType,
    pub cmd: u16,
    pub payload: Vec<u8>,
}

impl RacePacket {
    pub fn new(kind: RaceType, cmd: u16, payload: Vec<u8>) -> Self {
        Self { kind, cmd, payload }
    }

    pub fn encode(&self) -> Vec<u8> {
        let length = (self.payload.len() + 2) as u16;
        let mut out = Vec::with_capacity(6 + self.payload.len());
        out.extend_from_slice(&[HEAD, self.kind.byte()]);
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&self.cmd.to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

/// Cuts the concatenated payloads of input reports into RACE packets.
///
/// The transmitter's queue can start mid-packet (its oldest bytes get dropped), so bytes that cannot start a
/// packet are skipped until a plausible header (`05`, a known type, a sane length) comes along.
#[derive(Debug, Default)]
pub struct Reassembler {
    buf: Vec<u8>,
    skipped: usize,
}

impl Reassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one input report (report ID first); returns the packets it completed.
    pub fn feed(&mut self, report: &[u8]) -> Vec<RacePacket> {
        if report.len() < 3 || report[0] != REPORT_IN {
            return Vec::new();
        }
        let n = usize::from(report[1]).min(report.len() - 3);
        self.buf.extend_from_slice(&report[3..3 + n]);

        let mut out = Vec::new();
        loop {
            let start = self.buf.iter().position(|&b| b == HEAD).unwrap_or(self.buf.len());
            self.skip(start);
            if self.buf.len() < 2 {
                break;
            }
            let Some(kind) = RaceType::from_byte(self.buf[1]) else {
                self.skip(1);
                continue;
            };
            if self.buf.len() < 4 {
                break;
            }
            let length = usize::from(u16::from_le_bytes([self.buf[2], self.buf[3]]));
            if !(2..=MAX_LENGTH).contains(&length) {
                self.skip(1);
                continue;
            }
            if self.buf.len() < 4 + length {
                break;
            }
            let cmd = u16::from_le_bytes([self.buf[4], self.buf[5]]);
            let payload = self.buf[6..4 + length].to_vec();
            self.buf.drain(..4 + length);
            out.push(RacePacket::new(kind, cmd, payload));
        }
        out
    }

    /// Bytes thrown away while resyncing so far.
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    fn skip(&mut self, n: usize) {
        self.buf.drain(..n);
        self.skipped += n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(stream: &[u8]) -> Vec<u8> {
        let mut r = vec![REPORT_IN, stream.len() as u8, 0];
        r.extend_from_slice(stream);
        r.resize(crate::REPORT_SIZE, 0);
        r
    }

    #[test]
    fn encodes_header_and_length() {
        let p = RacePacket::new(RaceType::Command, 0x9902, vec![0x48, 0x01]);
        assert_eq!(p.encode(), [0x05, 0x5A, 0x04, 0x00, 0x02, 0x99, 0x48, 0x01]);
    }

    #[test]
    fn reassembles_across_reports() {
        let pkt = RacePacket::new(RaceType::Response, 0x9902, (0u8..100).collect()).encode();
        let mut r = Reassembler::new();
        let mut got = Vec::new();
        for chunk in pkt.chunks(crate::MAX_STREAM) {
            got.extend(r.feed(&report(chunk)));
        }
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].payload, (0u8..100).collect::<Vec<_>>());
    }

    #[test]
    fn several_packets_in_one_report() {
        // seen from the transmitter: 2c80 (5C) and 3103 (5D) back to back
        let stream = [0x05, 0x5C, 0x03, 0x00, 0x80, 0x2C, 0x01, 0x05, 0x5D, 0x03, 0x00, 0x03, 0x31, 0x01];
        let got = Reassembler::new().feed(&report(&stream));
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].kind, got[0].cmd, got[0].payload.as_slice()), (RaceType::CommandNoResponse, 0x2C80, &[1u8][..]));
        assert_eq!((got[1].kind, got[1].cmd), (RaceType::Indication, 0x3103));
    }

    #[test]
    fn resyncs_after_a_cut_off_start() {
        // the tail of a 2cb1 indication (contains no 05) followed by a whole packet
        let mut stream = vec![0x02, 0x00, 0x01, 0x93, 0x34, 0x92, 0xED, 0xCA, 0xC7, 0xFF, 0x00];
        stream.extend_from_slice(&[0x05, 0x5C, 0x03, 0x00, 0x80, 0x2C, 0x03]);
        let mut r = Reassembler::new();
        let got = r.feed(&report(&stream));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].cmd, 0x2C80);
        assert_eq!(r.skipped(), 11);
    }

    #[test]
    fn ignores_idle_and_foreign_reports() {
        let mut r = Reassembler::new();
        assert!(r.feed(&[REPORT_IN, 0, 0]).is_empty());
        assert!(r.feed(&[0x0C, 1, 0, 0x05]).is_empty());
    }
}
