use sha2::{Digest, Sha256};

/// Compute the SHA-256 hex digest of the given bytes.
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex_encode(&result)
}

/// A streaming SHA-256 hasher for large files (avoids loading the whole file into memory).
pub struct StreamingHash {
    hasher: Sha256,
}

impl StreamingHash {
    pub fn new() -> Self {
        Self {
            hasher: Sha256::new(),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    pub fn finalize(self) -> String {
        let result = self.hasher.finalize();
        hex_encode(&result)
    }
}

impl Default for StreamingHash {
    fn default() -> Self {
        Self::new()
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input() {
        // SHA-256 of empty input.
        assert_eq!(
            hash_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn known_vector() {
        // SHA-256("abc")
        assert_eq!(
            hash_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn deterministic() {
        assert_eq!(hash_bytes(b"hello"), hash_bytes(b"hello"));
        assert_ne!(hash_bytes(b"hello"), hash_bytes(b"world"));
    }

    #[test]
    fn hex_is_lowercase_and_even_length() {
        let h = hash_bytes(b"some prompt");
        assert_eq!(h.len() % 2, 0);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn streaming_matches_one_shot() {
        let mut s = StreamingHash::new();
        s.update(b"foo");
        s.update(b"bar");
        let streamed = s.finalize();
        assert_eq!(streamed, hash_bytes(b"foobar"));
    }

    #[test]
    fn streaming_empty() {
        let s = StreamingHash::new();
        assert_eq!(
            s.finalize(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
