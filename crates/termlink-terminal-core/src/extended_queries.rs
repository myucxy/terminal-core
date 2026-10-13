use crate::{Color, CsiSequence, EffectContext, Terminal, TerminalError};

impl Terminal {
    pub(super) fn dispatch_terminal_parameters(
        &self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        if csi.parameters().len() > 1 {
            return Ok(());
        }
        // Compatibility report for a virtual terminal, not measured network baud.
        match csi.parameter(0, 0) {
            0 => Self::emit_host_reply_v2(effect, b"\x1b[2;1;1;128;128;1;0x"),
            1 => Self::emit_host_reply_v2(effect, b"\x1b[3;1;1;128;128;1;0x"),
            _ => Ok(()),
        }
    }

    pub(super) fn dispatch_rectangle_checksum(
        &self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        if csi.parameters().len() > 6 {
            return Ok(());
        }
        let id = csi.parameter(0, 0);
        let page = csi.parameter(1, 0);
        let mut checksum = 0_u16;
        if let Some((top, left, bottom, right)) = self.rectangle_coordinates(csi, 2) {
            let last = if self.active_alternate {
                1
            } else {
                crate::pages::MAX_PAGES
            };
            let requested = page.clamp(1, u16::from(last)) as u8;
            let (first, last) = if page == 0 {
                (1, last)
            } else {
                (requested, requested)
            };
            for page in first..=last {
                if !self.active_alternate
                    && page != 1
                    && self.pages.extra[usize::from(page - 2)].is_none()
                {
                    // Unallocated pages are blank; a read-only checksum must not allocate them.
                    let blank_sum = (bottom - top) * (right - left) * (32 + (7 << 4));
                    checksum = checksum.wrapping_sub(blank_sum as u16);
                    continue;
                }
                let screen = self.page_screen(page);
                for row in top..bottom {
                    for column in left..right {
                        let cell = &screen.cells[screen.visible_cell_index(row, column)];
                        if !cell.continuation {
                            for scalar in
                                std::iter::once(cell.character).chain(cell.combining.chars())
                            {
                                let mut utf16 = [0; 2];
                                for unit in scalar.encode_utf16(&mut utf16) {
                                    checksum = checksum.wrapping_sub(if *unit == 0x2426 {
                                        0x1b
                                    } else {
                                        *unit
                                    });
                                }
                            }
                        }
                        let attr = cell.attributes;
                        let bits = u16::from(cell.protected) * 4
                            + u16::from(attr.conceal) * 8
                            + u16::from(attr.is_underlined()) * 16
                            + u16::from(attr.inverse) * 32
                            + u16::from(attr.blink) * 64
                            + u16::from(attr.bold) * 128;
                        let color = |color, default| match color {
                            Color::Indexed(index) if index < 16 => u16::from(index),
                            _ => default,
                        };
                        checksum = checksum.wrapping_sub(
                            bits + (color(attr.foreground, 7) << 4) + color(attr.background, 0),
                        );
                    }
                }
            }
        }
        Self::emit_host_reply_v2(
            effect,
            format!("\x1bP{id}!~{checksum:04X}\x1b\\").as_bytes(),
        )
    }
}

pub(crate) fn canonical_extended_reply(payload: &[u8]) -> bool {
    if let Some(body) = payload
        .strip_prefix(b"\x1b[?")
        .and_then(|body| body.strip_suffix(b"R"))
    {
        let values = body
            .split(|byte| *byte == b';')
            .map(canonical_number)
            .collect::<Option<Vec<_>>>();
        return values.is_some_and(|v| {
            v.len() == 3
                && (1..=512).contains(&v[0])
                && (1..=512).contains(&v[1])
                && (1..=u32::from(crate::pages::MAX_PAGES)).contains(&v[2])
        });
    }
    if let Some(body) = payload
        .strip_prefix(b"\x1b[")
        .and_then(|body| body.strip_suffix(b"*{"))
    {
        return canonical_number(body)
            .is_some_and(|value| value <= (crate::macros::MAX_BYTES / 16) as u32);
    }
    if matches!(
        payload,
        b"\x1bP!|00000000\x1b\\"
            | b"\x1b[2;1;1;128;128;1;0x"
            | b"\x1b[3;1;1;128;128;1;0x"
            | b"\x1b[?13n"
            | b"\x1b[?23n"
            | b"\x1b[?27;0;0;5n"
            | b"\x1b[?53n"
            | b"\x1b[?57;0n"
            | b"\x1b[?70n"
            | b"\x1b[?83n"
    ) {
        return true;
    }
    if let Some(body) = payload
        .strip_prefix(b"\x1b[")
        .and_then(|body| body.strip_suffix(b"\"w"))
    {
        let values = body
            .split(|byte| *byte == b';')
            .map(canonical_number)
            .collect::<Option<Vec<_>>>();
        return values.is_some_and(|v| {
            v.len() == 5
                && (1..=512).contains(&v[0])
                && (1..=512).contains(&v[1])
                && v[2] == 1
                && v[3] == 1
                && (1..=u32::from(crate::pages::MAX_PAGES)).contains(&v[4])
        });
    }
    if let Some(body) = payload
        .strip_prefix(b"\x1bP")
        .and_then(|body| body.strip_suffix(b"\x1b\\"))
        && let Some(separator) = body.windows(2).position(|pair| pair == b"!~")
    {
        let hex = &body[separator + 2..];
        return canonical_number(&body[..separator]).is_some_and(|value| value <= 65534)
            && hex.len() == 4
            && hex
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(byte));
    }
    false
}

fn canonical_number(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || (bytes.len() > 1 && bytes[0] == b'0') {
        return None;
    }
    bytes.iter().try_fold(0_u32, |value, &byte| {
        if !byte.is_ascii_digit() {
            return None;
        }
        value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, tests::host_effect_payloads};

    fn replies(terminal: &mut Terminal, sequence: u64, bytes: &[u8]) -> Vec<Vec<u8>> {
        let prepared = terminal
            .prepare_event_with_host_effects_v2(Event::Output {
                source_sequence: sequence,
                bytes,
            })
            .unwrap();
        let result = host_effect_payloads(prepared.host_effect_frame_v2())
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect();
        prepared.publish(|_| Ok::<(), ()>(())).unwrap();
        result
    }

    #[test]
    fn identification_and_device_reports_are_exact_at_every_split() {
        let fixtures = [
            (b"\x1b[=c".as_slice(), b"\x1bP!|00000000\x1b\\".as_slice()),
            (b"\x1b[=0c", b"\x1bP!|00000000\x1b\\"),
            (b"\x1b[x", b"\x1b[2;1;1;128;128;1;0x"),
            (b"\x1b[1x", b"\x1b[3;1;1;128;128;1;0x"),
            (b"\x1b[?15n", b"\x1b[?13n"),
            (b"\x1b[?25n", b"\x1b[?23n"),
            (b"\x1b[?26n", b"\x1b[?27;0;0;5n"),
            (b"\x1b[?55n", b"\x1b[?53n"),
            (b"\x1b[?56n", b"\x1b[?57;0n"),
            (b"\x1b[?75n", b"\x1b[?70n"),
            (b"\x1b[?85n", b"\x1b[?83n"),
            (b"\x1b[\"v", b"\x1b[24;80;1;1;1\"w"),
        ];
        for (query, expected) in fixtures {
            for split in 0..=query.len() {
                let mut terminal = Terminal::new(80, 24).unwrap();
                let mut result = replies(&mut terminal, 0, &query[..split]);
                result.extend(replies(&mut terminal, 1, &query[split..]));
                assert_eq!(result, [expected], "query {query:?} split {split}");
                assert_eq!(terminal.cursor(), (0, 0));
                assert!(canonical_extended_reply(expected));
            }
        }
    }

    #[test]
    fn optional_unknown_query_selectors_do_not_poison_following_text() {
        for query in [
            b"\x1b[999n".as_slice(),
            b"\x1b[?999n",
            b"\x1b[999x",
            b"\x1b[999c",
            b"\x1b[>999c",
            b"\x1b[=999c",
            b"\x1b[>999q",
        ] {
            let mut terminal = Terminal::new(80, 24).unwrap();
            assert!(replies(&mut terminal, 0, query).is_empty());
            replies(&mut terminal, 1, b"ALIVE");
            assert_eq!(terminal.cursor(), (0, 5));
        }
        let mut terminal = Terminal::new(80, 24).unwrap();
        assert!(
            terminal
                .apply_event(Event::Output {
                    source_sequence: 0,
                    bytes: b"\x1b[?999h"
                })
                .is_err()
        );
        assert!(terminal.is_failed_closed());
    }

    #[test]
    fn checksum_has_dec_attributes_colors_origin_and_checkpoint_semantics() {
        let mut terminal = Terminal::new(6, 3).unwrap();
        replies(&mut terminal, 0, b"AB");
        // A + B + default foreground(7 << 4) for each cell = 355.
        assert_eq!(
            replies(&mut terminal, 1, b"\x1b[42;1;1;1;1;2*y"),
            [b"\x1bP42!~FE9D\x1b\\".to_vec()]
        );
        replies(
            &mut terminal,
            2,
            b"\x1b[2;3H\x1b[1;4;5;7;8;31;44m\x1b[1\"qA",
        );
        // A + protected/invisible/underline/reverse/blink/bold + red/blue = 337.
        assert_eq!(
            replies(&mut terminal, 3, b"\x1b[3;1;2;3;2;3*y"),
            [b"\x1bP3!~FEAF\x1b\\".to_vec()]
        );
        replies(&mut terminal, 4, b"\x1b[?69h\x1b[3;5s\x1b[2;3r\x1b[?6h");
        let snapshot = terminal.export_snapshot_v2().unwrap();
        let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
        let query = b"\x1b[7;1;1;1;1;1*y";
        assert_eq!(
            replies(&mut terminal, 5, query),
            replies(&mut restored, 5, query)
        );
        assert_eq!(
            replies(&mut terminal, 6, b"\x1b[9;1;2;2;1;1*y"),
            [b"\x1bP9!~0000\x1b\\".to_vec()]
        );
    }

    #[test]
    fn new_reports_require_v2_without_poisoning_legacy_callers() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        assert!(matches!(
            terminal.prepare_event_with_host_effects_v1(Event::Output {
                source_sequence: 0,
                bytes: b"\x1b[=c"
            }),
            Err(TerminalError::HostEffectVersionRequired)
        ));
        assert_eq!(terminal.next_source_sequence(), 0);
        assert!(!terminal.is_failed_closed());
        assert_eq!(replies(&mut terminal, 0, b"\x1b[=c").len(), 1);
    }

    #[test]
    fn extended_report_grammar_rejects_wrong_fields_and_payload_injection() {
        for payload in [
            b"\x1bP!|12345678\x1b\\".as_slice(),
            b"\x1b[?99n",
            b"\x1b[0;80;1;1;1\"w",
            b"\x1b[24;513;1;1;1\"w",
            b"\x1bP00!~1234\x1b\\",
            b"\x1bP1!~ffff\x1b\\",
            b"\x1bP1!~FFFFX\x1b\\",
            b"\x1bP65535!~FFFF\x1b\\",
        ] {
            assert!(!canonical_extended_reply(payload), "{payload:?}");
        }
    }
}
