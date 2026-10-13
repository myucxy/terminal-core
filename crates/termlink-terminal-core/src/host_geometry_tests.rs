use crate::{Event, HostGeometryV1, Terminal, TerminalError};

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
fn set(t: &mut Terminal, g: HostGeometryV1) {
    t.apply_event(Event::HostGeometry {
        source_sequence: t.next_source_sequence(),
        geometry: g,
    })
    .unwrap();
}
fn measured() -> HostGeometryV1 {
    HostGeometryV1 {
        cell_width: 9,
        cell_height: 18,
        display_width: 1920,
        display_height: 1080,
        window_width: 744,
        window_height: 476,
    }
}

#[test]
fn geometry_uses_ordered_host_values_and_never_guesses_unknown_physical_pixels() {
    let queries = b"\x1b[14t\x1b[14;2t\x1b[15t\x1b[16t\x1b[18t\x1b[19t";
    for split in 0..=queries.len() {
        let mut t = Terminal::new(80, 24).unwrap();
        assert_eq!(output(&mut t, queries), [b"\x1b[8;24;80t".to_vec()]);
        set(&mut t, measured());
        let mut replies = output(&mut t, &queries[..split]);
        replies.extend(output(&mut t, &queries[split..]));
        assert_eq!(
            replies,
            [
                "\x1b[4;432;720t",
                "\x1b[4;476;744t",
                "\x1b[5;1080;1920t",
                "\x1b[6;18;9t",
                "\x1b[8;24;80t",
                "\x1b[9;60;213t"
            ]
            .map(|s| s.as_bytes().to_vec())
        );
        t.apply_event(Event::Resize {
            source_sequence: t.next_source_sequence(),
            columns: 40,
            rows: 10,
        })
        .unwrap();
        assert_eq!(
            output(&mut t, b"\x1b[14t\x1b[18t"),
            [b"\x1b[4;180;360t".to_vec(), b"\x1b[8;10;40t".to_vec()]
        );
        output(&mut t, b"\x1b[!p\x1bc");
        assert_eq!(t.host_geometry, measured());
        set(&mut t, HostGeometryV1::default());
        assert_eq!(output(&mut t, queries), [b"\x1b[8;10;40t".to_vec()]);
    }
}

#[test]
fn geometry_validation_rolls_back_the_whole_batch_without_poisoning() {
    let mut t = Terminal::new(8, 4).unwrap();
    let baseline = t.export_snapshot_v2().unwrap();
    for geometry in [
        HostGeometryV1 {
            cell_width: 9,
            ..Default::default()
        },
        HostGeometryV1 {
            display_width: 32769,
            display_height: 1,
            ..Default::default()
        },
        HostGeometryV1 {
            cell_width: 1025,
            cell_height: 1,
            ..Default::default()
        },
    ] {
        let seq = t.next_source_sequence();
        assert_eq!(
            t.apply_events_atomic(&[
                Event::Output {
                    source_sequence: seq,
                    bytes: b"must roll back"
                },
                Event::HostGeometry {
                    source_sequence: seq + 1,
                    geometry
                },
            ]),
            Err(TerminalError::InvalidDimensions)
        );
        assert_eq!(t.export_snapshot_v2().unwrap(), baseline);
    }
    let mut encoded = measured().encode();
    assert_eq!(HostGeometryV1::decode(&encoded), Ok(measured()));
    encoded[28] = 1;
    assert!(HostGeometryV1::decode(&encoded).is_err());
    assert!(HostGeometryV1::decode(&encoded[..31]).is_err());
}

#[test]
fn geometry_snapshots_are_canonical_and_old_profiles_keep_their_bytes() {
    let mut t = Terminal::new(80, 24).unwrap();
    set(&mut t, measured());
    output(&mut t, b"one\x1b[2 Ptwo");
    let snapshot = t.export_snapshot_v2().unwrap();
    assert_eq!(u16::from_le_bytes(snapshot[10..12].try_into().unwrap()), 18);
    let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
    assert_eq!(restored.export_snapshot_v2().unwrap(), snapshot);
    assert_eq!(
        output(&mut restored, b"\x1b[16t"),
        output(&mut t, b"\x1b[16t")
    );
    let mut unchanged = Terminal::new(80, 24).unwrap();
    set(&mut unchanged, HostGeometryV1::default());
    let mut legacy = Terminal::new(80, 24).unwrap();
    legacy
        .apply_event(Event::Advance {
            source_sequence: legacy.next_source_sequence(),
            micros: 0,
        })
        .unwrap();
    assert_eq!(
        unchanged.export_snapshot_v2().unwrap(),
        legacy.export_snapshot_v2().unwrap()
    );
}

#[test]
fn geometry_snapshot_rejects_bad_dimensions_reserved_bytes_and_feature_claims() {
    let mut t = Terminal::new(80, 24).unwrap();
    set(&mut t, measured());
    let snapshot = t.export_snapshot_v2().unwrap();
    let count = u32::from_le_bytes(snapshot[40..44].try_into().unwrap()) as usize;
    let entry = (0..count)
        .map(|i| 104 + i * 32)
        .find(|i| u32::from_le_bytes(snapshot[*i..*i + 4].try_into().unwrap()) == 0x400)
        .unwrap();
    let payload = u64::from_le_bytes(snapshot[entry + 8..entry + 16].try_into().unwrap()) as usize;
    for (offset, value) in [
        (payload, 2u32),
        (payload + 4, 0),
        (payload + 4, 1025),
        (payload + 12, 32769),
        (payload + 28, 1),
        (entry + 24, 2),
    ] {
        let mut bad = snapshot.clone();
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        crate::snapshot::refresh_crc_for_test(&mut bad);
        assert!(
            Terminal::import_snapshot_v2(&bad).is_err(),
            "offset {offset}"
        );
    }
    let mut missing_feature = snapshot;
    missing_feature[67] &= 0x7f;
    crate::snapshot::refresh_crc_for_test(&mut missing_feature);
    assert_eq!(
        Terminal::import_snapshot_v2(&missing_feature),
        Err(TerminalError::IncompatibleSnapshot)
    );
}

#[test]
fn geometry_reply_bounds_and_optional_variants_are_strict() {
    use crate::host_geometry::canonical_geometry_reply as valid;
    for reply in [
        "\x1b[4;524288;524288t",
        "\x1b[5;32768;32768t",
        "\x1b[6;1024;1024t",
        "\x1b[9;1;1t",
    ] {
        assert!(valid(reply.as_bytes()), "{reply:?}");
    }
    for reply in [
        "\x1b[4;524289;1t",
        "\x1b[5;32769;1t",
        "\x1b[6;1025;1t",
        "\x1b[8;513;1t",
        "\x1b[9;0;1t",
        "\x1b[4;01;1t",
        "\x1b[4;1;1;1t",
        "\x1b[4;42949672960;1t",
    ] {
        assert!(!valid(reply.as_bytes()), "{reply:?}");
    }
    let mut t = Terminal::new(80, 24).unwrap();
    set(&mut t, measured());
    assert!(
        output(
            &mut t,
            b"\x1b[14;1t\x1b[15;2t\x1b[16;0t\x1b[18;2t\x1b[19;9t"
        )
        .is_empty()
    );
}
