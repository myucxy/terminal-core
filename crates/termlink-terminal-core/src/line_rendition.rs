//! DEC line renditions. Cell coordinates remain logical; only presentation is scaled.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum LineRendition {
    #[default]
    SingleWidth = 0,
    DoubleWidth = 1,
    DoubleHeightTop = 2,
    DoubleHeightBottom = 3,
}

impl LineRendition {
    pub(crate) fn columns(self, physical_columns: u32) -> u32 {
        if self == Self::SingleWidth {
            physical_columns
        } else {
            (physical_columns / 2).max(1)
        }
    }

    pub(crate) fn from_wire(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::SingleWidth),
            1 => Some(Self::DoubleWidth),
            2 => Some(Self::DoubleHeightTop),
            3 => Some(Self::DoubleHeightBottom),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, OutputProfile, Terminal, TerminalError, WidthProfile};

    fn feed(terminal: &mut Terminal, bytes: &[u8]) {
        terminal
            .apply_event(Event::Output {
                source_sequence: terminal.next_source_sequence(),
                bytes,
            })
            .unwrap();
    }

    fn restored(terminal: &Terminal) -> Terminal {
        terminal.main.validate().unwrap();
        terminal.alternate.validate().unwrap();
        let snapshot = terminal.export_snapshot_v2().unwrap();
        let copy = Terminal::import_snapshot_v2(&snapshot).unwrap();
        assert_eq!(&copy, terminal);
        copy
    }

    #[test]
    fn dec_scaled_lines_survive_every_split_snapshot_and_suffix() {
        let data = b"abcdefghi\x1b#6\r1234Z\x1b[2;1H\x1b#3TOP\x1b[3;1H\x1b#4BOT";
        for width in [WidthProfile::LegacyV1, WidthProfile::Unicode16] {
            for split in 0..=data.len() {
                let mut terminal = Terminal::new_with_profiles(
                    9,
                    4,
                    0,
                    crate::TerminalOptions {
                        history_capacity_rows: 20,
                    },
                    OutputProfile::Utf8,
                    width,
                )
                .unwrap();
                feed(&mut terminal, &data[..split]);
                feed(&mut terminal, &data[split..]);
                assert_eq!(
                    terminal.main.line_renditions[..4],
                    [
                        LineRendition::DoubleWidth,
                        LineRendition::DoubleHeightTop,
                        LineRendition::DoubleHeightBottom,
                        LineRendition::SingleWidth
                    ]
                );
                assert_eq!(terminal.main.line_columns(0), 4);
                assert_eq!(terminal.main.cursor_column, 3);
                assert_eq!(
                    terminal.export_render_frame_v3(),
                    Err(TerminalError::RenderFrameVersionRequired)
                );
                terminal.export_render_frame_v4().unwrap();
                let snapshot = terminal.export_snapshot_v2().unwrap();
                assert_eq!(u16::from_le_bytes(snapshot[10..12].try_into().unwrap()), 13);
                let mut copy = restored(&terminal);
                let suffix = b"\x1b[1;99HZ\x1b#5\x1b[2;2H\x1b[P\x1b[2K\x1b[3;1H\x1b[L\x1b[?1049hALT\x1b[?1049l";
                feed(&mut terminal, suffix);
                feed(&mut copy, suffix);
                assert_eq!(restored(&terminal), restored(&copy));
            }
        }
    }

    #[test]
    fn dec_scaled_line_crop_erase_alignment_and_margins() {
        let mut terminal = Terminal::new(9, 4).unwrap();
        feed(&mut terminal, "ab界eFGHI\x1b#6".as_bytes());
        assert_eq!(terminal.main.cursor_column, 2);
        assert!(
            terminal.main.cells[4..9]
                .iter()
                .all(|cell| cell.character == ' ')
        );
        restored(&terminal);
        feed(&mut terminal, b"\x1b[2K");
        assert_eq!(terminal.main.line_columns(0), 4);
        feed(&mut terminal, b"\x1b[2J");
        assert_eq!(terminal.main.line_columns(0), 9);
        feed(&mut terminal, b"\x1b#3\x1b[?69h\x1b#4");
        assert_eq!(terminal.main.line_columns(0), 9);
        feed(&mut terminal, b"\x1b[?69l\x1b[2;3r\x1b[?6h\x1b#8");
        assert!(
            terminal
                .main
                .visible_cells()
                .iter()
                .all(|cell| cell.character == 'E')
        );
        assert_eq!(terminal.main.cursor_row, 0);
        assert_eq!(terminal.main.cursor_column, 0);
        assert!(!terminal.modes.origin_mode);
        assert!(!terminal.main.has_line_renditions());
        restored(&terminal);
    }

    #[test]
    fn dec_scaled_rows_crop_during_resize_while_normal_rows_reflow() {
        let mut terminal = Terminal::new_with_options(
            10,
            5,
            0,
            crate::TerminalOptions {
                history_capacity_rows: 20,
            },
        )
        .unwrap();
        feed(
            &mut terminal,
            b"\x1b#6ABCDE\x1b[2;1HabcdefghijK\x1b[4;1H\x1b#3TOP",
        );
        terminal
            .apply_event(Event::Resize {
                source_sequence: terminal.next_source_sequence(),
                columns: 6,
                rows: 6,
            })
            .unwrap();
        let row = terminal
            .main
            .line_renditions
            .iter()
            .position(|r| *r == LineRendition::DoubleWidth)
            .unwrap();
        assert_eq!(terminal.main.meaningful_end[row], 3);
        assert_eq!(
            terminal.main.cells[row * 6..row * 6 + 3]
                .iter()
                .map(|c| c.character)
                .collect::<String>(),
            "ABC"
        );
        assert!(terminal.main.soft_wrapped.iter().any(|value| *value));
        restored(&terminal);
        feed(&mut terminal, b"\x1b[6;1H\r\n\r\n");
        restored(&terminal);
        let history = terminal.main.render_window(terminal.main.viewport_top());
        history.validate().unwrap();
        assert!(history.has_line_renditions());
    }

    #[test]
    fn scaled_lines_seeded_edit_scroll_resize_and_restore() {
        let mut terminal = Terminal::new_with_profiles(
            12,
            5,
            0,
            crate::TerminalOptions {
                history_capacity_rows: 20,
            },
            OutputProfile::Utf8,
            WidthProfile::Unicode16,
        )
        .unwrap();
        let samples = [
            "abc中",
            "👩\u{200d}💻",
            "\r\n",
            "\x1b#3",
            "\x1b#4",
            "\x1b#5",
            "\x1b#6",
            "\x1b#8",
            "\x1b[2;99H",
            "\x1b[H",
            "\x1b[M",
            "\x1b[L",
            "\x1b[P",
            "\x1b[@",
            "\x1b[2K",
            "\x1b[1J",
            "\x1b[2J",
            "\x1b[?69h",
            "\x1b[?69l",
            "\x1b[?1049h",
            "\x1b[?1049l",
            "\x1b[2S",
            "\x1b[2T",
            "\x1b7",
            "\x1b8",
            "\x1bM",
        ];
        let mut seed = 0xc04_2026_u64;
        for iteration in 0..1000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let event = Event::Resize {
                source_sequence: terminal.next_source_sequence(),
                columns: 6 + (seed % 17) as u32,
                rows: 3 + ((seed >> 8) % 6) as u32,
            };
            terminal.apply_event(event).unwrap();
            let sample = samples[(seed >> 16) as usize % samples.len()];
            feed(&mut terminal, sample.as_bytes());
            let bytes = terminal.export_snapshot_v2().unwrap_or_else(|error| panic!("iteration {iteration}, sample {sample:?}: {error}, main={:?} alt={:?}, active={} margins={:?}, dims={:?}, cursor={:?}, saved={:?}, rend={:?}, meaningful={:?}, wrap={:?}", terminal.main.validate(), terminal.alternate.validate(), terminal.active_alternate, terminal.margins, (terminal.active_screen().columns, terminal.active_screen().rows), (terminal.active_screen().cursor_row, terminal.active_screen().cursor_column, terminal.active_screen().wrap_pending), terminal.active_screen().saved, terminal.active_screen().line_renditions, terminal.active_screen().meaningful_end, terminal.active_screen().soft_wrapped));
            let mut copy = Terminal::import_snapshot_v2(&bytes).unwrap();
            let suffix = samples[(seed >> 32) as usize % samples.len()].as_bytes();
            feed(&mut terminal, suffix);
            feed(&mut copy, suffix);
            assert_eq!(
                terminal.export_snapshot_v2().unwrap(),
                copy.export_snapshot_v2().unwrap(),
                "iteration {iteration}"
            );
            assert_eq!(
                terminal.export_render_frame_v4().unwrap(),
                copy.export_render_frame_v4().unwrap()
            );
        }
    }
}
