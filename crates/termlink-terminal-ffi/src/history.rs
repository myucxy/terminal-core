//! Tokens never dereference released native pointers. A capture owns its source
//! revision until explicit release or a fixed, non-renewing five-minute lease.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use termlink_terminal_core::MAX_HISTORY_PAGE_V2_BYTES;
use termlink_terminal_core::{
    HistoryCapture, HistoryPageError, HistoryPageRequest, MAX_HISTORY_PAGE_BYTES,
};

pub const STATUS_HISTORY_STALE_CAPTURE: u32 = 16;
pub const STATUS_HISTORY_STALE_REVISION: u32 = 17;
pub const STATUS_HISTORY_STALE_ANCHOR: u32 = 18;
pub const STATUS_HISTORY_PAGE_BUDGET: u32 = 19;
const MAX_CAPTURES: usize = 64;
const MAX_CAPTURE_BYTES: usize = 256 * 1024 * 1024;
const LEASE: Duration = Duration::from_secs(300);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TlTerminalHistoryCaptureV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub capture_id: u64,
    pub owner_id: u64,
    pub revision: u64,
    pub accounted_bytes: u64,
    pub lease_millis: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TlTerminalHistoryRequestV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub capture_id: u64,
    pub owner_id: u64,
    pub revision: u64,
    pub before_line_id: u64,
    pub max_lines: u32,
    pub max_cells: u32,
    pub max_utf8_bytes: u32,
    pub reserved0: u32,
    pub reserved: u64,
}

struct Entry {
    owner: u64,
    expires: Instant,
    capture: HistoryCapture,
}
struct Registry {
    next: u64,
    entries: HashMap<u64, Entry>,
}
impl Registry {
    fn new() -> Self {
        Self {
            next: 1,
            entries: HashMap::new(),
        }
    }
    fn purge(&mut self, now: Instant) {
        self.entries.retain(|_, entry| now < entry.expires);
    }
    fn begin(
        &mut self,
        owner: u64,
        terminal: &Terminal,
        now: Instant,
    ) -> Result<TlTerminalHistoryCaptureV1, u32> {
        self.purge(now);
        let charge = HistoryCapture::allocation_charge(terminal);
        let used: usize = self
            .entries
            .values()
            .map(|entry| entry.capture.accounted_bytes())
            .sum();
        if self.entries.len() >= MAX_CAPTURES
            || charge > MAX_CAPTURE_BYTES - used
            || self.entries.values().any(|entry| entry.owner == owner)
            || self.next == u64::MAX
        {
            return Err(STATUS_LIMIT_EXCEEDED);
        }
        let capture =
            HistoryCapture::capture(terminal).map_err(|error| status_for_error(&error))?;
        let id = self.next;
        let result = TlTerminalHistoryCaptureV1 {
            abi_version: ABI_VERSION,
            struct_size: mem::size_of::<TlTerminalHistoryCaptureV1>() as u32,
            capture_id: id,
            owner_id: owner,
            revision: capture.revision(),
            accounted_bytes: capture.accounted_bytes() as u64,
            lease_millis: LEASE.as_millis() as u64,
        };
        self.entries
            .try_reserve(1)
            .map_err(|_| STATUS_LIMIT_EXCEEDED)?;
        self.entries.insert(
            id,
            Entry {
                owner,
                expires: now + LEASE,
                capture,
            },
        );
        self.next += 1;
        Ok(result)
    }
    fn get(&mut self, id: u64, owner: u64, now: Instant) -> Result<&HistoryCapture, u32> {
        self.purge(now);
        self.entries
            .get(&id)
            .filter(|entry| entry.owner == owner)
            .map(|entry| &entry.capture)
            .ok_or(STATUS_HISTORY_STALE_CAPTURE)
    }
    fn release(&mut self, id: u64, owner: u64, now: Instant) -> u32 {
        if let Err(status) = self.get(id, owner, now) {
            return status;
        }
        self.entries.remove(&id);
        STATUS_OK
    }
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::new()))
}

pub(super) fn new_owner_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    // Never reuse identity even when the allocator reuses a terminal address.
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
        .expect("terminal identity exhausted")
}

#[unsafe(no_mangle)]
pub extern "C" fn tl_terminal_history_capture_v1_struct_size() -> u32 {
    catch_struct_size::<TlTerminalHistoryCaptureV1>()
}
#[unsafe(no_mangle)]
pub extern "C" fn tl_terminal_history_request_v1_struct_size() -> u32 {
    catch_struct_size::<TlTerminalHistoryRequestV1>()
}

/// # Safety
/// `terminal` must be live and exclusively accessible. `out_capture` is a
/// writable, versioned output with all fields after the ABI prefix zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_history_begin_v1(
    terminal: *const TlTerminal,
    out_capture: *mut TlTerminalHistoryCaptureV1,
) -> u32 {
    catch_status(|| {
        if terminal.is_null() || out_capture.is_null() {
            return STATUS_NULL_POINTER;
        }
        // SAFETY: Versioned output and live terminal are required by this API.
        let old: TlTerminalHistoryCaptureV1 = match unsafe { read_versioned_exact(out_capture) } {
            Ok(v) => v,
            Err(e) => return e,
        };
        if old.capture_id != 0
            || old.owner_id != 0
            || old.revision != 0
            || old.accounted_bytes != 0
            || old.lease_millis != 0
        {
            return STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: The caller keeps the terminal live and exclusive during capture.
        let terminal = unsafe { &*terminal };
        let mut registry = match registry().lock() {
            Ok(v) => v,
            Err(_) => return STATUS_PANIC,
        };
        let info = match registry.begin(terminal.history_owner, &terminal.inner, Instant::now()) {
            Ok(v) => v,
            Err(e) => return e,
        };
        // SAFETY: The exact prefix check established the writable record size.
        unsafe { ptr::write_unaligned(out_capture, info) };
        STATUS_OK
    })
}

/// # Safety
/// `request` is readable for its versioned exact size. `out_page` is a writable
/// empty versioned buffer; required-size pointers are writable and nonaliasing.
/// The token can outlive the original terminal. Failures never replace it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_history_page_v1(
    request: *const TlTerminalHistoryRequestV1,
    out_page: *mut TlTerminalBuffer,
    out_required_cells: *mut u32,
    out_required_utf8: *mut u32,
) -> u32 {
    // SAFETY: The public contract is forwarded unchanged.
    unsafe {
        history_page(
            request,
            out_page,
            out_required_cells,
            out_required_utf8,
            false,
        )
    }
}

/// Exports History Page V2 containing Render V5, for graphics or text captures.
/// # Safety
/// Uses the same exact request and empty output contract as history page V1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_history_page_v2(
    request: *const TlTerminalHistoryRequestV1,
    out_page: *mut TlTerminalBuffer,
    out_required_cells: *mut u32,
    out_required_utf8: *mut u32,
) -> u32 {
    // SAFETY: The public contract is forwarded unchanged.
    unsafe {
        history_page(
            request,
            out_page,
            out_required_cells,
            out_required_utf8,
            true,
        )
    }
}

unsafe fn history_page(
    request: *const TlTerminalHistoryRequestV1,
    out_page: *mut TlTerminalBuffer,
    out_required_cells: *mut u32,
    out_required_utf8: *mut u32,
    graphics: bool,
) -> u32 {
    catch_status(|| {
        if request.is_null()
            || out_page.is_null()
            || out_required_cells.is_null()
            || out_required_utf8.is_null()
        {
            return STATUS_NULL_POINTER;
        }
        // SAFETY: Readable versioned input and writable empty buffer are required.
        let query: TlTerminalHistoryRequestV1 = match unsafe { read_versioned_exact(request) } {
            Ok(v) => v,
            Err(e) => return e,
        };
        // SAFETY: Checked by the versioned output helper before publication.
        let capacity = match unsafe { empty_buffer_capacity(out_page) } {
            Ok(v) => v,
            Err(e) => return e,
        };
        // SAFETY: Output scalars are writable and do not alias the inputs/buffer.
        unsafe {
            ptr::write_unaligned(out_required_cells, 0);
            ptr::write_unaligned(out_required_utf8, 0);
        }
        if query.reserved0 != 0
            || query.reserved != 0
            || query.capture_id == 0
            || query.owner_id == 0
        {
            return STATUS_INVALID_ARGUMENT;
        }
        let mut registry = match registry().lock() {
            Ok(v) => v,
            Err(_) => return STATUS_PANIC,
        };
        let capture = match registry.get(query.capture_id, query.owner_id, Instant::now()) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let request = HistoryPageRequest {
            revision: query.revision,
            before_line_id: query.before_line_id,
            max_lines: query.max_lines,
            max_cells: query.max_cells,
            max_utf8_bytes: query.max_utf8_bytes,
        };
        let page = match if graphics {
            capture.page_v5(request)
        } else {
            capture.page(request)
        } {
            Ok(v) => v,
            Err(HistoryPageError::BudgetExceeded {
                required_cells,
                required_utf8_bytes,
            }) => {
                // SAFETY: These outputs were validated before processing the request.
                unsafe {
                    ptr::write_unaligned(out_required_cells, required_cells);
                    ptr::write_unaligned(out_required_utf8, required_utf8_bytes);
                }
                return STATUS_HISTORY_PAGE_BUDGET;
            }
            Err(error) => return history_status(error),
        };
        let bytes = match if graphics {
            page.encode_v2(query.capture_id)
        } else {
            page.encode(query.capture_id)
        } {
            Ok(v) => v,
            Err(e) => return history_status(e),
        };
        let limit = if graphics {
            MAX_HISTORY_PAGE_V2_BYTES
        } else {
            MAX_HISTORY_PAGE_BYTES
        };
        let buffer = match owned_buffer_with_limit(bytes, capacity, limit) {
            Ok(v) => v,
            Err(e) => return e,
        };
        // SAFETY: Empty output capacity was checked; transfer unique ownership.
        unsafe { ptr::write_unaligned(out_page, buffer) };
        STATUS_OK
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn tl_terminal_history_release_v1(capture_id: u64, owner_id: u64) -> u32 {
    catch_status(|| match registry().lock() {
        Ok(mut registry) => registry.release(capture_id, owner_id, Instant::now()),
        Err(_) => STATUS_PANIC,
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn tl_terminal_history_check_v1(
    capture_id: u64,
    owner_id: u64,
    revision: u64,
) -> u32 {
    catch_status(|| {
        let mut registry = match registry().lock() {
            Ok(v) => v,
            Err(_) => return STATUS_PANIC,
        };
        match registry.get(capture_id, owner_id, Instant::now()) {
            Ok(capture) if capture.revision() == revision => STATUS_OK,
            Ok(_) => STATUS_HISTORY_STALE_REVISION,
            Err(status) => status,
        }
    })
}

/// Frozen viewport projection, including the captured live tail. Unlike page
/// pagination, projection boundaries have no sealed-logical-line meaning.
/// # Safety
/// Output buffer must be writable, versioned and empty; scalar outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_history_viewport_v1(
    capture_id: u64,
    owner_id: u64,
    revision: u64,
    offset_rows: u32,
    out_frame: *mut TlTerminalBuffer,
    out_rows: *mut u32,
    out_generation: *mut u64,
) -> u32 {
    // SAFETY: The public contract is forwarded unchanged.
    unsafe {
        history_viewport(
            capture_id,
            owner_id,
            revision,
            offset_rows,
            out_frame,
            out_rows,
            out_generation,
            false,
        )
    }
}

/// Exports a frozen Render V5 viewport, for graphics or text captures.
/// # Safety
/// Uses the same token and empty output contract as history viewport V1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_history_viewport_v2(
    capture_id: u64,
    owner_id: u64,
    revision: u64,
    offset_rows: u32,
    out_frame: *mut TlTerminalBuffer,
    out_rows: *mut u32,
    out_generation: *mut u64,
) -> u32 {
    // SAFETY: The public contract is forwarded unchanged.
    unsafe {
        history_viewport(
            capture_id,
            owner_id,
            revision,
            offset_rows,
            out_frame,
            out_rows,
            out_generation,
            true,
        )
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn history_viewport(
    capture_id: u64,
    owner_id: u64,
    revision: u64,
    offset_rows: u32,
    out_frame: *mut TlTerminalBuffer,
    out_rows: *mut u32,
    out_generation: *mut u64,
    graphics: bool,
) -> u32 {
    catch_status(|| {
        if out_frame.is_null() || out_rows.is_null() || out_generation.is_null() {
            return STATUS_NULL_POINTER;
        }
        // SAFETY: Caller provides versioned empty output.
        let capacity = match unsafe { empty_buffer_capacity(out_frame) } {
            Ok(v) => v,
            Err(e) => return e,
        };
        let mut registry = match registry().lock() {
            Ok(v) => v,
            Err(_) => return STATUS_PANIC,
        };
        let capture = match registry.get(capture_id, owner_id, Instant::now()) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if revision != capture.revision() {
            return STATUS_HISTORY_STALE_REVISION;
        }
        let (frame, rows, generation) = match if graphics {
            capture.viewport_v5(offset_rows)
        } else {
            capture.viewport(offset_rows)
        } {
            Ok(v) => v,
            Err(e) => return status_for_error(&e),
        };
        let limit = if graphics {
            MAX_RENDER_FRAME_V5_BYTES
        } else {
            MAX_RENDER_FRAME_V4_BYTES
        };
        let buffer = match owned_buffer_with_limit(frame, capacity, limit) {
            Ok(v) => v,
            Err(e) => return e,
        };
        // SAFETY: All outputs are exclusively writable for this call.
        unsafe {
            ptr::write_unaligned(out_frame, buffer);
            ptr::write_unaligned(out_rows, rows);
            ptr::write_unaligned(out_generation, generation);
        }
        STATUS_OK
    })
}

/// Resolve a primary-screen logical anchor in an immutable capture.
/// # Safety
/// `out_offset_rows` must be writable. It is untouched on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_terminal_history_resolve_anchor_v1(
    capture_id: u64,
    owner_id: u64,
    revision: u64,
    line_id: u64,
    cell_offset: u32,
    out_offset_rows: *mut u32,
) -> u32 {
    catch_status(|| {
        if out_offset_rows.is_null() {
            return STATUS_NULL_POINTER;
        }
        let mut registry = match registry().lock() {
            Ok(v) => v,
            Err(_) => return STATUS_PANIC,
        };
        let capture = match registry.get(capture_id, owner_id, Instant::now()) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if revision != capture.revision() {
            return STATUS_HISTORY_STALE_REVISION;
        }
        match capture.offset_for_anchor(line_id, cell_offset) {
            Ok(offset) => {
                // SAFETY: caller supplies writable scalar storage.
                unsafe {
                    ptr::write_unaligned(out_offset_rows, offset);
                }
                STATUS_OK
            }
            Err(error) => history_status(error),
        }
    })
}

fn history_status(error: HistoryPageError) -> u32 {
    match error {
        HistoryPageError::Terminal(error) => status_for_error(&error),
        HistoryPageError::InvalidRequest => STATUS_INVALID_ARGUMENT,
        HistoryPageError::StaleRevision => STATUS_HISTORY_STALE_REVISION,
        HistoryPageError::StaleAnchor => STATUS_HISTORY_STALE_ANCHOR,
        HistoryPageError::BudgetExceeded { .. } => STATUS_HISTORY_PAGE_BUDGET,
        HistoryPageError::WireBudgetExceeded => STATUS_LIMIT_EXCEEDED,
        HistoryPageError::MalformedPage => STATUS_MALFORMED_SNAPSHOT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termlink_terminal_core::{HISTORY_LATEST_ANCHOR, HistoryPage};

    fn source() -> Terminal {
        let mut terminal = Terminal::new_with_options(
            8,
            3,
            1,
            TerminalOptions {
                history_capacity_rows: 30,
            },
        )
        .unwrap();
        terminal
            .apply_event(Event::Output {
                source_sequence: 1,
                bytes: b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\n",
            })
            .unwrap();
        terminal
    }

    #[test]
    fn registry_limits_expiry_and_wrong_owner_do_not_replace_a_capture() {
        let terminal = source();
        let now = Instant::now();
        let mut registry = Registry::new();
        let first = registry.begin(1, &terminal, now).unwrap();
        assert!(matches!(
            registry.begin(1, &terminal, now),
            Err(STATUS_LIMIT_EXCEEDED)
        ));
        assert!(matches!(
            registry.get(first.capture_id, 2, now),
            Err(STATUS_HISTORY_STALE_CAPTURE)
        ));
        assert_eq!(
            registry.release(first.capture_id, 2, now),
            STATUS_HISTORY_STALE_CAPTURE
        );
        assert!(
            registry
                .get(first.capture_id, 1, now + LEASE - Duration::from_millis(1))
                .is_ok()
        );
        assert!(matches!(
            registry.get(first.capture_id, 1, now + LEASE),
            Err(STATUS_HISTORY_STALE_CAPTURE)
        ));
        let second = registry.begin(1, &terminal, now + LEASE).unwrap();
        assert_ne!(first.capture_id, second.capture_id);
        assert_eq!(
            registry.release(first.capture_id, 1, now + LEASE),
            STATUS_HISTORY_STALE_CAPTURE
        );
        for owner in 2..=MAX_CAPTURES as u64 {
            registry.begin(owner, &terminal, now + LEASE).unwrap();
        }
        assert!(matches!(
            registry.begin(1000, &terminal, now + LEASE),
            Err(STATUS_LIMIT_EXCEEDED)
        ));
        assert_eq!(
            registry.release(second.capture_id, 1, now + LEASE),
            STATUS_OK
        );
        assert!(registry.begin(1000, &terminal, now + LEASE).is_ok());
        assert_eq!(registry.entries.len(), MAX_CAPTURES);
    }

    #[test]
    fn capture_memory_is_admitted_before_cloning_and_tokens_never_wrap() {
        let now = Instant::now();
        let terminal = Terminal::new_with_options(
            512,
            512,
            1,
            TerminalOptions {
                history_capacity_rows: 0,
            },
        )
        .unwrap();
        assert!(HistoryCapture::capture(&terminal).is_err());
        let mut registry = Registry::new();
        assert!(matches!(
            registry.begin(1, &terminal, now),
            Err(STATUS_LIMIT_EXCEEDED)
        ));
        assert!(registry.entries.is_empty());
        registry.next = u64::MAX;
        assert!(matches!(
            registry.begin(1, &source(), now),
            Err(STATUS_LIMIT_EXCEEDED)
        ));
    }

    #[test]
    fn abi_pages_survive_destroy_retry_and_reject_stale_inputs() {
        assert_eq!(tl_terminal_history_capture_v1_struct_size(), 48);
        assert_eq!(tl_terminal_history_request_v1_struct_size(), 64);
        let terminal = Box::into_raw(Box::new(TlTerminal {
            inner: source(),
            history_owner: new_owner_id(),
        }));
        let mut info = TlTerminalHistoryCaptureV1 {
            abi_version: ABI_VERSION,
            struct_size: 48,
            ..Default::default()
        };
        // SAFETY: The test owns the live terminal and all ABI buffers exclusively.
        unsafe {
            assert_eq!(tl_terminal_history_begin_v1(terminal, &mut info), STATUS_OK);
            assert_eq!(
                tl_terminal_history_begin_v1(terminal, &mut info),
                STATUS_INVALID_ARGUMENT
            );
            tl_terminal_destroy(terminal);
            let mut query = TlTerminalHistoryRequestV1 {
                abi_version: ABI_VERSION,
                struct_size: 64,
                capture_id: info.capture_id,
                owner_id: info.owner_id,
                revision: info.revision,
                before_line_id: HISTORY_LATEST_ANCHOR,
                max_lines: 128,
                max_cells: 65536,
                max_utf8_bytes: 4 * 1024 * 1024,
                ..Default::default()
            };
            let mut buffer: TlTerminalBuffer = mem::zeroed();
            buffer.abi_version = ABI_VERSION;
            buffer.struct_size = mem::size_of::<TlTerminalBuffer>() as u32;
            let (mut required_cells, mut required_utf8) = (0, 0);
            let mut original = Vec::new();
            for _ in 0..2 {
                assert_eq!(
                    tl_terminal_history_page_v1(
                        &query,
                        &mut buffer,
                        &mut required_cells,
                        &mut required_utf8
                    ),
                    STATUS_OK
                );
                let bytes = std::slice::from_raw_parts(buffer.data, buffer.len as usize).to_vec();
                HistoryPage::decode(&bytes, info.capture_id).unwrap();
                if original.is_empty() {
                    original = bytes;
                } else {
                    assert_eq!(bytes, original);
                }
                assert_eq!(
                    tl_terminal_history_page_v1(
                        &query,
                        &mut buffer,
                        &mut required_cells,
                        &mut required_utf8
                    ),
                    STATUS_INVALID_ARGUMENT
                );
                assert_eq!(tl_terminal_buffer_free(&mut buffer), STATUS_OK);
            }
            query.max_cells = 1;
            assert_eq!(
                tl_terminal_history_page_v1(
                    &query,
                    &mut buffer,
                    &mut required_cells,
                    &mut required_utf8
                ),
                STATUS_HISTORY_PAGE_BUDGET
            );
            assert_eq!((required_cells, required_utf8), (8, 5));
            query.max_cells = 65536;
            query.revision += 1;
            assert_eq!(
                tl_terminal_history_page_v1(
                    &query,
                    &mut buffer,
                    &mut required_cells,
                    &mut required_utf8
                ),
                STATUS_HISTORY_STALE_REVISION
            );
            assert_eq!((required_cells, required_utf8), (0, 0));
            query.revision = info.revision;
            query.before_line_id = 999999;
            assert_eq!(
                tl_terminal_history_page_v1(
                    &query,
                    &mut buffer,
                    &mut required_cells,
                    &mut required_utf8
                ),
                STATUS_HISTORY_STALE_ANCHOR
            );
            query.owner_id += 1;
            assert_eq!(
                tl_terminal_history_page_v1(
                    &query,
                    &mut buffer,
                    &mut required_cells,
                    &mut required_utf8
                ),
                STATUS_HISTORY_STALE_CAPTURE
            );
            assert!(buffer.data.is_null());
            assert_eq!(
                tl_terminal_history_release_v1(info.capture_id, info.owner_id),
                STATUS_OK
            );
            assert_eq!(
                tl_terminal_history_release_v1(info.capture_id, info.owner_id),
                STATUS_HISTORY_STALE_CAPTURE
            );
        }
    }
}
