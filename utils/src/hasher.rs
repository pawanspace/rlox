//fnv hash impl basic
pub(crate) fn hash(value: &str) -> u32 {
    let mut hash = 2166136261;
    let chars: Vec<char> = value.chars().collect();
    for i in 0..value.len() {
        hash ^= chars[i] as u32;
        hash = hash.wrapping_mul(16777619);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_calculate_hash() {
        assert_eq!(hash("one"), 3123124719);
    }
}
