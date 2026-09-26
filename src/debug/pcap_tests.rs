use super::*;

fn capture(little: bool, nano: bool) -> Vec<u8> {
    let magic: u32 = if nano { 0xa1b23c4d } else { 0xa1b2c3d4 };
    let mut bytes = if little { magic.to_le_bytes().to_vec() } else { magic.to_be_bytes().to_vec() };
    for value in [2u16, 4] {
        bytes.extend(if little { value.to_le_bytes() } else { value.to_be_bytes() });
    }
    for value in [0u32, 0, 65535, 1, 7, 123, 14, 14] {
        bytes.extend(if little { value.to_le_bytes() } else { value.to_be_bytes() });
    }
    bytes.extend([1u8; 14]);
    bytes
}

#[test]
fn timestamps_and_packets_are_read_in_both_byte_orders_and_resolutions() {
    for little in [true, false] {
        for nano in [true, false] {
            let bytes = capture(little, nano);
            let mut reader = CaptureReader::new(bytes.as_slice()).unwrap();
            let packet = reader.next_packet().unwrap().unwrap();
            assert_eq!(packet.bytes, vec![1; 14]);
            assert_eq!(packet.timestamp_ns, 7_000_000_000 + if nano { 123 } else { 123_000 });
            assert!(reader.next_packet().unwrap().is_none());
        }
    }
}

#[test]
fn partial_file_or_record_is_an_error_instead_of_a_successful_end() {
    let bytes = capture(true, false);
    for length in 0..bytes.len() {
        let outcome = CaptureReader::new(&bytes[..length]);
        if length < FILE_HEADER_BYTES {
            assert!(outcome.is_err());
        } else if length == FILE_HEADER_BYTES {
            assert!(outcome.unwrap().next_packet().unwrap().is_none());
        } else {
            assert!(outcome.unwrap().next_packet().is_err(), "length={length}");
        }
    }
}

#[test]
fn oversized_records_invalid_lengths_and_invalid_timestamps_are_rejected() {
    for (offset, value) in [(16, 0u32), (20, 113), (28, 1_000_000), (32, 65536), (36, 13)] {
        let mut bytes = capture(true, false);
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let outcome = CaptureReader::new(bytes.as_slice());
        if offset < FILE_HEADER_BYTES {
            assert!(outcome.is_err());
        } else {
            assert!(outcome.unwrap().next_packet().is_err());
        }
    }
}

#[test]
fn capture_truncation_is_retained_for_the_admission_check() {
    let mut bytes = capture(true, false);
    bytes[36..40].copy_from_slice(&60u32.to_le_bytes());
    let packet = CaptureReader::new(bytes.as_slice()).unwrap().next_packet().unwrap().unwrap();
    assert_eq!(packet.original_length, 60);
    assert_eq!(packet.bytes.len(), 14);
}
