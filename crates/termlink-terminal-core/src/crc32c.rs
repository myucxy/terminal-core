pub(super) fn checksum(bytes: &[u8]) -> u32 {
    checksum_parts(&[bytes])
}

pub(super) fn checksum_parts(parts: &[&[u8]]) -> u32 {
    let mut crc = !0_u32;
    for &byte in parts.iter().flat_map(|part| part.iter()) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0x82f6_3b78 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_vector() {
        assert_eq!(super::checksum(b"123456789"), 0xe306_9283);
    }
}
