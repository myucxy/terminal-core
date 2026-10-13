use crate::{MAX_GRAPHEME_BYTES, unicode_width, unicode16};

/// Frozen per-session layout rules. Existing snapshots always restore LegacyV1.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u16)]
pub enum WidthProfile {
    #[default]
    LegacyV1 = 1,
    Unicode16 = 2,
}

impl WidthProfile {
    pub(crate) const fn max_grapheme_bytes(self) -> usize {
        match self {
            Self::LegacyV1 => MAX_GRAPHEME_BYTES,
            Self::Unicode16 => 48,
        }
    }

    pub(crate) fn scalar_width(self, character: char) -> u8 {
        if (' '..='~').contains(&character) {
            return 1;
        }
        match self {
            Self::LegacyV1 => unicode_width::scalar_width(character),
            Self::Unicode16 => unicode16::cluster_width(character.encode_utf8(&mut [0; 4])),
        }
    }

    pub(crate) fn grapheme_width(self, character: char, suffix: &str) -> u8 {
        if suffix.is_empty() && (' '..='~').contains(&character) {
            return 1;
        }
        match self {
            Self::LegacyV1 => unicode_width::grapheme_width(character, suffix),
            Self::Unicode16 => {
                let mut bytes = [0; 48];
                let head = character.encode_utf8(&mut bytes).len();
                if head + suffix.len() > bytes.len() {
                    return 0;
                }
                bytes[head..head + suffix.len()].copy_from_slice(suffix.as_bytes());
                unicode16::cluster_width(
                    std::str::from_utf8(&bytes[..head + suffix.len()])
                        .expect("scalar and suffix UTF-8"),
                )
                .max(1)
            }
        }
    }

    pub(crate) fn extends(self, character: char, suffix: &str, next: char) -> bool {
        if self == Self::LegacyV1 {
            return false;
        }
        if suffix.is_empty() && character.is_ascii() && next.is_ascii() {
            return character == '\r' && next == '\n';
        }
        let mut segmenter = unicode16::GraphemeSegmenter::default();
        for scalar in std::iter::once(character).chain(suffix.chars()) {
            segmenter.break_before(scalar);
        }
        !segmenter.break_before(next)
    }

    pub(crate) fn valid_grapheme(self, character: char, suffix: &str, width: u8) -> bool {
        if suffix.is_empty() && (' '..='~').contains(&character) {
            return width == 1;
        }
        if character.is_control()
            || character.len_utf8() + suffix.len() > self.max_grapheme_bytes()
            || !matches!(width, 1 | 2)
            || width != self.grapheme_width(character, suffix)
        {
            return false;
        }
        if self == Self::Unicode16 {
            if suffix.is_empty() {
                return true;
            }
            let mut segmenter = unicode16::GraphemeSegmenter::default();
            segmenter.break_before(character);
            return suffix
                .chars()
                .all(|c| !c.is_control() && !segmenter.break_before(c));
        }
        if self.scalar_width(character) == 0 {
            return false;
        }
        let mut joined = false;
        suffix.chars().all(|scalar| {
            if scalar.is_control() && scalar != '\u{200d}' {
                return false;
            }
            if unicode_width::scalar_width(scalar) == 0 {
                joined = scalar == '\u{200d}';
                true
            } else if joined && unicode_width::scalar_width(scalar) == 2 {
                joined = false;
                true
            } else {
                false
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, OutputProfile, Terminal, TerminalOptions};

    fn modern(columns: u32, rows: u32) -> Terminal {
        Terminal::new_with_profiles(
            columns,
            rows,
            0,
            TerminalOptions {
                history_capacity_rows: 20,
            },
            OutputProfile::Utf8,
            WidthProfile::Unicode16,
        )
        .unwrap()
    }
    fn output(t: &mut Terminal, bytes: &[u8]) {
        t.apply_event(Event::Output {
            source_sequence: t.next_source_sequence(),
            bytes,
        })
        .unwrap();
    }

    #[test]
    fn unicode_grid_and_snapshot_suffix_equal_at_every_utf8_split() {
        let text = "🇨🇳👍🏽👨\u{200d}👩\u{200d}👧\u{200d}👦a\u{301}क्\u{200d}ष©\u{fe0f}";
        for split in 0..=text.len() {
            let mut t = modern(16, 4);
            output(&mut t, &text.as_bytes()[..split]);
            output(&mut t, &text.as_bytes()[split..]);
            assert_eq!(t.cursor(), (0, 10), "split {split}");
            assert_eq!(t.main.cells[4].grapheme_bytes(), 25);
            t.main.validate().unwrap();
            let bytes = t.export_snapshot_v2().unwrap();
            assert_eq!(u16::from_le_bytes(bytes[10..12].try_into().unwrap()), 12);
            assert_eq!(u32::from_le_bytes(bytes[48..52].try_into().unwrap()), 2);
            let mut restored = Terminal::import_snapshot_v2(&bytes).unwrap();
            assert_eq!(restored.width_profile(), WidthProfile::Unicode16);
            output(&mut t, "🧑\u{200d}🚀!".as_bytes());
            output(&mut restored, "🧑\u{200d}🚀!".as_bytes());
            assert_eq!(t, restored);
        }
    }

    #[test]
    fn mode_2027_really_controls_joining_and_ris_preserves_profile() {
        let mut t = modern(16, 4);
        output(&mut t, b"\x1b[?2027l");
        output(&mut t, "🇨🇳".as_bytes());
        assert_eq!(t.cursor(), (0, 4));
        output(&mut t, b"\x1b[?2027h");
        output(&mut t, "🇨🇳".as_bytes());
        assert_eq!(t.cursor(), (0, 6));
        assert_eq!(t.main.cells[0].combining, "");
        assert_eq!(t.main.cells[2].combining, "");
        assert_eq!(t.main.cells[4].character, '🇨');
        assert_eq!(t.main.cells[4].combining, "🇳");
        output(&mut t, b"\x1bc");
        assert_eq!(t.width_profile(), WidthProfile::Unicode16);
        assert!(t.modes.grapheme_clustering);
        output(&mut t, "🇨🇳".as_bytes());
        assert_eq!(t.cursor(), (0, 2));
    }

    #[test]
    fn legacy_factory_bytes_stay_legacy_and_profile_cannot_be_forged() {
        let a = Terminal::new(16, 4).unwrap();
        let b = Terminal::new_with_profiles(
            16,
            4,
            0,
            TerminalOptions::default(),
            OutputProfile::Utf8,
            WidthProfile::LegacyV1,
        )
        .unwrap();
        assert_eq!(
            a.export_snapshot_v2().unwrap(),
            b.export_snapshot_v2().unwrap()
        );
        let mut bytes = modern(16, 4).export_snapshot_v2().unwrap();
        bytes[48..52].copy_from_slice(&1_u32.to_le_bytes());
        crate::snapshot::refresh_crc_for_test(&mut bytes);
        assert_eq!(
            Terminal::import_snapshot_v2(&bytes),
            Err(crate::TerminalError::IncompatibleSnapshot)
        );
        let mut mixed = modern(16, 4);
        mixed.alternate.width_profile = WidthProfile::LegacyV1;
        assert_eq!(
            mixed.export_snapshot_v2(),
            Err(crate::TerminalError::MalformedSnapshot)
        );
    }

    #[test]
    fn width_changes_keep_tail_and_cursor_metadata_consistent() {
        let mut t = modern(4, 4);
        output(&mut t, "ab©\u{fe0f}\u{fe0e}".as_bytes());
        assert_eq!(t.cursor(), (0, 3));
        assert_eq!(t.main.meaningful_end[0], 3);
        assert!(!t.main.cells[3].continuation);
        let bytes = t.export_snapshot_v2().unwrap();
        assert_eq!(Terminal::import_snapshot_v2(&bytes).unwrap(), t);
        output(&mut t, b"X");
        assert_eq!(t.main.cells[3].character, 'X');

        // A combining mark after a cursor move may change the preceding row,
        // but must not move the cursor horizontally on the current row.
        let mut t = modern(4, 4);
        output(&mut t, "ab©\u{fe0f}X\x1b[2;1H\u{fe0e}".as_bytes());
        assert_eq!(t.cursor(), (1, 0));
        assert_eq!(t.main.meaningful_end[0], 3);
        assert_eq!(t.main.cells[4].character, 'X');
        t.export_snapshot_v2().unwrap();

        let mut t = modern(4, 4);
        output(&mut t, "abc©X\x1b[2;1H\u{fe0f}".as_bytes());
        assert_eq!(t.cursor(), (1, 2));
        assert_eq!(t.main.cells[4].character, '©');
        t.export_snapshot_v2().unwrap();
    }

    #[test]
    fn rep_isolates_clusters_and_restores_exact_source_after_sgr() {
        let mut t = modern(16, 4);
        output(&mut t, "🇨\x1b[2b".as_bytes());
        assert_eq!(t.cursor(), (0, 6));
        assert!(
            [0, 2, 4]
                .iter()
                .all(|&i| t.main.cells[i].combining.is_empty())
        );
        output(&mut t, "a\x1b[31m\u{301}\x1b[2b".as_bytes());
        assert_eq!(t.cursor(), (0, 9));
        assert!(
            [6, 7, 8].iter().all(
                |&i| t.main.cells[i].character == 'a' && t.main.cells[i].combining == "\u{301}"
            )
        );
        let mut t = modern(16, 4);
        output(&mut t, "\u{301}\x1b[2b".as_bytes());
        assert_eq!(t.cursor(), (0, 3));
        output(&mut t, "👨\u{200d}👩\u{200d}👧\u{200d}👦\x1b[b".as_bytes());
        assert_eq!(t.cursor(), (0, 7));
        assert_eq!(t.main.cells[3].combining, t.main.cells[5].combining);
        t.export_snapshot_v2().unwrap();
    }

    #[test]
    fn orphan_marks_family_limit_and_reflow_remain_recoverable() {
        let mut t = modern(8, 4);
        output(&mut t, "\u{301}🇨🇳🇺🇸abc🇯🇵".as_bytes());
        assert_eq!(t.main.cells[0].character, '\u{301}');
        assert_eq!(t.main.cells[0].width, 1);
        t.apply_event(Event::Resize {
            source_sequence: t.next_source_sequence(),
            columns: 6,
            rows: 4,
        })
        .unwrap();
        let bytes = t.export_snapshot_v2().unwrap();
        assert_eq!(Terminal::import_snapshot_v2(&bytes).unwrap(), t);
        output(&mut t, b"\x1b[?1049h");
        output(&mut t, "👩\u{200d}💻".as_bytes());
        output(&mut t, b"\x1b[?1049l");
        assert_eq!(t.width_profile(), WidthProfile::Unicode16);
        let long = format!("a{}", "\u{301}".repeat(24));
        let before = t.next_source_sequence();
        assert!(
            t.apply_event(Event::Output {
                source_sequence: before,
                bytes: long.as_bytes()
            })
            .is_err()
        );
        assert_eq!(t.next_source_sequence(), before);
        assert!(
            t.failure_diagnostic_v2()
                .unwrap()
                .contains("width_profile=2\n")
        );
    }

    #[test]
    fn render_v4_preserves_unicode_clusters_and_old_apis_reject_without_poison() {
        let mut t = modern(8, 4);
        output(
            &mut t,
            "👨\u{200d}👩\u{200d}👧\u{200d}👦\r\n2\r\n3\r\n4\r\n5".as_bytes(),
        );
        let live = t.export_render_frame_v4().unwrap();
        let history = t.export_render_frame_v4_at_scrollback(999).unwrap();
        assert_eq!(&live[..8], b"TLRFRM4\0");
        assert_eq!(u32::from_le_bytes(live[60..64].try_into().unwrap()), 2);
        assert!(
            history
                .windows(25)
                .any(|b| b == "👨\u{200d}👩\u{200d}👧\u{200d}👦".as_bytes())
        );
        assert_eq!(
            t.export_render_frame_v3(),
            Err(crate::TerminalError::RenderFrameVersionRequired)
        );
        assert_eq!(
            t.export_render_frame_v3_at_scrollback(1),
            Err(crate::TerminalError::RenderFrameVersionRequired)
        );
        assert!(!t.failed_closed);
        let restored = Terminal::import_snapshot_v2(&t.export_snapshot_v2().unwrap()).unwrap();
        assert_eq!(restored.export_render_frame_v4().unwrap(), live);
        let legacy = Terminal::new(8, 4)
            .unwrap()
            .export_render_frame_v4()
            .unwrap();
        assert_eq!(u32::from_le_bytes(legacy[60..64].try_into().unwrap()), 1);
        assert_eq!(
            crate::render_frame::validate_v3_header(&live),
            Err(crate::TerminalError::IncompatibleSnapshot)
        );
        let mut forged = live;
        forged[60..64].copy_from_slice(&3_u32.to_le_bytes());
        assert_eq!(
            crate::render_frame::validate_v4_header(&forged),
            Err(crate::TerminalError::IncompatibleSnapshot)
        );
    }

    #[test]
    fn unicode_seeded_resize_mode_edit_and_checkpoint_suffix_stays_canonical() {
        let mut terminal = modern(16, 6);
        let samples = [
            "a",
            "中",
            "🇨🇳",
            "👍🏽",
            "👩\u{200d}💻",
            "क्\u{200d}ष",
            "©\u{fe0f}\u{fe0e}",
            "\r\n",
            "\x1b[?2027h",
            "\x1b[?2027l",
            "\x1b[?1049h",
            "\x1b[?1049l",
            "\x1b[2K",
            "\x1b[1P",
            "\x1b[2@",
            "\x1b[H",
            "\x1b[31m",
            "\x1b[2b",
        ];
        let mut seed = 0x0316_2027_u64;
        for iteration in 0..1000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let columns = 4 + (seed % 29) as u32;
            terminal
                .apply_event(Event::Resize {
                    source_sequence: terminal.next_source_sequence(),
                    columns,
                    rows: 3 + ((seed >> 8) % 9) as u32,
                })
                .unwrap();
            output(
                &mut terminal,
                samples[(seed >> 16) as usize % samples.len()].as_bytes(),
            );
            let snapshot = terminal
                .export_snapshot_v2()
                .unwrap_or_else(|error| panic!("iteration {iteration}: {error}"));
            let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
            let suffix = samples[(seed >> 32) as usize % samples.len()].as_bytes();
            output(&mut terminal, suffix);
            output(&mut restored, suffix);
            assert_eq!(
                terminal.export_snapshot_v2().unwrap(),
                restored.export_snapshot_v2().unwrap(),
                "iteration {iteration}"
            );
            assert_eq!(
                terminal.export_render_frame_v4().unwrap(),
                restored.export_render_frame_v4().unwrap()
            );
        }
    }
}
