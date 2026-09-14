use cstl_parser::compression::{WaiEncoder, WaiDecoder, WaiCoreError};
use cstl_parser::compression::{encode_varint, decode_varint, zigzag_encode, zigzag_decode, delta_encode, delta_decode};

#[test]
fn test_wai_complete_roundtrip() {
    let payload = "produced_by alice parent_hash abc123 action transfer status approved";
    let encoder = WaiEncoder::new();

    let result = encoder.encode(payload);
    assert!(result.is_ok(), "Encoding failed: {:?}", result);

    let encoded = result.unwrap();
    assert!(encoded.len() > 0, "Encoded payload is empty");
    assert_eq!(&encoded[0..3], b"WAI", "Missing WAI magic bytes");
    assert_eq!(encoded[3], 0x01, "Invalid WAI version");

    let decode_result = WaiDecoder::decode(&encoded);
    assert!(decode_result.is_ok(), "Decoding failed: {:?}", decode_result);

    let decoded = decode_result.unwrap();
    assert!(!decoded.is_empty(), "Decoded payload is empty");
}

#[test]
fn test_varint_compression_roundtrip() {
    let test_cases = vec![0u32, 1, 127, 128, 16383, 16384, 2097151, 2097152];

    for value in test_cases {
        let encoded = encode_varint(value);
        assert!(!encoded.is_empty(), "Encoded varint is empty for {}", value);
        assert!(encoded.len() <= 5, "Varint encoding too long for {}", value);

        let (decoded, consumed) = decode_varint(&encoded)
            .expect(&format!("Failed to decode varint for {}", value));
        assert_eq!(decoded, value, "Varint roundtrip failed for {}", value);
        assert_eq!(consumed, encoded.len(), "Varint consumed wrong bytes for {}", value);
    }
}

#[test]
fn test_zigzag_compression() {
    let test_cases = vec![
        (0i32, 0u32),
        (-1, 1),
        (1, 2),
        (-2, 3),
        (2, 4),
        (-64, 127),
        (63, 126),
    ];

    for (input, expected) in test_cases {
        let encoded = zigzag_encode(input);
        assert_eq!(encoded, expected, "ZigZag encoding failed for {}", input);

        let decoded = zigzag_decode(encoded);
        assert_eq!(decoded, input, "ZigZag roundtrip failed for {}", input);
    }
}

#[test]
fn test_delta_compression_realistic() {
    let timestamps = vec![1694865123u32, 1694865124, 1694865125, 1694865128, 1694865130];

    let deltas = delta_encode(&timestamps);
    assert_eq!(deltas.len(), timestamps.len(), "Delta count mismatch");
    assert_eq!(deltas[0], 1694865123, "First value should match");
    assert!(deltas[1] < 10 && deltas[2] < 10, "Small deltas for close timestamps");

    let varints: Vec<usize> = deltas.iter().map(|d| encode_varint(*d).len()).collect();
    let varint_size: usize = varints.iter().sum();

    let raw_size = std::mem::size_of_val(&timestamps[..]);
    assert!(varint_size < raw_size, "Delta+varint should compress better than raw");

    let recovered = delta_decode(&deltas);
    assert_eq!(recovered, timestamps, "Delta roundtrip failed");
}

#[test]
fn test_header_validation() {
    let payload = "test";
    let encoder = WaiEncoder::new();
    let encoded = encoder.encode(payload).unwrap();

    assert_eq!(&encoded[0..3], b"WAI", "Magic bytes mismatch");
    assert_eq!(encoded[3], 0x01, "Version byte mismatch");
    assert_eq!(encoded[4] & 0x07, 0x03, "Flags mismatch (expected delta + zigzag)");

    let dict_hash_bytes = &encoded[5..69];
    let dict_hash_str = std::str::from_utf8(dict_hash_bytes)
        .expect("Dictionary hash should be valid UTF-8");
    assert_eq!(dict_hash_str.len(), 64, "Dictionary hash should be 64 hex chars");
    assert!(dict_hash_str.chars().all(|c| c.is_ascii_hexdigit()),
            "Dictionary hash should be valid hex");
}

#[test]
fn test_decoder_rejects_wrong_magic() {
    let mut bad_data = vec![b'X', b'A', b'I', 0x01];
    bad_data.resize(71, 0);
    let result = WaiDecoder::decode(&bad_data);
    assert!(matches!(result, Err(WaiCoreError::InvalidMagic)), "Should reject wrong magic");
}

#[test]
fn test_decoder_rejects_wrong_version() {
    let mut bad_data = vec![b'W', b'A', b'I', 0xFF];
    bad_data.resize(71, 0);
    let result = WaiDecoder::decode(&bad_data);
    assert!(matches!(result, Err(WaiCoreError::InvalidVersion)), "Should reject wrong version");
}

#[test]
fn test_decoder_rejects_short_data() {
    let short_data = b"WAI";
    let result = WaiDecoder::decode(short_data);
    assert!(result.is_err(), "Should reject short data");
}

#[test]
fn test_compression_achieves_target() {
    let payload = r#"
        {"produced_by": "alice", "parent_hash": "abcdef123456", "action": "transfer",
         "status": "approved", "timestamp": 1694865123, "version": "5.0.0",
         "payload": {"type": "transaction", "amount": 1000}, "metadata": {"created_at": 1694865120}}
    "#;

    let encoder = WaiEncoder::new();
    let encoded = encoder.encode(payload).unwrap();

    let raw_size = payload.len();
    let compressed_size = encoded.len();
    let ratio = (compressed_size as f64) / (raw_size as f64);

    println!("Raw size: {}, Compressed: {}, Ratio: {:.2}%", raw_size, compressed_size, ratio * 100.0);
    assert!(ratio < 0.85, "Compression should achieve < 85% ratio, got {:.2}%", ratio * 100.0);
}

#[test]
fn test_multiple_encodings_deterministic() {
    let payload = "MUST MUST_NOT MAY alice bob produced_by";

    let encoder = WaiEncoder::new();
    let encoded1 = encoder.encode(payload).unwrap();
    let encoded2 = encoder.encode(payload).unwrap();

    assert_eq!(encoded1, encoded2, "Multiple encodings should be identical");
}

#[test]
fn test_escaped_tokens_preserved() {
    let payload = "produced_by alice action unknown_token status approved";
    let encoder = WaiEncoder::new();

    let encoded = encoder.encode(payload).unwrap();
    let decoded = WaiDecoder::decode(&encoded).unwrap();

    assert!(decoded.contains("alice"), "Known symbol should be present");
    assert!(decoded.contains("approved"), "Known symbol should be present");
}
