use crate::{Event, OscColor, Terminal, TerminalError};

fn feed(terminal: &mut Terminal, bytes: &[u8]) -> Vec<u8> {
    let sequence = terminal.next_source_sequence();
    let apply = terminal
        .prepare_event_with_host_effects_v2(Event::Output {
            source_sequence: sequence,
            bytes,
        })
        .unwrap();
    let effects = apply.host_effect_frame_v2().to_vec();
    apply.commit_with(|_| Ok::<(), ()>(())).unwrap();
    effects
}

#[test]
fn color_alias_and_selection_roundtrip_at_every_split() {
    let data = b"\x1b]17;#123456\x1b\\\x1b[1;2;3,|\x1b[2;4;5,|\x1b]4;2;#abcdef;3;#112233\x1b\\text";
    for split in 0..=data.len() {
        let mut terminal = Terminal::new(20, 5).unwrap();
        feed(&mut terminal, &data[..split]);
        feed(&mut terminal, &data[split..]);
        assert_eq!(
            terminal.osc.selection_background(),
            Some(OscColor::new(0x12, 0x34, 0x56))
        );
        assert_eq!(
            terminal.osc.color_aliases(),
            &[Some(2), Some(3), Some(4), Some(5)]
        );
        assert_eq!(
            terminal.export_render_frame_v3(),
            Err(TerminalError::RenderFrameVersionRequired)
        );
        let snapshot = terminal.export_snapshot_v2().unwrap();
        assert_eq!(u16::from_le_bytes(snapshot[10..12].try_into().unwrap()), 14);
        let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
        assert_eq!(terminal, restored);
        let queries = b"\x1b]17;?\x1b\\\x1b]10;?\x1b\\\x1bP$q1,|\x1b\\\x1bP$q2,|\x1b\\";
        let replies = feed(&mut terminal, queries);
        assert_eq!(replies, feed(&mut restored, queries));
        for expected in [
            b"\x1b]17;rgb:1212/3434/5656\x1b\\".as_slice(),
            b"\x1b]10;rgb:abab/cdcd/efef\x1b\\",
            b"\x1bP1$r1;2;3,|\x1b\\",
            b"\x1bP1$r2;4;5,|\x1b\\",
        ] {
            assert!(
                replies
                    .windows(expected.len())
                    .any(|bytes| bytes == expected)
            );
        }
        let suffix = b"\x1b]10;#2468ac\x1b\\\x1b]117\x1b\\\x1b[?1049h\x1b#6ALT\x1b[?1049l";
        feed(&mut terminal, suffix);
        feed(&mut restored, suffix);
        assert_eq!(terminal, restored);
        assert_eq!(
            terminal.osc.palette(2),
            Some(OscColor::new(0x24, 0x68, 0xac))
        );
        assert_eq!(terminal.osc.foreground(), None);
        assert_eq!(terminal.osc.selection_background(), None);
        assert_eq!(
            terminal.export_render_frame_v4().unwrap(),
            restored.export_render_frame_v4().unwrap()
        );
        assert_eq!(
            Terminal::import_snapshot_v2(&terminal.export_snapshot_v2().unwrap()).unwrap(),
            terminal
        );
    }
}

#[test]
fn colors_restore_defaults_and_invalid_batches_do_not_commit() {
    let mut terminal = Terminal::new(10, 5).unwrap();
    feed(
        &mut terminal,
        b"\x1b]17;#010203\x1b\\\x1b[1;2;3,|\x1b[2;4;5,|",
    );
    feed(
        &mut terminal,
        b"\x1b]117\x1b\\\x1b[1;256;257,|\x1b[2;256;257,|",
    );
    assert!(!terminal.osc.has_extended_colors());
    terminal.export_render_frame_v3().unwrap();
    for invalid in [
        b"\x1b[1;258;1,|".as_slice(),
        b"\x1b[3;1;1,|",
        b"\x1b]17;not-a-color\x1b\\",
        b"\x1b]117;#123456\x1b\\",
    ] {
        let mut candidate = terminal.clone();
        let before = candidate.osc.clone();
        let mut bytes = b"\x1b]17;#abcdef\x1b\\".to_vec();
        bytes.extend_from_slice(invalid);
        assert!(
            candidate
                .apply_event(Event::Output {
                    source_sequence: candidate.next_source_sequence(),
                    bytes: &bytes
                })
                .is_err()
        );
        assert_eq!(candidate.osc, before);
        assert_eq!(
            candidate.next_source_sequence(),
            terminal.next_source_sequence()
        );
    }
}

#[test]
fn invalid_color_assignment_identifies_original_parameter_spelling() {
    let source = b"\x1b[1;000258;3,|";
    for split in 0..=source.len() {
        let mut terminal = Terminal::new(10, 5).unwrap();
        for part in [&source[..split], &source[split..]] {
            if terminal
                .apply_event(Event::Output {
                    source_sequence: terminal.next_source_sequence(),
                    bytes: part,
                })
                .is_err()
            {
                break;
            }
        }
        let diagnostic = terminal.failure_diagnostic_v2().unwrap();
        assert!(diagnostic.contains("reason=DECAC foreground color index\n"));
        assert!(diagnostic.contains("rejected_parameter_index=1\nrejected_parameter=000258\n"));
    }
}
