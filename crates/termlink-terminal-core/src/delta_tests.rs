use crate::{
    DeltaApply, DeltaCheckpoint, DeltaContext, DeltaError, DeltaJournal, DeltaJournalConfig, Event,
    STATE_DELTA_PROFILE_STANDALONE_C1_V1, STATE_DELTA_PROFILE_UNICODE16_STANDALONE_C1_V1,
    STATE_DELTA_PROFILE_UNICODE16_V1, STATE_DELTA_PROFILE_V1, Terminal, apply_delta_v1,
    export_delta_v1,
};

fn context() -> DeltaContext {
    DeltaContext {
        id: [7; 32],
        profile: STATE_DELTA_PROFILE_V1,
    }
}

fn output(terminal: &mut Terminal, bytes: &[u8]) {
    terminal
        .apply_event(Event::Output {
            source_sequence: terminal.next_source_sequence(),
            bytes,
        })
        .unwrap();
}

fn pair() -> (DeltaCheckpoint, DeltaCheckpoint, Vec<u8>) {
    let mut terminal = Terminal::new(120, 80).unwrap();
    terminal.graphics = Some(crate::graphics_state::GraphicsState::default());
    for row in 0..70 {
        let line = format!("{row:03}: {}\r\n", "term-link canonical state ".repeat(4));
        output(&mut terminal, line.as_bytes());
    }
    let base = DeltaCheckpoint::capture(context(), &terminal).unwrap();
    output(&mut terminal, b"one more line\r\n");
    let target = DeltaCheckpoint::capture(context(), &terminal).unwrap();
    let expected = terminal.export_snapshot_v2().unwrap();
    (base, target, expected)
}

#[test]
fn unicode16_profile_is_separate_and_restores_canonical_state() {
    let mut terminal = Terminal::new_with_graphics_profile(
        120,
        80,
        0,
        crate::TerminalOptions {
            history_capacity_rows: 300,
        },
        crate::OutputProfile::Utf8,
        crate::WidthProfile::Unicode16,
    )
    .unwrap();
    let text = (0..250)
        .map(|row| format!("{row:03}: Unicode 16 👩🏽‍💻 🚀\r\n"))
        .collect::<String>();
    output(&mut terminal, text.as_bytes());
    let unicode_context = DeltaContext {
        id: [7; 32],
        profile: STATE_DELTA_PROFILE_UNICODE16_V1,
    };
    assert!(matches!(
        DeltaCheckpoint::capture(context(), &terminal),
        Err(DeltaError::Incompatible)
    ));
    let base = DeltaCheckpoint::capture(unicode_context, &terminal).unwrap();
    output(&mut terminal, b"Unicode suffix\r\n");
    let target = DeltaCheckpoint::capture(unicode_context, &terminal).unwrap();
    let expected = terminal.export_snapshot_v2().unwrap();
    let delta = export_delta_v1(&base, &target, usize::MAX).unwrap();
    let DeltaApply::Applied { terminal, .. } = apply_delta_v1(&base, &delta, usize::MAX).unwrap()
    else {
        panic!("expected detached Unicode16 state");
    };
    assert_eq!(terminal.export_snapshot_v2().unwrap(), expected);
    assert!(matches!(
        DeltaCheckpoint::from_snapshot(context(), &expected),
        Err(DeltaError::Incompatible)
    ));
}

#[test]
fn standalone_c1_profiles_are_distinct_and_restore_state() {
    for (profile, width) in [
        (
            STATE_DELTA_PROFILE_STANDALONE_C1_V1,
            crate::WidthProfile::LegacyV1,
        ),
        (
            STATE_DELTA_PROFILE_UNICODE16_STANDALONE_C1_V1,
            crate::WidthProfile::Unicode16,
        ),
    ] {
        let mut terminal = Terminal::new_with_graphics_profile(
            120,
            80,
            0,
            crate::TerminalOptions {
                history_capacity_rows: 300,
            },
            crate::OutputProfile::Utf8WithStandaloneC1,
            width,
        )
        .unwrap();
        let lines = (0..250)
            .map(|row| format!("{row:03}: {}\r\n", "standalone C1 state ".repeat(5)))
            .collect::<String>();
        output(&mut terminal, lines.as_bytes());
        let context = DeltaContext {
            id: [7; 32],
            profile,
        };
        let wrong = DeltaContext {
            profile: if width == crate::WidthProfile::Unicode16 {
                STATE_DELTA_PROFILE_UNICODE16_V1
            } else {
                STATE_DELTA_PROFILE_V1
            },
            ..context
        };
        assert!(matches!(
            DeltaCheckpoint::capture(wrong, &terminal),
            Err(DeltaError::Incompatible)
        ));
        let base = DeltaCheckpoint::capture(context, &terminal).unwrap();
        output(&mut terminal, b"\x9b31mC1 red\x9b0m\r\n");
        let target = DeltaCheckpoint::capture(context, &terminal).unwrap();
        let expected = terminal.export_snapshot_v2().unwrap();
        let delta = export_delta_v1(&base, &target, usize::MAX).unwrap();
        let DeltaApply::Applied { terminal, .. } =
            apply_delta_v1(&base, &delta, usize::MAX).unwrap()
        else {
            panic!("expected detached C1 state");
        };
        assert_eq!(terminal.export_snapshot_v2().unwrap(), expected);
        assert!(matches!(
            DeltaCheckpoint::from_snapshot(wrong, &expected),
            Err(DeltaError::Incompatible)
        ));
    }
}

#[test]
fn complete_canonical_state_and_suffix_continue() {
    let (base, target, expected) = pair();
    let delta = export_delta_v1(&base, &target, usize::MAX).unwrap();
    assert!(delta.len() < expected.len());
    let DeltaApply::Applied { mut terminal, info } =
        apply_delta_v1(&base, &delta, usize::MAX).unwrap()
    else {
        panic!("expected detached state");
    };
    assert_eq!(info, target.info());
    assert_eq!(terminal.export_snapshot_v2().unwrap(), expected);
    let mut original = Terminal::import_snapshot_v2(&expected).unwrap();
    output(&mut terminal, b"suffix\r\n");
    output(&mut original, b"suffix\r\n");
    assert_eq!(
        terminal.export_snapshot_v2().unwrap(),
        original.export_snapshot_v2().unwrap()
    );
    assert!(matches!(
        apply_delta_v1(&target, &delta, usize::MAX).unwrap(),
        DeltaApply::AlreadyApplied { .. }
    ));
}

#[test]
fn wrong_branch_and_corrupt_packet_never_install() {
    let (base, target, _) = pair();
    let mut delta = export_delta_v1(&base, &target, usize::MAX).unwrap();
    let mut wrong_context = context();
    wrong_context.id = [8; 32];
    let wrong =
        DeltaCheckpoint::from_snapshot(wrong_context, &base.snapshot_bytes().unwrap()).unwrap();
    assert!(matches!(
        apply_delta_v1(&wrong, &delta, usize::MAX),
        Err(DeltaError::NeedsFullSnapshot)
    ));
    let last = delta.len() - 1;
    delta[last] ^= 1;
    assert!(matches!(
        apply_delta_v1(&base, &delta, usize::MAX),
        Err(DeltaError::Malformed)
    ));
    assert_eq!(
        base.snapshot_bytes().unwrap(),
        Terminal::import_snapshot_v2(&base.snapshot_bytes().unwrap())
            .unwrap()
            .export_snapshot_v2()
            .unwrap()
    );
}

#[test]
fn packet_requires_known_features_and_exact_base_hash() {
    let (base, target, _) = pair();
    let mut delta = export_delta_v1(&base, &target, usize::MAX).unwrap();
    delta[20..24].copy_from_slice(&2u32.to_le_bytes());
    crate::delta::reseal_for_test(&mut delta);
    assert!(matches!(
        apply_delta_v1(&base, &delta, usize::MAX),
        Err(DeltaError::Incompatible)
    ));
    delta[20..24].copy_from_slice(&1u32.to_le_bytes());
    delta[88] ^= 1;
    crate::delta::reseal_for_test(&mut delta);
    assert!(matches!(
        apply_delta_v1(&base, &delta, usize::MAX),
        Err(DeltaError::NeedsFullSnapshot)
    ));
    assert!(matches!(
        export_delta_v1(&base, &target, 64),
        Err(DeltaError::NeedsFullSnapshot)
    ));
}

#[test]
fn resource_replacement_uses_new_id_and_deletes_old_id() {
    let mut terminal = Terminal::new_with_options(
        120,
        80,
        0,
        crate::TerminalOptions {
            history_capacity_rows: 300,
        },
    )
    .unwrap();
    terminal.graphics = Some(crate::graphics_state::GraphicsState::default());
    let text = (0..250)
        .map(|row| format!("{row:03}: {}\r\n", "graphics retained text ".repeat(5)))
        .collect::<String>();
    output(&mut terminal, text.as_bytes());
    let font = |payload: &[u8]| crate::drcs::decode(&[1, 1, 0, 5, 1, 2, 8, 0], payload).unwrap();
    terminal
        .graphics
        .as_mut()
        .unwrap()
        .install_font(font(b" @~"), &[])
        .unwrap();
    let old = terminal.graphics.as_ref().unwrap().resources().entries()[0].id();
    let base = DeltaCheckpoint::capture(context(), &terminal).unwrap();
    terminal
        .graphics
        .as_mut()
        .unwrap()
        .install_font(font(b" @@"), &[])
        .unwrap();
    output(&mut terminal, b"resource replacement\r\n");
    let entries = terminal.graphics.as_ref().unwrap().resources().entries();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].id() > old);
    let target = DeltaCheckpoint::capture(context(), &terminal).unwrap();
    let delta = export_delta_v1(&base, &target, usize::MAX).unwrap();
    let DeltaApply::Applied {
        terminal: restored, ..
    } = apply_delta_v1(&base, &delta, usize::MAX).unwrap()
    else {
        panic!("expected resource replacement");
    };
    assert_eq!(
        restored.export_snapshot_v2().unwrap(),
        terminal.export_snapshot_v2().unwrap()
    );
}

#[test]
fn journal_evicts_without_poisoning_live_state() {
    let mut terminal = Terminal::new(120, 80).unwrap();
    terminal.graphics = Some(crate::graphics_state::GraphicsState::default());
    for row in 0..70 {
        output(
            &mut terminal,
            format!("{row:03}: {}\r\n", "retained text ".repeat(6)).as_bytes(),
        );
    }
    let mut journal = DeltaJournal::new(
        context(),
        17,
        DeltaJournalConfig {
            max_entries: 2,
            max_bytes: 2 * 1024 * 1024,
            max_age_millis: 1000,
        },
    )
    .unwrap();
    let first = journal.capture(&terminal, 17, 0).unwrap();
    output(&mut terminal, b"second\r\n");
    let second = journal.capture(&terminal, 17, 1).unwrap();
    output(&mut terminal, b"third\r\n");
    let third = journal.capture(&terminal, 17, 2).unwrap();
    assert_eq!(journal.info(2).unwrap().entries, 2);
    assert!(matches!(
        journal.export(first, third, 2, usize::MAX),
        Err(DeltaError::NeedsFullSnapshot)
    ));
    assert!(
        !journal
            .export(second, third, 2, usize::MAX)
            .unwrap()
            .is_empty()
    );
    assert_eq!(journal.info(1002).unwrap().entries, 0);
    assert!(terminal.export_snapshot_v2().is_ok());
}

#[test]
fn duplicate_capture_refreshes_journal_time_and_retention() {
    let mut terminal = Terminal::new(80, 24).unwrap();
    terminal.graphics = Some(crate::graphics_state::GraphicsState::default());
    let mut journal = DeltaJournal::new(
        context(),
        17,
        DeltaJournalConfig {
            max_entries: 2,
            max_bytes: 2 * 1024 * 1024,
            max_age_millis: 100,
        },
    )
    .unwrap();
    let original = journal.capture(&terminal, 17, 0).unwrap();
    assert_eq!(journal.capture(&terminal, 17, 100).unwrap(), original);
    assert_eq!(journal.info(100).unwrap().oldest, Some(original));
    assert!(matches!(
        journal.info(99),
        Err(DeltaError::NeedsFullSnapshot)
    ));
    assert_eq!(journal.info(200).unwrap().entries, 0);
}
