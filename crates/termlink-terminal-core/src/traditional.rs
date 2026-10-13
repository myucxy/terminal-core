//! Bounded VT52 emulation and explicit legacy byte coding. UTF-8 stays the default.
use crate::{
    EffectContext, Terminal, TerminalError,
    charset::{CharsetSize, GraphicRegister},
};

pub(crate) fn encode_c1(payload: &[u8]) -> Result<Vec<u8>, TerminalError> {
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(payload.len())
        .map_err(|_| TerminalError::LimitExceeded)?;
    let mut offset = 0;
    while offset < payload.len() {
        if payload[offset] == 0x1b
            && payload
                .get(offset + 1)
                .is_some_and(|byte| (0x40..=0x5f).contains(byte))
        {
            encoded.push(payload[offset + 1] + 0x40);
            offset += 2;
        } else {
            encoded.push(payload[offset]);
            offset += 1;
        }
    }
    Ok(encoded)
}

pub(crate) fn decode_c1_reply(payload: &[u8]) -> Option<Vec<u8>> {
    let (&prefix, remaining) = payload.split_first()?;
    if !matches!(prefix, 0x90 | 0x9b | 0x9d | 0x9f) {
        return None;
    }
    let remaining = if prefix == 0x9b {
        remaining
    } else {
        remaining.strip_suffix(&[0x9c])?
    };
    if remaining.iter().any(|byte| *byte < 0x20 || *byte >= 0x7f) {
        return None;
    }
    let mut result = Vec::new();
    result.try_reserve_exact(payload.len() + 2).ok()?;
    result.extend_from_slice(&[0x1b, prefix - 0x40]);
    result.extend_from_slice(remaining);
    if prefix != 0x9b {
        result.extend_from_slice(b"\x1b\\");
    }
    Some(result)
}

impl Terminal {
    pub(super) fn report_user_preference(
        &self,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        let value = self.user_preferred_charset;
        let reply = format!(
            "\x1bP{}!u{}\x1b\\",
            u8::from(value.size == CharsetSize::Characters96),
            value.identifier()
        );
        Self::emit_host_reply_v2(effect, reply.as_bytes())
    }

    pub(super) fn assign_user_preference(
        &mut self,
        sequence: &crate::DcsSequence,
    ) -> Result<(), TerminalError> {
        let size = match sequence.parameters() {
            [] | [0] | [crate::parser::PARAM_MISSING] => CharsetSize::Characters94,
            [1] => CharsetSize::Characters96,
            _ => {
                self.diagnostics.trace.reject_parameter(0);
                return Err(TerminalError::Unsupported("DECAUPSS charset size"));
            }
        };
        let payload = sequence.payload();
        let (&last, prefix) = payload
            .split_last()
            .ok_or(TerminalError::Unsupported("empty DECAUPSS designation"))?;
        let designation = crate::charset::CharsetDesignation::decode_with_soft(
            size,
            prefix,
            last,
            self.graphics.is_some(),
        )
        .map_err(|_| TerminalError::Unsupported("DECAUPSS charset designation"))?;
        if designation.charset == crate::charset::GraphicCharset::UserPreferred {
            return Err(TerminalError::Unsupported("recursive DECAUPSS designation"));
        }
        self.user_preferred_charset = designation;
        Ok(())
    }
    pub(crate) fn set_ansi_mode(&mut self, enabled: bool) {
        self.modes.vt52 = !enabled;
        self.charset.hard_reset();
    }

    pub(super) fn dispatch_vt52(
        &mut self,
        command: u8,
        address: Option<(u8, u8)>,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        self.repeat_source = None;
        match command {
            b'A' => self.move_cursor(-1, 0),
            b'B' => self.move_cursor(1, 0),
            b'C' => self.move_cursor(0, 1),
            b'D' => self.move_cursor(0, -1),
            b'F' | b'G' => self
                .charset
                .designate(
                    GraphicRegister::G0,
                    CharsetSize::Characters94,
                    if command == b'F' { b'0' } else { b'B' },
                )
                .map_err(|_| TerminalError::Unsupported("VT52 graphic designation"))?,
            b'H' => self.home_cursor(),
            b'I' => self.reverse_index_active()?,
            b'J' => self.active_screen_mut().erase_display(0)?,
            b'K' => self.active_screen_mut().erase_line(0)?,
            b'Y' => {
                let (row, column) =
                    address.ok_or(TerminalError::Unsupported("VT52 cursor address"))?;
                if !(0x20..=0x7f).contains(&row) || !(0x20..=0x7f).contains(&column) {
                    return Err(TerminalError::Unsupported("VT52 cursor coordinate byte"));
                }
                let row = self.absolute_row(u16::from(row - 0x20) + 1);
                let column = self.absolute_column(u16::from(column - 0x20) + 1);
                self.active_screen_mut().position_cursor(row, column);
            }
            b'Z' => Self::emit_host_reply_v2(effect, b"\x1b/Z")?,
            b'=' => self.modes.set_keypad_application(true),
            b'>' => self.modes.set_keypad_application(false),
            b'<' => self.set_ansi_mode(true),
            _ => return Err(TerminalError::Unsupported("VT52 escape command")),
        }
        if self.active_screen().has_line_renditions() {
            self.active_screen_mut().finish_line_edit()?;
        }
        self.revalidate_active_wrap_pending();
        Ok(())
    }
}

pub(crate) fn canonical_upss_report(payload: &[u8]) -> bool {
    let Some(body) = payload
        .strip_prefix(b"\x1bP")
        .and_then(|v| v.strip_suffix(b"\x1b\\"))
    else {
        return false;
    };
    let size = match body.get(..3) {
        Some(b"0!u") => CharsetSize::Characters94,
        Some(b"1!u") => CharsetSize::Characters96,
        _ => return false,
    };
    let Some((&last, prefix)) = body[3..].split_last() else {
        return false;
    };
    crate::charset::CharsetDesignation::decode_with_soft(size, prefix, last, true).is_ok_and(
        |designation| {
            designation.charset != crate::charset::GraphicCharset::UserPreferred
                && designation.designator() == &body[3..]
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(graphics: bool) -> Terminal {
        if graphics {
            Terminal::new_with_graphics_profile(
                80,
                24,
                0,
                crate::TerminalOptions::default(),
                crate::OutputProfile::Utf8,
                crate::WidthProfile::Unicode16,
            )
            .unwrap()
        } else {
            Terminal::new(80, 24).unwrap()
        }
    }

    fn output(t: &mut Terminal, bytes: &[u8]) -> Result<(), TerminalError> {
        t.apply_event(crate::Event::Output {
            source_sequence: t.next_source_sequence(),
            bytes,
        })
        .map(|_| ())
    }

    #[test]
    fn soft_upss_preserves_names_and_reports_them_at_every_input_split() {
        for (size, name) in [(0, b"1".as_slice()), (0, b" @"), (1, b" %;")] {
            let command = format!("\x1bP{size}!u{}\x1b\\", std::str::from_utf8(name).unwrap());
            assert!(canonical_upss_report(command.as_bytes()));
            for split in 0..=command.len() {
                let mut t = terminal(true);
                output(&mut t, &command.as_bytes()[..split]).unwrap();
                output(&mut t, &command.as_bytes()[split..]).unwrap();
                assert_eq!(t.user_preferred_charset.designator(), name);
                let pending = t
                    .prepare_event_with_host_effects_v2(crate::Event::Output {
                        source_sequence: t.next_source_sequence(),
                        bytes: b"\x1b[&u",
                    })
                    .unwrap();
                assert_eq!(
                    crate::tests::host_effect_payloads(pending.host_effect_frame_v2()),
                    [command.as_bytes()]
                );
            }
        }
        let mut legacy = terminal(false);
        output(&mut legacy, b"\x1bP0!u1\x1b\\").unwrap();
        assert_eq!(legacy.user_preferred_charset.designator(), b"B");
        assert!(output(&mut legacy, b"\x1bP0!u @\x1b\\").is_err());
    }

    #[test]
    fn upss_rejects_recursion_and_invalid_bounded_designators_atomically() {
        for command in [
            b"\x1bP0!u<\x1b\\".as_slice(),
            b"\x1bP1!u<\x1b\\",
            b"\x1bP0!u   @\x1b\\",
            b"\x1bP0!uA@\x1b\\",
            b"\x1bP0!u\x1b\\",
        ] {
            assert!(!canonical_upss_report(command));
            let mut t = terminal(true);
            let before = t.user_preferred_charset;
            assert!(output(&mut t, command).is_err());
            assert_eq!(t.user_preferred_charset, before);
        }
    }
}
