//! Recoverable progress and inert platform-action requests. This module never calls an OS API.
use crate::{EffectContext, TerminalError, parser::CsiSequence};

pub const HOST_SIGNALS_V1_BYTES: usize = 12;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostSignalsV1 {
    /// 0 hidden, 1 normal, 2 error, 3 indeterminate, 4 paused.
    pub progress_state: u8,
    pub progress_percent: u8,
}

impl HostSignalsV1 {
    pub fn encode(self) -> [u8; HOST_SIGNALS_V1_BYTES] {
        let mut bytes = [0; HOST_SIGNALS_V1_BYTES];
        bytes[..8].copy_from_slice(b"TLHS1\0\0\0");
        bytes[8] = self.progress_state;
        bytes[9] = self.progress_percent;
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, TerminalError> {
        if bytes.len() != HOST_SIGNALS_V1_BYTES
            || &bytes[..8] != b"TLHS1\0\0\0"
            || bytes[8] > 4
            || bytes[9] > 100
            || bytes[10..] != [0; 2]
            || (matches!(bytes[8], 0 | 3) && bytes[9] != 0)
        {
            return Err(TerminalError::MalformedSnapshot);
        }
        Ok(Self {
            progress_state: bytes[8],
            progress_percent: bytes[9],
        })
    }

    pub(crate) fn apply_progress(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        let mut parts = bytes.split(|&b| b == b';');
        if parts.next() != Some(b"4") {
            return Err(TerminalError::Unsupported(
                "OSC 9 action; only progress action 4 is supported",
            ));
        }
        fn number(bytes: Option<&[u8]>) -> Result<u32, TerminalError> {
            let mut result = 0u32;
            for &b in bytes.unwrap_or_default() {
                if !b.is_ascii_digit() {
                    return Err(TerminalError::Unsupported(
                        "OSC 9 progress decimal parameter",
                    ));
                }
                result = result
                    .checked_mul(10)
                    .and_then(|n| n.checked_add(u32::from(b - b'0')))
                    .ok_or(TerminalError::Unsupported(
                        "OSC 9 progress parameter overflow",
                    ))?;
            }
            Ok(result)
        }
        let state = number(parts.next())?;
        let percent = number(parts.next())?.min(100);
        if state > 4 || parts.next().is_some() {
            return Err(TerminalError::Unsupported(
                "OSC 9 progress state or parameter count",
            ));
        }
        *self = Self {
            progress_state: state as u8,
            progress_percent: if matches!(state, 0 | 3) {
                0
            } else {
                percent as u8
            },
        };
        Ok(())
    }
}

pub(crate) fn bell(effect: Option<EffectContext<'_>>) -> Result<(), TerminalError> {
    if let Some(effect) = effect {
        effect.capture.push_platform_action(
            effect.source_sequence,
            effect.source_offset_end,
            &[1, 0, 0, 0],
        )?;
    }
    Ok(())
}

pub(crate) fn sound(
    csi: &CsiSequence,
    effect: Option<EffectContext<'_>>,
) -> Result<(), TerminalError> {
    // DECPS: volume 0..7; duration in 1/32 second; up to the parser's 30 notes.
    // Saturation follows VT behavior. Note 0 is silence, 1..25 are C5..C7.
    let count = csi.parameters().len().saturating_sub(2);
    if count > 30 {
        return Err(TerminalError::LimitExceeded);
    }
    if count == 0 {
        return Ok(());
    }
    let mut payload = [0u8; 34];
    payload[0] = 2;
    payload[1] = csi.parameter(0, 0).min(7) as u8;
    payload[2] = csi.parameter(1, 0).min(255) as u8;
    payload[3] = count as u8;
    for index in 0..count {
        payload[4 + index] = csi.parameter(index + 2, 0).min(25) as u8;
    }
    if let Some(effect) = effect {
        effect.capture.push_platform_action(
            effect.source_sequence,
            effect.source_offset_end,
            &payload[..4 + count],
        )?;
    }
    Ok(())
}

impl crate::Terminal {
    pub const fn host_signals(&self) -> HostSignalsV1 {
        self.host_signals
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, Terminal};
    fn u32_at(bytes: &[u8], index: usize) -> u32 {
        u32::from_le_bytes(bytes[index..index + 4].try_into().unwrap())
    }
    fn output(t: &mut Terminal, bytes: &[u8]) {
        t.apply_event(Event::Output {
            source_sequence: t.next_source_sequence(),
            bytes,
        })
        .unwrap();
    }
    #[test]
    fn progress_default_clamp_clear_reset_and_snapshot() {
        let mut t = Terminal::new(10, 3).unwrap();
        for (bytes, state, percent) in [
            ("4;1;35", 1, 35),
            ("4;2;999", 2, 100),
            ("4;3;40", 3, 0),
            ("4;4", 4, 0),
            ("4;;", 0, 0),
        ] {
            output(&mut t, format!("\x1b]9;{bytes}\x1b\\").as_bytes());
            assert_eq!(
                t.host_signals(),
                HostSignalsV1 {
                    progress_state: state,
                    progress_percent: percent
                }
            );
            let snapshot = t.export_snapshot_v2().unwrap();
            if state != 0 {
                assert_eq!(u16::from_le_bytes(snapshot[10..12].try_into().unwrap()), 21);
            }
            assert_eq!(
                Terminal::import_snapshot_v2(&snapshot)
                    .unwrap()
                    .host_signals(),
                t.host_signals()
            );
        }
        output(&mut t, b"\x1b]9;4;1;50\x1b\\\x1b]133;A\x1b\\");
        let snapshot = t.export_snapshot_v2().unwrap();
        let restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
        assert_eq!(restored.shell_integration(), t.shell_integration());
        assert_eq!(restored.host_signals(), t.host_signals());
        output(&mut t, b"\x1bc");
        assert_eq!(t.host_signals(), HostSignalsV1::default());
    }
    #[test]
    fn bell_and_decps_are_typed_effects_and_never_snapshot_replay_actions() {
        let bytes = b"\x07\x1b[7;32;1;0;25,~\x1b]9;4;1;50\x07";
        let mut t = Terminal::new(10, 3).unwrap();
        let event = Event::Output {
            source_sequence: 0,
            bytes,
        };
        let prepared = t.prepare_event_with_host_effects_v2(event).unwrap();
        let frame = prepared.host_effect_frame_v2();
        assert_eq!(u32_at(frame, 16), 2); // OSC terminator BEL is not a second bell.
        assert_eq!(u32_at(frame, 40), 5);
        assert_eq!(u32_at(frame, 64), 3);
        assert_eq!(u32_at(frame, 104), 3);
        assert_eq!(&frame[144..148], &[1, 0, 0, 0]);
        assert_eq!(&frame[148..], &[2, 7, 32, 3, 1, 0, 25]);
        prepared.publish(|_| Ok::<(), ()>(())).unwrap();
        let mut replay = Terminal::new(10, 3).unwrap();
        replay.apply_event(event).unwrap();
        assert_eq!(
            t.export_snapshot_v2().unwrap(),
            replay.export_snapshot_v2().unwrap()
        );
        for split in 0..=bytes.len() {
            let mut candidate = Terminal::new(10, 3).unwrap();
            let mut effects = 0;
            for part in [&bytes[..split], &bytes[split..]] {
                let sequence = candidate.next_source_sequence();
                let prepared = candidate
                    .prepare_event_with_host_effects_v2(Event::Output {
                        source_sequence: sequence,
                        bytes: part,
                    })
                    .unwrap();
                effects += u32_at(prepared.host_effect_frame_v2(), 16);
                prepared.publish(|_| Ok::<(), ()>(())).unwrap();
            }
            assert_eq!(effects, 2);
            assert_eq!(candidate.host_signals(), t.host_signals());
        }
        let mut old = Terminal::new(10, 3).unwrap();
        assert!(matches!(
            old.prepare_event_with_host_effects_v1(event),
            Err(TerminalError::HostEffectVersionRequired)
        ));
        assert_eq!(old.host_signals(), HostSignalsV1::default());
    }
    #[test]
    fn invalid_progress_rolls_back_prior_output_and_retains_diagnostic() {
        for bytes in [
            b"4;5;0".as_slice(),
            b"4;1;z",
            b"4;1;2;3",
            b"other",
            b"4;1;999999999999",
        ] {
            let mut t = Terminal::new(10, 3).unwrap();
            let output = [b"ok\x1b]9;", bytes, b"\x1b\\"].concat();
            assert!(
                t.apply_event(Event::Output {
                    source_sequence: 0,
                    bytes: &output
                })
                .is_err()
            );
            assert_eq!(t.host_signals(), HostSignalsV1::default());
            assert_eq!(t.next_source_sequence(), 0);
        }
        for bytes in [[0u8; 12], [1u8; 12]] {
            assert!(HostSignalsV1::decode(&bytes).is_err());
        }
        let mut bytes = HostSignalsV1 {
            progress_state: 2,
            progress_percent: 50,
        }
        .encode();
        assert!(HostSignalsV1::decode(&bytes[..11]).is_err());
        bytes[10] = 1;
        assert!(HostSignalsV1::decode(&bytes).is_err());
    }

    #[test]
    fn bell_bursts_coalesce_before_allocation_and_sound_saturates_protocol_parameters() {
        let mut t = Terminal::new(10, 3).unwrap();
        let bytes = vec![7u8; 1024 * 1024];
        let prepared = t
            .prepare_event_with_host_effects_v2(Event::Output {
                source_sequence: 0,
                bytes: &bytes,
            })
            .unwrap();
        assert_eq!(u32_at(prepared.host_effect_frame_v2(), 16), 1);
        assert_eq!(prepared.host_effect_frame_v2().len(), 108);
        prepared.publish(|_| Ok::<(), ()>(())).unwrap();
        let prepared = t
            .prepare_event_with_host_effects_v2(Event::Output {
                source_sequence: 1,
                bytes: b"\x1b[99;999;99;0,~",
            })
            .unwrap();
        assert_eq!(
            &prepared.host_effect_frame_v2()[104..],
            &[2, 7, 255, 2, 25, 0]
        );
    }
}
