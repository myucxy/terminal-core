use super::*;

/// Applies one source event atomically, exporting effects plus a safe legacy display stream.
/// The buffer is TLPJ1 followed by effect-length u32, projection-length u32, then both bodies.
/// # Safety
/// Same event/handle/output ownership requirements as the V2 host-effect entry point.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_apply_event_with_legacy_projection_v1(
    terminal: *mut TlTerminal,
    event: *const TlTerminalEvent,
    output: *mut TlTerminalBuffer,
) -> u32 {
    catch_status(|| {
        if terminal.is_null() || event.is_null() || output.is_null() {
            return STATUS_NULL_POINTER;
        }
        // SAFETY: The caller provides readable versioned event and writable empty buffer.
        let event = match unsafe { read_versioned(event) } {
            Ok(v) => v,
            Err(e) => return e,
        };
        let event = match decode_terminal_event(&event) {
            Ok(v) => v,
            Err(e) => return e,
        };
        // SAFETY: Validate output ownership before terminal mutation.
        let capacity = match unsafe { empty_buffer_capacity(output) } {
            Ok(v) => v,
            Err(e) => return e,
        };
        // SAFETY: Validated pointers and caller's exclusive handle access.
        let mut prepared = match unsafe { &mut *terminal }
            .inner
            .prepare_event_with_legacy_projection(event)
        {
            Ok(v) => v,
            Err(e) => return status_for_error(&e),
        };
        let projection = prepared.take_legacy_projection();
        let mut pending = None;
        let committed = prepared.commit_with(|effects| {
            let size = 16 + effects.len() + projection.len();
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(size)
                .map_err(|_| STATUS_LIMIT_EXCEEDED)?;
            bytes.extend_from_slice(b"TLPJ1\0\0\0");
            bytes.extend_from_slice(&(effects.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&(projection.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&effects);
            bytes.extend_from_slice(&projection);
            pending = Some(PendingOwnedBuffer::new(
                bytes,
                capacity,
                16 + MAX_HOST_EFFECT_FRAME_V2_BYTES
                    + termlink_terminal_core::MAX_LEGACY_PROJECTION_BYTES,
            )?);
            #[cfg(test)]
            pending_buffer_test_gate()?;
            Ok(())
        });
        if let Err(status) = committed {
            return status;
        }
        let Some(pending) = pending else {
            return STATUS_PANIC;
        };
        // SAFETY: Output validated before prepare; token remains guarded until publication.
        unsafe { ptr::write_unaligned(output, pending.buffer) };
        pending.disarm();
        STATUS_OK
    })
}
