//! Build codes: ImportTab.lua :505 `common.base64.encode(Deflate(xml)):gsub("+","-"):gsub("/","_")` and :632
//! `Inflate(common.base64.decode(buf:gsub("-","+"):gsub("_","/")))`.
//!
//! `base64` is `runtime/lua/base64.lua` ported verbatim (no crate): the encoder is standard padded base64; the decoder is
//! lenient -- it first drops every byte outside the alphabet and `=` (:26), then ignores `=` wherever it is (:28) and
//! drops a trailing partial byte (:33). `Deflate`/`Inflate` are SimpleGraphic's zlib (RFC 1950) streams; byte equality of
//! the deflate output with PoB is not required (R8-ARCH §1 p. 3) -- any valid zlib stream is accepted by PoB/pobb.in.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

/// base64.lua:5 `b`.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// base64.lua:11-22 `base64.encode(data)`: 6-bit groups over the bit string (+ `0000` padding bits, a group shorter
/// than 6 dropped), then `({ '', '==', '=' })[#data % 3 + 1]`.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        // 1 byte -> 8 bits + "0000" -> 2 groups; 2 bytes -> 16 + 4 -> 3 groups; 3 bytes -> 24 (+4 dropped) -> 4 groups.
        let groups = chunk.len() + 1;
        for i in 0..groups {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out.push_str(["", "==", "="][data.len() % 3]);
    out
}

/// base64.lua:25-38 `base64.decode(data)`.
pub fn base64_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in data {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue, // :26 stripped; `=` (:28) contributes no bits either
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out // :33 `#x ~= 8` -> the trailing partial byte is dropped
}

/// `Deflate(data)`: a zlib stream (default level).
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::with_capacity(data.len() / 3), Compression::default());
    enc.write_all(data).expect("writing into a Vec cannot fail");
    enc.finish().expect("writing into a Vec cannot fail")
}

/// `Inflate(data)`: `None` where SimpleGraphic returns nil (not a valid zlib stream).
pub fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() * 4);
    ZlibDecoder::new(data).read_to_end(&mut out).ok()?;
    Some(out)
}

/// ImportTab.lua:505: the share code of a `SaveDB` text.
pub fn encode_build_code(xml: &[u8]) -> String {
    base64_encode(&deflate(xml)).replace('+', "-").replace('/', "_")
}

/// ImportTab.lua:632: the XML text of a share code (`None` = `Inflate` failed).
pub fn decode_build_code(code: &str) -> Option<Vec<u8>> {
    let swapped: Vec<u8> = code
        .bytes()
        .map(|c| match c {
            b'-' => b'+',
            b'_' => b'/',
            c => c,
        })
        .collect();
    inflate(&base64_decode(&swapped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_lua_semantics() {
        for (raw, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("\u{ff}\u{fe}", "w7/Dvg=="),
        ] {
            assert_eq!(base64_encode(raw.as_bytes()), enc);
            assert_eq!(base64_decode(enc.as_bytes()), raw.as_bytes());
        }
        // :26/:28/:33 leniency: junk and inner `=` ignored, trailing partial byte dropped.
        assert_eq!(base64_decode(b"Zm\n9v=Yg"), b"foob");
        assert_eq!(base64_decode(b"Zm9vY"), b"foo");
        assert_eq!(decode_build_code("!!"), None);
    }
}
