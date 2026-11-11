//! Bytes as text, and the few pieces of cryptography the phone's way needs: hashes, HMAC and random bytes, on ring.

use ring::rand::SecureRandom;

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode(data: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(alphabet[((bits >> (18 - 6 * at)) & 0x3F) as usize] as char);
            } else if pad {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 with its padding, as Python's `base64.b64encode`.
pub fn base64(data: &[u8]) -> String {
    encode(data, STANDARD, true)
}

/// Base64 for an address or a token, without padding: what a JWT and `secrets.token_urlsafe` use.
pub fn url64(data: &[u8]) -> String {
    encode(data, URL, false)
}

/// Python's `base64.b64decode(text, validate=True)`: the alphabet, at most two `=` at the end, whole quads; None
/// for anything else.
pub fn unbase64(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let body = bytes.iter().position(|&byte| byte == b'=').unwrap_or(bytes.len());
    let padding = bytes.len() - body;
    if padding > 2 || bytes[body..].iter().any(|&byte| byte != b'=') || bytes.len() % 4 != 0 {
        return None;
    }
    let value = |byte: u8| STANDARD.iter().position(|&known| known == byte).map(|at| at as u32);
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for quad in bytes.chunks(4) {
        let mut bits = 0u32;
        let mut real = 0;
        for &byte in quad {
            bits <<= 6;
            if byte != b'=' {
                bits |= value(byte)?;
                real += 1;
            }
        }
        let taken = [(bits >> 16) as u8, (bits >> 8) as u8, bits as u8];
        out.extend_from_slice(&taken[..real.max(1) - 1]);
    }
    Some(out)
}

pub fn hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Python's `bytes.fromhex`, for a key as it is kept: pairs of hex digits.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() % 2 != 0 {
        return None;
    }
    digits.chunks(2).map(|pair| Some((pair[0].to_digit(16)? * 16 + pair[1].to_digit(16)?) as u8)).collect()
}

/// Bytes from the system's source of randomness.
pub fn random(count: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; count];
    ring::rand::SystemRandom::new().fill(&mut bytes).expect("the system gives random bytes");
    bytes
}

/// `secrets.token_urlsafe(count)`.
pub fn token_urlsafe(count: usize) -> String {
    url64(&random(count))
}

/// `secrets.token_hex(count)`.
pub fn token_hex(count: usize) -> String {
    hex(&random(count))
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let digest = ring::digest::digest(&ring::digest::SHA256, data);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_ref());
    out
}

/// SHA-1, which a WebSocket's handshake is answered with and nothing else.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let digest = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, data);
    let mut out = [0u8; 20];
    out.copy_from_slice(digest.as_ref());
    out
}

/// HMAC-SHA256.
pub fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let tag = ring::hmac::sign(&ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key), data);
    let mut out = [0u8; 32];
    out.copy_from_slice(tag.as_ref());
    out
}

/// Whether two byte strings are the same, in a time that does not say where they differ (`hmac.compare_digest`).
pub fn same(one: &[u8], other: &[u8]) -> bool {
    one.len() == other.len() && one.iter().zip(other).fold(0u8, |differ, (a, b)| differ | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_as_python_writes_and_reads_it() {
        for (data, text) in [(&b""[..], ""), (b"f", "Zg=="), (b"fo", "Zm8="), (b"foo", "Zm9v"), (b"foob", "Zm9vYg==")] {
            assert_eq!(base64(data), text);
            assert_eq!(unbase64(text).as_deref(), Some(data));
        }
        assert_eq!(url64(&[0xfb, 0xff]), "-_8");
        for bad in ["Zg", "Zg=", "Z!==", "Zg===", "Zm9v\n", "=Zg="] {
            assert_eq!(unbase64(bad), None, "{bad}");
        }
    }

    #[test]
    fn hashes_and_hmac_are_the_known_ones() {
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        // RFC 4231, test case 2.
        assert_eq!(hex(&hmac(b"Jefe", b"what do ya want for nothing?")),
                   "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        assert_eq!(unhex("a7 0F"), Some(vec![0xa7, 0x0f]));
        assert_eq!(unhex("a7f"), None);
        assert!(same(b"ab", b"ab") && !same(b"ab", b"ac") && !same(b"ab", b"abc"));
        assert_eq!((token_urlsafe(32).len(), token_hex(16).len()), (43, 32));
    }
}
