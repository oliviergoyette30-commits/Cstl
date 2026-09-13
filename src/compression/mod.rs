//! Compression Module
//! ANS (Asymmetric Numeral Systems) encoding/decoding for CSTL payloads

pub mod ans_table;
pub mod ans_encoder;
pub mod ans_decoder;

pub use ans_table::ANSTable;
pub use ans_encoder::ANSEncoder;
pub use ans_decoder::ANSDecoder;

/// Compress a sequence of symbols using ANS
pub fn compress_ans(data: &[u8]) -> Vec<u8> {
    let table = ANSTable::build_cstl_default();
    let mut encoder = ANSEncoder::new(table);
    encoder.encode_sequence(data);
    encoder.finalize()
}

/// Decompress ANS-compressed data
pub fn decompress_ans(data: &[u8], expected_len: usize) -> Result<Vec<u8>, String> {
    let table = ANSTable::build_cstl_default();
    let mut decoder = ANSDecoder::new(table, data)?;
    decoder.decode_sequence(expected_len)
}
