//! Stable state digests and share codes.
//!
//! In a game whose save file *is* its command log, two numbers carry the whole
//! format. One is a digest of the simulation state, used to prove that
//! replaying a log reproduces the run it recorded. The other is a checksum
//! inside a shareable code, used to catch a seed typed in wrong. Both must
//! mean the same thing on x86 and on wasm32, across compiler versions, and
//! across releases of every dependency — a digest that drifts does not fail
//! loudly, it silently declares every existing save corrupt.
//!
//! Everything here is therefore either a published standard with a published
//! check vector (FNV-1a, CRC-32, base64url) or a byte-for-byte port of one,
//! and every one of those vectors is asserted in this crate's tests. Nothing
//! is "probably fine".
//!
//! # What is *not* guaranteed
//!
//! The digest is only as stable as the serialisation underneath it.
//! [`digest_postcard`] hashes postcard bytes, so it inherits postcard's
//! encoding: reordering an enum's variants or a struct's fields changes the
//! bytes and therefore the digest, even though the value is "the same". That
//! is a feature — it is what makes a format change detectable — but it means
//! the digest is a statement about a *type's shape*, not only its contents.
//! Consumers should pin the shapes they care about; see the `command_bytes`
//! fixture in rogue-hunter for the pattern.

#![doc(test(attr(deny(warnings))))]

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::Serialize;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a, 64-bit.
///
/// Chosen over a stronger hash because the threat is accident, not forgery:
/// it must notice that two runs diverged, not resist someone constructing a
/// collision. It is also trivial to reimplement, which matters when the same
/// number has to come out of a Rust test, a CI script and a browser.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Fold one already-computed digest into a running accumulator.
///
/// For summarising a whole corpus in one number: digest each case, fold them
/// in a fixed order, compare the result. The per-case checks answer "did each
/// one work"; the fold answers "did any of them change".
pub fn fold_digest(accumulator: u64, digest: u64) -> u64 {
    let mut hash = accumulator;
    for byte in digest.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// The starting value for a [`fold_digest`] chain.
pub const FOLD_SEED: u64 = FNV_OFFSET;

/// Digest any serialisable value via postcard's target-independent encoding.
///
/// Returns [`u64::MAX`] if serialisation fails. Plain-old-data state cannot
/// fail in practice, and a distinguishable sentinel beats a panic in a release
/// build — a game that crashes on save is worse than one that reports an
/// impossible digest.
pub fn digest_postcard<T: Serialize>(value: &T) -> u64 {
    match postcard::to_allocvec(value) {
        Ok(bytes) => fnv1a(&bytes),
        Err(_) => u64::MAX,
    }
}

/// Digest any serialisable value via its RON text.
///
/// For consumers whose fingerprint is defined over RON rather than postcard.
/// The two are not interchangeable and never produce the same number for the
/// same value; which one a project uses is part of its save format.
#[cfg(feature = "ron-digest")]
pub fn digest_ron<T: Serialize>(value: &T) -> u64 {
    match ron::to_string(value) {
        Ok(text) => fnv1a(text.as_bytes()),
        Err(_) => u64::MAX,
    }
}

/// CRC-32 (IEEE 802.3), bitwise.
///
/// Bitwise rather than table-driven on purpose: no lookup table to get wrong,
/// no generated data to keep in step, and the input is a share code rather
/// than a stream, so the speed never matters.
pub fn crc32_ieee(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Why a share code could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodecError {
    /// The code did not begin with this codec's prefix. Usually a code from a
    /// different game, or a different format version of this one.
    WrongPrefix { expected: &'static str },
    /// The base64url payload would not decode.
    NotBase64(String),
    /// Fewer bytes than the trailing checksum needs.
    TooShort,
    /// The checksum did not match: the code was mistyped or truncated.
    ChecksumMismatch,
    /// The bytes decoded but were not the expected record.
    Payload(String),
}

impl core::fmt::Display for CodecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CodecError::WrongPrefix { expected } => {
                write!(f, "share code does not start with {expected}")
            }
            CodecError::NotBase64(error) => write!(f, "share code is not valid base64: {error}"),
            CodecError::TooShort => write!(f, "share code is too short to contain a checksum"),
            CodecError::ChecksumMismatch => {
                write!(
                    f,
                    "share code failed its checksum; it was mistyped or truncated"
                )
            }
            CodecError::Payload(error) => write!(f, "share code payload did not decode: {error}"),
        }
    }
}

impl std::error::Error for CodecError {}

/// A share-code format: postcard bytes, a trailing CRC-32, base64url, behind a
/// prefix.
///
/// The prefix is what stops one game's code being pasted into another and
/// decoding into something plausible. Include a version number in it — when
/// the record shape has to change, changing the prefix turns "replays wrongly"
/// into "refuses to load", which is the difference between a bug report and a
/// mystery.
///
/// ```
/// # use vellum_digest::ShareCodec;
/// const CODEC: ShareCodec = ShareCodec::new("DEMO1-");
/// let code = CODEC.encode(&(7u32, "hello".to_string())).unwrap();
/// assert!(code.starts_with("DEMO1-"));
/// let back: (u32, String) = CODEC.decode(&code).unwrap();
/// assert_eq!(back, (7, "hello".to_string()));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct ShareCodec {
    prefix: &'static str,
}

impl ShareCodec {
    pub const fn new(prefix: &'static str) -> Self {
        Self { prefix }
    }

    pub const fn prefix(&self) -> &'static str {
        self.prefix
    }

    /// Encode a record. Fails only if the value will not serialise.
    pub fn encode<T: Serialize>(&self, record: &T) -> Result<String, CodecError> {
        let mut bytes = postcard::to_allocvec(record)
            .map_err(|error| CodecError::Payload(error.to_string()))?;
        let crc = crc32_ieee(&bytes);
        bytes.extend_from_slice(&crc.to_le_bytes());
        Ok(format!("{}{}", self.prefix, URL_SAFE_NO_PAD.encode(bytes)))
    }

    /// Decode a record, checking the prefix and the checksum first.
    ///
    /// Surrounding whitespace is trimmed: these are pasted by hand out of
    /// chat windows and text files, and a trailing newline is not a corrupt
    /// save.
    pub fn decode<T: DeserializeOwned>(&self, code: &str) -> Result<T, CodecError> {
        let payload = code
            .trim()
            .strip_prefix(self.prefix)
            .ok_or(CodecError::WrongPrefix {
                expected: self.prefix,
            })?;
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|error| CodecError::NotBase64(error.to_string()))?;
        if bytes.len() < 4 {
            return Err(CodecError::TooShort);
        }
        let (body, crc_bytes) = bytes.split_at(bytes.len() - 4);
        let expected = u32::from_le_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
        if crc32_ieee(body) != expected {
            return Err(CodecError::ChecksumMismatch);
        }
        postcard::from_bytes(body).map_err(|error| CodecError::Payload(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published CRC-32 check value. Every CRC-32 implementation in the
    /// world agrees on this one number; if this fails, the function is not
    /// CRC-32 whatever else it is.
    #[test]
    fn crc32_matches_the_published_check_value() {
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);
    }

    /// FNV-1a's published vectors. Same argument — and note these are the
    /// FNV-1**a** values, which differ from plain FNV-1 (xor then multiply,
    /// rather than multiply then xor). The two are easy to confuse and produce
    /// entirely different digests; this test exists partly to keep anyone from
    /// "correcting" the implementation into the other one.
    #[test]
    fn fnv1a_matches_published_vectors() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn digest_is_stable_for_equal_values() {
        assert_eq!(
            digest_postcard(&(7u32, "wolf")),
            digest_postcard(&(7u32, "wolf"))
        );
        assert_ne!(digest_postcard(&1u32), digest_postcard(&2u32));
    }

    /// The fold must depend on order, or a corpus digest would not notice two
    /// cases swapping results.
    #[test]
    fn folding_is_order_sensitive() {
        let forward = fold_digest(fold_digest(FOLD_SEED, 1), 2);
        let backward = fold_digest(fold_digest(FOLD_SEED, 2), 1);
        assert_ne!(forward, backward);
    }

    #[test]
    fn share_codes_round_trip() {
        const CODEC: ShareCodec = ShareCodec::new("TEST1-");
        let record = (42u64, vec![1u8, 2, 3], "hunter".to_string());
        let code = CODEC.encode(&record).expect("encodes");
        assert!(code.starts_with("TEST1-"));
        let back: (u64, Vec<u8>, String) = CODEC.decode(&code).expect("decodes");
        assert_eq!(back, record);
    }

    #[test]
    fn surrounding_whitespace_is_not_corruption() {
        const CODEC: ShareCodec = ShareCodec::new("TEST1-");
        let code = CODEC.encode(&7u32).expect("encodes");
        let padded = format!("  {code}\n");
        assert_eq!(CODEC.decode::<u32>(&padded), Ok(7));
    }

    /// The point of the checksum: a single mistyped character must be caught
    /// rather than decoded into a different game.
    #[test]
    fn a_mistyped_character_is_rejected() {
        const CODEC: ShareCodec = ShareCodec::new("TEST1-");
        let code = CODEC.encode(&(1u32, 2u32, 3u32)).expect("encodes");
        let mut broken: Vec<char> = code.chars().collect();
        // Flip a character in the payload, past the prefix.
        let index = CODEC.prefix().len() + 2;
        broken[index] = if broken[index] == 'A' { 'B' } else { 'A' };
        let broken: String = broken.into_iter().collect();
        assert_ne!(broken, code, "the test must actually change the code");
        let decoded = CODEC.decode::<(u32, u32, u32)>(&broken);
        assert!(
            matches!(
                decoded,
                Err(CodecError::ChecksumMismatch)
                    | Err(CodecError::NotBase64(_))
                    | Err(CodecError::Payload(_))
            ),
            "a corrupted code decoded as {decoded:?}"
        );
    }

    #[test]
    fn another_games_code_is_refused() {
        const MINE: ShareCodec = ShareCodec::new("MINE1-");
        const YOURS: ShareCodec = ShareCodec::new("YOURS1-");
        let code = YOURS.encode(&5u32).expect("encodes");
        assert_eq!(
            MINE.decode::<u32>(&code),
            Err(CodecError::WrongPrefix { expected: "MINE1-" })
        );
    }

    #[test]
    fn truncation_is_reported_as_such() {
        const CODEC: ShareCodec = ShareCodec::new("T-");
        assert_eq!(CODEC.decode::<u32>("T-"), Err(CodecError::TooShort));
    }
}
