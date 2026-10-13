use crate::{Event, Terminal, TerminalError};

#[test]
fn page_size_requests_wait_for_ordered_host_resize_and_keep_exact_source_keys() {
    let bytes = b"\x1b[132$|\x1b[40*|\x1b[18t";
    for split in 0..=bytes.len() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        let mut records = Vec::new();
        for part in [&bytes[..split], &bytes[split..]] {
            let sequence = terminal.next_source_sequence();
            let prepared = terminal
                .prepare_event_with_host_effects_v2(Event::Output {
                    source_sequence: sequence,
                    bytes: part,
                })
                .unwrap();
            let frame = prepared.host_effect_frame_v2();
            let count = u32::from_le_bytes(frame[16..20].try_into().unwrap()) as usize;
            let payloads = crate::tests::host_effect_payloads(frame);
            for (index, payload) in payloads.iter().enumerate().take(count) {
                let offset = 64 + index * 40;
                let kind = u32::from_le_bytes(frame[offset..offset + 4].try_into().unwrap());
                let event = u64::from_le_bytes(frame[offset + 8..offset + 16].try_into().unwrap());
                let end = u32::from_le_bytes(frame[offset + 16..offset + 20].try_into().unwrap());
                assert_eq!(event, sequence);
                assert!(end > 0 && end as usize <= part.len());
                records.push((kind, payload.to_vec()));
            }
            prepared.publish(|_| Ok::<(), ()>(())).unwrap();
        }
        assert_eq!(
            records,
            vec![
                (2, [132_u32.to_le_bytes(), 0_u32.to_le_bytes()].concat()),
                (2, [0_u32.to_le_bytes(), 40_u32.to_le_bytes()].concat()),
                (1, b"\x1b[8;24;80t".to_vec()),
            ]
        );
        assert_eq!((terminal.columns, terminal.rows), (80, 24));
        terminal
            .apply_event(Event::Resize {
                source_sequence: terminal.next_source_sequence(),
                columns: 132,
                rows: 40,
            })
            .unwrap();
        assert_eq!(
            output(&mut terminal, b"\x1b[18t"),
            [b"\x1b[8;40;132t".to_vec()]
        );
        let snapshot = terminal.export_snapshot_v2().unwrap();
        assert_eq!(
            Terminal::import_snapshot_v2(&snapshot)
                .unwrap()
                .export_snapshot_v2()
                .unwrap(),
            snapshot
        );
    }
}

#[test]
fn page_size_requests_defaults_limits_and_v1_rollback() {
    let mut terminal = Terminal::new(80, 24).unwrap();
    assert_eq!(
        output(&mut terminal, b"\x1b[$|\x1b[0*|"),
        vec![
            [80_u32.to_le_bytes(), 0_u32.to_le_bytes()].concat(),
            [0_u32.to_le_bytes(), 24_u32.to_le_bytes()].concat(),
        ]
    );
    let mut terminal = Terminal::new(80, 24).unwrap();
    let before = terminal.export_snapshot_v2().unwrap();
    assert!(matches!(
        terminal.prepare_event_with_host_effects_v1(Event::Output {
            source_sequence: terminal.next_source_sequence(),
            bytes: b"X\x1b[132$|",
        }),
        Err(TerminalError::HostEffectVersionRequired)
    ));
    assert_eq!(terminal.export_snapshot_v2().unwrap(), before);
    for bytes in [b"\x1b[513$|".as_slice(), b"\x1b[1;2*|", b"\x1b[1:2$|"] {
        let mut terminal = Terminal::new(80, 24).unwrap();
        assert!(
            terminal
                .prepare_event_with_host_effects_v2(Event::Output {
                    source_sequence: terminal.next_source_sequence(),
                    bytes
                })
                .is_err()
        );
    }
    let repeated = b"\x1b[132$|".repeat(65);
    let mut terminal = Terminal::new(80, 24).unwrap();
    assert!(matches!(
        terminal.prepare_event_with_host_effects_v2(Event::Output {
            source_sequence: terminal.next_source_sequence(),
            bytes: &repeated,
        }),
        Err(TerminalError::LimitExceeded)
    ));
}

fn output(t: &mut Terminal, bytes: &[u8]) -> Vec<Vec<u8>> {
    let pending = t
        .prepare_event_with_host_effects_v2(Event::Output {
            source_sequence: t.next_source_sequence(),
            bytes,
        })
        .unwrap();
    let replies = crate::tests::host_effect_payloads(pending.host_effect_frame_v2())
        .into_iter()
        .map(<[u8]>::to_vec)
        .collect();
    pending.publish(|_| Ok::<(), ()>(())).unwrap();
    replies
}

#[test]
fn pages_preserve_contents_and_follow_or_hide_the_active_cursor() {
    let stream = b"ONE\x1b[2;3H\x1b[3 PTHREE\x1b[?64l\x1b[1 P\x1b[HHIDDEN";
    for split in 0..=stream.len() {
        let mut t = Terminal::new(12, 4).unwrap();
        output(&mut t, &stream[..split]);
        output(&mut t, &stream[split..]);
        assert_eq!(t.active_page(), 1);
        assert_eq!(t.visible_page(), 3);
        assert_eq!(t.cell(0, 0).unwrap().character, 'H');
        assert_eq!(
            t.visible_screen().cells[t.visible_screen().visible_cell_index(1, 2)].character,
            'T'
        );
        let frame = t.export_render_frame_v4().unwrap();
        assert_eq!(
            output(&mut t, b"\x1b[\"v"),
            [b"\x1b[4;12;1;1;3\"w".to_vec()]
        );
        let snapshot = t.export_snapshot_v2().unwrap();
        assert_eq!(u16::from_le_bytes(snapshot[10..12].try_into().unwrap()), 17);
        let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
        assert_eq!(restored.export_snapshot_v2().unwrap(), snapshot);
        assert_eq!(
            restored.export_render_frame_v4().unwrap()[48..],
            t.export_render_frame_v4().unwrap()[48..]
        );
        assert!(!frame.is_empty());
        output(&mut restored, b"\x1b[?64h");
        assert_eq!(restored.visible_page(), 1);
        assert_eq!(restored.visible_screen().cells[0].character, 'H');
        output(&mut restored, b"\x1b[3 P");
        assert_eq!(restored.cell(1, 2).unwrap().character, 'T');
    }
}

#[test]
fn page_movement_save_restore_alt_reset_and_resize_keep_the_correct_page() {
    let mut t = Terminal::new(12, 4).unwrap();
    output(&mut t, b"\x1b[3;4H\x1b[3 P\x1b7X\x1b[2 Q");
    assert_eq!((t.active_page(), t.cursor()), (5, (2, 4)));
    output(&mut t, b"\x1b8");
    assert_eq!((t.active_page(), t.cursor()), (3, (2, 3)));
    output(&mut t, b"\x1b[999 Q");
    assert_eq!(t.active_page(), 6);
    output(&mut t, b"\x1b[999 R");
    assert_eq!(t.active_page(), 1);
    output(&mut t, b"\x1b[2U");
    assert_eq!((t.active_page(), t.cursor()), (3, (0, 0)));
    output(&mut t, b"\x1b[V");
    assert_eq!(t.active_page(), 2);
    output(&mut t, b"\x1b[3 P\x1b[?1049hALT\x1b[6 P");
    assert_eq!(t.active_page(), 1);
    let saved = t.export_snapshot_v2().unwrap();
    let mut restored = Terminal::import_snapshot_v2(&saved).unwrap();
    output(&mut restored, b"\x1b[?1049l");
    assert_eq!(restored.active_page(), 3);
    assert_eq!(restored.cell(2, 3).unwrap().character, 'X');
    restored
        .apply_event(Event::Resize {
            source_sequence: restored.next_source_sequence(),
            columns: 8,
            rows: 3,
        })
        .unwrap();
    let resized = restored.export_snapshot_v2().unwrap();
    assert_eq!(
        Terminal::import_snapshot_v2(&resized)
            .unwrap()
            .export_snapshot_v2()
            .unwrap(),
        resized
    );
    output(&mut restored, b"\x1bc");
    assert!(!restored.pages.has_state());
    assert_eq!(restored.active_page(), 1);
}

#[test]
fn cross_page_copy_preserves_wide_cells_links_and_destination_cursor() {
    let mut t = Terminal::new(12, 4).unwrap();
    output(
        &mut t,
        "\x1b]8;;https://example.invalid/\x1b\\中A\x1b]8;;\x1b\\\x1b[2 P\x1b[3;5H".as_bytes(),
    );
    output(&mut t, b"\x1b[1;1;1;3;1;2;2;2$v");
    assert_eq!(t.cursor(), (2, 4));
    assert_eq!(t.cell(1, 1).unwrap().character, '中');
    assert!(t.cell(1, 2).unwrap().continuation);
    assert!(t.cell(1, 1).unwrap().hyperlink.is_some());
    output(&mut t, b"\x1b[2;2;2;4;2;1;1;4$v\x1b[4 P");
    assert_eq!(t.cell(0, 0).unwrap().character, '中');
    let snapshot = t.export_snapshot_v2().unwrap();
    assert_eq!(
        Terminal::import_snapshot_v2(&snapshot)
            .unwrap()
            .export_snapshot_v2()
            .unwrap(),
        snapshot
    );
    let replies = output(&mut t, b"\x1b[1$w");
    assert!(replies[0].starts_with(b"\x1bP1$u3;5;4;"));
    let restore = replies[0].windows(2).position(|p| p == b"$u").unwrap();
    let mut command = replies[0].clone();
    command[restore + 1] = b't';
    output(&mut t, b"\x1b[1 P");
    output(&mut t, &command);
    assert_eq!(t.active_page(), 4);
}

#[test]
fn page_storage_and_resize_limits_reject_without_partial_changes() {
    let mut t = Terminal::new(256, 128).unwrap();
    output(&mut t, b"\x1b[2 P");
    let snapshot = t.export_snapshot_v2().unwrap();
    assert!(matches!(
        t.apply_event(Event::Output {
            source_sequence: t.next_source_sequence(),
            bytes: b"\x1b[3 P"
        }),
        Err(TerminalError::ProtocolResourceLimit(_))
    ));
    assert_eq!(t.export_snapshot_v2().unwrap(), snapshot);
    assert!(matches!(
        t.apply_event(Event::Resize {
            source_sequence: t.next_source_sequence(),
            columns: 257,
            rows: 128
        }),
        Err(TerminalError::ProtocolResourceLimit(_))
    ));
    assert_eq!(t.export_snapshot_v2().unwrap(), snapshot);
    output(&mut t, b"OK");
    assert_eq!(t.cell(0, 0).unwrap().character, 'O');
}

#[test]
fn copying_into_a_background_double_width_line_crops_hidden_columns() {
    let mut t = Terminal::new(12, 4).unwrap();
    output(&mut t, b"ABCDEFGHIJKL\x1b[2 P\x1b[H\x1b#6\x1b[1 P");
    output(&mut t, b"\x1b[1;1;1;12;1;1;1;2$v");
    let snapshot = t.export_snapshot_v2().unwrap();
    let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
    output(&mut restored, b"\x1b[2 P");
    assert_eq!(restored.cell(0, 5).unwrap().character, 'F');
    assert_eq!(restored.cell(0, 6).unwrap().character, ' ');
}

#[test]
fn checksums_address_each_page_and_do_not_allocate_blank_pages() {
    let mut t = Terminal::new(6, 3).unwrap();
    output(&mut t, b"AB\x1b[2 P\x1b[HCD");
    assert_eq!(
        output(&mut t, b"\x1b[1;1;1;1;1;2*y"),
        [b"\x1bP1!~FE9D\x1b\\".to_vec()]
    );
    assert_eq!(
        output(&mut t, b"\x1b[1;2;1;1;1;2*y"),
        [b"\x1bP1!~FE99\x1b\\".to_vec()]
    );
    let count = t.pages.screens().count();
    assert_eq!(
        output(&mut t, b"\x1b[1;3;1;1;1;2*y"),
        [b"\x1bP1!~FEE0\x1b\\".to_vec()]
    );
    assert_eq!(t.pages.screens().count(), count);
    assert_eq!(output(&mut t, b"\x1b[?6n"), [b"\x1b[?1;3;2R".to_vec()]);
}

#[test]
fn page_edits_and_visibility_remain_snapshot_closed_through_seeded_resizes() {
    let mut t = Terminal::new_with_options(
        20,
        8,
        0,
        crate::TerminalOptions {
            history_capacity_rows: 40,
        },
    )
    .unwrap();
    let mut seed = 0x72e5c1a3_u32;
    let commands = [
        "\x1b[2 P",
        "\x1b[6 P",
        "\x1b[1 P",
        "\x1b[?64l",
        "\x1b[?64h",
        "\x1b7",
        "\x1b8",
        "中e\u{301}\r\n",
        "\x1b#6界AB\r\n",
        "\x1b#3HEAD\r\n\x1b#4HEAD\r\n",
        "\x1b[1;1;2;9;2;1;1;6$v",
        "\x1b[1;1;3;8;6;1;2;1$v",
        "\x1b[?1049hALT",
        "\x1b[?1049l",
        "\x1b[2J",
        "\x1b[2;5r",
        "\x1b[r",
        "\x1b[!p",
        "\x1bc",
    ];
    for step in 0..1000 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        if step % 7 == 0 {
            t.apply_event(Event::Resize {
                source_sequence: t.next_source_sequence(),
                columns: 12 + seed % 20,
                rows: 6 + seed % 5,
            })
            .unwrap();
        } else {
            output(&mut t, commands[seed as usize % commands.len()].as_bytes());
        }
        let snapshot = t
            .export_snapshot_v2()
            .unwrap_or_else(|e| panic!("step {step}: {e:?}"));
        let restored = Terminal::import_snapshot_v2(&snapshot)
            .unwrap_or_else(|e| panic!("import step {step}: {e:?}"));
        assert_eq!(
            restored.export_snapshot_v2().unwrap(),
            snapshot,
            "step {step}"
        );
        assert_eq!(
            restored.export_render_frame_v4().unwrap(),
            t.export_render_frame_v4().unwrap(),
            "step {step}"
        );
    }
}
