//! classic PCAP 2.4のEthernetレコードを、ファイル全体を保持せずに読む。
use amitoki_relay::MAX_FRAME_SIZE;
use std::io::{self, Read};

// PCAPの固定長ヘッダ（libpcap pcap-savefile(5)）。
const FILE_HEADER_BYTES: usize = 24;
const RECORD_HEADER_BYTES: usize = 16;
const LINKTYPE_ETHERNET: u32 = 1;
const NANOS_PER_SECOND: u64 = 1_000_000_000;

pub struct CapturePacket {
    pub bytes: Vec<u8>,
    pub original_length: u32,
    pub timestamp_ns: u64,
}

pub struct CaptureReader<R> {
    reader: R,
    little_endian: bool,
    nanoseconds: bool,
    snaplen: u32,
}

impl<R: Read> CaptureReader<R> {
    pub fn new(mut reader: R) -> io::Result<Self> {
        let mut header = [0; FILE_HEADER_BYTES];
        reader.read_exact(&mut header)?;
        let (little_endian, nanoseconds) = match header[..4] {
            [0xd4, 0xc3, 0xb2, 0xa1] => (true, false),
            [0xa1, 0xb2, 0xc3, 0xd4] => (false, false),
            [0x4d, 0x3c, 0xb2, 0xa1] => (true, true),
            [0xa1, 0xb2, 0x3c, 0x4d] => (false, true),
            _ => return Err(invalid("classic PCAPを指定してください。PCAPNGは未対応です")),
        };
        let mut capture = Self {
            reader,
            little_endian,
            nanoseconds,
            snaplen: 0,
        };
        if capture.u16(&header[4..6]) != 2 || capture.u16(&header[6..8]) != 4 {
            return Err(invalid("PCAP 2.4以外の版には対応していません"));
        }
        if capture.u32(&header[20..24]) != LINKTYPE_ETHERNET {
            return Err(invalid("Ethernet（LINKTYPE_ETHERNET、FCS情報なし）のPCAPを指定してください"));
        }
        capture.snaplen = capture.u32(&header[16..20]);
        if capture.snaplen == 0 {
            return Err(invalid("PCAPのsnaplenが0です"));
        }
        Ok(capture)
    }

    pub fn next_packet(&mut self) -> io::Result<Option<CapturePacket>> {
        let mut header = [0; RECORD_HEADER_BYTES];
        match self.reader.read_exact(&mut header[..1]) {
            Ok(()) => {},
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(error) => return Err(error),
        }
        self.reader.read_exact(&mut header[1..])?;
        let seconds = self.u32(&header[..4]) as u64;
        let fraction = self.u32(&header[4..8]) as u64;
        let length = self.u32(&header[8..12]);
        let original_length = self.u32(&header[12..16]);
        let resolution = if self.nanoseconds { NANOS_PER_SECOND } else { 1_000_000 };
        if fraction >= resolution || length > self.snaplen || length > original_length || length as usize > MAX_FRAME_SIZE {
            return Err(invalid("PCAPの時刻またはパケット長が不正・上限超過です"));
        }
        let mut bytes = vec![0; length as usize];
        self.reader.read_exact(&mut bytes)?;
        Ok(Some(CapturePacket {
            bytes,
            original_length,
            timestamp_ns: seconds * NANOS_PER_SECOND + fraction * (NANOS_PER_SECOND / resolution),
        }))
    }

    fn u16(&self, bytes: &[u8]) -> u16 {
        let bytes = [bytes[0], bytes[1]];
        if self.little_endian {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    }
    fn u32(&self, bytes: &[u8]) -> u32 {
        let bytes = [bytes[0], bytes[1], bytes[2], bytes[3]];
        if self.little_endian {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
#[path = "pcap_tests.rs"]
mod tests;
