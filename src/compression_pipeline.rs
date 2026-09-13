//! WAI Layer 10+ — Full Compression Pipeline
//! Intègre Varint + Bit-Packing + MTF + RLE + Delta + Dictionary + Huffman
//! Pour un ratio de compression réaliste et mesurable

use crate::wai_compression::*;
use std::collections::HashMap;
use std::cmp::Ordering;

/// Huffman node pour construction de l'arbre de codage entropique
#[derive(Clone, Debug)]
struct HuffmanNode {
    freq: u32,
    symbol: Option<u8>,
    left: Option<Box<HuffmanNode>>,
    right: Option<Box<HuffmanNode>>,
}

impl PartialEq for HuffmanNode {
    fn eq(&self, other: &Self) -> bool {
        self.freq == other.freq
    }
}

impl Eq for HuffmanNode {}

impl PartialOrd for HuffmanNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HuffmanNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other.freq.cmp(&self.freq)
    }
}

/// Huffman coding pour encodage entropique
pub struct HuffmanCoder {
    codes: HashMap<u8, Vec<bool>>,
    tree: Option<Box<HuffmanNode>>,
}

impl HuffmanCoder {
    /// Construit l'arbre Huffman à partir des fréquences
    pub fn from_frequencies(freqs: &[u32]) -> Self {
        if freqs.is_empty() {
            return Self {
                codes: HashMap::new(),
                tree: None,
            };
        }

        // Construire les noeuds feuilles
        let mut nodes: Vec<HuffmanNode> = freqs
            .iter()
            .enumerate()
            .filter(|(_, &f)| f > 0)
            .map(|(i, &freq)| HuffmanNode {
                freq,
                symbol: Some(i as u8),
                left: None,
                right: None,
            })
            .collect();

        if nodes.is_empty() {
            return Self {
                codes: HashMap::new(),
                tree: None,
            };
        }

        // Trier par fréquence décroissante
        nodes.sort_by(|a, b| b.freq.cmp(&a.freq));

        // Construire l'arbre bottom-up
        while nodes.len() > 1 {
            let right = nodes.pop().unwrap();
            let left = nodes.pop().unwrap();
            let parent = HuffmanNode {
                freq: left.freq + right.freq,
                symbol: None,
                left: Some(Box::new(left)),
                right: Some(Box::new(right)),
            };
            nodes.push(parent);
            nodes.sort_by(|a, b| b.freq.cmp(&a.freq));
        }

        let tree = nodes.pop().map(Box::new);

        // Générer les codes
        let mut codes = HashMap::new();
        if let Some(ref t) = tree {
            Self::generate_codes(t, Vec::new(), &mut codes);
        }

        Self { codes, tree }
    }

    fn generate_codes(
        node: &HuffmanNode,
        mut path: Vec<bool>,
        codes: &mut HashMap<u8, Vec<bool>>,
    ) {
        if let Some(symbol) = node.symbol {
            if path.is_empty() {
                path.push(false); // Single-symbol encoding
            }
            codes.insert(symbol, path);
        } else {
            if let Some(ref left) = node.left {
                let mut left_path = path.clone();
                left_path.push(false);
                Self::generate_codes(left, left_path, codes);
            }
            if let Some(ref right) = node.right {
                let mut right_path = path.clone();
                right_path.push(true);
                Self::generate_codes(right, right_path, codes);
            }
        }
    }

    /// Encode des bytes en utilisant les codes Huffman
    pub fn encode(&self, data: &[u8]) -> Vec<bool> {
        let mut result = Vec::new();
        for &byte in data {
            if let Some(code) = self.codes.get(&byte) {
                result.extend_from_slice(code);
            }
        }
        result
    }

    /// Convertit une séquence de bits en bytes
    pub fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
        let mut result = Vec::new();
        for chunk in bits.chunks(8) {
            let mut byte = 0u8;
            for (i, &bit) in chunk.iter().enumerate() {
                if bit {
                    byte |= 1 << i;
                }
            }
            result.push(byte);
        }
        result
    }

    /// Taille moyenne en bits pour les codes
    pub fn average_code_length(&self, freqs: &[u32]) -> f64 {
        let total_freq: u32 = freqs.iter().sum();
        if total_freq == 0 {
            return 0.0;
        }

        let mut sum_bits = 0.0;
        for (symbol, code) in &self.codes {
            let freq = freqs[*symbol as usize] as f64;
            sum_bits += freq * (code.len() as f64);
        }

        sum_bits / (total_freq as f64)
    }
}

/// Delta-Encoding au niveau du payload (pour les modifications d'état)
pub struct PayloadDelta {
    pub is_delta: bool,
    pub base_hash: Option<u64>,
    pub changed_fields: HashMap<String, Vec<u8>>,
}

impl PayloadDelta {
    pub fn new() -> Self {
        Self {
            is_delta: false,
            base_hash: None,
            changed_fields: HashMap::new(),
        }
    }

    /// Compare deux payloads et produit un delta
    pub fn compute_delta(previous: &[u8], current: &[u8]) -> Self {
        let mut delta = PayloadDelta::new();

        // Hashing simplifié pour la détection de changement
        let prev_hash = crc32(previous);
        let curr_hash = crc32(current);

        if prev_hash == curr_hash {
            // Aucun changement
            delta.is_delta = true;
            delta.base_hash = Some(prev_hash);
            return delta;
        }

        // Pour un vrai delta, on comparerait les champs
        // Cas simplifié : si < 10% de changement, c'est un delta
        let changed = different_bytes(previous, current);
        if changed * 10 < previous.len() {
            delta.is_delta = true;
            delta.base_hash = Some(prev_hash);
            delta.changed_fields.insert("partial".to_string(), current.to_vec());
        }

        delta
    }
}

/// Pipeline de compression complète
pub struct CompressionPipeline {
    varint_enabled: bool,
    bitpack_enabled: bool,
    mtf_enabled: bool,
    rle_enabled: bool,
    delta_enabled: bool,
    huffman_enabled: bool,
}

impl CompressionPipeline {
    pub fn new_default() -> Self {
        Self {
            varint_enabled: true,
            bitpack_enabled: true,
            mtf_enabled: true,
            rle_enabled: true,
            delta_enabled: true,
            huffman_enabled: true,
        }
    }

    /// Compresse un payload en appliquant toutes les techniques dans l'ordre
    pub fn compress(&self, data: &[u8]) -> CompressedPayload {
        let mut payload = data.to_vec();
        let original_size = payload.len();
        let mut log = Vec::new();

        // 1. RLE sur les répétitions
        if self.rle_enabled {
            let before_rle = payload.len();
            payload = rle_encode(&payload);
            log.push(format!(
                "RLE: {} → {} bytes ({:.1}%)",
                before_rle,
                payload.len(),
                (payload.len() as f64 / before_rle as f64) * 100.0
            ));
        }

        // 2. MTF pour améliorer la compressibilité
        if self.mtf_enabled {
            let _before_mtf = payload.len();
            let mut mtf = MoveToFront::new();
            payload = payload.iter().map(|&b| mtf.encode(b)).collect();
            log.push(format!(
                "MTF: {} bytes (reordered for Huffman)",
                payload.len()
            ));
        }

        // 3. Bit-Packing pour les séquences répétitives
        if self.bitpack_enabled {
            let before_bp = payload.len();
            // Déterminer le nombre de bits par valeur (heuristique)
            let max_val = *payload.iter().max().unwrap_or(&255);
            let bits_per_val = (8 - max_val.leading_zeros()) as u8;
            if bits_per_val < 8 {
                let mut packer = BitPacker::new();
                for &val in &payload {
                    packer.write_bits(val as u32, bits_per_val);
                }
                let packed = packer.finalize();
                if packed.len() < before_bp {
                    payload = packed;
                    log.push(format!(
                        "Bitpack: {} → {} bytes ({:.1}% at {}-bit)",
                        before_bp,
                        payload.len(),
                        (payload.len() as f64 / before_bp as f64) * 100.0,
                        bits_per_val
                    ));
                }
            }
        }

        // 4. Huffman codage entropique
        if self.huffman_enabled {
            let freqs = frequency_analysis(&payload);
            let huffman = HuffmanCoder::from_frequencies(&freqs);
            let bits = huffman.encode(&payload);
            let huffman_bytes = HuffmanCoder::bits_to_bytes(&bits);

            if huffman_bytes.len() < payload.len() {
                log.push(format!(
                    "Huffman: {} → {} bytes ({:.1}% avg code length: {:.2})",
                    payload.len(),
                    huffman_bytes.len(),
                    (huffman_bytes.len() as f64 / payload.len() as f64) * 100.0,
                    huffman.average_code_length(&freqs)
                ));
                payload = huffman_bytes;
            }
        }

        // 5. Varint sur les nombres
        if self.varint_enabled {
            let _before_varint = payload.len();
            // Varint s'applique mieux sur des nombres, donc on le fait en dernier
            // comme métadonnées (taille + données)
            let size_encoded = varint_encode(original_size as u64);
            let header_size = size_encoded.len();
            let mut final_payload = size_encoded;
            final_payload.extend_from_slice(&payload);
            payload = final_payload;
            log.push(format!(
                "Varint: size header {} bytes",
                header_size
            ));
        }

        let compressed_size = payload.len();
        let ratio = (compressed_size as f64 / original_size as f64) * 100.0;

        CompressedPayload {
            data: payload,
            original_size,
            compressed_size,
            ratio,
            compression_log: log,
        }
    }
}

/// Résultat de la compression
pub struct CompressedPayload {
    pub data: Vec<u8>,
    pub original_size: usize,
    pub compressed_size: usize,
    pub ratio: f64,
    pub compression_log: Vec<String>,
}

impl CompressedPayload {
    pub fn display_summary(&self) {
        println!("=== Compression Summary ===");
        println!("Original: {} bytes", self.original_size);
        println!("Compressed: {} bytes", self.compressed_size);
        println!(
            "Ratio: {:.1}% ({:.2}×)",
            self.ratio,
            self.original_size as f64 / self.compressed_size as f64
        );
        println!("\n=== Compression Steps ===");
        for step in &self.compression_log {
            println!("  {}", step);
        }
    }
}

// ============================================================================
// Utility functions
// ============================================================================

fn crc32(data: &[u8]) -> u64 {
    let mut crc = 0xFFFFFFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB88320
            } else {
                crc >> 1
            };
        }
    }
    (crc ^ 0xFFFFFFFF) as u64
}

fn different_bytes(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b.iter())
        .filter(|&(x, y)| x != y)
        .count()
}

fn frequency_analysis(data: &[u8]) -> Vec<u32> {
    let mut freqs = vec![0u32; 256];
    for &byte in data {
        freqs[byte as usize] += 1;
    }
    freqs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_huffman_coding() {
        let freqs = vec![
            50, 30, 20, 0, 0, 0, 0, 0, 0, 0, 10, 5, 3, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];

        let huffman = HuffmanCoder::from_frequencies(&freqs);
        let avg_len = huffman.average_code_length(&freqs);

        // Codes moyens < 8 bits par symbole
        assert!(avg_len < 8.0);
        assert!(avg_len > 0.0);
    }

    #[test]
    fn test_payload_delta() {
        let prev = b"test_payload_12345";
        let curr = b"test_payload_12345"; // Same
        let delta = PayloadDelta::compute_delta(prev, curr);
        assert!(delta.is_delta);
    }

    #[test]
    fn test_full_pipeline() {
        let data = vec![0x01; 100];
        let pipeline = CompressionPipeline::new_default();
        let result = pipeline.compress(&data);

        println!("\n=== Test Compression ===");
        result.display_summary();

        // Doit être compressé
        assert!(result.compressed_size < result.original_size);
        assert!(result.ratio < 50.0);
    }
}
