use crate::{Event, Terminal, TerminalError, tests::host_effect_payloads};

fn feed(t: &mut Terminal, bytes: &[u8]) -> Vec<Vec<u8>> {
    let sequence = t.next_source_sequence();
    let prepared = t
        .prepare_event_with_host_effects_v2(Event::Output {
            source_sequence: sequence,
            bytes,
        })
        .unwrap();
    let replies = host_effect_payloads(prepared.host_effect_frame_v2())
        .into_iter()
        .map(<[u8]>::to_vec)
        .collect();
    prepared.commit_with(|_| Ok::<(), ()>(())).unwrap();
    replies
}
fn definition(id: u8, bytes: &[u8]) -> Vec<u8> {
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!("\x1bP{id};0;1!z{hex}\x1b\\").into_bytes()
}
fn row(t: &Terminal) -> String {
    (0..t.columns)
        .map(|col| t.cell(0, col).unwrap().character)
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn macro_text_hex_repeats_and_reports_restore_at_every_split() {
    let bytes = b"\x1bP7;1;1!z!3;4142;\x1b\\\x1b[7*z\x1bP1;0;0!zXYZ\x1b\\";
    for split in 0..=bytes.len() {
        let mut t = Terminal::new(30, 4).unwrap();
        feed(&mut t, &bytes[..split]);
        feed(&mut t, &bytes[split..]);
        assert_eq!(row(&t), "ABABAB");
        let snapshot = t.export_snapshot_v2().unwrap();
        assert_eq!(u16::from_le_bytes(snapshot[10..12].try_into().unwrap()), 16);
        let mut copy = Terminal::import_snapshot_v2(&snapshot).unwrap();
        assert_eq!(copy, t);
        let suffix = b"\x1b[1*z\x1b[?62n\x1b[?63;42n";
        let replies = feed(&mut t, suffix);
        assert_eq!(replies, feed(&mut copy, suffix));
        assert_eq!(row(&t), "ABABABXYZ");
        assert_eq!(replies[0], b"\x1b[16383*{");
        let checksum = b"ABABABXYZ"
            .iter()
            .fold(0u16, |v, b| v.wrapping_sub(u16::from(*b)));
        assert_eq!(
            replies[1],
            format!("\x1bP42!~{checksum:04X}\x1b\\").as_bytes()
        );
        assert_eq!(
            copy.export_snapshot_v2().unwrap(),
            t.export_snapshot_v2().unwrap()
        );
    }
}

#[test]
fn recursive_macros_fail_atomically_without_publishing_replies() {
    let mut t = Terminal::new(20, 4).unwrap();
    feed(&mut t, &definition(0, b"\x1b[5nX\x1b[0*z"));
    let before = t.export_snapshot_v2().unwrap();
    assert!(matches!(
        t.prepare_event_with_host_effects_v2(Event::Output {
            source_sequence: 1,
            bytes: b"\x1b[0*z"
        }),
        Err(TerminalError::ProtocolResourceLimit(
            "DECINVM nesting exceeds 16 invocations"
        ))
    ));
    assert_eq!(before, t.export_snapshot_v2().unwrap());
    let text = t.failure_diagnostic_v2().unwrap();
    assert!(text.contains("category=limit_exceeded\n") && text.contains("sequence=<ESC>[0*z\n"));
    assert_eq!(t.next_source_sequence(), 1);
}

#[test]
fn macro_event_budget_cannot_be_reset_by_multiple_invocations() {
    let mut t = Terminal::new(20, 4).unwrap();
    feed(&mut t, b"\x1bP0;0;1!z!65534;00;\x1b\\");
    let before = t.export_snapshot_v2().unwrap();
    let bytes = b"\x1b[0*z".repeat(17);
    assert!(matches!(
        t.apply_event(Event::Output {
            source_sequence: 1,
            bytes: &bytes
        }),
        Err(TerminalError::ProtocolResourceLimit(
            "DECINVM expansion exceeds 1048576 bytes per source event"
        ))
    ));
    assert_eq!(before, t.export_snapshot_v2().unwrap());
    feed(&mut t, b"OK");
    assert_eq!(row(&t), "OK");
}

#[test]
fn macro_replacement_deletion_reset_and_cancellation_have_no_partial_state() {
    let mut t = Terminal::new(30, 4).unwrap();
    feed(&mut t, b"\x1bP0!zOLD\x1b\\");
    for abort in [b"\x18".as_slice(), b"\x1a", b"\x1b[0m", b"\x1b\x1b[0m"] {
        feed(&mut t, b"\x1bP0!zCANCELLED");
        assert_eq!(
            t.export_snapshot_v2(),
            Err(TerminalError::SnapshotNotEligible)
        );
        feed(&mut t, abort);
        assert_eq!(t.macros.entries().next().unwrap().1, b"OLD");
    }
    feed(&mut t, b"\x1bP0!zNEW\x1b\\\x1b[0*z");
    assert_eq!(row(&t), "NEW");
    feed(&mut t, b"\x1bP0!z\x1b\\");
    assert!(t.macros.is_empty());
    feed(&mut t, &definition(1, b"\x1bcBAD"));
    feed(&mut t, &definition(0, b"BEFORE\x1b[1*zAFTER"));
    feed(&mut t, b"\x1b[0*zGOOD");
    assert_eq!(row(&t), "GOOD");
    assert!(t.macros.is_empty());
    assert_eq!(
        Terminal::import_snapshot_v2(&t.export_snapshot_v2().unwrap()).unwrap(),
        t
    );
}

#[test]
fn malformed_and_oversized_macros_keep_existing_definition_and_hide_payload() {
    for bytes in [
        b"\x1bP0;1;1!z41X\x1b\\".as_slice(),
        b"\x1bP64!zSECRET\x1b\\",
        b"\x1bP0;2!zSECRET\x1b\\",
        b"\x1bP0;1;2!zSECRET\x1b\\",
        b"\x1bP0;1;1!z!65534;4142434445;\x1b\\",
    ] {
        let mut t = Terminal::new(20, 4).unwrap();
        feed(&mut t, b"\x1bP0!zORIGINAL\x1b\\");
        assert!(
            t.apply_event(Event::Output {
                source_sequence: 1,
                bytes
            })
            .is_err()
        );
        assert_eq!(t.macros.entries().next().unwrap().1, b"ORIGINAL");
        assert_eq!(t.next_source_sequence(), 1);
        let text = t.failure_diagnostic_v2().unwrap();
        assert!(!text.contains("SECRET"));
    }
}

#[test]
fn macro_can_span_parser_sequences_but_cannot_redefine_macro_storage() {
    let mut t = Terminal::new(20, 4).unwrap();
    feed(&mut t, &definition(0, b"\x1b[31"));
    feed(&mut t, b"\x1b[0*z");
    assert_eq!(
        t.export_snapshot_v2(),
        Err(TerminalError::SnapshotNotEligible)
    );
    feed(&mut t, b"mR");
    assert_eq!(
        t.cell(0, 0).unwrap().attributes.foreground,
        crate::Color::Indexed(1)
    );
    feed(&mut t, &definition(0, b"\x1bP1!zNEW\x1b\\"));
    assert!(matches!(
        t.apply_event(Event::Output {
            source_sequence: 4,
            bytes: b"\x1b[0*z"
        }),
        Err(TerminalError::Unsupported(
            "DECDMAC definition inside macro invocation"
        ))
    ));
    assert_eq!(t.macros.entries().count(), 1);
}

#[test]
fn expanded_diagnostic_labels_macro_bytes_separately_from_physical_invocation() {
    let mut t = Terminal::new(20, 4).unwrap();
    feed(&mut t, &definition(5, b"OK\x1b[31;999m"));
    assert!(
        t.apply_event(Event::Output {
            source_sequence: 1,
            bytes: b"x\x1b[5*z"
        })
        .is_err()
    );
    let v2 = t.failure_diagnostic_v2().unwrap();
    assert!(
        v2.contains("sequence=<ESC>[5*z\n")
            && v2.contains("rejected_parameter_index=unavailable\n")
    );
    let v3 = t.failure_diagnostic_v3().unwrap();
    assert!(v3.starts_with("TL-DIAG/3\n"));
    assert!(
        v3.contains("expanded_source=macro\nexpanded_category=unsupported\n"),
        "{v3}"
    );
    assert!(
        v3.contains("expanded_sequence=<ESC>[31;999m\n")
            && v3.contains("expanded_start=5:2\nexpanded_end=5:11\n"),
        "{v3}"
    );
    assert!(v3.contains("expanded_rejected_parameter_index=1\nexpanded_rejected_parameter=999\n"));
    assert!(v3.contains("start=1:1\nend=1:6\n"));
    assert!(
        t.apply_event(Event::Output {
            source_sequence: 1,
            bytes: b"later"
        })
        .is_err()
    );
    assert_eq!(v3, t.failure_diagnostic_v3().unwrap());

    let mut private = Terminal::new(20, 4).unwrap();
    feed(&mut private, &definition(3, b"\x1b]10;PRIVATE\x1b\\"));
    assert!(
        private
            .apply_event(Event::Output {
                source_sequence: 1,
                bytes: b"\x1b[3*z"
            })
            .is_err()
    );
    let v3 = private.failure_diagnostic_v3().unwrap();
    assert!(!v3.contains("PRIVATE") && !v3.contains("50 52 49"));
    assert!(v3.contains("expanded_last_byte=redacted\n"));
}

#[test]
fn invalid_macro_hex_reports_only_the_rejected_token() {
    for split in 0..14 {
        let bytes = b"\x1bP0;0;1!z41X\x1b\\";
        let mut t = Terminal::new(20, 4).unwrap();
        feed(&mut t, &bytes[..split]);
        assert!(
            t.apply_event(Event::Output {
                source_sequence: 1,
                bytes: &bytes[split..]
            })
            .is_err()
        );
        let text = t.failure_diagnostic_v2().unwrap();
        assert!(
            text.contains("retained payload byte 2: 0x58 U+0058 'X'\n"),
            "{text}"
        );
        assert!(!text.contains("41X"));
        assert_eq!(t.next_source_sequence(), 1);
    }
}

#[test]
fn macro_continuations_keep_exact_mixed_source_evidence() {
    for split in 0..=3 {
        let mut t = Terminal::new(20, 4).unwrap();
        feed(&mut t, &definition(5, b"OK\x1b[9"));
        feed(&mut t, b"\x1b[5*z");
        feed(&mut t, &b"99m"[..split.min(2)]);
        assert!(
            t.apply_event(Event::Output {
                source_sequence: 3,
                bytes: &b"99m"[split.min(2)..]
            })
            .is_err()
        );
        let diagnostic = t.failure_diagnostic_v3().unwrap();
        assert!(
            diagnostic.contains("expanded_source=mixed\n"),
            "{diagnostic}"
        );
        assert!(
            diagnostic.contains("expanded_sequence=<ESC>[999m\n"),
            "{diagnostic}"
        );
        assert!(diagnostic.contains("expanded_start=0:0\nexpanded_end=0:6\n"));
        assert!(diagnostic.contains("expanded_origins=macro,5,2,3;output,"));
        assert!(diagnostic.contains("expanded_rejected_parameter=999\n"));
        assert!(!diagnostic.contains("sequence=<ESC>[5*z99m\nrejected_parameter_index=0"));
    }
    let mut nested = Terminal::new(20, 4).unwrap();
    feed(&mut nested, &definition(5, b"\x1b[9"));
    feed(&mut nested, &definition(1, b"\x1b[5*z99m"));
    assert!(
        nested
            .apply_event(Event::Output {
                source_sequence: 2,
                bytes: b"\x1b[1*z"
            })
            .is_err()
    );
    let text = nested.failure_diagnostic_v3().unwrap();
    assert!(
        text.contains("expanded_origins=macro,5,0,3;macro,1,5,3\n"),
        "{text}"
    );
    assert!(text.contains("expanded_sequence=<ESC>[999m\n"));
}

#[test]
fn mixed_macro_evidence_is_bounded_redacted_and_does_not_survive_cancellation() {
    let mut t = Terminal::new(20, 4).unwrap();
    feed(&mut t, &definition(0, b"\x1b]10;SECRET"));
    feed(&mut t, b"\x1b[0*z");
    assert!(
        t.apply_event(Event::Output {
            source_sequence: 2,
            bytes: b"PRIVATE\x1b\\"
        })
        .is_err()
    );
    let text = t.failure_diagnostic_v3().unwrap();
    assert!(text.contains("expanded_source=mixed\n"));
    assert!(!text.contains("SECRET") && !text.contains("PRIVATE") && !text.contains("50 52 49"));

    let mut t = Terminal::new(20, 4).unwrap();
    feed(&mut t, &definition(0, b"\x1b[999"));
    feed(&mut t, b"\x1b[0*z");
    for _ in 0..24 {
        feed(&mut t, b"\0");
    }
    assert!(
        t.apply_event(Event::Output {
            source_sequence: 26,
            bytes: b"m"
        })
        .is_err()
    );
    let text = t.failure_diagnostic_v3().unwrap();
    assert!(text.len() <= crate::diagnostic::MAX_DIAGNOSTIC_V3_BYTES);
    assert!(
        text.contains("expanded_origin_omitted_bytes=10\n"),
        "{text}"
    );
    let mut t = Terminal::new(20, 4).unwrap();
    feed(&mut t, &definition(0, b"\x1b[999"));
    feed(&mut t, b"\x1b[0*z");
    feed(&mut t, b"\x18OK");
    assert!(
        t.apply_event(Event::Output {
            source_sequence: 3,
            bytes: b"\x1b[888m"
        })
        .is_err()
    );
    assert!(
        t.failure_diagnostic_v3()
            .unwrap()
            .starts_with("TL-DIAG/2\n")
    );
}

#[test]
fn macro_batch_budget_limits_many_small_physical_events_atomically() {
    let mut t = Terminal::new(20, 4).unwrap();
    feed(&mut t, b"\x1bP0;0;1!z!65534;00;\x1b\\");
    let before = t.export_snapshot_v2().unwrap();
    let bytes = b"\x1b[0*z".repeat(16);
    let events = (1..=5)
        .map(|source_sequence| Event::Output {
            source_sequence,
            bytes: &bytes,
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        t.apply_events_atomic(&events),
        Err(TerminalError::ProtocolResourceLimit(
            "DECINVM expansion exceeds 4194304 bytes per transaction"
        ))
    ));
    assert_eq!(before, t.export_snapshot_v2().unwrap());
    feed(&mut t, b"OK");
    assert_eq!(row(&t), "OK");
}
