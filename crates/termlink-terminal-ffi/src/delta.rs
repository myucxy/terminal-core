//! Explicit, bounded state-delta C ABI. The old base-free export remains unsupported.
use super::*;
#[cfg(test)]
use termlink_terminal_core::STATE_DELTA_PROFILE_V1;
use termlink_terminal_core::{
    DeltaApply, DeltaCheckpoint, DeltaContext, DeltaError, DeltaJournal, DeltaJournalConfig,
    DeltaStateInfo, MAX_STATE_DELTA_BYTES, apply_delta_v1, export_delta_v1,
};

pub const STATUS_MALFORMED_DELTA: u32 = 20;
pub const STATUS_INCOMPATIBLE_DELTA: u32 = 21;
pub const STATUS_NEEDS_FULL_SNAPSHOT: u32 = 22;
pub const DELTA_APPLIED: u32 = 1;
pub const DELTA_ALREADY_APPLIED: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TlTerminalDeltaContextV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub profile: u32,
    pub reserved0: u32,
    pub id: [u8; 32],
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TlTerminalDeltaStateInfoV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub next_source_sequence: u64,
    pub snapshot_bytes: u64,
    pub sha256: [u8; 32],
    pub reserved: [u64; 1],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TlTerminalDeltaJournalConfigV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub profile: u32,
    pub max_entries: u32,
    pub context_id: [u8; 32],
    pub max_bytes: u64,
    pub max_age_millis: u64,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TlTerminalDeltaJournalInfoV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub entries: u32,
    pub reserved0: u32,
    pub retained_bytes: u64,
    pub oldest: TlTerminalDeltaStateInfoV1,
    pub newest: TlTerminalDeltaStateInfoV1,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TlTerminalDeltaApplyInfoV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub outcome: u32,
    pub reserved0: u32,
    pub state: TlTerminalDeltaStateInfoV1,
    pub reserved: [u64; 2],
}

#[repr(C)]
pub struct TlTerminalDeltaJournal {
    inner: DeltaJournal,
}

const _: () = {
    assert!(mem::size_of::<TlTerminalDeltaContextV1>() == 64);
    assert!(mem::offset_of!(TlTerminalDeltaContextV1, id) == 16);
    assert!(mem::size_of::<TlTerminalDeltaStateInfoV1>() == 64);
    assert!(mem::offset_of!(TlTerminalDeltaStateInfoV1, sha256) == 24);
    assert!(mem::size_of::<TlTerminalDeltaJournalConfigV1>() == 80);
    assert!(mem::size_of::<TlTerminalDeltaJournalInfoV1>() == 168);
    assert!(mem::offset_of!(TlTerminalDeltaJournalInfoV1, oldest) == 24);
    assert!(mem::size_of::<TlTerminalDeltaApplyInfoV1>() == 96);
    assert!(mem::offset_of!(TlTerminalDeltaApplyInfoV1, state) == 16);
};

fn delta_status(error: DeltaError) -> u32 {
    match error {
        DeltaError::Malformed => STATUS_MALFORMED_DELTA,
        DeltaError::Incompatible => STATUS_INCOMPATIBLE_DELTA,
        DeltaError::LimitExceeded => STATUS_LIMIT_EXCEEDED,
        DeltaError::NeedsFullSnapshot => STATUS_NEEDS_FULL_SNAPSHOT,
        DeltaError::Terminal(error) => status_for_error(&error),
    }
}

fn context(value: TlTerminalDeltaContextV1) -> Result<DeltaContext, u32> {
    if value.reserved0 != 0 || value.reserved != [0; 2] {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    if value.id == [0; 32] {
        return Err(STATUS_INCOMPATIBLE_DELTA);
    }
    Ok(DeltaContext {
        id: value.id,
        profile: value.profile,
    })
}

fn state(value: TlTerminalDeltaStateInfoV1) -> Result<DeltaStateInfo, u32> {
    if value.reserved != [0; 1] {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    Ok(DeltaStateInfo {
        next_source_sequence: value.next_source_sequence,
        snapshot_bytes: value.snapshot_bytes,
        sha256: value.sha256,
    })
}

fn state_info(value: DeltaStateInfo) -> TlTerminalDeltaStateInfoV1 {
    TlTerminalDeltaStateInfoV1 {
        abi_version: ABI_VERSION,
        struct_size: struct_size::<TlTerminalDeltaStateInfoV1>(),
        next_source_sequence: value.next_source_sequence,
        snapshot_bytes: value.snapshot_bytes,
        sha256: value.sha256,
        reserved: [0; 1],
    }
}

fn empty_state_info() -> TlTerminalDeltaStateInfoV1 {
    state_info(DeltaStateInfo {
        next_source_sequence: 0,
        snapshot_bytes: 0,
        sha256: [0; 32],
    })
}

fn max_bytes(value: u64) -> Result<usize, u32> {
    let limit = checked_length(value, MAX_STATE_DELTA_BYTES)?;
    if limit == 0 {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    Ok(limit)
}

/// Captures a graphics checkpoint in the authenticated context and writes its full-state identity.
/// # Safety
/// The checkpoint and context must remain readable; output must be writable and versioned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_checkpoint_delta_info_v1(
    checkpoint: *const TlTerminalCheckpoint,
    expected_context: *const TlTerminalDeltaContextV1,
    out_info: *mut TlTerminalDeltaStateInfoV1,
) -> u32 {
    catch_status(|| {
        if checkpoint.is_null() || expected_context.is_null() || out_info.is_null() {
            return STATUS_NULL_POINTER;
        }
        let ctx = match unsafe { read_versioned_exact(expected_context) }.and_then(context) {
            Ok(ctx) => ctx,
            Err(status) => return status,
        };
        if let Err(status) = unsafe { output_capacity(out_info) } {
            return status;
        }
        let captured =
            match DeltaCheckpoint::capture(ctx, unsafe { &(*checkpoint).frozen_terminal }) {
                Ok(value) => value,
                Err(error) => return delta_status(error),
            };
        unsafe { ptr::write_unaligned(out_info, state_info(captured.info())) };
        STATUS_OK
    })
}

/// Exports [base,target) from two authenticated immutable checkpoints.
/// # Safety
/// All inputs must remain readable; out_delta must be an empty owned-buffer slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_checkpoint_export_delta_v1(
    base: *const TlTerminalCheckpoint,
    target: *const TlTerminalCheckpoint,
    expected_context: *const TlTerminalDeltaContextV1,
    maximum: u64,
    out_delta: *mut TlTerminalBuffer,
) -> u32 {
    catch_status(|| {
        if base.is_null() || target.is_null() || expected_context.is_null() || out_delta.is_null() {
            return STATUS_NULL_POINTER;
        }
        let capacity = match unsafe { empty_buffer_capacity(out_delta) } {
            Ok(value) => value,
            Err(status) => return status,
        };
        let ctx = match unsafe { read_versioned_exact(expected_context) }.and_then(context) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let maximum = match max_bytes(maximum) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let (base, target) = unsafe { (&*base, &*target) };
        let base = match DeltaCheckpoint::capture(ctx, &base.frozen_terminal) {
            Ok(value) => value,
            Err(error) => return delta_status(error),
        };
        let target = match DeltaCheckpoint::capture(ctx, &target.frozen_terminal) {
            Ok(value) => value,
            Err(error) => return delta_status(error),
        };
        let bytes = match export_delta_v1(&base, &target, maximum) {
            Ok(value) => value,
            Err(error) => return delta_status(error),
        };
        let buffer = match owned_buffer_with_limit(bytes, capacity, MAX_STATE_DELTA_BYTES) {
            Ok(value) => value,
            Err(status) => return status,
        };
        unsafe { ptr::write_unaligned(out_delta, buffer) };
        STATUS_OK
    })
}

/// Applies a validated delta into a detached terminal. ALREADY_APPLIED returns no handle.
/// # Safety
/// All borrowed inputs must remain readable; out_terminal must contain null and out_info must
/// be versioned and writable. The caller owns a returned handle and commits durable ACK itself.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_checkpoint_apply_delta_v1(
    current: *const TlTerminalCheckpoint,
    expected_context: *const TlTerminalDeltaContextV1,
    delta: *const u8,
    delta_len: u64,
    maximum: u64,
    out_terminal: *mut *mut TlTerminal,
    out_info: *mut TlTerminalDeltaApplyInfoV1,
) -> u32 {
    catch_status(|| {
        if current.is_null()
            || expected_context.is_null()
            || out_terminal.is_null()
            || out_info.is_null()
        {
            return STATUS_NULL_POINTER;
        }
        if let Err(status) = unsafe { require_empty_handle(out_terminal) } {
            return status;
        }
        if let Err(status) = unsafe { output_capacity(out_info) } {
            return status;
        }
        let ctx = match unsafe { read_versioned_exact(expected_context) }.and_then(context) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let maximum = match max_bytes(maximum) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let bytes = match borrowed_bytes(delta, delta_len, maximum) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let current = match DeltaCheckpoint::capture(ctx, unsafe { &(*current).frozen_terminal }) {
            Ok(value) => value,
            Err(error) => return delta_status(error),
        };
        let outcome = match apply_delta_v1(&current, bytes, maximum) {
            Ok(value) => value,
            Err(error) => return delta_status(error),
        };
        let (outcome, info, terminal) = match outcome {
            DeltaApply::Applied { terminal, info } => (DELTA_APPLIED, info, Some(terminal)),
            DeltaApply::AlreadyApplied { info } => (DELTA_ALREADY_APPLIED, info, None),
        };
        let info = TlTerminalDeltaApplyInfoV1 {
            abi_version: ABI_VERSION,
            struct_size: struct_size::<TlTerminalDeltaApplyInfoV1>(),
            outcome,
            reserved0: 0,
            state: state_info(info),
            reserved: [0; 2],
        };
        if let Some(inner) = terminal {
            let handle = Box::into_raw(Box::new(TlTerminal {
                inner,
                history_owner: history::new_owner_id(),
            }));
            unsafe { ptr::write_unaligned(out_terminal, handle) };
        }
        unsafe { ptr::write_unaligned(out_info, info) };
        STATUS_OK
    })
}

/// Creates a bounded journal tied to the terminal's local ownership lineage.
/// # Safety
/// Terminal/config must be readable; out_journal must be a writable null slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_delta_journal_create_v1(
    terminal: *const TlTerminal,
    config: *const TlTerminalDeltaJournalConfigV1,
    out_journal: *mut *mut TlTerminalDeltaJournal,
) -> u32 {
    catch_status(|| {
        if terminal.is_null() || config.is_null() || out_journal.is_null() {
            return STATUS_NULL_POINTER;
        }
        if let Err(status) = unsafe { require_empty_handle(out_journal) } {
            return status;
        }
        let config = match unsafe { read_versioned_exact(config) } {
            Ok(value) => value,
            Err(status) => return status,
        };
        if config.reserved != [0; 2] {
            return STATUS_INVALID_ARGUMENT;
        }
        let ctx = DeltaContext {
            id: config.context_id,
            profile: config.profile,
        };
        let max_bytes = match checked_length(config.max_bytes, 256 * 1024 * 1024) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let lineage = unsafe { (*terminal).history_owner };
        let journal = match DeltaJournal::new(
            ctx,
            lineage,
            DeltaJournalConfig {
                max_entries: config.max_entries as usize,
                max_bytes,
                max_age_millis: config.max_age_millis,
            },
        ) {
            Ok(value) => value,
            Err(error) => return delta_status(error),
        };
        unsafe {
            ptr::write_unaligned(
                out_journal,
                Box::into_raw(Box::new(TlTerminalDeltaJournal { inner: journal })),
            )
        };
        STATUS_OK
    })
}

/// # Safety
/// journal must be live and exclusively accessible, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_delta_journal_destroy(journal: *mut TlTerminalDeltaJournal) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !journal.is_null() {
            drop(unsafe { Box::from_raw(journal) });
        }
    }));
}

/// # Safety
/// journal must have exclusive access; checkpoint must remain live and output writable/versioned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_delta_journal_capture_v1(
    journal: *mut TlTerminalDeltaJournal,
    checkpoint: *const TlTerminalCheckpoint,
    now_millis: u64,
    out_state: *mut TlTerminalDeltaStateInfoV1,
) -> u32 {
    catch_status(|| {
        if journal.is_null() || checkpoint.is_null() || out_state.is_null() {
            return STATUS_NULL_POINTER;
        }
        if let Err(status) = unsafe { output_capacity(out_state) } {
            return status;
        }
        let (journal, checkpoint) = unsafe { (&mut *journal, &*checkpoint) };
        let info = match journal.inner.capture(
            &checkpoint.frozen_terminal,
            checkpoint.history_owner,
            now_millis,
        ) {
            Ok(value) => value,
            Err(error) => return delta_status(error),
        };
        unsafe { ptr::write_unaligned(out_state, state_info(info)) };
        STATUS_OK
    })
}

/// # Safety
/// journal must have exclusive access and output must be writable/versioned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_delta_journal_get_info_v1(
    journal: *mut TlTerminalDeltaJournal,
    now_millis: u64,
    out_info: *mut TlTerminalDeltaJournalInfoV1,
) -> u32 {
    catch_status(|| {
        if journal.is_null() || out_info.is_null() {
            return STATUS_NULL_POINTER;
        }
        if let Err(status) = unsafe { output_capacity(out_info) } {
            return status;
        }
        let info = match unsafe { &mut *journal }.inner.info(now_millis) {
            Ok(value) => value,
            Err(error) => return delta_status(error),
        };
        unsafe {
            ptr::write_unaligned(
                out_info,
                TlTerminalDeltaJournalInfoV1 {
                    abi_version: ABI_VERSION,
                    struct_size: struct_size::<TlTerminalDeltaJournalInfoV1>(),
                    entries: info.entries as u32,
                    reserved0: 0,
                    retained_bytes: info.retained_bytes as u64,
                    oldest: info.oldest.map_or_else(empty_state_info, state_info),
                    newest: info.newest.map_or_else(empty_state_info, state_info),
                    reserved: [0; 2],
                },
            )
        };
        STATUS_OK
    })
}

/// # Safety
/// journal must have exclusive access; base/target must be readable and out_delta an empty buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_delta_journal_export_v1(
    journal: *mut TlTerminalDeltaJournal,
    base: *const TlTerminalDeltaStateInfoV1,
    target: *const TlTerminalDeltaStateInfoV1,
    now_millis: u64,
    maximum: u64,
    out_delta: *mut TlTerminalBuffer,
) -> u32 {
    catch_status(|| {
        if journal.is_null() || base.is_null() || target.is_null() || out_delta.is_null() {
            return STATUS_NULL_POINTER;
        }
        let capacity = match unsafe { empty_buffer_capacity(out_delta) } {
            Ok(value) => value,
            Err(status) => return status,
        };
        let base = match unsafe { read_versioned_exact(base) }.and_then(state) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let target = match unsafe { read_versioned_exact(target) }.and_then(state) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let maximum = match max_bytes(maximum) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let bytes = match unsafe { &mut *journal }
            .inner
            .export(base, target, now_millis, maximum)
        {
            Ok(value) => value,
            Err(error) => return delta_status(error),
        };
        let buffer = match owned_buffer_with_limit(bytes, capacity, MAX_STATE_DELTA_BYTES) {
            Ok(value) => value,
            Err(status) => return status,
        };
        unsafe { ptr::write_unaligned(out_delta, buffer) };
        STATUS_OK
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> TlTerminalConfigV4 {
        TlTerminalConfigV4 {
            abi_version: ABI_VERSION,
            struct_size: 112,
            columns: 80,
            rows: 40,
            initial_source_sequence: 0,
            reserved_v1_compat: [0; 2],
            history_capacity_rows: 200,
            reserved_v2_compat: [0; 2],
            output_profile: OUTPUT_PROFILE_UTF8,
            reserved0: 0,
            reserved_v3_compat: [0; 2],
            width_profile: 1,
            reserved1: 0,
            reserved: [0; 2],
        }
    }
    fn context() -> TlTerminalDeltaContextV1 {
        TlTerminalDeltaContextV1 {
            abi_version: ABI_VERSION,
            struct_size: 64,
            profile: STATE_DELTA_PROFILE_V1,
            reserved0: 0,
            id: [7; 32],
            reserved: [0; 2],
        }
    }
    fn buffer() -> TlTerminalBuffer {
        TlTerminalBuffer {
            abi_version: ABI_VERSION,
            struct_size: 24,
            data: ptr::null(),
            #[cfg(target_pointer_width = "32")]
            data_pointer_padding: 0,
            len: 0,
        }
    }
    fn info() -> TlTerminalDeltaStateInfoV1 {
        TlTerminalDeltaStateInfoV1 {
            abi_version: ABI_VERSION,
            struct_size: 64,
            next_source_sequence: 0,
            snapshot_bytes: 0,
            sha256: [0; 32],
            reserved: [0; 1],
        }
    }
    unsafe fn checkpoint(terminal: *mut TlTerminal) -> *mut TlTerminalCheckpoint {
        let mut checkpoint = ptr::null_mut();
        assert_eq!(
            unsafe { tl_terminal_checkpoint_begin(terminal, &mut checkpoint) },
            STATUS_OK
        );
        checkpoint
    }
    unsafe fn snapshot(checkpoint: *mut TlTerminalCheckpoint) -> Vec<u8> {
        let mut bytes = buffer();
        assert_eq!(
            unsafe { tl_terminal_checkpoint_export_snapshot_graphics_v1(checkpoint, &mut bytes) },
            STATUS_OK
        );
        let value = unsafe { std::slice::from_raw_parts(bytes.data, bytes.len as usize) }.to_vec();
        assert_eq!(unsafe { tl_terminal_buffer_free(&mut bytes) }, STATUS_OK);
        value
    }
    unsafe fn output(terminal: *mut TlTerminal, sequence: u64, data: &[u8]) {
        let event = TlTerminalEvent {
            abi_version: ABI_VERSION,
            struct_size: 56,
            source_sequence: sequence,
            kind: EVENT_OUTPUT,
            reserved: 0,
            data: data.as_ptr(),
            #[cfg(target_pointer_width = "32")]
            data_pointer_padding: 0,
            data_len: data.len() as u64,
            columns: 0,
            rows: 0,
            advance_micros: 0,
        };
        assert_eq!(
            unsafe { tl_terminal_apply_event(terminal, &event) },
            STATUS_OK
        );
    }

    #[test]
    fn delta_abi_is_atomic_idempotent_and_preserves_slots() {
        unsafe {
            let mut terminal = ptr::null_mut();
            assert_eq!(
                tl_terminal_create_graphics_v1(&test_config(), &mut terminal),
                STATUS_OK
            );
            let mut text = Vec::new();
            for index in 0..7000 {
                text.push(b'!' + (index % 90) as u8);
            }
            output(terminal, 0, &text);
            let base = checkpoint(terminal);
            output(terminal, 1, b"Z");
            let target = checkpoint(terminal);
            let mut delta = buffer();
            let ctx = context();
            assert_eq!(
                tl_terminal_checkpoint_export_delta_v1(
                    base,
                    target,
                    &ctx,
                    MAX_STATE_DELTA_BYTES as u64,
                    &mut delta
                ),
                STATUS_OK
            );
            assert!(delta.len < snapshot(target).len() as u64);
            let mut applied = ptr::null_mut();
            let mut result = TlTerminalDeltaApplyInfoV1 {
                abi_version: ABI_VERSION,
                struct_size: 96,
                outcome: 0,
                reserved0: 0,
                state: info(),
                reserved: [0; 2],
            };
            let mut wrong = ctx;
            wrong.id = [8; 32];
            assert_eq!(
                tl_terminal_checkpoint_apply_delta_v1(
                    base,
                    &wrong,
                    delta.data,
                    delta.len,
                    MAX_STATE_DELTA_BYTES as u64,
                    &mut applied,
                    &mut result
                ),
                STATUS_NEEDS_FULL_SNAPSHOT
            );
            assert!(applied.is_null());
            assert_eq!(result.outcome, 0);
            let mut broken = std::slice::from_raw_parts(delta.data, delta.len as usize).to_vec();
            let last = broken.len() - 1;
            broken[last] ^= 1;
            assert_eq!(
                tl_terminal_checkpoint_apply_delta_v1(
                    base,
                    &ctx,
                    broken.as_ptr(),
                    broken.len() as u64,
                    MAX_STATE_DELTA_BYTES as u64,
                    &mut applied,
                    &mut result
                ),
                STATUS_MALFORMED_DELTA
            );
            assert!(applied.is_null());
            assert_eq!(
                tl_terminal_checkpoint_apply_delta_v1(
                    base,
                    &ctx,
                    delta.data,
                    delta.len,
                    MAX_STATE_DELTA_BYTES as u64,
                    &mut applied,
                    &mut result
                ),
                STATUS_OK
            );
            assert_eq!(result.outcome, DELTA_APPLIED);
            let restored = checkpoint(applied);
            assert_eq!(snapshot(restored), snapshot(target));
            tl_terminal_checkpoint_destroy(restored);
            tl_terminal_destroy(applied);
            applied = ptr::null_mut();
            assert_eq!(
                tl_terminal_checkpoint_apply_delta_v1(
                    target,
                    &ctx,
                    delta.data,
                    delta.len,
                    MAX_STATE_DELTA_BYTES as u64,
                    &mut applied,
                    &mut result
                ),
                STATUS_OK
            );
            assert_eq!(result.outcome, DELTA_ALREADY_APPLIED);
            assert!(applied.is_null());

            let config = TlTerminalDeltaJournalConfigV1 {
                abi_version: ABI_VERSION,
                struct_size: 80,
                profile: STATE_DELTA_PROFILE_V1,
                max_entries: 2,
                context_id: ctx.id,
                max_bytes: 32 * 1024 * 1024,
                max_age_millis: 1000,
                reserved: [0; 2],
            };
            let mut journal = ptr::null_mut();
            assert_eq!(
                tl_terminal_delta_journal_create_v1(terminal, &config, &mut journal),
                STATUS_OK
            );
            let mut base_info = info();
            let mut target_info = info();
            assert_eq!(
                tl_terminal_delta_journal_capture_v1(journal, base, 1, &mut base_info),
                STATUS_OK
            );
            assert_eq!(
                tl_terminal_delta_journal_capture_v1(journal, target, 2, &mut target_info),
                STATUS_OK
            );
            let mut journal_delta = buffer();
            assert_eq!(
                tl_terminal_delta_journal_export_v1(
                    journal,
                    &base_info,
                    &target_info,
                    2,
                    MAX_STATE_DELTA_BYTES as u64,
                    &mut journal_delta
                ),
                STATUS_OK
            );
            assert_eq!(
                std::slice::from_raw_parts(delta.data, delta.len as usize),
                std::slice::from_raw_parts(journal_delta.data, journal_delta.len as usize)
            );
            assert_eq!(tl_terminal_buffer_free(&mut journal_delta), STATUS_OK);
            assert_eq!(
                tl_terminal_delta_journal_export_v1(
                    journal,
                    &base_info,
                    &target_info,
                    1003,
                    MAX_STATE_DELTA_BYTES as u64,
                    &mut journal_delta
                ),
                STATUS_NEEDS_FULL_SNAPSHOT
            );
            assert!(journal_delta.data.is_null());
            tl_terminal_delta_journal_destroy(journal);
            assert_eq!(tl_terminal_buffer_free(&mut delta), STATUS_OK);
            tl_terminal_checkpoint_destroy(base);
            tl_terminal_checkpoint_destroy(target);
            tl_terminal_destroy(terminal);
        }
    }

    #[test]
    fn delta_profile_must_match_canonical_snapshot_width_and_output() {
        unsafe {
            for (width, output_profile, profile) in [(1, 0, 1), (2, 0, 2), (1, 1, 3), (2, 1, 4)] {
                let mut terminal = ptr::null_mut();
                let mut config = test_config();
                config.width_profile = width;
                config.output_profile = output_profile;
                assert_eq!(
                    tl_terminal_create_graphics_v1(&config, &mut terminal),
                    STATUS_OK
                );
                let frozen = checkpoint(terminal);
                let mut state = info();
                let mut ctx = context();
                ctx.profile = if profile == 1 { 2 } else { 1 };
                assert_eq!(
                    tl_terminal_checkpoint_delta_info_v1(frozen, &ctx, &mut state),
                    STATUS_INCOMPATIBLE_DELTA
                );
                ctx.profile = profile;
                assert_eq!(
                    tl_terminal_checkpoint_delta_info_v1(frozen, &ctx, &mut state),
                    STATUS_OK
                );
                assert!(state.snapshot_bytes > 0);
                tl_terminal_checkpoint_destroy(frozen);
                tl_terminal_destroy(terminal);
            }
        }
    }
}
