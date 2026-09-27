//! 同じ入力と加工結果から同じ配送IDを作り、再配送時の重複を抑制する。
use amitoki_relay::Frame;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(super) fn rewritten_frame(original: &Frame, bytes: Bytes) -> Frame {
    if bytes == original.bytes {
        return original.clone();
    }
    let mut hash = Sha256::new();
    hash.update(b"amitoki/rewrite/v1\0");
    hash.update(original.id.as_bytes());
    hash.update(&bytes);
    let mut id: [u8; 16] = hash.finalize()[..16].try_into().expect("SHA256 contains 16 bytes");
    // UUID v8のアプリケーション定義領域を使用する。
    id[6] = (id[6] & 0x0f) | 0x80;
    id[8] = (id[8] & 0x3f) | 0x80;
    Frame { id: Uuid::from_bytes(id), bytes }
}

#[cfg(test)]
#[path = "rewrite_tests.rs"]
mod tests;
