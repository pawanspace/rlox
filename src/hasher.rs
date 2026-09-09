//! FNV-1a string hashing.
//!
//! WHAT: turns a `&str` into a 32-bit number used to pick a bucket in the
//! hash table (`hash_map.rs`) and to cache alongside interned strings.
//!
//! WHY FNV-1a: it is a *non-cryptographic* hash — extremely cheap (one XOR and
//! one multiply per byte, no allocation, no rounds) and gives a decent spread
//! of bits for short keys like identifiers and string literals. A VM hashes
//! strings constantly (every variable lookup), so speed matters far more than
//! resistance to adversaries; a crypto hash (SHA, etc.) would be pointlessly
//! slow here. This mirrors clox, which uses FNV-1a for exactly this reason.
//!
//! The two magic constants are the standard 32-bit FNV parameters:
//!   - 2166136261  = the FNV *offset basis* (the starting value)
//!   - 16777619    = the FNV *prime* (the multiplier)
//! FNV-*1a* means: for each byte, XOR first, then multiply (FNV-1 does the
//! reverse order; the "a" variant mixes bits slightly better).

/// Compute the 32-bit FNV-1a hash of `value`.
pub(crate) fn hash(value: &str) -> u32 {
    let mut hash = 2166136261; // FNV offset basis (u32 inferred from the constant)
    let chars: Vec<char> = value.chars().collect();
    // NOTE: this loops over `value.len()` (the number of *bytes* in the UTF-8
    // string) but indexes `chars[i]` (a vector of Unicode *chars*). For pure
    // ASCII the byte count equals the char count so it works, but for any
    // multi-byte character byte_len > char_count, so `chars[i]` will panic with
    // an out-of-bounds index. A byte-oriented hash would iterate `value.bytes()`.
    for i in 0..value.len() {
        hash ^= chars[i] as u32; // FNV-1a: XOR the byte/char value into the hash first
        hash = hash.wrapping_mul(16777619); // then multiply by the FNV prime.
                                            // `wrapping_mul` lets the u32 overflow and wrap
                                            // around instead of panicking — overflow is the
                                            // intended, defined behaviour for a hash.
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    // Demonstrates the hash is deterministic and pins the exact FNV-1a output
    // for a known ASCII input, guarding against accidental changes to the
    // constants or the XOR/multiply order.
    #[test]
    fn can_calculate_hash() {
        assert_eq!(hash("one"), 3123124719);
    }
}
