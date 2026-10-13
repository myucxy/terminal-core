//! RFC1950/RFC1951 decoder into an already budgeted exact-size destination.
use crate::TerminalError;

const MAX_BLOCKS: usize = 65_536;
const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

struct Bits<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl Bits<'_> {
    fn error(&self, reason: &'static str) -> TerminalError {
        TerminalError::UnsupportedPayload {
            offset: 2 + self.position / 8,
            reason,
            byte: self.bytes.get(self.position / 8).copied(),
        }
    }
    fn read(&mut self, count: u8) -> Result<u32, TerminalError> {
        if self.position + usize::from(count) > self.bytes.len() * 8 {
            return Err(self.error("truncated zlib deflate bitstream"));
        }
        let mut value = 0;
        for index in 0..count {
            value |= u32::from((self.bytes[self.position / 8] >> (self.position % 8)) & 1) << index;
            self.position += 1;
        }
        Ok(value)
    }
}

struct Huffman {
    counts: [u16; 16],
    symbols: [u16; 288],
}
impl Huffman {
    fn new(lengths: &[u8], allow_empty: bool, allow_single: bool) -> Result<Self, &'static str> {
        if lengths.len() > 288 || lengths.iter().any(|&value| value > 15) {
            return Err("invalid deflate Huffman code lengths");
        }
        let mut result = Self {
            counts: [0; 16],
            symbols: [0; 288],
        };
        for &length in lengths {
            result.counts[usize::from(length)] += 1;
        }
        let nonempty = lengths.len() - usize::from(result.counts[0]);
        if nonempty == 0 {
            return if allow_empty {
                Ok(result)
            } else {
                Err("empty deflate Huffman tree")
            };
        }
        let mut remaining = 1i32;
        for count in &result.counts[1..] {
            remaining = remaining * 2 - i32::from(*count);
            if remaining < 0 {
                return Err("oversubscribed deflate Huffman tree");
            }
        }
        if remaining != 0 && !(allow_single && nonempty == 1 && result.counts[1] == 1) {
            return Err("incomplete deflate Huffman tree");
        }
        let mut offsets = [0usize; 16];
        for length in 1..15 {
            offsets[length + 1] = offsets[length] + usize::from(result.counts[length]);
        }
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                let offset = &mut offsets[usize::from(length)];
                result.symbols[*offset] = symbol as u16;
                *offset += 1;
            }
        }
        Ok(result)
    }
    fn symbol(&self, bits: &mut Bits<'_>) -> Result<usize, TerminalError> {
        let (mut code, mut first, mut index) = (0usize, 0usize, 0usize);
        for &count in &self.counts[1..] {
            code |= bits.read(1)? as usize;
            if code >= first && code - first < usize::from(count) {
                return Ok(usize::from(self.symbols[index + code - first]));
            }
            index += usize::from(count);
            first = (first + usize::from(count)) << 1;
            code <<= 1;
        }
        Err(bits.error("invalid deflate Huffman symbol"))
    }
}

/// Mutates only the caller's uncommitted scratch buffer. No allocation or unbounded output growth.
pub fn inflate_exact(input: &[u8], output: &mut [u8]) -> Result<(), TerminalError> {
    let invalid = |offset, reason| TerminalError::UnsupportedPayload {
        offset,
        reason,
        byte: input.get(offset).copied(),
    };
    if input.len() < 6
        || input[0] & 15 != 8
        || input[0] >> 4 > 7
        || !u16::from_be_bytes([input[0], input[1]]).is_multiple_of(31)
    {
        return Err(invalid(0, "invalid zlib compression header"));
    }
    if input[1] & 32 != 0 {
        return Err(invalid(
            1,
            "zlib preset dictionaries are not permitted for terminal images",
        ));
    }
    let mut bits = Bits {
        bytes: &input[2..input.len() - 4],
        position: 0,
    };
    let mut offset = 0;
    let window = 1usize << ((input[0] >> 4) + 8);
    for block in 0..MAX_BLOCKS {
        let final_block = bits.read(1)? != 0;
        match bits.read(2)? {
            0 => {
                bits.position = bits.position.next_multiple_of(8);
                let length = bits.read(16)? as usize;
                let complement = bits.read(16)? as u16;
                if length as u16 != !complement {
                    return Err(bits.error("deflate stored block LEN/NLEN mismatch"));
                }
                if length > output.len() - offset {
                    return Err(bits.error("deflate output exceeds declared image size"));
                }
                let start = bits.position / 8;
                if length > bits.bytes.len() - start {
                    return Err(bits.error("truncated deflate stored block"));
                }
                output[offset..offset + length].copy_from_slice(&bits.bytes[start..start + length]);
                offset += length;
                bits.position += length * 8;
            }
            kind @ (1 | 2) => {
                let (literal, distance) = if kind == 1 {
                    let mut lengths = [8; 288];
                    lengths[144..256].fill(9);
                    lengths[256..280].fill(7);
                    (
                        Huffman::new(&lengths, false, false).expect("fixed tree"),
                        Huffman::new(&[5; 32], false, false).expect("fixed distances"),
                    )
                } else {
                    dynamic(&mut bits)?
                };
                loop {
                    match literal.symbol(&mut bits)? {
                        value @ 0..=255 => {
                            if offset == output.len() {
                                return Err(
                                    bits.error("deflate output exceeds declared image size")
                                );
                            }
                            output[offset] = value as u8;
                            offset += 1;
                        }
                        256 => break,
                        value @ 257..=285 => {
                            let index = value - 257;
                            let length =
                                LENGTH_BASE[index] + bits.read(LENGTH_EXTRA[index])? as usize;
                            let symbol = distance.symbol(&mut bits)?;
                            if symbol >= 30 {
                                return Err(bits.error("reserved deflate distance symbol"));
                            }
                            let distance =
                                DISTANCE_BASE[symbol] + bits.read(DISTANCE_EXTRA[symbol])? as usize;
                            if distance > offset || distance > window {
                                return Err(bits.error(
                                    "deflate back-reference exceeds decoded data or window",
                                ));
                            }
                            if length > output.len() - offset {
                                return Err(
                                    bits.error("deflate output exceeds declared image size")
                                );
                            }
                            for _ in 0..length {
                                output[offset] = output[offset - distance];
                                offset += 1;
                            }
                        }
                        _ => return Err(bits.error("reserved deflate length symbol")),
                    }
                }
            }
            _ => return Err(bits.error("reserved deflate block type")),
        }
        if final_block {
            break;
        }
        if block + 1 == MAX_BLOCKS {
            return Err(TerminalError::ProtocolResourceLimit(
                "deflate block count exceeds 65536",
            ));
        }
    }
    if offset != output.len() || bits.position.div_ceil(8) != bits.bytes.len() {
        return Err(bits.error("deflate size mismatch or trailing data"));
    }
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in output.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    if (b << 16) | a
        != u32::from_be_bytes(input[input.len() - 4..].try_into().expect("zlib checksum"))
    {
        return Err(invalid(input.len() - 4, "zlib Adler32 checksum mismatch"));
    }
    Ok(())
}

fn dynamic(bits: &mut Bits<'_>) -> Result<(Huffman, Huffman), TerminalError> {
    let literals = bits.read(5)? as usize + 257;
    let distances = bits.read(5)? as usize + 1;
    let codes = bits.read(4)? as usize + 4;
    if literals > 286 {
        return Err(bits.error("invalid deflate literal tree size"));
    }
    let order = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut code_lengths = [0; 19];
    for &index in &order[..codes] {
        code_lengths[index] = bits.read(3)? as u8;
    }
    let tree = Huffman::new(&code_lengths, false, false).map_err(|reason| bits.error(reason))?;
    let mut lengths = [0u8; 318];
    let mut offset = 0;
    let total = literals + distances;
    while offset < total {
        let (value, repeat) = match tree.symbol(bits)? {
            value @ 0..=15 => (value as u8, 1),
            16 => {
                if offset == 0 {
                    return Err(bits.error("deflate code repeat has no previous length"));
                }
                (lengths[offset - 1], bits.read(2)? as usize + 3)
            }
            17 => (0, bits.read(3)? as usize + 3),
            18 => (0, bits.read(7)? as usize + 11),
            _ => return Err(bits.error("invalid deflate code-length symbol")),
        };
        if repeat > total - offset {
            return Err(bits.error("deflate code-length repeat overflows tree"));
        }
        lengths[offset..offset + repeat].fill(value);
        offset += repeat;
    }
    if lengths[256] == 0 {
        return Err(bits.error("deflate tree has no end-of-block marker"));
    }
    Ok((
        Huffman::new(&lengths[..literals], false, true).map_err(|reason| bits.error(reason))?,
        Huffman::new(&lengths[literals..total], true, true).map_err(|reason| bits.error(reason))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hex(input: &str) -> Vec<u8> {
        assert_eq!(input.len() % 2, 0);
        input
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
    fn fixtures() -> Vec<(Vec<u8>, Vec<u8>)> {
        vec![
            (
                hex("7801011300ecff626f756e646564207a6c69622073746f7265644a0f0764"),
                b"bounded zlib stored".to_vec(),
            ),
            (
                hex(
                    "78014b4c4a3634324e1c2547c9614132303231b3b0b2b173707271f3f0f2f10b080a098b888a894b484a49cbc8cac92b282a29aba8aaa96b686a69ebe8eae91b00f598989a995b585a59dbd8dad93b383a39bbb8bab97b787a79fbf8faf9070406058784868547444645c7c4c6c52700ad49494d4bcfc8cccacec9cdcb2f282c2a2e292d2bafa8acaaaea9adab6f686c6a6e696d6befe8eceaeee9edeb9f3071d2e42953a74d9f3173d6ec3973e7cd5fb070d1e2254b972d5fb172d5ea356bd7addfb071d3e62d5bb76ddfb173d7ee3d7bf7ed3f70f0d0e123478f1d3f71f2d4e93367cf9dbf70f1d2e52b57af5dbf71f3d6ed3b77efdd7ff0f0d1e3274f9f3d7ff1f2d5eb376fdfbdfff0f1d3e72f5fbf7dfff1f3d7ef3f7ffffd070066a04fb0",
                ),
                [b"abc123".repeat(120), (0..=255).collect()].concat(),
            ),
            (
                hex(
                    "789cedcf675f0c0000c0e19491448452a44b143242562915c926a321e42475dad729a3a5a22823295bd1b01315b2b38ab247a1a8b4cda4d2f639fcfcdf3eef1e8983d8d545e4e62c70140b3d9c44f65e02098220088220088220088220088220088220088220088220088220088220088220088220088220088220088220088220088220088220088220088220088220ff8848759096e9d8a97317d9ae72dde4bbf750e8d94bb1779fbe4acafd5454fb0f501ba82ed018a439788896f6d061c375468c1c355a77ccd8717ae3274c9c3459df608aa1d1546313d369d3cd6698cf9c357bcedc79f3172cb458b478c9524b2b6b9b65b6cb57acb45b255c6dbfc661ada393689db38bab9bbb87a7d84bb2dedb67c3c64d9b7dfdfc0302b70405876cdd161ab67d4778c4ce5dbbf744ee8dda171db3ffc0c143878f1c3d161b77fc447c4262d2c953a7cf9c3d773ef942cac54ba969e997af5ccdb876fdc6cd5bb7ef64debd77ffc1c3acec478f73729f3c7df6fcc5cb57afdfbccdcb7ff7fe4341e1c74f45c5259f4bcbca2b2aabaabf7cfdf6fdc7cf9a5fb5bfebea1bfe343635b7b4b6b5f3e7cf9f3f7ffeffe3ff2fa189a3ed",
                ),
                [
                    b"termlink graphics ".repeat(1200),
                    (0..=255).collect::<Vec<u8>>().repeat(8),
                ]
                .concat(),
            ),
        ]
    }
    #[test]
    fn python_zlib_stored_fixed_dynamic_and_overlapping_backreferences() {
        for (encoded, expected) in fixtures() {
            let mut decoded = vec![0; expected.len()];
            inflate_exact(&encoded, &mut decoded).unwrap();
            assert_eq!(decoded, expected);
            assert!(inflate_exact(&encoded, &mut vec![0; expected.len() - 1]).is_err());
            assert!(inflate_exact(&encoded, &mut vec![0; expected.len() + 1]).is_err());
        }
    }
    #[test]
    fn truncation_checksum_bad_trees_and_trailing_data_fail_without_panics() {
        for (bytes, expected) in fixtures() {
            for end in 0..bytes.len() {
                assert!(inflate_exact(&bytes[..end], &mut vec![0; expected.len()]).is_err());
            }
            for offset in 0..bytes.len() {
                let mut bad = bytes.clone();
                bad[offset] ^= 0x80;
                let _ = inflate_exact(&bad, &mut vec![0; expected.len()]);
            }
            let mut trailing = bytes.clone();
            trailing.insert(trailing.len() - 4, 0);
            assert!(inflate_exact(&trailing, &mut vec![0; expected.len()]).is_err());
            let mut bad = bytes;
            let last = bad.len() - 1;
            bad[last] ^= 1;
            assert!(inflate_exact(&bad, &mut vec![0; expected.len()]).is_err());
        }
        assert!(Huffman::new(&[1, 1, 1], false, false).is_err());
        assert!(Huffman::new(&[2, 2], false, true).is_err());
        assert!(Huffman::new(&[1], false, true).is_ok());
    }
}
