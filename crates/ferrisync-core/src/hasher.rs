//! BLAKE3 streaming hash helpers.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::error::{Error, Result};

pub type Digest = [u8; 32];

/// Hash all bytes from a reader, streaming in chunks.
pub fn hash_reader<R: Read>(mut reader: R) -> Result<Digest> {
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(*hasher.finalize().as_bytes())
}

/// Hash a file on disk.
pub fn hash_file(path: impl AsRef<Path>) -> Result<Digest> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|e| Error::io(path, e))?;
    let reader = BufReader::new(file);
    hash_reader(reader)
}

/// Encode a digest as lowercase hex.
pub fn hex_encode(digest: &Digest) -> String {
    hex::encode(digest)
}

/// Decode a lowercase hex digest.
pub fn hex_decode(s: &str) -> Result<Digest> {
    let bytes = hex::decode(s).map_err(|e| Error::Other(format!("invalid hex digest: {e}")))?;
    if bytes.len() != 32 {
        return Err(Error::Other(format!(
            "digest must be 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn empty_input_hashes_correctly() {
        let digest = hash_reader(Cursor::new(b"")).unwrap();
        let expected = blake3::hash(b"");
        assert_eq!(digest, *expected.as_bytes());
    }

    #[test]
    fn known_input_matches_blake3() {
        let input = b"ferrisync-test-vector";
        let digest = hash_reader(Cursor::new(input)).unwrap();
        assert_eq!(digest, *blake3::hash(input).as_bytes());
    }

    #[test]
    fn hashing_is_deterministic() {
        let input = b"repeat-me";
        let a = hash_reader(Cursor::new(input)).unwrap();
        let b = hash_reader(Cursor::new(input)).unwrap();
        assert_eq!(a, b);
        assert_eq!(hex_encode(&a), hex_encode(&b));
    }

    #[test]
    fn hash_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.bin");
        std::fs::write(&path, b"hello-nas").unwrap();
        let digest = hash_file(&path).unwrap();
        assert_eq!(digest, *blake3::hash(b"hello-nas").as_bytes());
    }
}
