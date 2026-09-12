//! WAI Layer 10 — Advanced Compression (All Lossless Tactics)
//! Intégration de Varint, Bit-Packing, MTF, RLE, Delta, Succinct Dictionary

use std::collections::HashMap;

/// Encode un i64 signé en varint zigzag
pub fn varint_zigzag_encode(n: i64) -> Vec<u8> {
    let zigzag = ((n << 1) ^ (n >> 63)) as u64;
    varint_encode(zigzag)
}

/// Encode un u64 en varint 7-bit continuation
pub fn varint_encode(mut n: u64) -> Vec<u8> {
    let mut result = Vec::new();
    while n >= 0x80 {
        result.push((n as u8) | 0x80);
        n >>= 7;
    }
    result.push(n as u8);
    result
}

/// Décode une séquence varint
pub fn varint_decode(bytes: &[u8]) -> (u64, usize) {
    let mut result = 0u64;
    let mut shift = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        result |= ((byte & 0x7F) as u64) << shift;
        if byte < 0x80 {
            return (result, i + 1);
        }
        shift += 7;
    }
    (result, bytes.len())
}

/// Bit-packing pour aligner les données au niveau du bit
pub struct BitPacker {
    bits: Vec<u8>,
    current_byte: u8,
    bit_pos: u8,
}

impl BitPacker {
    pub fn new() -> Self {
        Self {
            bits: Vec::new(),
            current_byte: 0,
            bit_pos: 0,
        }
    }

    pub fn write_bits(&mut self, value: u32, num_bits: u8) {
        let mask = (1u32 << num_bits) - 1;
        let value = value & mask;

        for i in 0..num_bits {
            let bit = (value >> i) & 1;
            if bit == 1 {
                self.current_byte |= 1 << self.bit_pos;
            }
            self.bit_pos += 1;

            if self.bit_pos == 8 {
                self.bits.push(self.current_byte);
                self.current_byte = 0;
                self.bit_pos = 0;
            }
        }
    }

    pub fn finalize(mut self) -> Vec<u8> {
        if self.bit_pos > 0 {
            self.bits.push(self.current_byte);
        }
        self.bits
    }
}

/// Move-To-Front encoding
pub struct MoveToFront {
    symbols: Vec<u8>,
}

impl MoveToFront {
    pub fn new() -> Self {
        Self {
            symbols: (0..=255).collect(),
        }
    }

    pub fn encode(&mut self, symbol: u8) -> u8 {
        let pos = self.symbols.iter().position(|&s| s == symbol).unwrap() as u8;
        self.symbols.remove(pos as usize);
        self.symbols.insert(0, symbol);
        pos
    }

    pub fn decode(&mut self, index: u8) -> u8 {
        let symbol = self.symbols.remove(index as usize);
        self.symbols.insert(0, symbol);
        symbol
    }
}

/// RLE (Run-Length Encoding)
pub fn rle_encode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;

    while i < data.len() {
        let byte = data[i];
        let mut count = 1u32;

        while i + (count as usize) < data.len()
            && data[i + (count as usize)] == byte
            && count < 255
        {
            count += 1;
        }

        if count >= 3 {
            result.push(0xFF);
            result.push(byte);
            result.push(count as u8);
            i += count as usize;
        } else {
            for _ in 0..count {
                result.push(byte);
            }
            i += count as usize;
        }
    }

    result
}

pub fn rle_decode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;

    while i < data.len() {
        if data[i] == 0xFF && i + 2 < data.len() {
            let byte = data[i + 1];
            let count = data[i + 2] as usize;
            for _ in 0..count {
                result.push(byte);
            }
            i += 3;
        } else {
            result.push(data[i]);
            i += 1;
        }
    }

    result
}

/// Delta encoding pour les séquences numériques
pub fn delta_encode(values: &[u32]) -> Vec<u8> {
    let mut result = Vec::new();

    if values.is_empty() {
        return result;
    }

    result.extend_from_slice(&values[0].to_le_bytes());

    for i in 1..values.len() {
        let delta = values[i].wrapping_sub(values[i - 1]);
        result.extend_from_slice(&varint_encode(delta as u64));
    }

    result
}

pub fn delta_decode(data: &[u8]) -> Vec<u32> {
    let mut result = Vec::new();

    if data.len() < 4 {
        return result;
    }

    let mut first = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    result.push(first);

    let mut pos = 4;
    while pos < data.len() {
        let (delta, consumed) = varint_decode(&data[pos..]);
        first = first.wrapping_add(delta as u32);
        result.push(first);
        pos += consumed;
    }

    result
}

/// Succinct Dictionary
pub struct SuccinctDictionary {
    pub symbols: Vec<String>,
    pub index: HashMap<String, u8>,
    pub frequency: Vec<u32>,
}

impl SuccinctDictionary {
    pub fn new(symbols: Vec<String>) -> Self {
        let mut index = HashMap::new();
        let mut frequency = vec![0u32; symbols.len()];

        for (id, symbol) in symbols.iter().enumerate() {
            index.insert(symbol.clone(), id as u8);
            frequency[id] = (symbols.len() - id) as u32;
        }

        Self {
            symbols,
            index,
            frequency,
        }
    }

    pub fn encode_symbol(&self, symbol: &str) -> Option<u8> {
        self.index.get(symbol).copied()
    }

    pub fn decode_symbol(&self, id: u8) -> Option<&str> {
        self.symbols.get(id as usize).map(|s| s.as_str())
    }

    pub fn bits_for_id(&self) -> u8 {
        8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varint_zigzag() {
        let encoded = varint_zigzag_encode(0);
        assert_eq!(encoded.len(), 1);

        let encoded = varint_zigzag_encode(-1);
        assert_eq!(encoded.len(), 1);

        let encoded = varint_zigzag_encode(1000);
        assert!(encoded.len() >= 1);
    }

    #[test]
    fn test_bit_packer() {
        let mut packer = BitPacker::new();
        for i in 0..16 {
            packer.write_bits(i, 4);
        }
        let result = packer.finalize();
        assert_eq!(result.len(), 8);
    }

    #[test]
    fn test_mtf() {
        let data = b"aaaaabbbcc";
        let mut mtf = MoveToFront::new();
        let encoded: Vec<u8> = data.iter().map(|&b| mtf.encode(b)).collect();
        let zero_count = encoded.iter().filter(|&&b| b == 0).count();
        assert!(zero_count >= 4);
    }

    #[test]
    fn test_rle() {
        let data = vec![0xFF; 100];
        let compressed = rle_encode(&data);
        assert!(compressed.len() < 10);
        assert_eq!(rle_decode(&compressed), data);
    }

    #[test]
    fn test_delta_encode() {
        let values = vec![100, 105, 110, 115, 200];
        let compressed = delta_encode(&values);
        let decompressed = delta_decode(&compressed);
        assert_eq!(values, decompressed);
    }

    #[test]
    fn test_succinct_dict() {
        let symbols = vec![
            "agent".to_string(),
            "registry".to_string(),
            "signature".to_string(),
        ];
        let dict = SuccinctDictionary::new(symbols);
        let id = dict.encode_symbol("registry").unwrap();
        assert_eq!(dict.decode_symbol(id).unwrap(), "registry");
    }
}
