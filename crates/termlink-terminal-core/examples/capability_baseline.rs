//! Reproducible synthetic text/restore benchmark. Emits measurements, never thresholds-as-results.
use std::{hint::black_box, time::Instant};
use termlink_terminal_core::{Event, OutputProfile, Terminal, TerminalOptions, WidthProfile};

const WARMUP: usize = 5;
const SAMPLES: usize = 25;

fn measure(mut action: impl FnMut()) -> Vec<u128> {
    for _ in 0..WARMUP {
        action();
    }
    (0..SAMPLES)
        .map(|_| {
            let start = Instant::now();
            action();
            start.elapsed().as_micros()
        })
        .collect()
}

fn report(name: &str, samples: &[u128], units: usize) {
    let mut ordered = samples.to_vec();
    ordered.sort_unstable();
    println!(
        "{{\"case\":\"{name}\",\"units\":{units},\"samples_us\":{samples:?},\"median_us\":{},\"p95_us\":{}}}",
        ordered[SAMPLES / 2],
        ordered[(SAMPLES * 95).div_ceil(100) - 1]
    );
}

fn main() {
    let modern = std::env::args().any(|arg| arg == "--unicode16");
    let profile = if modern {
        WidthProfile::Unicode16
    } else {
        WidthProfile::LegacyV1
    };
    let create = |history_capacity_rows| {
        Terminal::new_with_profiles(
            120,
            40,
            0,
            TerminalOptions {
                history_capacity_rows,
            },
            OutputProfile::Utf8,
            profile,
        )
        .unwrap()
    };
    let chunk = "0123456789 hello terminal 中文\r\n".repeat(64).into_bytes();
    let samples = measure(|| {
        let mut terminal = create(0);
        for sequence in 0..256 {
            terminal
                .apply_event(Event::Output {
                    source_sequence: sequence,
                    bytes: black_box(&chunk),
                })
                .unwrap();
        }
        black_box(terminal.cursor());
    });
    report("text_output", &samples, chunk.len() * 256);

    let mut terminal = create(2000);
    for sequence in 0..40 {
        terminal
            .apply_event(Event::Output {
                source_sequence: sequence,
                bytes: &chunk,
            })
            .unwrap();
    }
    let snapshot = terminal.export_snapshot_v2().unwrap();
    report(
        "snapshot_export",
        &measure(|| {
            black_box(terminal.export_snapshot_v2().unwrap());
        }),
        snapshot.len(),
    );
    report(
        "snapshot_import",
        &measure(|| {
            black_box(Terminal::import_snapshot_v2(black_box(&snapshot)).unwrap());
        }),
        snapshot.len(),
    );
    report(
        if modern {
            "render_frame_v4"
        } else {
            "render_frame_v3"
        },
        &measure(|| {
            black_box(if modern {
                terminal.export_render_frame_v4().unwrap()
            } else {
                terminal.export_render_frame_v3().unwrap()
            });
        }),
        120 * 40,
    );
}
