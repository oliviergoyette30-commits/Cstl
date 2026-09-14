//! Couche 5 Payload Compression Module
//! Provides optional gzip compression for ADN store payloads to reduce DB size
//! on long-running systems without breaking retrieval semantics.
//!
//! Strategy:
//!   - Payloads > 10KB are automatically compressed (configurable threshold)
//!   - Compression uses flate2::Compression::default() (gzip format)
//!   - Compressed flag stored in database via `payload_compressed` column
//!   - Decompression is transparent on retrieval

use flate2::Compression;
use std::io::{Read, Write};

/// Compression threshold: payloads larger than this are compressed
pub const COMPRESSION_THRESHOLD: usize = 10_240; // 10 KB

/// Custom error type for compression operations
#[derive(Debug, Clone)]
pub enum CompressionError {
    CompressionFailed(String),
    DecompressionFailed(String),
    InvalidUtf8,
}

impl std::fmt::Display for CompressionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompressionError::CompressionFailed(e) => write!(f, "Compression failed: {}", e),
            CompressionError::DecompressionFailed(e) => write!(f, "Decompression failed: {}", e),
            CompressionError::InvalidUtf8 => write!(f, "Decompressed data is not valid UTF-8"),
        }
    }
}

impl std::error::Error for CompressionError {}

/// Compress a payload string to gzip bytes
///
/// Uses flate2::Compression::default() for balanced speed/ratio.
/// Returns Ok(bytes) on success, Err on compression failure.
pub fn compress_payload(payload: &str) -> Result<Vec<u8>, CompressionError> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(payload.as_bytes())
        .map_err(|e| CompressionError::CompressionFailed(e.to_string()))?;
    encoder
        .finish()
        .map_err(|e| CompressionError::CompressionFailed(e.to_string()))
}

/// Decompress gzip bytes back to a UTF-8 string
///
/// Validates the decompressed content is valid UTF-8.
/// Returns Ok(String) on success, Err on decompression failure or invalid UTF-8.
pub fn decompress_payload(bytes: &[u8]) -> Result<String, CompressionError> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| CompressionError::DecompressionFailed(e.to_string()))?;

    String::from_utf8(decompressed).map_err(|_| CompressionError::InvalidUtf8)
}

/// Determine whether a payload should be compressed based on size threshold
pub fn should_compress(payload: &str) -> bool {
    payload.len() > COMPRESSION_THRESHOLD
}

/// Estimate compression ratio for a payload (0.0..1.0)
/// Returns 1.0 if payload is incompressible (incompressible_size >= original_size)
pub fn estimate_compression_ratio(payload: &str) -> Result<f64, CompressionError> {
    let original_size = payload.len() as f64;
    if original_size == 0.0 {
        return Ok(1.0);
    }

    let compressed = compress_payload(payload)?;
    let compressed_size = compressed.len() as f64;
    Ok(compressed_size / original_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress_roundtrip() {
        let original = "The quick brown fox jumps over the lazy dog. ".repeat(100);
        let compressed = compress_payload(&original).unwrap();
        let decompressed = decompress_payload(&compressed).unwrap();
        assert_eq!(original, decompressed);
    }

    #[test]
    fn test_compress_reduces_size_for_repetitive_data() {
        let original = "AAAA".repeat(1000);
        let compressed = compress_payload(&original).unwrap();
        assert!(
            compressed.len() < original.len(),
            "Compressed size {} should be less than original {}",
            compressed.len(),
            original.len()
        );
    }

    #[test]
    fn test_decompress_invalid_bytes() {
        let invalid = b"this is not gzip data";
        assert!(decompress_payload(invalid).is_err());
    }

    #[test]
    fn test_should_compress_threshold() {
        let small_payload = "small";
        assert!(!should_compress(small_payload));

        let large_payload = "X".repeat(COMPRESSION_THRESHOLD + 1);
        assert!(should_compress(&large_payload));
    }

    #[test]
    fn test_compress_empty_string() {
        let original = "";
        let compressed = compress_payload(original).unwrap();
        let decompressed = decompress_payload(&compressed).unwrap();
        assert_eq!(original, decompressed);
    }

    #[test]
    fn test_compress_utf8_special_chars() {
        let original = "Héllo Wörld! 你好世界 🚀🎉";
        let compressed = compress_payload(original).unwrap();
        let decompressed = decompress_payload(&compressed).unwrap();
        assert_eq!(original, decompressed);
    }

    #[test]
    fn test_compression_ratio_estimate() {
        let repetitive = "AAAA".repeat(1000);
        let ratio = estimate_compression_ratio(&repetitive).unwrap();
        assert!(ratio < 1.0, "Repetitive data should compress");

        let empty = "";
        let ratio = estimate_compression_ratio(empty).unwrap();
        assert_eq!(ratio, 1.0);
    }

    #[test]
    fn test_compress_1mb_payload() {
        // Test with 1MB payload to verify performance and correctness
        let original = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(18_000);
        assert!(original.len() > 1_000_000, "Payload must be > 1MB");

        let compressed = compress_payload(&original).unwrap();
        let ratio = compressed.len() as f64 / original.len() as f64;
        println!(
            "1MB payload compressed to {:.2}% ({}KB → {}KB)",
            ratio * 100.0,
            original.len() / 1024,
            compressed.len() / 1024
        );

        // Verify roundtrip
        let decompressed = decompress_payload(&compressed).unwrap();
        assert_eq!(original, decompressed, "Byte-for-byte match required");
    }
}
