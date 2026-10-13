use crate::{Event, InputEvent, Key, KeyEvent, KeypadKey, Modifiers, Terminal, TerminalError};

fn feed(terminal: &mut Terminal, bytes: &[u8]) -> Vec<u8> {
    let prepared = terminal
        .prepare_event_with_host_effects_v2(Event::Output {
            source_sequence: terminal.next_source_sequence(),
            bytes,
        })
        .unwrap();
    let result = prepared.host_effect_frame_v2().to_vec();
    prepared.commit_with(|_| Ok::<(), ()>(())).unwrap();
    result
}
fn key(terminal: &Terminal, key: Key) -> Vec<u8> {
    terminal
        .encode_input(InputEvent::Key(KeyEvent {
            key,
            modifiers: Modifiers::NONE,
        }))
        .unwrap()
}
fn restore(terminal: &Terminal) -> Terminal {
    let snapshot = terminal.export_snapshot_v2().unwrap();
    let copy = Terminal::import_snapshot_v2(&snapshot).unwrap();
    assert_eq!(&copy, terminal);
    copy
}

#[test]
fn vt52_chunked_output_input_and_restore_are_consistent() {
    let output = b"\x1b[?2l\x1bHABC\x1bD!\x1bY\x22\x25\x1bFq\x1bGX\x1b=";
    for split in 0..=output.len() {
        let mut terminal = Terminal::new(20, 6).unwrap();
        feed(&mut terminal, &output[..split]);
        feed(&mut terminal, &output[split..]);
        assert_eq!(terminal.cell(0, 2).unwrap().character, '!');
        assert_eq!(terminal.cell(2, 5).unwrap().character, '─');
        assert_eq!(terminal.cell(2, 6).unwrap().character, 'X');
        assert_eq!(key(&terminal, Key::ArrowUp), b"\x1bA");
        assert_eq!(key(&terminal, Key::F3), b"\x1bR");
        assert_eq!(key(&terminal, Key::Keypad(KeypadKey::Digit2)), b"\x1b?r");
        let mut copy = restore(&terminal);
        let identity = feed(&mut terminal, b"\x1bZ");
        assert!(identity.windows(3).any(|s| s == b"\x1b/Z"));
        assert_eq!(identity, feed(&mut copy, b"\x1bZ"));
        feed(&mut terminal, b"\x1b>\x1b<\x1b[?2$p");
        assert_eq!(key(&terminal, Key::ArrowUp), b"\x1b[A");
        restore(&terminal);
    }
}

#[test]
fn vt52_pending_address_cancellation_and_failure_do_not_commit() {
    let mut terminal = Terminal::new(20, 6).unwrap();
    feed(&mut terminal, b"\x1b[?2l\x1bY ");
    assert_eq!(
        terminal.export_snapshot_v2(),
        Err(TerminalError::SnapshotNotEligible)
    );
    feed(&mut terminal, b"\x18OK\x1bY!\x1bH@");
    assert_eq!(terminal.cell(0, 0).unwrap().character, '@');
    restore(&terminal);
    let before = terminal.main.clone();
    let sequence = terminal.next_source_sequence();
    let result = terminal.prepare_event_with_host_effects_v2(Event::Output {
        source_sequence: sequence,
        bytes: b"bad\x1b[",
    });
    assert!(matches!(
        result,
        Err(TerminalError::Unsupported("VT52 escape command"))
    ));
    assert_eq!(terminal.main, before);
    assert_eq!(terminal.next_source_sequence(), sequence);
}

#[test]
fn docs_c1_and_gr_are_explicit_and_survive_snapshots() {
    let output = b"\x1b%@\xe9\xa3\x1b)0\x1b~\xf1\x1b*0\x8e\xf8\x9b2;1H\x1b%G\xc3\xa9";
    for split in 0..=output.len() {
        let mut terminal = Terminal::new(20, 6).unwrap();
        feed(&mut terminal, &output[..split]);
        feed(&mut terminal, &output[split..]);
        assert_eq!(terminal.cell(0, 0).unwrap().character, 'é');
        assert_eq!(terminal.cell(0, 1).unwrap().character, '£');
        assert_eq!(terminal.cell(0, 2).unwrap().character, '─');
        assert_eq!(terminal.cell(0, 3).unwrap().character, '│');
        assert_eq!(terminal.cell(1, 0).unwrap().character, 'é');
        let mut copy = restore(&terminal);
        feed(&mut terminal, b"\x1b%@\xf1\x1b%G");
        feed(&mut copy, b"\x1b%@\xf1\x1b%G");
        assert_eq!(terminal, copy);
        feed(&mut terminal, b"\x1bc");
        assert_eq!(terminal.modes.receive_c1, 0);
        assert!(!terminal.modes.iso2022);
        restore(&terminal);
    }
}

#[test]
fn c1_reply_and_semantic_input_preserve_literal_escape_text() {
    let mut terminal = Terminal::new(20, 6).unwrap();
    feed(&mut terminal, b"\x1b G");
    let mut copy = restore(&terminal);
    let queries = b"\x1b[5n\x1b[=c\x1b]17;#123456\x1b\\\x1b]17;?\x1b\\";
    let frame = feed(&mut terminal, queries);
    assert_eq!(frame, feed(&mut copy, queries));
    for reply in [
        b"\x9b0n".as_slice(),
        b"\x90!|00000000\x9c",
        b"\x9d17;rgb:1212/3434/5656\x9c",
    ] {
        assert!(frame.windows(reply.len()).any(|bytes| bytes == reply));
    }
    assert_eq!(key(&terminal, Key::ArrowUp), b"\x9bA");
    let literal = terminal
        .encode_input(InputEvent::Key(KeyEvent {
            key: Key::Character('['),
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        }))
        .unwrap();
    assert_eq!(literal, b"\x1b[");
    feed(&mut terminal, b"\x1b F");
    assert_eq!(key(&terminal, Key::ArrowUp), b"\x1b[A");
    feed(&mut terminal, b"\x1b[?2l");
    assert_eq!(key(&terminal, Key::ArrowUp), b"\x1bA");
}

#[test]
fn decac1_retains_utf8_boundaries_and_docs_can_disable_it() {
    let mut terminal = Terminal::new(20, 6).unwrap();
    feed(&mut terminal, b"\x1b 7\xd0\x9b\x9b2;3HX");
    assert_eq!(terminal.cell(0, 0).unwrap().character, 'Л');
    assert_eq!(terminal.cell(1, 2).unwrap().character, 'X');
    restore(&terminal);
    feed(&mut terminal, b"\x1b%G\x9b2;3H");
    assert_eq!(terminal.cell(1, 3).unwrap().character, '\u{fffd}');
    restore(&terminal);
}

#[test]
fn upss_assignment_is_dynamic_and_survives_saved_cursor_and_restore() {
    let mut terminal = Terminal::new(20, 6).unwrap();
    let reply = feed(&mut terminal, b"\x1b[&u");
    assert!(reply.windows(8).any(|bytes| bytes == b"\x1bP1!uA\x1b\\"));
    let output = b"\x1b%@\x1b.<\x1bP1!uB\x1b\\\xa1\x1b7\x1bP1!uL\x1b\\\xb0\x1b8\xb0";
    for split in 0..=output.len() {
        let mut value = Terminal::new(20, 6).unwrap();
        feed(&mut value, &output[..split]);
        feed(&mut value, &output[split..]);
        assert_eq!(value.cell(0, 0).unwrap().character, 'Ą');
        assert_eq!(value.cell(0, 1).unwrap().character, 'А');
        let mut restored = restore(&value);
        let suffix = b"\x1bP1!uB\x1b\\\xa1\x1b[&u";
        assert_eq!(feed(&mut value, suffix), feed(&mut restored, suffix));
        assert_eq!(value, restored);
    }
}

#[test]
fn invalid_upss_assignment_rolls_back_and_reports_real_cause() {
    for (bytes, reason) in [
        (b"\x1bP2!uB\x1b\\".as_slice(), "DECAUPSS charset size"),
        (b"\x1bP1!u<\x1b\\", "recursive DECAUPSS designation"),
        (b"\x1bP1!uBAD\x1b\\", "DECAUPSS charset designation"),
    ] {
        let mut terminal = Terminal::new(20, 6).unwrap();
        let before = terminal.user_preferred_charset;
        let result = terminal.prepare_event_with_host_effects_v2(Event::Output {
            source_sequence: 0,
            bytes,
        });
        assert!(matches!(result, Err(TerminalError::Unsupported(value)) if value == reason));
        assert_eq!(terminal.user_preferred_charset, before);
        assert_eq!(terminal.next_source_sequence(), 0);
    }
}

#[test]
fn extended_designations_use_frozen_mapping_and_restore_every_register() {
    // Oracle points from the fixed mapping source recorded in third_party/legacy-charsets.
    for (id, size, byte, expected) in [
        ("B", 96, 0x21, 'Ą'),
        ("L", 96, 0x21, 'Ё'),
        ("F", 96, 0x21, '‘'),
        ("H", 96, 0x2a, '×'),
        ("M", 96, 0x50, 'Ğ'),
        ("%5", 94, 0x28, '¤'),
        ("&4", 94, 0x40, 'ю'),
        ("\"?", 94, 0x28, '¤'),
        ("\"4", 94, 0x28, '¤'),
        ("%0", 94, 0x28, '¤'),
        ("f", 94, 0x23, '£'),
        ("`", 94, 0x5b, 'Æ'),
        ("\">", 94, 0x40, 'ϊ'),
        ("%=", 94, 0x60, 'א'),
        ("&5", 94, 0x60, 'Ю'),
        ("%2", 94, 0x21, 'ı'),
    ] {
        for register in 0..4 {
            if size == 96 && register == 0 {
                continue;
            }
            let selector = if size == 96 {
                b" -./"[register]
            } else {
                b"()*+"[register]
            };
            let mut program = b"\x1b%@\x1b[?42h\x1b".to_vec();
            program.push(selector);
            program.extend_from_slice(id.as_bytes());
            program.extend_from_slice(match register {
                0 => b"\x0f",
                1 => b"\x0e",
                2 => b"\x1bn",
                _ => b"\x1bo",
            });
            program.push(byte);
            for split in 0..=program.len() {
                let mut terminal = Terminal::new(20, 6).unwrap();
                feed(&mut terminal, &program[..split]);
                feed(&mut terminal, &program[split..]);
                assert_eq!(
                    terminal.cell(0, 0).unwrap().character,
                    expected,
                    "{id} G{register}"
                );
                let mut restored = restore(&terminal);
                let suffix = b"\x1b7\x1b(B\x0fZ\x1b8";
                feed(&mut terminal, suffix);
                feed(&mut restored, suffix);
                feed(&mut terminal, &[byte]);
                feed(&mut restored, &[byte]);
                assert_eq!(terminal, restored);
                restore(&terminal);
            }
        }
    }
}

#[test]
fn gl_96_maps_del_but_94_keeps_space_and_single_shift() {
    let mut terminal = Terminal::new(20, 6).unwrap();
    feed(&mut terminal, b"\x1b-A\x0e\x7f\x1b*0\x1bN\x7fq");
    assert_eq!(terminal.cell(0, 0).unwrap().character, 'ÿ');
    assert_eq!(terminal.cell(0, 1).unwrap().character, 'ÿ');
    assert_eq!(terminal.cell(0, 2).unwrap().character, '─');
    feed(&mut terminal, b"\x1b)%5\x0e ");
    assert_eq!(terminal.cell(0, 3).unwrap().character, ' ');
    restore(&terminal);
}

#[test]
fn gr_boundary_uses_active_96_set_without_consuming_a_94_single_shift() {
    let bytes = b"\x1b%@\x1b-B\x1b~\x1b*0\x1bN\xff\xf1";
    for split in 0..=bytes.len() {
        let mut terminal = Terminal::new(20, 6).unwrap();
        feed(&mut terminal, &bytes[..split]);
        feed(&mut terminal, &bytes[split..]);
        assert_eq!(terminal.cell(0, 0).unwrap().character, '˙');
        assert_eq!(terminal.cell(0, 1).unwrap().character, '─');
        restore(&terminal);
    }
}

#[test]
fn vt52_address_del_reaches_last_encodable_coordinate_and_docs_titles_are_latin1() {
    let mut terminal = Terminal::new(100, 100).unwrap();
    feed(&mut terminal, b"\x1b[?2l\x1bY\x7f\x7fX\x1b<");
    assert_eq!(terminal.cell(95, 95).unwrap().character, 'X');
    let bytes = b"\x1b%@\x1b-B\x0e\x1b]2;caf\xe9\x1b\\\x1b%G";
    for split in 0..=bytes.len() {
        let mut t = Terminal::new(20, 6).unwrap();
        feed(&mut t, &bytes[..split]);
        feed(&mut t, &bytes[split..]);
        assert_eq!(t.osc.window_title(), "café");
        restore(&t);
    }
}

#[test]
fn upss_diagnostics_preserve_exact_identifier_and_numeric_spelling() {
    for (bytes, token) in [
        (
            b"\x1bP0002!uB\x1b\\".as_slice(),
            "rejected_parameter=0002\n",
        ),
        (b"\x1bP1!uBAD\x1b\\", "sequence=<ESC>P1!uBAD<ESC>\\\\\n"),
    ] {
        for split in 0..bytes.len() {
            let mut terminal = Terminal::new(20, 6).unwrap();
            feed(&mut terminal, &bytes[..split]);
            assert!(
                terminal
                    .apply_event(Event::Output {
                        source_sequence: 1,
                        bytes: &bytes[split..]
                    })
                    .is_err()
            );
            let text = terminal.failure_diagnostic_v2().unwrap();
            assert!(text.contains(token), "{text}");
            assert!(text.contains("hidden_bytes=0\n"), "{text}");
            assert_eq!(terminal.next_source_sequence(), 1);
        }
    }
    let mut terminal = Terminal::new(20, 6).unwrap();
    assert!(
        terminal
            .apply_event(Event::Output {
                source_sequence: 0,
                bytes: b"\x1bPqPRIVATE-CONTENT\x1b\\"
            })
            .is_err()
    );
    let text = terminal.failure_diagnostic_v2().unwrap();
    assert!(!text.contains("PRIVATE"));
    assert!(text.contains("hidden_bytes=17\n"), "{text}");
}
