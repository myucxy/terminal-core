//! Strict RFC4648 standard alphabet for terminal payloads; no URL alphabet or ignored whitespace.
use crate::TerminalError;

fn invalid(offset: usize, reason: &'static str, byte: Option<u8>) -> TerminalError {
    TerminalError::UnsupportedPayload {
        offset,
        reason,
        byte,
    }
}
pub(crate) fn decoded_length(encoded: &[u8], source_offset: usize) -> Result<usize, TerminalError> {
    if !encoded.len().is_multiple_of(4) {
        return Err(invalid(
            source_offset + encoded.len(),
            "Base64 requires complete padded quartets",
            None,
        ));
    }
    let padding = encoded
        .iter()
        .rev()
        .take(2)
        .take_while(|&&b| b == b'=')
        .count();
    Ok(encoded.len() / 4 * 3 - padding)
}
/// Caller supplies a bounded, uncommitted destination. Failure may leave scratch partially decoded.
pub(crate) fn decode_into(
    encoded: &[u8],
    source_offset: usize,
    output: &mut [u8],
) -> Result<(), TerminalError> {
    if output.len() != decoded_length(encoded, source_offset)? {
        return Err(TerminalError::LimitExceeded);
    }
    let mut written = 0;
    for (index, quartet) in encoded.chunks_exact(4).enumerate() {
        let offset = source_offset + index * 4;
        let digit = |position: usize| -> Result<u8, TerminalError> {
            match quartet[position] {
                b'A'..=b'Z' => Ok(quartet[position] - b'A'),
                b'a'..=b'z' => Ok(quartet[position] - b'a' + 26),
                b'0'..=b'9' => Ok(quartet[position] - b'0' + 52),
                b'+' => Ok(62),
                b'/' => Ok(63),
                _ => Err(invalid(
                    offset + position,
                    "invalid Base64 digit or padding",
                    Some(quartet[position]),
                )),
            }
        };
        let a = digit(0)?;
        let b = digit(1)?;
        let last = (index + 1) * 4 == encoded.len();
        let (c, d, count) = if quartet[2] == b'=' {
            if !last || quartet[3] != b'=' || b & 15 != 0 {
                return Err(invalid(
                    offset + 2,
                    "noncanonical Base64 padding",
                    Some(quartet[2]),
                ));
            }
            (0, 0, 1)
        } else if quartet[3] == b'=' {
            let c = digit(2)?;
            if !last || c & 3 != 0 {
                return Err(invalid(
                    offset + 3,
                    "noncanonical Base64 padding",
                    Some(quartet[3]),
                ));
            }
            (c, 0, 2)
        } else {
            (digit(2)?, digit(3)?, 3)
        };
        let bytes = [(a << 2) | (b >> 4), (b << 4) | (c >> 2), (c << 6) | d];
        output[written..written + count].copy_from_slice(&bytes[..count]);
        written += count;
    }
    Ok(())
}
