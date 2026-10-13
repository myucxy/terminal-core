//! OSC 52 write requests only. No clipboard read or platform API is available to the core.
use crate::{EffectContext, TerminalError};

pub(crate) const MAX_TEXT_BYTES: usize = 16 * 1024;
pub(crate) const MAX_OSC_BYTES: usize = MAX_TEXT_BYTES.div_ceil(3) * 4 + 2;

fn malformed(offset: usize, reason: &'static str, byte: Option<u8>) -> TerminalError {
    TerminalError::UnsupportedPayload {
        offset,
        reason,
        byte,
    }
}

pub(crate) fn write(bytes: &[u8], effect: Option<EffectContext<'_>>) -> Result<(), TerminalError> {
    let separator = bytes.iter().position(|&byte| byte == b';').ok_or_else(|| {
        malformed(
            bytes.len(),
            "OSC 52 selection/data separator is missing",
            None,
        )
    })?;
    if !matches!(&bytes[..separator], b"" | b"c") {
        return Err(malformed(
            0,
            "OSC 52 selection; only the system clipboard c is supported",
            bytes.first().copied(),
        ));
    }
    let encoded = &bytes[separator + 1..];
    if encoded == b"?" {
        return Err(TerminalError::Unsupported(
            "OSC 52 clipboard reads are not permitted",
        ));
    }
    let text = decode(encoded, separator + 1)?;
    if let Err(error) = std::str::from_utf8(&text) {
        let offset = separator + 1 + error.valid_up_to() / 3 * 4;
        return Err(malformed(
            offset,
            "OSC 52 decoded text is not UTF-8 (position identifies its Base64 quartet)",
            bytes.get(offset).copied(),
        ));
    }
    if let Some(index) = text.iter().position(|&byte| byte == 0) {
        let offset = separator + 1 + index / 3 * 4;
        return Err(malformed(
            offset,
            "OSC 52 decoded text contains U+0000 (position identifies its Base64 quartet)",
            bytes.get(offset).copied(),
        ));
    }
    if let Some(effect) = effect {
        let mut payload = Vec::new();
        payload
            .try_reserve_exact(8 + text.len())
            .map_err(|_| TerminalError::LimitExceeded)?;
        payload.extend_from_slice(&[3, 0, 0, 0]);
        payload.extend_from_slice(&(text.len() as u32).to_le_bytes());
        payload.extend_from_slice(&text);
        effect.capture.push_platform_action(
            effect.source_sequence,
            effect.source_offset_end,
            &payload,
        )?;
    }
    Ok(())
}

fn decode(encoded: &[u8], source_offset: usize) -> Result<Vec<u8>, TerminalError> {
    if encoded.len() > MAX_TEXT_BYTES.div_ceil(3) * 4 {
        return Err(TerminalError::ProtocolResourceLimit(
            "OSC 52 decoded text exceeds 16384 bytes",
        ));
    }
    let length = crate::base64::decoded_length(encoded, source_offset)?;
    if length > MAX_TEXT_BYTES {
        return Err(TerminalError::ProtocolResourceLimit(
            "OSC 52 decoded text exceeds 16384 bytes",
        ));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(length)
        .map_err(|_| TerminalError::LimitExceeded)?;
    output.resize(length, 0);
    crate::base64::decode_into(encoded, source_offset, &mut output)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, Terminal};

    #[test]
    fn strict_base64_and_text_bounds() {
        assert_eq!(decode(b"", 0).unwrap(), b"");
        assert_eq!(decode(b"Zg==", 0).unwrap(), b"f");
        assert_eq!(decode(b"Zm8=", 0).unwrap(), b"fo");
        assert_eq!(decode(b"Zm9v", 0).unwrap(), b"foo");
        for invalid in [
            b"Zg=".as_slice(),
            b"Zh==",
            b"Zm9=",
            b"====",
            b"Zg==AAAA",
            b"Z g=",
            b"-g==",
        ] {
            assert!(decode(invalid, 0).is_err(), "{invalid:?}");
        }
        for invalid in [b"c;?".as_slice(), b"p;Zg==", b"c;/w==", b"c;AA=="] {
            assert!(write(invalid, None).is_err());
        }
        let mut maximum = b"YWFh".repeat(MAX_TEXT_BYTES / 3);
        maximum.extend_from_slice(b"YQ==");
        assert_eq!(decode(&maximum, 0).unwrap().len(), MAX_TEXT_BYTES);
        maximum.truncate(maximum.len() - 4);
        maximum.extend_from_slice(b"YWE=");
        assert!(decode(&maximum, 0).is_err());
    }

    #[test]
    fn clipboard_is_an_inert_effect_at_every_split_and_never_legacy_output() {
        let stream = b"\x1b]52;c;5Lit5paHCg==\x1b\\";
        for split in 0..=stream.len() {
            let mut terminal = Terminal::new(20, 4).unwrap();
            let mut effects = Vec::new();
            for (sequence, bytes) in [&stream[..split], &stream[split..]].into_iter().enumerate() {
                let mut prepared = terminal
                    .prepare_event_with_legacy_projection(Event::Output {
                        source_sequence: sequence as u64,
                        bytes,
                    })
                    .unwrap();
                assert!(prepared.take_legacy_projection().is_empty());
                prepared
                    .commit_with(|frame| {
                        if u32::from_le_bytes(frame[16..20].try_into().unwrap()) != 0 {
                            effects.push(frame);
                        }
                        Ok::<_, ()>(())
                    })
                    .unwrap();
            }
            assert_eq!(effects.len(), 1);
            let payload = &effects[0][104..];
            assert_eq!(&payload[..8], &[3, 0, 0, 0, 7, 0, 0, 0]);
            assert_eq!(&payload[8..], "中文\n".as_bytes());
            let checkpoint = terminal.export_snapshot_v2().unwrap();
            assert_eq!(
                Terminal::import_snapshot_v2(&checkpoint)
                    .unwrap()
                    .export_snapshot_v2()
                    .unwrap(),
                checkpoint
            );
        }
    }

    #[test]
    fn clipboard_burst_and_v1_capture_fail_atomically() {
        let mut terminal = Terminal::new(20, 4).unwrap();
        let mut repeated = b"\x1b]52;c;eA==\x07".repeat(5);
        assert!(
            terminal
                .prepare_event_with_host_effects_v2(Event::Output {
                    source_sequence: 0,
                    bytes: &repeated
                })
                .is_err()
        );
        repeated.clear();
        repeated.extend_from_slice(b"\x1b]52;;\x07");
        let mut terminal = Terminal::new(20, 4).unwrap();
        assert!(matches!(
            terminal.prepare_event_with_host_effects_v1(Event::Output {
                source_sequence: 0,
                bytes: &repeated
            }),
            Err(TerminalError::HostEffectVersionRequired)
        ));
    }
}
