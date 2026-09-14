# WAI Specification v5.1 — Complete Reference

## Executive Summary

WAI (Compression Layer) v5.1 is an **optional, lossless compression system** for CSTL payloads. It applies **four deterministic transformations in sequence**:

1. **Bit-packing** — Symbols (12 bits) packed 4 per 6 octets
2. **Varints** — Run-length counts and offsets as LEB128
3. **ZigZag** — Signed integers → unsigned (signed delta negatives compress better)
4. **Delta Encoding** — Consecutive values store deltas, not absolutes

**Total typical reduction:** 70% (vs gzip-only 45% on raw CSTL).

---

## Layer Architecture

### Input
- **CSTL Payload:** JSON-LD canonical (NFC-normalized, sorted keys via BTreeMap)
- **Size threshold:** Payloads > 10KB automatically qualify for compression
- **Encoding:** UTF-8 text

### Output
- **Compressed blob:** Magic bytes `0x57 0x41 0x49` (WAI) + metadata + compressed data
- **Backward compatibility:** Non-WAI clients fall back to raw payload (stored separately)

### Canonical Byte Order
```
[Magic: 0x57 0x41 0x49] [1 byte version] [1 byte flags] [SHA256 dict hash: 32 bytes]
[Bit-packed symbols][Varint counts][ZigZag deltas][Delta stream]
```

---

## Transformation Pipeline

### Stage 1: Lexical Analysis → Symbol Indices

**Input:** Canonical CSTL JSON string
**Output:** Array of u16 symbol indices (0x0001–0x0FFF, mapped via WAI_SYMBOLS)

**Algorithm:**
```
for each token T in tokenize(payload):
  if T ∈ WAI_SYMBOLS:
    emit WAI_SYMBOLS[T]          // 0x0001–0x0FFF
  else:
    emit 0x0000 (escape)
    emit utf8_bytes(T)            // Literal token
```

**Escape handling:** Tokens not in dictionary are prefixed with 0x0000 (escape), then their UTF-8 bytes follow as varints (byte count, then bytes). This preserves semantic correctness for arbitrary payloads.

---

### Stage 2: Bit-Packing

**Input:** Array of u16 symbol indices
**Output:** Packed bitstream (12 bits per symbol)

**Specification:**
- Each symbol = 12 bits (range 0x0000–0x0FFF)
- Pack 4 symbols = 48 bits = 6 octets
- Final group padded with zeros if not multiple of 4

**Example:**
```
Symbols:     [0x0042, 0x00FF, 0x0001, 0x0100, 0x0050, ...]
12-bit repr: 000001000010 | 000011111111 | 000000000001 | 000100000000 | 000001010000 | ...
Packed:      Octet 0     Octet 1     Octet 2     (group of 6)
             0x10 0xBE 0x04 0x40 0x14 0x00  (bit-by-bit: 0001_0000_1011_1110_0000_0100_...)
```

**C pseudocode:**
```rust
fn pack_symbols(indices: &[u16]) -> Vec<u8> {
    let mut result = Vec::new();
    for chunk in indices.chunks(4) {
        let mut bits: u64 = 0;
        for (i, &idx) in chunk.iter().enumerate() {
            bits |= (idx as u64) << (48 - i * 12);
        }
        for shift in [40, 32, 24, 16, 8, 0] {
            if shift <= 48 { result.push((bits >> shift) as u8); }
        }
    }
    result
}
```

---

### Stage 3: Varints (LEB128)

**Input:** Metadata (symbol count, escape tokens count, delta sequence length)
**Output:** Variable-length encoded integers (1–5 octets each)

**Specification (LEB128):**
```
0xxxxxxx                          (1 byte: 0–127)
1xxxxxxx 0xxxxxxx                 (2 bytes: 128–16383)
1xxxxxxx 1xxxxxxx 0xxxxxxx        (3 bytes: 16384–2097151)
...
```

**Usage in WAI:**
- Payload size (u32 → 1–5 bytes)
- Escape token count (u16 → 1–3 bytes)
- Delta sequence length (u32 → 1–5 bytes)

**Codec:**
```rust
fn encode_varint(value: u32) -> Vec<u8> {
    let mut result = Vec::new();
    let mut v = value;
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            result.push(byte);
            break;
        } else {
            result.push(byte | 0x80);
        }
    }
    result
}

fn decode_varint(bytes: &[u8]) -> (u32, usize) {
    let mut result = 0u32;
    let mut shift = 0;
    let mut pos = 0;
    loop {
        let byte = bytes[pos];
        pos += 1;
        result |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 { break; }
        shift += 7;
    }
    (result, pos)
}
```

---

### Stage 4: ZigZag Encoding

**Input:** Signed integers (timestamps, hash offsets)
**Output:** Unsigned integers (negative values compress better as unsigned zigzag)

**Specification:**
```
ZigZag(n) = (n << 1) ^ (n >> 31)     // For i32
ZigZag(n) = (n << 1) ^ (n >> 63)     // For i64

Inverse:
UnZigZag(n) = (n >> 1) ^ -(n & 1)
```

**Example:**
```
0     → 0
-1    → 1
1     → 2
-2    → 3
2     → 4
...
```

**Usage:** Timestamp deltas (most payloads have timestamps close to payload creation time, so deltas are small).

---

### Stage 5: Delta Encoding

**Input:** Sorted array of u32 values (hash offsets, timestamps)
**Output:** First value + array of deltas (typically much smaller than originals)

**Specification:**
```
D[0] = V[0]
D[i] = V[i] - V[i-1]  (for i > 0)
```

**Reverse:**
```
V[0] = D[0]
V[i] = V[i-1] + D[i]
```

**Compression:** If values are 32-bit timestamps with typical density (1–5 ms apart), deltas are < 1KB integers → 1–2 bytes per varint. Original array → 4 bytes each → 4x compression.

---

## Wire Format

```
Byte 0–2:      Magic 0x57 0x41 0x49
Byte 3:        Version (0x01 for v5.1)
Byte 4:        Flags
               Bit 0: Delta encoding enabled
               Bit 1: ZigZag encoding enabled
               Bit 2: Escape tokens present
               Bit 3–7: Reserved (0)
Byte 5–36:     SHA-256(WAI dictionary) — Sync Rust/Python
Varint:        Symbol count (u32 LEB128)
Varint:        Escape token count (u16 LEB128)
Varint:        Delta sequence length (u32 LEB128)
[Bit-packed symbols]
[Escape tokens: count + UTF-8 bytes]
[ZigZag deltas (if flag Bit 1)]
[Delta stream (if flag Bit 0)]
```

---

## Dictionary Synchronization

**Dictionary hash = SHA-256(WAI_SYMBOLS sorted by ID)**

**Computation (Rust):**
```rust
let dict_bytes = WAI_SYMBOLS
    .iter()
    .map(|(k, v)| format!("{}:{}", v, k))
    .collect::<Vec<_>>()
    .sort()
    .join("\n")
    .as_bytes();
let hash = Sha256::digest(dict_bytes);
```

**Validation (decoder):**
```
Read dictionary hash from header (bytes 5–36)
Recompute local WAI_VERSION_HASH
If mismatch: reject with DictionaryMismatch error
  (Dictionary was updated on encoder, decoder is stale)
```

---

## Roundtrip Example

### Input CSTL Payload
```json
{
  "produced_by": "alice",
  "parent_hash": "abc123...",
  "action": "transfer",
  "status": "approved",
  "timestamp": 1694865123
}
```

### Step 1: Canonicalize
(JSON-LD, sorted keys, NFC)
```json
{
  "action": "transfer",
  "parent_hash": "abc123...",
  "produced_by": "alice",
  "status": "approved",
  "timestamp": 1694865123
}
```

### Step 2: Tokenize → Symbol Indices
```
"action"         → 0x031A (hypothetical WAI ID)
"transfer"       → 0x0000 (escape) + UTF-8
"parent_hash"    → 0x0202
"abc123..."      → 0x0000 (escape) + UTF-8
"produced_by"    → 0x0201
"alice"          → 0x0101
"status"         → 0x0250
"approved"       → 0x0316
"timestamp"      → 0x0204
1694865123       → 0x0000 (escape) + UTF-8 or varint
```

### Step 3: Bit-Pack
(4 symbols per 6 octets)
```
[0x031A, 0x0000, ..., 0x0101, 0x0250, 0x0316, 0x0204]
→ Packed bitstream (12 bits each)
```

### Step 4: Varints
```
Symbol count: 28 → Varint: 0x1C
Escape tokens: 3 → Varint: 0x03
Delta length: 0  → Varint: 0x00
```

### Step 5: Output
```
[0x57 0x41 0x49][0x01][0x04][SHA256...][0x1C][0x03][0x00][bit-packed][escape bytes]
```

---

## Limits & Constraints

### v5.1 Scope
1. **No adaptive dictionary** — Static 4096-entry table only
2. **No FSE/TANS** — Varints sufficient for typical payload distribution
3. **No streaming decompression** — Full payload must be decoded at once
4. **No key rotation** — Dictionary hash fixed at compile time

### Forward Compatibility
- Version byte (currently 0x01) allows v5.2+ to introduce new flags without breaking v5.1 decoders
- Unknown flags are treated as errors (fail-safe)

---

## Testing Strategy

### Unit Tests
1. **Bit-packing roundtrip:** Random u16 arrays → pack → unpack → original
2. **Varints roundtrip:** 0, 1, 127, 128, 16383, 16384, MAX_U32
3. **ZigZag roundtrip:** Negative/positive range, edge cases
4. **Delta encoding:** Sorted arrays with small deltas

### Integration Tests
1. **Full pipeline:** CSTL payload → compressed → decompressed → canonical match
2. **Size reduction:** Measure on realistic payloads (Couche 5 ADN store samples)
3. **Dictionary hash sync:** Rust → write hash, Python SDK → read & validate

### Edge Cases
- Empty payload
- Payload with no WAI symbols (all escapes)
- Payload with only WAI symbols (no escapes)
- Maximum delta sequence (MAX_U32 values)
- Corrupted magic bytes
- Mismatched dictionary hash

---

## Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| Compression ratio | 70% | vs gzip 45% |
| Encode latency | <5ms | For 50KB payload |
| Decode latency | <3ms | For 50KB compressed |
| Memory peak | 2x payload size | Intermediate buffers |
| Dictionary load | <1ms | Lazy-static init once per process |

---

## References

- WAI_DICTIONARY: src/wai_dictionary.rs (4096 symbols)
- PAYLOAD_COMPRESSION: src/payload_compression.rs (legacy gzip-only)
- WAI_CORE_TRANSFORMS: src/compression/wai_core.rs (NEW — bit-packing, varints, zigzag, delta)
- FSE_ENCODER: src/compression/fse_encoder_rs.rs (NEW — Pre-trained TANS, optional future)

---

End of specification.
