use std::fmt::Write;

use crate::TerminalError;

const MAX_SEQUENCE_BYTES: usize = 192;
pub const MAX_DIAGNOSTIC_V1_BYTES: usize = 4096;
pub const MAX_DIAGNOSTIC_V3_BYTES: usize = 8192;

#[derive(Clone, Debug, Default)]
pub(crate) struct OutputDiagnostics {
    pub(crate) trace: SequenceTrace,
    pub(crate) physical_trace: Option<SequenceTrace>,
    pub(crate) failure: Option<String>,
    pub(crate) failure_v2: Option<String>,
    pub(crate) expanded_failure_v2: Option<ExpandedFailure>,
}

#[derive(Clone, Debug)]
pub(crate) struct ExpandedFailure {
    pub(crate) text: String,
    pub(crate) origins: Option<String>,
}

// Terminal equality describes deterministic parser/screen state. Diagnostics
// describe rejected operations and physical chunk coordinates, neither of
// which changes that state or its Snapshot/atomic rollback semantics.
impl PartialEq for OutputDiagnostics {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}
impl Eq for OutputDiagnostics {}

/// Runtime-only evidence, excluded from Snapshot and terminal revisions.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SequenceTrace {
    active: ByteTrace,
    escape: Option<ByteTrace>,
    current: Option<(u8, u64, u32)>,
    current_is_macro: bool,
    standalone_c1: bool,
    rejected_parameter: Option<usize>,
}

impl SequenceTrace {
    pub(crate) fn observe(
        &mut self,
        byte: u8,
        sequence: u64,
        offset: u32,
        restart: bool,
        sensitive: bool,
    ) {
        self.observe_origin(byte, sequence, offset, restart, sensitive, false);
    }

    pub(crate) fn observe_macro(
        &mut self,
        byte: u8,
        sequence: u64,
        offset: u32,
        restart: bool,
        sensitive: bool,
    ) {
        self.observe_origin(byte, sequence, offset, restart, sensitive, true);
    }

    fn observe_origin(
        &mut self,
        byte: u8,
        sequence: u64,
        offset: u32,
        restart: bool,
        sensitive: bool,
        is_macro: bool,
    ) {
        if restart {
            self.clear();
        }
        self.current = Some((byte, sequence, offset));
        self.current_is_macro = is_macro;
        self.standalone_c1 = false;
        self.active
            .observe(byte, sequence, offset, sensitive, is_macro);
        if byte == 0x1b {
            self.escape = Some(ByteTrace::default());
        }
        if let Some(escape) = &mut self.escape {
            // Only publish this suffix once the parser confirms a fresh ESC
            // instruction. Until then it may terminate a private string.
            escape.observe(byte, sequence, offset, false, is_macro);
        }
    }

    pub(crate) fn restart_escape(&mut self) {
        if self.standalone_c1 {
            let (byte, sequence, offset) = self.current.expect("observed physical C1 byte");
            let is_macro = self.current_is_macro;
            self.active = ByteTrace::default();
            self.active.observe(byte, sequence, offset, false, is_macro);
            self.escape = None;
        } else if let Some(escape) = self.escape.take() {
            self.active = escape;
        }
    }

    pub(crate) fn mark_standalone_c1(&mut self) {
        self.standalone_c1 = true;
    }

    pub(crate) fn clear(&mut self) {
        self.active.clear();
        self.escape = None;
        self.current = None;
        self.current_is_macro = false;
        self.standalone_c1 = false;
        self.rejected_parameter = None;
    }

    pub(crate) fn reject_parameter(&mut self, index: usize) {
        self.rejected_parameter = Some(index);
    }

    pub(crate) fn diagnostic(&self, error: &TerminalError) -> Option<String> {
        self.active.diagnostic(error)
    }

    pub(crate) fn expanded_diagnostic(
        &self,
        error: &TerminalError,
        profile: crate::OutputProfile,
        width_profile: crate::WidthProfile,
    ) -> Option<ExpandedFailure> {
        if !self.active.has_macro {
            return None;
        }
        let mixed = self.active.origins.len() != 1 || self.active.omitted_origin_bytes != 0;
        let mut trace = self.clone();
        let origins = if mixed {
            // Mixed streams use logical coordinates; physical/macro offsets stay
            // in the bounded origin runs instead of pretending they are contiguous.
            let last = trace.active.observed_bytes - 1;
            let sequence = last / u64::from(u32::MAX);
            let offset = (last % u64::from(u32::MAX)) as u32;
            trace.active.start_sequence = 0;
            trace.active.start_offset = 0;
            trace.active.end_sequence = sequence;
            trace.active.end_offset = offset + 1;
            if let Some(current) = trace.current.as_mut() {
                current.1 = sequence;
                current.2 = offset;
            }
            let runs = self
                .active
                .origins
                .iter()
                .map(|origin| {
                    format!(
                        "{},{},{},{}",
                        if origin.is_macro { "macro" } else { "output" },
                        origin.sequence,
                        origin.offset,
                        origin.length
                    )
                })
                .collect::<Vec<_>>()
                .join(";");
            Some(format!(
                "expanded_origins={runs}\nexpanded_origin_omitted_bytes={}\n",
                self.active.omitted_origin_bytes
            ))
        } else {
            None
        };
        Some(ExpandedFailure {
            text: trace.diagnostic_v2(error, profile, width_profile)?,
            origins,
        })
    }

    pub(crate) fn diagnostic_v2(
        &self,
        error: &TerminalError,
        profile: crate::OutputProfile,
        width_profile: crate::WidthProfile,
    ) -> Option<String> {
        let v1 = self.diagnostic(error)?;
        let (_, fields) = v1.split_once('\n')?;
        let mut text = format!("TL-DIAG/2\n{fields}");
        let (byte, sequence, offset) = self.current?;
        let last_byte = if self.active.hidden_bytes == 0 {
            format!("{byte:02X}")
        } else {
            "redacted".into()
        };
        let codepoint = if self.active.hidden_bytes != 0 {
            "redacted".into()
        } else if self.active.truncated_bytes != 0 {
            "unavailable".into()
        } else if let Ok(visible) = std::str::from_utf8(&self.active.bytes) {
            visible.chars().last().map_or_else(
                || "unavailable".into(),
                |value| format!("U+{:04X}", u32::from(value)),
            )
        } else {
            "invalid_or_incomplete_utf8".into()
        };
        write!(text, "detected_at={sequence}:{offset}\nlast_byte={last_byte}\nlast_codepoint={codepoint}\ncore_revision={}\nsource_dirty={}\noutput_profile={}\nwidth_profile={}\ndirection=output\n",
            env!("TERMLINK_CORE_REVISION"), env!("TERMLINK_CORE_SOURCE_DIRTY"),
            profile as u8, width_profile as u16).expect("writing diagnostic");
        let rejected = self.rejected_parameter.and_then(|index| {
            // CSI parameter bytes are not payload. Preserve the original numeric
            // spelling and report the exact zero-based parser parameter index.
            let header: Vec<_> = self
                .active
                .bytes
                .iter()
                .copied()
                .filter(|b| (*b >= 0x20 && *b != 0x7f) || *b == 0x1b)
                .collect();
            let body = header
                .strip_prefix(b"\x1b[")
                .or_else(|| header.strip_prefix(b"\x9b"))
                .or_else(|| header.strip_prefix(b"\x1bP"))
                .or_else(|| header.strip_prefix(b"\x90"))?;
            let body = if body.first().is_some_and(|b| (b'<'..=b'?').contains(b)) {
                &body[1..]
            } else {
                body
            };
            let count = body
                .iter()
                .take_while(|b| b.is_ascii_digit() || **b == b';' || **b == b':')
                .count();
            let field = body[..count]
                .split(|b| *b == b';' || *b == b':')
                .nth(index)?;
            if count == body.len()
                && index
                    == body[..count]
                        .iter()
                        .filter(|b| **b == b';' || **b == b':')
                        .count()
            {
                return None; // A truncated token is not evidence of its complete spelling.
            }
            Some((
                index,
                if field.is_empty() {
                    "<default>".into()
                } else {
                    String::from_utf8(field.to_vec()).ok()?
                },
            ))
        });
        if let Some((index, token)) = rejected {
            write!(
                text,
                "rejected_parameter_index={index}\nrejected_parameter={token}\n"
            )
            .expect("writing diagnostic");
        } else {
            text.push_str("rejected_parameter_index=unavailable\nrejected_parameter=unavailable\n");
        }
        debug_assert!(text.len() <= MAX_DIAGNOSTIC_V1_BYTES);
        Some(text)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ByteTrace {
    bytes: Vec<u8>,
    start_sequence: u64,
    start_offset: u32,
    end_sequence: u64,
    end_offset: u32,
    observed_bytes: u64,
    hidden_bytes: u64,
    truncated_bytes: u64,
    origins: Vec<OriginRun>,
    omitted_origin_bytes: u64,
    has_macro: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OriginRun {
    is_macro: bool,
    sequence: u64,
    offset: u32,
    length: u32,
}

impl ByteTrace {
    fn observe(&mut self, byte: u8, sequence: u64, offset: u32, sensitive: bool, is_macro: bool) {
        if self.observed_bytes == 0 {
            self.clear();
            self.start_sequence = sequence;
            self.start_offset = offset;
        }
        self.end_sequence = sequence;
        self.end_offset = offset + 1;
        self.observed_bytes = self.observed_bytes.saturating_add(1);
        self.has_macro |= is_macro;
        if self.omitted_origin_bytes != 0 {
            self.omitted_origin_bytes = self.omitted_origin_bytes.saturating_add(1);
        } else if let Some(last) = self.origins.last_mut().filter(|o| {
            o.is_macro == is_macro
                && o.sequence == sequence
                && o.offset.checked_add(o.length) == Some(offset)
        }) {
            last.length += 1;
        } else if self.origins.len() < 16 {
            self.origins.push(OriginRun {
                is_macro,
                sequence,
                offset,
                length: 1,
            });
        } else {
            self.omitted_origin_bytes = 1;
        }
        if sensitive || self.hidden_bytes != 0 {
            self.hidden_bytes = self.hidden_bytes.saturating_add(1);
        } else if self.bytes.len() < MAX_SEQUENCE_BYTES {
            self.bytes.push(byte);
        } else {
            self.truncated_bytes = self.truncated_bytes.saturating_add(1);
        }
    }

    fn clear(&mut self) {
        self.bytes.clear();
        self.start_sequence = 0;
        self.start_offset = 0;
        self.end_sequence = 0;
        self.end_offset = 0;
        self.observed_bytes = 0;
        self.hidden_bytes = 0;
        self.truncated_bytes = 0;
        self.origins.clear();
        self.omitted_origin_bytes = 0;
        self.has_macro = false;
    }

    fn diagnostic(&self, error: &TerminalError) -> Option<String> {
        let payload_reason;
        let reason = match error {
            TerminalError::Unsupported(reason) => *reason,
            TerminalError::LimitExceeded => "terminal resource limit",
            TerminalError::ProtocolResourceLimit(reason) => reason,
            TerminalError::UnsupportedPayload { .. }
            | TerminalError::UnsupportedDecodedPayload { .. } => {
                payload_reason = error.to_string();
                &payload_reason
            }
            _ => return None,
        };
        let category = if matches!(error, TerminalError::ProtocolResourceLimit(_)) {
            "limit_exceeded"
        } else {
            match reason {
                "escape sequence syntax"
                | "CSI syntax"
                | "DCS syntax"
                | "OSC selector syntax"
                | "malformed OSC state"
                | "malformed DEC state report"
                | "malformed SGR colon color"
                | "incomplete extended color" => "malformed",
                "control sequence parameter overflow"
                | "control sequence parameter scratch limit"
                | "too many control sequence intermediates"
                | "OSC selector overflow"
                | "OSC selector scratch limit"
                | "OSC payload limit"
                | "DCS query payload limit"
                | "OSC state limit"
                | "grapheme size limit"
                | "title stack limit"
                | "terminal resource limit" => "limit_exceeded",
                _ => "unsupported",
            }
        };
        let header: Vec<_> = self
            .bytes
            .iter()
            .copied()
            .filter(|byte| *byte == 0x1b || (*byte >= 0x20 && *byte != 0x7f))
            .take(2)
            .collect();
        let family = match header.as_slice() {
            [0x1b, b'[', ..] | [0x9b, ..] => "CSI",
            [0x1b, b']', ..] | [0x9d, ..] => "OSC",
            [0x1b, b'P', ..] | [0x90, ..] => "DCS",
            [0x1b, b'_', ..] | [0x9f, ..] => "APC",
            [0x1b, ..] => "ESC",
            [] if self.bytes.first().is_some_and(|byte| *byte < 0x20) => "C0",
            [0x80..=0x9f, ..] => "C1",
            _ => "TEXT",
        };
        let mut escaped = String::new();
        let mut hex = String::new();
        for (index, byte) in self.bytes.iter().copied().enumerate() {
            if index != 0 {
                hex.push(' ');
            }
            write!(hex, "{byte:02X}").expect("writing a string");
            match byte {
                0x1b => escaped.push_str("<ESC>"),
                b'\\' => escaped.push_str("\\\\"),
                b' '..=b'~' => escaped.push(char::from(byte)),
                _ => write!(escaped, "\\x{byte:02X}").expect("writing a string"),
            }
        }
        // Original string payloads are excluded before formatting.
        let text = format!(
            "TL-DIAG/1\ncategory={category}\nreason={reason}\nfamily={family}\nsequence={escaped}\nhex={hex}\nstart={}:{}\nend={}:{}\nobserved_bytes={}\nhidden_bytes={}\ntruncated_bytes={}\n",
            self.start_sequence,
            self.start_offset,
            self.end_sequence,
            self.end_offset,
            self.observed_bytes,
            self.hidden_bytes,
            self.truncated_bytes,
        );
        debug_assert!(text.len() <= MAX_DIAGNOSTIC_V1_BYTES);
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Event, OutputProfile, Terminal, TerminalError, TerminalOptions};

    #[test]
    fn rejected_parameter_retains_physical_spelling_at_every_split() {
        for (bytes, expected) in [
            (
                b"\x1b[?25;09999h".as_slice(),
                "rejected_parameter_index=1\nrejected_parameter=09999\n",
            ),
            (
                b"\x1b[4;09\x7f999h".as_slice(),
                "rejected_parameter_index=1\nrejected_parameter=09999\n",
            ),
            (
                b"\x1b[4;h".as_slice(),
                "rejected_parameter_index=1\nrejected_parameter=<default>\n",
            ),
            (
                b"\x1b[1;38;2;255;00256;1m",
                "rejected_parameter_index=4\nrejected_parameter=00256\n",
            ),
            (
                b"\x1b[1;58:2::255:00256:1m",
                "rejected_parameter_index=5\nrejected_parameter=00256\n",
            ),
            (
                b"\x1b[48:2:01:2:3:4m",
                "rejected_parameter_index=2\nrejected_parameter=01\n",
            ),
            (
                b"\x1b[38;09;1m",
                "rejected_parameter_index=1\nrejected_parameter=09\n",
            ),
            (
                b"\x1b[38:09:1m",
                "rejected_parameter_index=1\nrejected_parameter=09\n",
            ),
            (
                b"\x1b[4:06m",
                "rejected_parameter_index=1\nrejected_parameter=06\n",
            ),
            (
                b"\x1b[4:1:07m",
                "rejected_parameter_index=2\nrejected_parameter=07\n",
            ),
            (
                b"\x1b[38;2;1;2m",
                "rejected_parameter_index=unavailable\nrejected_parameter=unavailable\n",
            ),
            (
                b"\x1b[38;2;1:2;3m",
                "rejected_parameter_index=3\nrejected_parameter=2\n",
            ),
        ] {
            for split in 0..bytes.len() {
                let mut terminal = Terminal::new(8, 2).unwrap();
                terminal
                    .apply_event(Event::Output {
                        source_sequence: 0,
                        bytes: &bytes[..split],
                    })
                    .unwrap();
                assert!(
                    terminal
                        .apply_event(Event::Output {
                            source_sequence: 1,
                            bytes: &bytes[split..]
                        })
                        .is_err()
                );
                let diagnostic = terminal.failure_diagnostic_v2().unwrap();
                assert!(diagnostic.contains(expected), "{diagnostic}");
                assert_eq!(terminal.next_source_sequence(), 1);
            }
        }
    }

    #[test]
    fn v2_preserves_detection_build_identity_and_first_failed_closed_impact() {
        let mut terminal = Terminal::new(8, 2).unwrap();
        terminal
            .apply_event(Event::Output {
                source_sequence: 0,
                bytes: b"\x1b[?",
            })
            .unwrap();
        assert!(
            terminal
                .apply_event(Event::Output {
                    source_sequence: 1,
                    bytes: b"9999h"
                })
                .is_err()
        );
        let text = terminal.failure_diagnostic_v2().unwrap();
        assert!(text.starts_with("TL-DIAG/2\n"));
        assert!(text.contains("detected_at=1:4\nlast_byte=68\nlast_codepoint=U+0068\n"));
        assert!(text.contains(&format!(
            "core_revision={}\n",
            env!("TERMLINK_CORE_REVISION")
        )));
        assert!(text.contains("output_profile=0\nwidth_profile=1\ndirection=output\n"));
        assert!(text.contains("core_impact=failed_closed\n"));
        assert!(text.contains("rejected_parameter_index=0\nrejected_parameter=9999\n"));
        assert!(
            terminal
                .apply_event(Event::Output {
                    source_sequence: 1,
                    bytes: b"x"
                })
                .is_err()
        );
        assert_eq!(terminal.failure_diagnostic_v2().unwrap(), text);
        assert!(
            terminal
                .failure_diagnostic_v1()
                .unwrap()
                .starts_with("TL-DIAG/1\n")
        );
    }

    #[test]
    fn v2_never_leaks_redacted_payload_detection_byte() {
        let mut terminal = Terminal::new(8, 2).unwrap();
        assert!(
            terminal
                .apply_event(Event::Output {
                    source_sequence: 0,
                    bytes: b"\x1b]10;SECRET\x1b\\"
                })
                .is_err()
        );
        let text = terminal.failure_diagnostic_v2().unwrap();
        assert!(!text.contains("SECRET"));
        assert!(text.contains("last_byte=redacted\nlast_codepoint=redacted\n"));
        assert!(text.len() <= super::MAX_DIAGNOSTIC_V1_BYTES);
    }

    #[test]
    fn incomplete_preview_never_claims_a_complete_parameter() {
        let mut trace = super::SequenceTrace::default();
        let bytes = format!("\x1b[?{}9999h", "0".repeat(300));
        for (offset, byte) in bytes.bytes().enumerate() {
            trace.observe(byte, 0, offset as u32, offset == 0, false);
        }
        trace.reject_parameter(0);
        let text = trace
            .diagnostic_v2(
                &TerminalError::Unsupported("private mode"),
                OutputProfile::Utf8,
                crate::WidthProfile::LegacyV1,
            )
            .unwrap();
        assert!(
            text.contains("rejected_parameter_index=unavailable\nrejected_parameter=unavailable\n")
        );
    }

    #[test]
    fn invalid_non_c1_escape_byte_preserves_escape_and_raw_byte() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        assert!(
            terminal
                .apply_event(Event::Output {
                    source_sequence: 0,
                    bytes: b"\x1b]2;secret\x1b\x80"
                })
                .is_err()
        );
        let text = terminal.failure_diagnostic_v1().unwrap();
        assert!(text.contains("category=malformed\n"));
        assert!(text.contains("sequence=<ESC>\\x80\nhex=1B 80\n"), "{text}");
    }

    #[test]
    fn retryable_failure_diagnostic_does_not_survive_a_different_operation() {
        struct NoReplyBudget;
        impl crate::host_effect::CaptureSink for NoReplyBudget {
            fn push_reply(
                &mut self,
                _: u64,
                _: u32,
                _: &[u8],
                _: bool,
            ) -> Result<(), TerminalError> {
                Err(TerminalError::LimitExceeded)
            }
        }
        let mut terminal = Terminal::new(80, 24).unwrap();
        assert!(
            terminal
                .build_candidate(
                    &[Event::Output {
                        source_sequence: 0,
                        bytes: b"\x1b[5n"
                    }],
                    Some(&mut NoReplyBudget)
                )
                .is_err()
        );
        assert!(
            terminal
                .failure_diagnostic_v1()
                .unwrap()
                .contains("sequence=<ESC>[5n\n")
        );
        assert_eq!(terminal.next_source_sequence(), 0);
        assert!(
            terminal
                .apply_event(Event::Resize {
                    source_sequence: 0,
                    columns: 0,
                    rows: 24
                })
                .is_err()
        );
        assert!(terminal.failure_diagnostic_v1().is_none());
        terminal
            .apply_event(Event::Output {
                source_sequence: 0,
                bytes: b"retry succeeded",
            })
            .unwrap();
        assert!(terminal.failure_diagnostic_v1().is_none());
    }

    #[test]
    fn interrupted_strings_diagnose_the_new_instruction_at_every_split() {
        for prefix in [b"\x1b]2;private".as_slice(), b"\x1b_private", b"\x1bP$qm"] {
            let mut bytes = prefix.to_vec();
            bytes.extend_from_slice(b"\x1b\0[?9999h");
            for split in 0..bytes.len() {
                let mut terminal = Terminal::new(80, 24).unwrap();
                terminal
                    .apply_event(Event::Output {
                        source_sequence: 0,
                        bytes: &bytes[..split],
                    })
                    .unwrap();
                assert!(
                    terminal
                        .apply_event(Event::Output {
                            source_sequence: 1,
                            bytes: &bytes[split..]
                        })
                        .is_err()
                );
                let text = terminal.failure_diagnostic_v1().unwrap();
                assert!(
                    text.contains(
                        "family=CSI\nsequence=<ESC>\\x00[?9999h\nhex=1B 00 5B 3F 39 39 39 39 68\n"
                    ),
                    "{text}"
                );
                let (sequence, offset) = if prefix.len() < split {
                    (0, prefix.len())
                } else {
                    (1, prefix.len() - split)
                };
                assert!(
                    text.contains(&format!(
                        "start={sequence}:{offset}\nend=1:{}\n",
                        bytes.len() - split
                    )),
                    "{text}"
                );
                assert!(!text.contains("70 72 69 76 61 74 65"));
            }
        }
    }

    #[test]
    fn c1_replacing_partial_csi_or_string_reports_physical_new_sequence() {
        for prefix in [b"\x1b[31".as_slice(), b"\x1b]2;secret", b"\x1b_private"] {
            let mut terminal = Terminal::new_with_output_profile(
                80,
                24,
                0,
                TerminalOptions::default(),
                OutputProfile::Utf8WithStandaloneC1,
            )
            .unwrap();
            terminal
                .apply_event(Event::Output {
                    source_sequence: 0,
                    bytes: prefix,
                })
                .unwrap();
            assert!(
                terminal
                    .apply_event(Event::Output {
                        source_sequence: 1,
                        bytes: b"\x9b?9999h"
                    })
                    .is_err()
            );
            let text = terminal.failure_diagnostic_v1().unwrap();
            assert!(
                text.contains(
                    "sequence=\\x9B?9999h\nhex=9B 3F 39 39 39 39 68\nstart=1:0\nend=1:7\n"
                ),
                "{text}"
            );
            assert!(!text.contains("secret"));
        }
    }

    #[test]
    fn interrupted_unsupported_dcs_keeps_the_prior_root_and_redacts_payload() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        assert!(
            terminal
                .apply_event(Event::Output {
                    source_sequence: 0,
                    bytes: b"\x1bPqsecret\x1b[?9999h"
                })
                .is_err()
        );
        let text = terminal.failure_diagnostic_v1().unwrap();
        assert!(
            text.contains("reason=SIXEL\nfamily=DCS\nsequence=<ESC>Pq\n"),
            "{text}"
        );
        assert!(!text.contains("secret"));
        assert!(!text.contains("9999"));
    }

    #[test]
    fn byte_chunks_c1_and_cancelled_prefix_keep_physical_evidence() {
        let bytes = b"ignored\x1b[31\x18\x9b?9999h";
        let mut terminal = Terminal::new_with_output_profile(
            80,
            24,
            0,
            TerminalOptions::default(),
            OutputProfile::Utf8WithStandaloneC1,
        )
        .unwrap();
        for (sequence, byte) in bytes.iter().enumerate() {
            let result = terminal.apply_event(Event::Output {
                source_sequence: sequence as u64,
                bytes: std::slice::from_ref(byte),
            });
            if sequence + 1 == bytes.len() {
                assert!(result.is_err());
            } else {
                result.unwrap();
            }
        }
        let text = terminal.failure_diagnostic_v1().unwrap();
        assert!(text.contains("sequence=\\x9B?9999h\nhex=9B 3F 39 39 39 39 68\n"));
        assert!(text.contains("start=12:0\nend=18:1\n"));
        assert!(!text.contains("ignored"));
    }

    #[test]
    fn utf8_control_and_long_prefix_are_bounded_without_replacement() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        terminal
            .apply_event(Event::Output {
                source_sequence: 0,
                bytes: &[0xc2],
            })
            .unwrap();
        assert!(
            terminal
                .apply_event(Event::Output {
                    source_sequence: 1,
                    bytes: &[0x85]
                })
                .is_err()
        );
        let text = terminal.failure_diagnostic_v1().unwrap();
        assert!(text.contains("sequence=\\xC2\\x85\nhex=C2 85\n"));
        let mut trace = super::SequenceTrace::default();
        for index in 0..1000 {
            trace.observe(b'0', 3, index, index == 0, false);
        }
        let text = trace
            .diagnostic(&TerminalError::Unsupported("test sequence"))
            .unwrap();
        assert!(text.len() <= super::MAX_DIAGNOSTIC_V1_BYTES);
        assert!(text.contains("observed_bytes=1000\nhidden_bytes=0\ntruncated_bytes=808\n"));
    }

    #[test]
    fn failure_retains_exact_split_sequence_and_original_cause() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        terminal
            .apply_event(Event::Output {
                source_sequence: 0,
                bytes: b"ok\x1b[?",
            })
            .unwrap();
        assert!(matches!(
            terminal.apply_event(Event::Output {
                source_sequence: 1,
                bytes: b"9999hafter"
            }),
            Err(TerminalError::Unsupported(_))
        ));
        let diagnostic = terminal.failure_diagnostic_v1().unwrap().to_owned();
        assert!(diagnostic.contains("reason=private mode\n"));
        assert!(diagnostic.contains("sequence=<ESC>[?9999h\nhex=1B 5B 3F 39 39 39 39 68\n"));
        assert!(diagnostic.contains("start=0:2\nend=1:5\n"));
        assert_eq!(terminal.next_source_sequence(), 1);
        assert_eq!(
            terminal.apply_event(Event::Output {
                source_sequence: 1,
                bytes: b"retry"
            }),
            Err(TerminalError::FailedClosed)
        );
        assert_eq!(terminal.failure_diagnostic_v1(), Some(diagnostic.as_str()));
    }

    #[test]
    fn failed_instruction_survives_atomic_rollback() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        let events = [
            Event::Output {
                source_sequence: 0,
                bytes: b"\x1b[31mhello",
            },
            Event::Output {
                source_sequence: 1,
                bytes: b"\x1b[999m",
            },
        ];
        assert!(
            terminal
                .prepare_events_atomic_with_host_effects_v2(&events)
                .is_err()
        );
        assert_eq!(terminal.next_source_sequence(), 0);
        let diagnostic = terminal.failure_diagnostic_v1().unwrap();
        assert!(diagnostic.contains("sequence=<ESC>[999m\n"));
        assert!(diagnostic.contains("start=1:0\nend=1:6\n"));
        assert!(!diagnostic.contains("hello"));
    }

    #[test]
    fn string_payload_and_terminator_are_not_disclosed() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        assert!(
            terminal
                .apply_event(Event::Output {
                    source_sequence: 0,
                    bytes: b"\x1bPqprivate\x1b\\"
                })
                .is_err()
        );
        let diagnostic = terminal.failure_diagnostic_v1().unwrap();
        assert!(diagnostic.contains("sequence=<ESC>Pq\nhex=1B 50 71\n"));
        assert!(diagnostic.contains("hidden_bytes=9\n"));
        assert!(!diagnostic.contains("private"));
        assert!(!diagnostic.contains("70 72"));
    }

    #[test]
    fn malformed_sequence_is_not_an_unimplemented_feature() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        assert!(
            terminal
                .apply_event(Event::Output {
                    source_sequence: 0,
                    bytes: b"\x1b[1?m"
                })
                .is_err()
        );
        let diagnostic = terminal.failure_diagnostic_v1().unwrap();
        assert!(diagnostic.contains("category=malformed\n"));
        assert!(diagnostic.contains("sequence=<ESC>[1?\n"));
    }
}
