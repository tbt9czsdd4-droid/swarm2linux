use std::collections::VecDeque;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use race_hid::{HidrawDevice, RacePacket, RaceType, Reassembler, ReportIo, discover};
use serde_json::Value;

use crate::doc::Document;
use crate::frame::{CMD_KVP, Message, MessageKind, Request, Token};
use crate::{Error, READ_ONCE, Result, keys};

/// Talks to one transmitter or headset.
///
/// Replies are matched to requests by their echoed token. Documents pushed for subscriptions arrive
/// interleaved with replies; they are queued and handed out by [`Client::take_updates`].
pub struct Client<T: ReportIo> {
    io: T,
    reasm: Reassembler,
    last: Token,
    updates: VecDeque<Document>,
    /// Pause between polls while the device has nothing to say.
    pub poll_interval: Duration,
    /// How long to wait for a reply.
    pub timeout: Duration,
    /// How often an unanswered request is sent again (with a fresh token). Writes are absolute values, so
    /// repeating one is harmless.
    pub retries: u8,
}

/// Open a device (the given hidraw path, or the first Turtle Beach device with the vendor collection) and
/// start a session the way Swarm II does: drop stale queued bytes, then subscribe to `Inf`. An idle
/// transmitter ignores other requests until it has seen that opener.
pub fn open(path: Option<&Path>) -> Result<Client<HidrawDevice>> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => discover()?.into_iter().find(|d| d.vendor_collection).ok_or(Error::NoDevice)?.path,
    };
    let mut client = Client::new(HidrawDevice::open(path)?);
    client.drain()?;
    client.subscribe("Inf")?;
    Ok(client)
}

impl<T: ReportIo> Client<T> {
    pub fn new(io: T) -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let session = 0x10 + (nanos % 0xE0) as u8;
        Self::with_token(io, Token { session, counter: (nanos >> 8) as u8, seq: 0 })
    }

    /// A client whose next request uses the token after `last` (for reproducing captured frames).
    pub fn with_token(io: T, last: Token) -> Self {
        Self {
            io,
            reasm: Reassembler::new(),
            last,
            updates: VecDeque::new(),
            poll_interval: Duration::from_millis(60),
            timeout: Duration::from_secs(2),
            retries: 1,
        }
    }

    pub fn io(&self) -> &T {
        &self.io
    }

    /// Discard whatever the device queued while nobody was polling. Pushes found are kept.
    pub fn drain(&mut self) -> Result<()> {
        for _ in 0..256 {
            if !self.pump()? {
                break;
            }
        }
        Ok(())
    }

    pub fn index(&mut self) -> Result<Vec<String>> {
        let req = Request::Index;
        let reply = self.transact(&req)?;
        let names = reply.json().and_then(|v| v.as_array().cloned()).ok_or_else(|| Error::BadReply(req.describe()))?;
        Ok(names.iter().filter_map(Value::as_str).map(str::to_string).collect())
    }

    /// Read a record and subscribe to its changes (records in [`READ_ONCE`] are only read).
    pub fn subscribe(&mut self, record: &str) -> Result<Document> {
        if READ_ONCE.contains(&record) {
            return self.read(record);
        }
        self.document(Request::Subscribe(record.to_string()))
    }

    /// Read a record once.
    pub fn read(&mut self, record: &str) -> Result<Document> {
        self.document(Request::Read(record.to_string()))
    }

    /// Write one key. Only keys and values Swarm II was seen writing are accepted.
    pub fn set(&mut self, key: u16, value: i64) -> Result<()> {
        let s = keys::writable(key).ok_or(Error::NotWritable(key))?;
        if !(s.min..=s.max).contains(&value) {
            return Err(Error::OutOfRange { name: s.name, value, min: s.min, max: s.max });
        }
        let json = serde_json::json!({ format!("{key:#x}"): value.to_string() }).to_string();
        let reply = self.transact(&Request::SetKvp(json))?;
        if reply.is_set_ack() { Ok(()) } else { Err(Error::Nack(key)) }
    }

    /// Poll for `duration`, collecting pushed documents.
    pub fn wait_updates(&mut self, duration: Duration) -> Result<Vec<Document>> {
        let end = Instant::now() + duration;
        while Instant::now() < end {
            if !self.pump()? {
                sleep(self.poll_interval.min(end.saturating_duration_since(Instant::now())));
            }
        }
        Ok(self.take_updates())
    }

    /// Pushed documents received so far.
    pub fn take_updates(&mut self) -> Vec<Document> {
        self.updates.drain(..).collect()
    }

    fn document(&mut self, req: Request) -> Result<Document> {
        let reply = self.transact(&req)?;
        reply.json().as_ref().and_then(Document::from_json).ok_or_else(|| Error::BadReply(req.describe()))
    }

    fn next_token(&mut self) -> Token {
        self.last = Token {
            session: self.last.session,
            counter: self.last.counter.wrapping_add(1),
            seq: self.last.seq.wrapping_add(1),
        };
        self.last
    }

    fn transact(&mut self, req: &Request) -> Result<Message> {
        for _ in 0..=self.retries {
            let token = self.next_token();
            let race = RacePacket::new(RaceType::Command, CMD_KVP, req.encode(token)).encode();
            self.io.send_race(0x00, &race)?;
            let deadline = Instant::now() + self.timeout;
            loop {
                let (data, reply) = self.pump_for(Some(token.bytes()))?;
                if let Some(reply) = reply {
                    return Ok(reply);
                }
                if Instant::now() >= deadline {
                    break;
                }
                if !data {
                    sleep(self.poll_interval);
                }
            }
        }
        Err(Error::Timeout(req.describe()))
    }

    /// Read one report; returns whether it carried data.
    fn pump(&mut self) -> Result<bool> {
        Ok(self.pump_for(None)?.0)
    }

    /// Read one report, queue pushes, and return the reply with `token` if it completed in this report.
    fn pump_for(&mut self, token: Option<[u8; 3]>) -> Result<(bool, Option<Message>)> {
        let report = self.io.read_report()?;
        let data = report.len() > 1 && report[1] > 0;
        let mut reply = None;
        for packet in self.reasm.feed(&report) {
            if packet.cmd != CMD_KVP {
                continue;
            }
            let Some(msg) = Message::decode(&packet.payload) else { continue };
            match msg.kind {
                MessageKind::Reply if Some(msg.token) == token => reply = Some(msg),
                MessageKind::Push => {
                    if let Some(doc) = msg.json().as_ref().and_then(Document::from_json) {
                        self.updates.push_back(doc);
                    }
                }
                _ => {}
            }
        }
        Ok((data, reply))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocKind;
    use std::io;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// Answers each request with scripted input reports.
    struct Scripted {
        sent: Vec<Vec<u8>>,
        replies: VecDeque<Vec<Vec<u8>>>,
        queue: VecDeque<Vec<u8>>,
    }

    impl Scripted {
        fn new(replies: Vec<Vec<&str>>) -> Self {
            let replies = replies.into_iter().map(|r| r.into_iter().map(hex).collect()).collect();
            Self { sent: Vec::new(), replies, queue: VecDeque::new() }
        }
    }

    impl ReportIo for Scripted {
        fn send_race(&mut self, _target: u8, race: &[u8]) -> io::Result<()> {
            self.sent.push(race.to_vec());
            self.queue.extend(self.replies.pop_front().unwrap_or_default());
            Ok(())
        }

        fn read_report(&mut self) -> io::Result<Vec<u8>> {
            Ok(self.queue.pop_front().unwrap_or_else(|| vec![0x07, 0, 0]))
        }
    }

    fn client(io: Scripted, last: Token) -> Client<Scripted> {
        let mut c = Client::with_token(io, last);
        c.poll_interval = Duration::ZERO;
        c.timeout = Duration::from_millis(20);
        c
    }

    #[test]
    fn reads_a_record_split_over_reports() {
        // the GSI reply of our Linux probe, token 44 14 04, in four input reports
        let io = Scripted::new(vec![vec![
            "073b00055bd000029968454414040000000000000061096232202101ff7b224f52223a22475349222c224b5650223a7b22323030223a2231222c22323130",
            "073b00223a2230222c22323230223a224d792048656164736574222c22323330223a2232222c22323430223a223635222c22323530223a2230222c223236",
            "073b0030223a22313030222c22323730223a2230222c22323830223a2234222c22323930223a2233222c22326130223a223432222c22326230223a223630",
            "072300222c22326330223a223530222c22326430223a223738222c22326530223a2230227d7d",
        ]]);
        let mut c = client(io, Token { session: 0x44, counter: 0x13, seq: 0x03 });
        let doc = c.subscribe("GSI").unwrap();
        assert_eq!(doc.kind, DocKind::Reply);
        assert_eq!(doc.kvp["240"], "65");
        assert_eq!(doc.kvp["220"], "My Headset");
        assert_eq!(doc.kvp.len(), 15);
    }

    #[test]
    fn reads_the_index() {
        let io = Scripted::new(vec![vec![
            "073b00055baf0002996845aa8a0100000000000000c1322101ff5b22566572222c22496e66222c22475349222c224254222c22545831222c22545832222c",
            "073b0022545833222c22545834222c22334454222c224d6963222c22534146222c22456e63222c2242746e222c22415147222c2241514d222c2243454322",
            "073b002c22434731222c22434732222c22434733222c22434734222c22434735222c22434d31222c22434d32222c22434d33222c22434d34222c22434d35",
            "070200225d",
        ]]);
        let mut c = client(io, Token { session: 0xAA, counter: 0x89, seq: 0x00 });
        let names = c.index().unwrap();
        assert_eq!(names.len(), 26);
        assert_eq!((names[0].as_str(), names[25].as_str()), ("Ver", "CM5"));
    }

    #[test]
    fn set_is_acked_and_pushes_are_queued() {
        // Swarm II selecting Bass Boost: ack, then the AQG push and an unrelated BT push
        let io = Scripted::new(vec![vec![
            "071200055b0e0002996843aaa51c00000000000000",
            "073b00055b6d000299584500630f0000000000000061076232ef2101ff7b225550223a22415147222c224b5650223a7b2231323130223a2232222c223132",
            "0736003230223a223530222c2231323330223a223530222c2231323430223a223335222c2231326330223a224261737320426f6f7374227d7d",
            "073900055b3500029958450064050000000000000061296232ef2101ff7b225550223a224254222c224b5650223a7b22333230223a222d3533227d7d",
        ]]);
        let mut c = client(io, Token { session: 0xAA, counter: 0xA4, seq: 0x1B });
        c.set(keys::GAME_EQ_PRESET, 2).unwrap();
        assert_eq!(
            c.io().sent[0],
            hex("055a250002994803aaa51c00000000000000b77365745f6b7670ff7b22307831323130223a2232227d")
        );
        let updates = c.wait_updates(Duration::from_millis(5)).unwrap();
        assert_eq!(updates.len(), 2);
        assert_eq!((updates[0].record.as_str(), &updates[0].kvp["12c0"]), ("AQG", &Value::from("Bass Boost")));
        assert_eq!(updates[1].record, "BT");
    }

    #[test]
    fn refuses_unknown_keys_and_ranges() {
        let mut c = client(Scripted::new(vec![]), Token { session: 1, counter: 1, seq: 1 });
        assert!(matches!(c.set(keys::NAME, 1), Err(Error::NotWritable(0x220))));
        assert!(matches!(c.set(keys::GAME_EQ_PRESET, 9), Err(Error::OutOfRange { .. })));
        assert!(c.io().sent.is_empty());
    }

    #[test]
    fn times_out_without_reply() {
        let mut c = client(Scripted::new(vec![]), Token { session: 1, counter: 1, seq: 1 });
        assert!(matches!(c.subscribe("GSI"), Err(Error::Timeout(_))));
        assert_eq!(c.io().sent.len(), 2, "one retry");
    }

    #[test]
    fn retry_uses_a_fresh_token() {
        // no answer to the first ask; the Ver reply carries the second ask's token aa 8b 02
        let io = Scripted::new(vec![
            vec![],
            vec!["073a00055b360002996845aa8b020000000000000061026232d52101ff7b224f52223a22566572222c224b5650223a7b2230223a22322e302e30227d7d"],
        ]);
        let mut c = client(io, Token { session: 0xAA, counter: 0x89, seq: 0x00 });
        assert_eq!(c.subscribe("Ver").unwrap().kvp["0"], "2.0.0");
        assert_eq!(c.io().sent.len(), 2);
    }
}
