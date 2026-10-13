//! A display-only stream from the authoritative parser's completed actions.
//! Never reparse untrusted bytes in a second host interpreter to find OS actions.
use crate::TerminalError;
use crate::parser::{Action, PARAM_MISSING, ParameterSeparator, Sequence};

pub const MAX_LEGACY_PROJECTION_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Projection {
    pub(crate) bytes: Vec<u8>,
}

impl Projection {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        if bytes.len() > MAX_LEGACY_PROJECTION_BYTES - self.bytes.len() {
            return Err(TerminalError::ProtocolResourceLimit(
                "legacy display projection exceeds 8388608 bytes",
            ));
        }
        self.bytes
            .try_reserve(bytes.len())
            .map_err(|_| TerminalError::LimitExceeded)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn header(
        &mut self,
        prefix: Option<u8>,
        parameters: &[u16],
        separators: &[ParameterSeparator],
        intermediates: &[u8],
        final_byte: u8,
    ) -> Result<(), TerminalError> {
        if let Some(prefix) = prefix {
            self.push(&[prefix])?;
        }
        for (index, &parameter) in parameters.iter().enumerate() {
            if index != 0 {
                self.push(&[
                    if separators.get(index - 1) == Some(&ParameterSeparator::Colon) {
                        b':'
                    } else {
                        b';'
                    },
                ])?;
            }
            if parameter != PARAM_MISSING {
                self.push(parameter.to_string().as_bytes())?;
            }
        }
        self.push(intermediates)?;
        self.push(&[final_byte])
    }

    pub(crate) fn observe(&mut self, action: &Action) -> Result<(), TerminalError> {
        match action {
            Action::None | Action::Execute(7) => Ok(()),
            Action::Print(byte) | Action::Execute(byte) => self.push(&[*byte]),
            Action::Vt52 { command, address } => {
                self.push(&[0x1b, *command])?;
                if let Some((row, column)) = address {
                    self.push(&[*row, *column])?;
                }
                Ok(())
            }
            Action::Sequence(sequence) => match sequence {
                // Graphic bytes belong to the native renderer, never an outer host or its file broker.
                Sequence::Apc(_) => Ok(()),
                Sequence::Esc(sequence) => {
                    self.push(b"\x1b")?;
                    self.push(sequence.intermediates())?;
                    self.push(&[sequence.final_byte()])
                }
                // Definitions stay in the authority; invocations emit their expanded actions here.
                Sequence::Csi(sequence)
                    if (sequence.intermediates() == b"*" && sequence.final_byte() == b'z')
                        || (sequence.intermediates() == b"," && sequence.final_byte() == b'~')
                        || (matches!(sequence.intermediates(), b"$" | b"*")
                            && sequence.final_byte() == b'|') =>
                {
                    Ok(())
                }
                Sequence::Csi(sequence) => {
                    self.push(b"\x1b[")?;
                    self.header(
                        sequence.private_prefix(),
                        sequence.parameters(),
                        sequence.parameter_separators(),
                        sequence.intermediates(),
                        sequence.final_byte(),
                    )
                }
                Sequence::Osc(sequence) if matches!(sequence.selector(), 9 | 52) => Ok(()),
                Sequence::Osc(sequence) => {
                    self.push(b"\x1b]")?;
                    self.push(sequence.selector().to_string().as_bytes())?;
                    self.push(b";")?;
                    self.push(sequence.payload())?;
                    self.push(b"\x1b\\")
                }
                Sequence::Dcs(sequence)
                    if (sequence.intermediates() == b"!" && sequence.final_byte() == b'z')
                        || (sequence.intermediates().is_empty()
                            && matches!(sequence.final_byte(), b'q' | b'{')) =>
                {
                    Ok(())
                }
                Sequence::Dcs(sequence) => {
                    self.push(b"\x1bP")?;
                    self.header(
                        sequence.private_prefix(),
                        sequence.parameters(),
                        &[],
                        sequence.intermediates(),
                        sequence.final_byte(),
                    )?;
                    self.push(sequence.payload())?;
                    self.push(b"\x1b\\")
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Event, Terminal};

    fn apply(terminal: &mut Terminal, sequence: u64, bytes: &[u8]) -> Vec<u8> {
        let mut prepared = terminal
            .prepare_event_with_legacy_projection(Event::Output {
                source_sequence: sequence,
                bytes,
            })
            .unwrap();
        let output = prepared.take_legacy_projection();
        prepared.commit_with(|_| Ok::<_, ()>(())).unwrap();
        output
    }

    #[test]
    fn split_actions_remove_side_effects_but_preserve_text_sgr_and_title() {
        let bytes =
            "中😀\x07\x1b[1;31mred\x1b[0m\x1b]2;title\x07\x1b]9;4;1;30\x1b\\\x1b[7;1;1,~done"
                .as_bytes();
        let expected = "中😀\x1b[1;31mred\x1b[0m\x1b]2;title\x1b\\done".as_bytes();
        for split in 0..=bytes.len() {
            let mut terminal = Terminal::new(80, 24).unwrap();
            let mut projected = apply(&mut terminal, 0, &bytes[..split]);
            projected.extend(apply(&mut terminal, 1, &bytes[split..]));
            assert_eq!(projected, expected, "split {split}");
            assert!(terminal.legacy_projection.is_none());
        }
    }

    #[test]
    fn macros_expand_once_and_cannot_hide_bells_or_sound() {
        let body = b"A\x07\x1b[7;1;1,~B";
        let hex = body.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let stream = format!("\x1bP1;0;1!z{hex}\x1b\\\x1b[1*zC");
        let mut terminal = Terminal::new(80, 24).unwrap();
        let mut projected = Vec::new();
        for (seq, byte) in stream.bytes().enumerate() {
            projected.extend(apply(&mut terminal, seq as u64, &[byte]));
        }
        assert_eq!(projected, b"ABC");
    }

    #[test]
    fn incomplete_macro_control_is_completed_by_the_same_parser() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        assert_eq!(
            apply(&mut terminal, 0, b"\x1bP1;0;1!z1b5b\x1b\\\x1b[1*z"),
            b""
        );
        assert_eq!(apply(&mut terminal, 1, b"7;1;1,~safe"), b"safe");
    }

    #[test]
    fn canonical_projection_handles_c1_cancelled_strings_and_colon_sgr() {
        let mut terminal = Terminal::new_with_output_profile(
            80,
            24,
            0,
            crate::TerminalOptions::default(),
            crate::OutputProfile::Utf8WithStandaloneC1,
        )
        .unwrap();
        assert_eq!(
            apply(
                &mut terminal,
                0,
                b"\x9d9;4;3\x9c\x9b7;1;1,~\x1b]2;discard\x18\x1b[38:2::1:2:3mX"
            ),
            b"\x1b[38:2::1:2:3mX"
        );
    }

    #[test]
    fn publication_failure_never_commits_a_projection_or_state() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        let before = terminal.clone();
        let mut prepared = terminal
            .prepare_event_with_legacy_projection(Event::Output {
                source_sequence: 0,
                bytes: b"hello\x07",
            })
            .unwrap();
        assert_eq!(prepared.take_legacy_projection(), b"hello");
        assert!(prepared.commit_with(|_| Err::<(), _>(())).is_err());
        assert_eq!(terminal, before);
    }

    #[test]
    fn discarded_action_cannot_join_two_invalid_utf8_fragments() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        assert_eq!(apply(&mut terminal, 0, b"\xc2\x07\xa3"), b"\xc2\x1b\\\xa3");
    }

    #[test]
    fn removed_actions_do_not_leave_repeat_source_or_change_visible_cells() {
        let stream = b"x\x07\x1b[3b\x1b[31mA\x1b[2b\x1b[3b\x1bP1;0;1!z1b5b3362\x1b\\\x1b[1*zZ\x1b[96$|\x1b[25*|";
        let mut authority = Terminal::new(80, 24).unwrap();
        let mut display = Terminal::new(80, 24).unwrap();
        for (sequence, &byte) in stream.iter().enumerate() {
            let projected = apply(&mut authority, sequence as u64, &[byte]);
            display
                .apply_event(Event::Output {
                    source_sequence: sequence as u64,
                    bytes: &projected,
                })
                .unwrap();
        }
        assert_eq!(authority.main, display.main);
        assert_eq!(authority.alternate, display.alternate);
    }
}
