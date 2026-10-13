use crate::{Event, HostThemeV1, OscBaseline, OscColor, Terminal, TerminalError};

fn theme(scheme: u8) -> Vec<u8> {
    let white = OscColor::new(255, 255, 255);
    let black = OscColor::new(0, 0, 0);
    HostThemeV1 {
        scheme,
        baseline: OscBaseline {
            palette: [Some(if scheme == 1 { black } else { white }); 256],
            foreground: Some(if scheme == 1 { white } else { black }),
            background: Some(if scheme == 1 { black } else { white }),
            cursor: Some(white),
        },
    }
    .encode()
    .unwrap()
    .to_vec()
}

fn apply(t: &mut Terminal, bytes: &[u8], host: bool) -> Vec<Vec<u8>> {
    let source_sequence = t.next_source_sequence();
    let event = if host {
        Event::HostTheme {
            source_sequence,
            bytes,
        }
    } else {
        Event::Output {
            source_sequence,
            bytes,
        }
    };
    let pending = t.prepare_event_with_host_effects_v2(event).unwrap();
    let replies = crate::tests::host_effect_payloads(pending.host_effect_frame_v2())
        .into_iter()
        .map(<[u8]>::to_vec)
        .collect();
    pending.publish(|_| Ok::<(), ()>(())).unwrap();
    replies
}

#[test]
fn theme_queries_are_honest_ordered_and_unsolicited_only_while_enabled() {
    let mut t = Terminal::new(8, 4).unwrap();
    assert!(apply(&mut t, b"\x1b[?996n", false).is_empty());
    assert_eq!(
        apply(&mut t, b"\x1b[?2031$p", false),
        [b"\x1b[?2031;0$y".to_vec()]
    );
    assert!(apply(&mut t, &theme(1), true).is_empty());
    for split in 0..=7 {
        let query = b"\x1b[?996n";
        let mut replies = apply(&mut t, &query[..split], false);
        replies.extend(apply(&mut t, &query[split..], false));
        assert_eq!(replies, [b"\x1b[?997;1n".to_vec()]);
    }
    assert_eq!(
        apply(&mut t, b"\x1b[?2031h", false),
        [b"\x1b[?997;1n".to_vec()]
    );
    assert!(apply(&mut t, &theme(1), true).is_empty());
    assert_eq!(apply(&mut t, &theme(2), true), [b"\x1b[?997;2n".to_vec()]);
    assert_eq!(
        apply(&mut t, b"\x1b[?2031$p", false),
        [b"\x1b[?2031;1$y".to_vec()]
    );
    apply(&mut t, b"\x1b[?2031l", false);
    assert!(apply(&mut t, &theme(1), true).is_empty());
    apply(&mut t, b"\x1bc", false);
    assert_eq!(
        apply(&mut t, b"\x1b[?996n\x1b[?2031$p", false),
        [b"\x1b[?997;1n".to_vec(), b"\x1b[?2031;2$y".to_vec()]
    );
}

#[test]
fn theme_inheritance_preserves_explicit_equal_colors_aliases_and_reset_ownership() {
    let mut t = Terminal::new(8, 4).unwrap();
    apply(&mut t, &theme(1), true);
    // Assigning the same current baseline is still an explicit application override.
    apply(&mut t, b"\x1b]4;1;#000000\x07\x1b]11;#000000\x07", false);
    let snapshot = t.export_snapshot_v2().unwrap();
    assert_eq!(u16::from_le_bytes(snapshot[10..12].try_into().unwrap()), 19);
    let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
    assert_eq!(snapshot, restored.export_snapshot_v2().unwrap());
    for terminal in [&mut t, &mut restored] {
        apply(terminal, &theme(2), true);
        assert_eq!(terminal.osc.palette(1), Some(OscColor::new(0, 0, 0)));
        assert_eq!(terminal.osc.palette(2), Some(OscColor::new(255, 255, 255)));
        assert_eq!(terminal.osc.background(), Some(OscColor::new(0, 0, 0)));
        apply(terminal, b"\x1b]104;1\x07\x1b]111;\x07", false);
        assert_eq!(
            terminal.osc.background(),
            Some(OscColor::new(255, 255, 255))
        );
        // DECAC directs OSC 10 to palette index 5; theme changes must preserve that slot.
        apply(terminal, b"\x1b[1;5;257,|\x1b]10;#112233\x07", false);
        apply(terminal, &theme(1), true);
        assert_eq!(terminal.osc.palette(5), Some(OscColor::new(17, 34, 51)));
        assert_eq!(terminal.osc.palette(1), Some(OscColor::new(0, 0, 0)));
        apply(terminal, b"\x1bc", false);
        apply(terminal, &theme(2), true);
        assert_eq!(terminal.osc.palette(5), Some(OscColor::new(255, 255, 255)));
    }
    assert_eq!(
        t.export_snapshot_v2().unwrap(),
        restored.export_snapshot_v2().unwrap()
    );
}

#[test]
fn theme_migration_keeps_old_snapshots_and_existing_colors_without_guessing_ownership() {
    let mut t = Terminal::new(8, 4).unwrap();
    apply(&mut t, b"\x1b]11;#ffffff\x07", false);
    let snapshot = t.export_snapshot_v2().unwrap();
    let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
    assert_eq!(restored.export_snapshot_v2().unwrap(), snapshot);
    for terminal in [&mut t, &mut restored] {
        apply(terminal, &theme(2), true);
        apply(terminal, &theme(1), true);
        assert_eq!(
            terminal.osc.background(),
            Some(OscColor::new(255, 255, 255))
        );
    }
    assert_eq!(
        t.export_snapshot_v2().unwrap(),
        restored.export_snapshot_v2().unwrap()
    );
}

#[test]
fn malformed_or_failed_theme_publication_cannot_partially_change_state() {
    let mut t = Terminal::new(8, 4).unwrap();
    let before = t.export_snapshot_v2().unwrap();
    for offset in [0, 4, 5, 11, 1043] {
        let mut bytes = theme(1);
        bytes[offset] = 255;
        assert_eq!(
            t.apply_event(Event::HostTheme {
                source_sequence: 0,
                bytes: &bytes
            }),
            Err(TerminalError::InvalidDimensions)
        );
        assert_eq!(t.export_snapshot_v2().unwrap(), before);
    }
    let bytes = theme(1);
    let prepared = t
        .prepare_event_with_host_effects_v2(Event::HostTheme {
            source_sequence: 0,
            bytes: &bytes,
        })
        .unwrap();
    assert!(
        prepared
            .publish(|_| Err::<(), _>("journal failed"))
            .is_err()
    );
    assert_eq!(t.export_snapshot_v2().unwrap(), before);
    apply(&mut t, &bytes, true);
    apply(&mut t, b"\x1b[?2031h", false);
    let before = t.export_snapshot_v2().unwrap();
    let light = theme(2);
    assert!(matches!(
        t.prepare_event_with_host_effects_v1(Event::HostTheme {
            source_sequence: t.next_source_sequence(),
            bytes: &light
        }),
        Err(TerminalError::HostEffectVersionRequired)
    ));
    assert_eq!(t.export_snapshot_v2().unwrap(), before);
}
