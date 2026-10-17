use super::*;
use termlink_terminal_core::kitty_graphics::{
    BrokerCapture, BrokerFailure, BrokerOutcome, BrokerRequest, MAX_BROKER_CAPTURE_BYTES,
};

pub const BROKER_CAPTURE_BYTES: u32 = 0;
pub const BROKER_CAPTURE_DENIED: u32 = 1;
pub const BROKER_CAPTURE_UNAVAILABLE: u32 = 2;
pub const BROKER_CAPTURE_UNSUPPORTED: u32 = 3;
pub const BROKER_CAPTURE_OVER_LIMIT: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TlTerminalBrokerRequestV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub has_request: u32,
    pub medium: u32,
    pub source_sequence: u64,
    pub source_offset_end: u32,
    pub ordinal: u32,
    pub control_digest: [u8; 32],
    pub read_offset: u64,
    pub read_length: u32,
    pub has_read_length: u32,
    pub reserved: [u64; 2],
}

pub struct TlTerminalBrokerOutputV1 {
    terminal: Box<TlTerminal>,
    source_sequence: u64,
    output: Vec<u8>,
    captures: Vec<BrokerCapture>,
    pending: Option<BrokerRequest>,
    error: Option<String>,
}

impl TlTerminalBrokerOutputV1 {
    fn prepared(
        &mut self,
    ) -> Result<termlink_terminal_core::PreparedBrokerOutputV1<'_>, TerminalError> {
        // Borrow only the terminal field here. Returning the preparation ties
        // the borrow to `terminal.inner`; keeping the call explicit lets the
        // error branch update this transaction's separate diagnostic field.
        let result = self
            .terminal
            .inner
            .prepare_output_with_broker_captures_v1_with_diagnostic(
                self.source_sequence,
                &self.output,
                &self.captures,
            );
        match result {
            Ok(preparation) => Ok(preparation),
            Err(failure) => {
                let (error, diagnostic) = failure.into_parts();
                self.error = Some(diagnostic.unwrap_or_else(|| error.to_string()));
                Err(error)
            }
        }
    }

    fn replay(&mut self, capture: Option<BrokerCapture>) -> Result<(), TerminalError> {
        let result = self
            .terminal
            .inner
            .prepare_output_with_broker_captures_v1_with_diagnostic(
                self.source_sequence,
                &self.output,
                &self.captures,
            );
        let mut preparation = match result {
            Ok(preparation) => preparation,
            Err(failure) => {
                let (error, diagnostic) = failure.into_parts();
                self.error = Some(diagnostic.unwrap_or_else(|| error.to_string()));
                return Err(error);
            }
        };
        if let Some(ref capture) = capture {
            let result = preparation.supply(capture.clone());
            if let Err(error) = result {
                let diagnostic = preparation.failure_diagnostic_v3().map(str::to_owned);
                drop(preparation);
                self.error = Some(diagnostic.unwrap_or_else(|| error.to_string()));
                return Err(error);
            }
        }
        let next = preparation.request().cloned();
        drop(preparation);
        if let Some(capture) = capture {
            if self.captures.try_reserve(1).is_err() {
                let error = TerminalError::LimitExceeded;
                self.error = Some(error.to_string());
                return Err(error);
            }
            self.captures.push(capture);
        }
        self.pending = next;
        self.error = None;
        Ok(())
    }
}

fn request_identity(request: &BrokerRequest, size: u32) -> TlTerminalBrokerRequestV1 {
    TlTerminalBrokerRequestV1 {
        abi_version: ABI_VERSION,
        struct_size: size,
        has_request: 1,
        medium: u32::from(request.medium),
        source_sequence: request.source_sequence,
        source_offset_end: request.source_offset_end,
        ordinal: u32::from(request.ordinal),
        control_digest: request.control_digest,
        read_offset: request.read_offset,
        read_length: request.read_length.unwrap_or(0),
        has_read_length: u32::from(request.read_length.is_some()),
        reserved: [0; 2],
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tl_terminal_broker_request_v1_struct_size() -> u32 {
    catch_struct_size::<TlTerminalBrokerRequestV1>()
}

/// Transfers the terminal from `owner` to a broker transaction; the owner slot is
/// NULL until commit or cancel restores it. All pointers must be live, exclusive,
/// and readable/writable for their stated size. Do not access a saved alias.
///
/// # Safety
/// The caller owns all pointer slots and must not use the terminal through an
/// alias while the transaction owns it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_broker_begin_v1(
    owner: *mut *mut TlTerminal,
    event: *const TlTerminalEvent,
    out_transaction: *mut *mut TlTerminalBrokerOutputV1,
    out_diagnostic: *mut TlTerminalBuffer,
) -> u32 {
    catch_status(|| {
        if owner.is_null()
            || event.is_null()
            || out_transaction.is_null()
            || out_diagnostic.is_null()
        {
            return STATUS_NULL_POINTER;
        }
        if let Err(status) = unsafe { require_empty_handle(out_transaction) } {
            return status;
        }
        let diagnostic_capacity = match unsafe { empty_buffer_capacity(out_diagnostic) } {
            Ok(capacity) => capacity,
            Err(status) => return status,
        };
        let terminal = unsafe { ptr::read_unaligned(owner) };
        if terminal.is_null() {
            return STATUS_INVALID_ARGUMENT;
        }
        let event = match unsafe { read_versioned(event) } {
            Ok(event) => event,
            Err(status) => return status,
        };
        let (source_sequence, bytes) = match decode_terminal_event(&event) {
            Ok(Event::Output {
                source_sequence,
                bytes,
            }) => (source_sequence, bytes),
            Ok(_) => return STATUS_INVALID_ARGUMENT,
            Err(status) => return status,
        };
        let mut output = Vec::new();
        if output.try_reserve_exact(bytes.len()).is_err() {
            return STATUS_LIMIT_EXCEEDED;
        }
        output.extend_from_slice(bytes);
        let pending = match unsafe { &mut (*terminal).inner }
            .prepare_output_with_broker_v1_with_diagnostic(source_sequence, &output)
        {
            Ok(preparation) => preparation.request().cloned(),
            Err(failure) => {
                let (error, diagnostic) = failure.into_parts();
                let status = status_for_error(&error);
                let buffer = match owned_buffer_with_limit(
                    diagnostic.unwrap_or_else(|| error.to_string()).into_bytes(),
                    diagnostic_capacity,
                    8192,
                ) {
                    Ok(buffer) => buffer,
                    Err(buffer_status) => return buffer_status,
                };
                unsafe { ptr::write_unaligned(out_diagnostic, buffer) };
                return status;
            }
        };
        let transaction = Box::new(TlTerminalBrokerOutputV1 {
            terminal: unsafe { Box::from_raw(terminal) },
            source_sequence,
            output,
            captures: Vec::new(),
            pending,
            error: None,
        });
        unsafe { ptr::write_unaligned(owner, ptr::null_mut()) };
        unsafe { ptr::write_unaligned(out_transaction, Box::into_raw(transaction)) };
        STATUS_OK
    })
}

/// Redacted request view. It never copies the file/shared-memory reference.
///
/// # Safety
/// `transaction` must be live; `out_request` must be writable and versioned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_broker_request_redacted_v1(
    transaction: *const TlTerminalBrokerOutputV1,
    out_request: *mut TlTerminalBrokerRequestV1,
) -> u32 {
    catch_status(|| {
        if transaction.is_null() || out_request.is_null() {
            return STATUS_NULL_POINTER;
        }
        let size = match unsafe { output_capacity(out_request) } {
            Ok(size) => size,
            Err(status) => return status,
        };
        let identity = match &unsafe { &*transaction }.pending {
            Some(request) => request_identity(request, size),
            None => TlTerminalBrokerRequestV1 {
                abi_version: ABI_VERSION,
                struct_size: size,
                has_request: 0,
                medium: 0,
                source_sequence: 0,
                source_offset_end: 0,
                ordinal: 0,
                control_digest: [0; 32],
                read_offset: 0,
                read_length: 0,
                has_read_length: 0,
                reserved: [0; 2],
            },
        };
        unsafe { ptr::write_unaligned(out_request, identity) };
        STATUS_OK
    })
}

/// Returns the exact parser-produced identity and a separately owned UTF-8
/// reference. Only trusted Windows file broker code should request the latter;
/// Android must use its redacted JNI request view.
///
/// # Safety
/// `transaction` must be live and both outputs must be writable, versioned,
/// with `out_reference` empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_broker_request_v1(
    transaction: *const TlTerminalBrokerOutputV1,
    out_request: *mut TlTerminalBrokerRequestV1,
    out_reference: *mut TlTerminalBuffer,
) -> u32 {
    catch_status(|| {
        if transaction.is_null() || out_request.is_null() || out_reference.is_null() {
            return STATUS_NULL_POINTER;
        }
        let size = match unsafe { output_capacity(out_request) } {
            Ok(size) => size,
            Err(status) => return status,
        };
        let capacity = match unsafe { empty_buffer_capacity(out_reference) } {
            Ok(size) => size,
            Err(status) => return status,
        };
        let transaction = unsafe { &*transaction };
        let Some(request) = &transaction.pending else {
            let identity = TlTerminalBrokerRequestV1 {
                abi_version: ABI_VERSION,
                struct_size: size,
                has_request: 0,
                medium: 0,
                source_sequence: 0,
                source_offset_end: 0,
                ordinal: 0,
                control_digest: [0; 32],
                read_offset: 0,
                read_length: 0,
                has_read_length: 0,
                reserved: [0; 2],
            };
            unsafe { ptr::write_unaligned(out_request, identity) };
            return STATUS_OK;
        };
        let reference = match owned_buffer_with_limit(
            request.reference_utf8.as_bytes().to_vec(),
            capacity,
            MAX_OUTPUT_BYTES,
        ) {
            Ok(reference) => reference,
            Err(status) => return status,
        };
        unsafe { ptr::write_unaligned(out_reference, reference) };
        unsafe { ptr::write_unaligned(out_request, request_identity(request, size)) };
        STATUS_OK
    })
}

/// Supplies bytes or a bounded failure for the exact pending identity. A failed
/// supply keeps the request pending and leaves the live terminal unchanged.
///
/// # Safety
/// `transaction` must be live and exclusive, `identity` readable and versioned,
/// and nonempty `data` readable for `data_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_broker_supply_v1(
    transaction: *mut TlTerminalBrokerOutputV1,
    identity: *const TlTerminalBrokerRequestV1,
    outcome: u32,
    data: *const u8,
    data_len: u64,
) -> u32 {
    catch_status(|| {
        if transaction.is_null() || identity.is_null() {
            return STATUS_NULL_POINTER;
        }
        let identity = match unsafe { read_versioned(identity) } {
            Ok(identity) => identity,
            Err(status) => return status,
        };
        let transaction = unsafe { &mut *transaction };
        let Some(request) = &transaction.pending else {
            return STATUS_INVALID_ARGUMENT;
        };
        if identity.has_request != 1
            || identity.reserved != [0; 2]
            || identity.medium != u32::from(request.medium)
            || identity.source_sequence != request.source_sequence
            || identity.source_offset_end != request.source_offset_end
            || identity.ordinal != u32::from(request.ordinal)
            || identity.control_digest != request.control_digest
            || identity.read_offset != request.read_offset
            || identity.read_length != request.read_length.unwrap_or(0)
            || identity.has_read_length != u32::from(request.read_length.is_some())
        {
            return STATUS_INVALID_ARGUMENT;
        }
        let outcome = match outcome {
            BROKER_CAPTURE_BYTES => {
                let bytes = match borrowed_bytes(data, data_len, MAX_BROKER_CAPTURE_BYTES) {
                    Ok(bytes) => bytes,
                    Err(status) => return status,
                };
                BrokerOutcome::Bytes(bytes.to_vec())
            }
            kind if data_len == 0 && data.is_null() => BrokerOutcome::Failure(match kind {
                BROKER_CAPTURE_DENIED => BrokerFailure::Denied,
                BROKER_CAPTURE_UNAVAILABLE => BrokerFailure::Unavailable,
                BROKER_CAPTURE_UNSUPPORTED => BrokerFailure::UnsupportedMedium,
                BROKER_CAPTURE_OVER_LIMIT => BrokerFailure::OverLimit,
                _ => return STATUS_INVALID_ARGUMENT,
            }),
            _ => return STATUS_INVALID_ARGUMENT,
        };
        let capture = BrokerCapture {
            request: request.clone(),
            outcome,
        };
        match transaction.replay(Some(capture)) {
            Ok(()) => STATUS_OK,
            Err(error) => status_for_error(&error),
        }
    })
}

/// Returns the last supply error, including its unsupported byte or parameter
/// when provided by the core. No file reference is included.
///
/// # Safety
/// `transaction` must be live and `out_diagnostic` writable, versioned and empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_broker_diagnostic_v1(
    transaction: *const TlTerminalBrokerOutputV1,
    out_diagnostic: *mut TlTerminalBuffer,
) -> u32 {
    catch_status(|| {
        if transaction.is_null() || out_diagnostic.is_null() {
            return STATUS_NULL_POINTER;
        }
        let capacity = match unsafe { empty_buffer_capacity(out_diagnostic) } {
            Ok(capacity) => capacity,
            Err(status) => return status,
        };
        let Some(error) = &unsafe { &*transaction }.error else {
            return STATUS_OK;
        };
        let buffer = match owned_buffer_with_limit(error.as_bytes().to_vec(), capacity, 8192) {
            Ok(buffer) => buffer,
            Err(status) => return status,
        };
        unsafe { ptr::write_unaligned(out_diagnostic, buffer) };
        STATUS_OK
    })
}

/// Returns a copy of the fully resolved effect frame without changing state.
/// JNI may prepare its Java output before the final commit.
///
/// # Safety
/// `transaction` must be live and exclusive; `out_effect_frame` must be
/// writable, versioned and empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_broker_preview_v1(
    transaction: *mut TlTerminalBrokerOutputV1,
    out_effect_frame: *mut TlTerminalBuffer,
) -> u32 {
    catch_status(|| {
        if transaction.is_null() || out_effect_frame.is_null() {
            return STATUS_NULL_POINTER;
        }
        let capacity = match unsafe { empty_buffer_capacity(out_effect_frame) } {
            Ok(capacity) => capacity,
            Err(status) => return status,
        };
        let item = unsafe { &mut *transaction };
        if item.pending.is_some() {
            return STATUS_UNSUPPORTED;
        }
        let preparation = match item.prepared() {
            Ok(preparation) => preparation,
            Err(error) => return status_for_error(&error),
        };
        let ready = match preparation.finish() {
            Ok(ready) => ready,
            Err(error) => return status_for_error(&error),
        };
        let frame = match owned_buffer_with_limit(
            ready.host_effect_frame_v2().to_vec(),
            capacity,
            MAX_HOST_EFFECT_FRAME_V2_BYTES,
        ) {
            Ok(frame) => frame,
            Err(status) => return status,
        };
        unsafe { ptr::write_unaligned(out_effect_frame, frame) };
        STATUS_OK
    })
}

/// Publishes the V2 Host Effect Frame before committing and returning terminal
/// ownership. On failure the transaction, owner slot, and frame remain unchanged.
///
/// # Safety
/// `transaction` must own a live handle; both outputs must be exclusive,
/// writable and empty, and no saved alias may access the terminal.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_broker_commit_v1(
    transaction: *mut *mut TlTerminalBrokerOutputV1,
    out_terminal: *mut *mut TlTerminal,
    out_effect_frame: *mut TlTerminalBuffer,
) -> u32 {
    catch_status(|| {
        if transaction.is_null() || out_terminal.is_null() || out_effect_frame.is_null() {
            return STATUS_NULL_POINTER;
        }
        let handle = unsafe { ptr::read_unaligned(transaction) };
        if handle.is_null() {
            return STATUS_INVALID_ARGUMENT;
        }
        if let Err(status) = unsafe { require_empty_handle(out_terminal) } {
            return status;
        }
        let capacity = match unsafe { empty_buffer_capacity(out_effect_frame) } {
            Ok(capacity) => capacity,
            Err(status) => return status,
        };
        let item = unsafe { &mut *handle };
        if item.pending.is_some() {
            return STATUS_UNSUPPORTED;
        }
        let preparation = match item.prepared() {
            Ok(preparation) => preparation,
            Err(error) => return status_for_error(&error),
        };
        let ready = match preparation.finish() {
            Ok(ready) => ready,
            Err(error) => return status_for_error(&error),
        };
        let status = unsafe { publish_prepared_host_effects_v2(ready, capacity, out_effect_frame) };
        if status != STATUS_OK {
            return status;
        }
        let item = unsafe { Box::from_raw(handle) };
        unsafe { ptr::write_unaligned(transaction, ptr::null_mut()) };
        unsafe { ptr::write_unaligned(out_terminal, Box::into_raw(item.terminal)) };
        STATUS_OK
    })
}

/// Cancels without applying the Output and restores the same terminal owner.
///
/// # Safety
/// `transaction` must own a live handle and `out_terminal` must be exclusive,
/// writable and empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_broker_cancel_v1(
    transaction: *mut *mut TlTerminalBrokerOutputV1,
    out_terminal: *mut *mut TlTerminal,
) -> u32 {
    catch_status(|| {
        if transaction.is_null() || out_terminal.is_null() {
            return STATUS_NULL_POINTER;
        }
        let handle = unsafe { ptr::read_unaligned(transaction) };
        if handle.is_null() {
            return STATUS_INVALID_ARGUMENT;
        }
        if let Err(status) = unsafe { require_empty_handle(out_terminal) } {
            return status;
        }
        let item = unsafe { Box::from_raw(handle) };
        unsafe { ptr::write_unaligned(transaction, ptr::null_mut()) };
        unsafe { ptr::write_unaligned(out_terminal, Box::into_raw(item.terminal)) };
        STATUS_OK
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_buffer() -> TlTerminalBuffer {
        TlTerminalBuffer {
            abi_version: ABI_VERSION,
            struct_size: struct_size::<TlTerminalBuffer>(),
            data: ptr::null(),
            #[cfg(target_pointer_width = "32")]
            data_pointer_padding: 0,
            len: 0,
        }
    }

    fn config() -> TlTerminalConfigV4 {
        TlTerminalConfigV4 {
            abi_version: ABI_VERSION,
            struct_size: struct_size::<TlTerminalConfigV4>(),
            columns: 10,
            rows: 3,
            initial_source_sequence: 0,
            reserved_v1_compat: [0; 2],
            history_capacity_rows: 20,
            reserved_v2_compat: [0; 2],
            output_profile: OUTPUT_PROFILE_UTF8,
            reserved0: 0,
            reserved_v3_compat: [0; 2],
            width_profile: 2,
            reserved1: 0,
            reserved: [0; 2],
        }
    }

    fn output(sequence: u64, bytes: &[u8]) -> TlTerminalEvent {
        TlTerminalEvent {
            abi_version: ABI_VERSION,
            struct_size: struct_size::<TlTerminalEvent>(),
            source_sequence: sequence,
            kind: EVENT_OUTPUT,
            reserved: 0,
            data: bytes.as_ptr(),
            #[cfg(target_pointer_width = "32")]
            data_pointer_padding: 0,
            data_len: bytes.len() as u64,
            columns: 0,
            rows: 0,
            advance_micros: 0,
        }
    }

    unsafe fn graphics_terminal() -> *mut TlTerminal {
        let mut terminal = ptr::null_mut();
        assert_eq!(
            unsafe { tl_terminal_create_graphics_v1(&config(), &mut terminal) },
            STATUS_OK
        );
        let geometry = termlink_terminal_core::HostGeometryV1 {
            cell_width: 10,
            cell_height: 20,
            ..Default::default()
        }
        .encode();
        let mut event = output(0, &geometry);
        event.kind = EVENT_HOST_GEOMETRY_V1;
        assert_eq!(
            unsafe { tl_terminal_apply_event(terminal, &event) },
            STATUS_OK
        );
        terminal
    }

    #[test]
    fn request_supply_commit_and_cancel_keep_ownership_atomic() {
        unsafe {
            let mut terminal = graphics_terminal();
            let before = (*terminal).inner.clone();
            let output_bytes = b"\x1b_Ga=T,t=f,f=32,s=1,v=1,S=4,i=31;QzovYS5yZ2Jh\x1b\\";
            let mut transaction = ptr::null_mut();
            let mut diagnostic = empty_buffer();
            let mut occupied = ptr::dangling_mut::<TlTerminalBrokerOutputV1>();
            assert_eq!(
                tl_terminal_broker_begin_v1(
                    &mut terminal,
                    &output(1, output_bytes),
                    &mut occupied,
                    &mut diagnostic
                ),
                STATUS_INVALID_ARGUMENT
            );
            assert!(!terminal.is_null() && diagnostic.data.is_null());
            assert_eq!(
                tl_terminal_broker_begin_v1(
                    &mut terminal,
                    &output(1, output_bytes),
                    &mut transaction,
                    &mut diagnostic,
                ),
                STATUS_OK
            );
            assert!(terminal.is_null());
            assert!(diagnostic.data.is_null());
            let mut request = TlTerminalBrokerRequestV1 {
                abi_version: ABI_VERSION,
                struct_size: struct_size::<TlTerminalBrokerRequestV1>(),
                has_request: 0,
                medium: 0,
                source_sequence: 0,
                source_offset_end: 0,
                ordinal: 0,
                control_digest: [0; 32],
                read_offset: 0,
                read_length: 0,
                has_read_length: 0,
                reserved: [0; 2],
            };
            let mut reference = empty_buffer();
            assert_eq!(
                tl_terminal_broker_request_v1(transaction, &mut request, &mut reference),
                STATUS_OK
            );
            assert_eq!(request.has_request, 1);
            assert_eq!(request.medium, u32::from(b'f'));
            assert_eq!(
                std::slice::from_raw_parts(reference.data, reference.len as usize),
                b"C:/a.rgba"
            );
            assert_eq!(tl_terminal_buffer_free(&mut reference), STATUS_OK);
            let original = request;
            request.control_digest[0] ^= 1;
            assert_eq!(
                tl_terminal_broker_supply_v1(
                    transaction,
                    &request,
                    BROKER_CAPTURE_BYTES,
                    [1, 2, 3, 4].as_ptr(),
                    4
                ),
                STATUS_INVALID_ARGUMENT
            );
            request = original;
            assert_eq!(
                tl_terminal_broker_supply_v1(transaction, &request, 99, ptr::null(), 0),
                STATUS_INVALID_ARGUMENT
            );
            assert_eq!(
                tl_terminal_broker_supply_v1(
                    transaction,
                    &request,
                    BROKER_CAPTURE_BYTES,
                    [1, 2, 3].as_ptr(),
                    3
                ),
                STATUS_UNSUPPORTED
            );
            assert_eq!(
                tl_terminal_broker_diagnostic_v1(transaction, &mut diagnostic),
                STATUS_OK
            );
            let diagnostic_text = std::str::from_utf8(std::slice::from_raw_parts(
                diagnostic.data,
                diagnostic.len as usize,
            ))
            .unwrap();
            assert!(diagnostic_text.contains("capture length does not match S"));
            assert!(!diagnostic_text.contains("C:/a.rgba"));
            assert_eq!(tl_terminal_buffer_free(&mut diagnostic), STATUS_OK);
            assert_eq!(
                tl_terminal_broker_supply_v1(
                    transaction,
                    &request,
                    BROKER_CAPTURE_BYTES,
                    [1, 2, 3, 4].as_ptr(),
                    4
                ),
                STATUS_OK
            );
            let mut redacted = request;
            assert_eq!(
                tl_terminal_broker_request_redacted_v1(transaction, &mut redacted),
                STATUS_OK
            );
            assert_eq!(redacted.has_request, 0);
            let mut preview = empty_buffer();
            assert_eq!(
                tl_terminal_broker_preview_v1(transaction, &mut preview),
                STATUS_OK
            );
            assert!(preview.len >= 64);
            assert_eq!(tl_terminal_buffer_free(&mut preview), STATUS_OK);
            let mut occupied_terminal = ptr::dangling_mut::<TlTerminal>();
            let mut rejected_frame = empty_buffer();
            assert_eq!(
                tl_terminal_broker_commit_v1(
                    &mut transaction,
                    &mut occupied_terminal,
                    &mut rejected_frame
                ),
                STATUS_INVALID_ARGUMENT
            );
            assert!(!transaction.is_null() && terminal.is_null() && rejected_frame.data.is_null());
            // Publication failure and panic both retain the transaction and baseline.
            for mode in [1, 2] {
                PENDING_BUFFER_TEST_MODE.with(|cell| cell.set(mode));
                let mut frame = empty_buffer();
                let result =
                    tl_terminal_broker_commit_v1(&mut transaction, &mut terminal, &mut frame);
                assert_eq!(
                    result,
                    if mode == 1 {
                        STATUS_LIMIT_EXCEEDED
                    } else {
                        STATUS_PANIC
                    }
                );
                assert!(terminal.is_null() && !transaction.is_null() && frame.data.is_null());
            }
            let mut frame = empty_buffer();
            assert_eq!(
                tl_terminal_broker_commit_v1(&mut transaction, &mut terminal, &mut frame),
                STATUS_OK
            );
            assert!(transaction.is_null() && !terminal.is_null());
            assert_eq!((*terminal).inner.next_source_sequence(), 2);
            let mut empty_terminal = ptr::null_mut();
            let mut empty_effect = empty_buffer();
            assert_eq!(
                tl_terminal_broker_commit_v1(
                    &mut transaction,
                    &mut empty_terminal,
                    &mut empty_effect
                ),
                STATUS_INVALID_ARGUMENT
            );
            assert_eq!(tl_terminal_buffer_free(&mut frame), STATUS_OK);

            let committed = (*terminal).inner.clone();
            let mut second = ptr::null_mut();
            let mut diagnostic = empty_buffer();
            assert_eq!(
                tl_terminal_broker_begin_v1(
                    &mut terminal,
                    &output(2, output_bytes),
                    &mut second,
                    &mut diagnostic
                ),
                STATUS_OK
            );
            let mut occupied_terminal = ptr::dangling_mut::<TlTerminal>();
            assert_eq!(
                tl_terminal_broker_cancel_v1(&mut second, &mut occupied_terminal),
                STATUS_INVALID_ARGUMENT
            );
            assert!(!second.is_null() && terminal.is_null());
            assert_eq!(
                tl_terminal_broker_cancel_v1(&mut second, &mut terminal),
                STATUS_OK
            );
            assert!(second.is_null());
            assert_eq!((*terminal).inner, committed);
            assert_ne!((*terminal).inner, before);
            tl_terminal_destroy(terminal);
        }
    }

    #[test]
    fn invalid_begin_keeps_owner_and_exposes_unsupported_byte() {
        unsafe {
            let mut terminal = graphics_terminal();
            let original = terminal;
            let mut transaction = ptr::null_mut();
            let mut diagnostic = empty_buffer();
            let invalid = b"\x1b_Ga=x,i=20;\x1b\\";
            let status = tl_terminal_broker_begin_v1(
                &mut terminal,
                &output(1, invalid),
                &mut transaction,
                &mut diagnostic,
            );
            assert_eq!(status, STATUS_UNSUPPORTED);
            assert_eq!(terminal, original);
            assert!(transaction.is_null());
            assert!(!diagnostic.data.is_null());
            let text = std::str::from_utf8(std::slice::from_raw_parts(
                diagnostic.data,
                diagnostic.len as usize,
            ))
            .unwrap();
            // This stream is a direct APC rejection (no macro expansion), so
            // the core deliberately preserves its ordinary V2 envelope.
            assert!(text.starts_with("TL-DIAG/2\n"), "{text}");
            assert!(text.contains("category=unsupported\n"), "{text}");
            assert!(text.contains("family="), "{text}");
            assert!(text.contains("sequence="), "{text}");
            assert!(text.contains("sequence=<ESC>_Ga=x,i=20;"), "{text}");
            assert!(text.contains("hex="), "{text}");
            assert!(text.contains("hex=1B 5F 47"), "{text}");
            assert!(text.contains("start=") && text.contains("end="), "{text}");
            assert!(text.contains("0x"), "{text}");
            assert_eq!(tl_terminal_buffer_free(&mut diagnostic), STATUS_OK);
            tl_terminal_destroy(terminal);
        }
    }
}
