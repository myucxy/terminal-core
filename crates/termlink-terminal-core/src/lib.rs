//! Deterministic TermLink terminal state engine.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

mod charset;
mod crc32c;
mod delta;
mod delta_journal;
#[cfg(test)]
mod delta_tests;
mod diagnostic;
pub use delta::{
    DeltaApply, DeltaCheckpoint, DeltaContext, DeltaError, DeltaStateInfo, MAX_STATE_DELTA_BYTES,
    STATE_DELTA_PROFILE_STANDALONE_C1_V1, STATE_DELTA_PROFILE_UNICODE16_STANDALONE_C1_V1,
    STATE_DELTA_PROFILE_UNICODE16_V1, STATE_DELTA_PROFILE_V1, apply_delta_v1, export_delta_v1,
};
pub use delta_journal::{DeltaJournal, DeltaJournalConfig, DeltaJournalInfo};
pub mod drcs;
pub mod drcs_bank;
mod extended_queries;
pub mod graphics_layout;
pub mod graphics_png;
pub mod graphics_resources;
pub mod graphics_state;
pub mod graphics_zlib;
mod history_frame;
mod history_page;
mod host_effect;
mod host_geometry;
#[cfg(test)]
mod host_geometry_tests;
mod host_signals;
mod host_theme;
mod shell_integration;
pub mod sixel;
pub use host_signals::{HOST_SIGNALS_V1_BYTES, HostSignalsV1};
mod base64;
mod clipboard;
pub mod kitty_graphics;
mod legacy_projection;
pub use legacy_projection::MAX_LEGACY_PROJECTION_BYTES;
pub use shell_integration::{MAX_SHELL_STATE_BYTES, ShellCommand, ShellIntegration, ShellPoint};
#[cfg(test)]
mod host_theme_tests;
mod input;
mod legacy_charset_data;
mod legacy_charsets;
mod line_rendition;
#[cfg(test)]
mod macro_tests;
mod macros;
mod modes;
mod osc;
#[cfg(test)]
mod page_tests;
mod pages;
mod parser;
mod query_reply;
mod render_frame;
pub mod render_frame_v5;
mod screen;
mod sgr_stack;
mod snapshot;
mod state_reports;
mod tabs;
mod terminal_drcs;
#[cfg(test)]
mod terminal_drcs_tests;
mod terminal_graphics;
#[cfg(test)]
mod terminal_graphics_edit_tests;
mod terminal_graphics_motion;
mod terminal_sixel;
mod traditional;
mod unicode16;
mod unicode_width;
mod width_profile;
pub use history_page::{
    HISTORY_LATEST_ANCHOR, HistoryCapture, HistoryLine, HistoryPage, HistoryPageError,
    HistoryPageRequest, MAX_HISTORY_CAPTURE_BYTES, MAX_HISTORY_GRAPHICS_CAPTURE_BYTES,
    MAX_HISTORY_PAGE_BYTES, MAX_HISTORY_PAGE_CELLS, MAX_HISTORY_PAGE_LINES, MAX_HISTORY_PAGE_UTF8,
    MAX_HISTORY_PAGE_V2_BYTES,
};
pub use host_geometry::{HOST_GEOMETRY_V1_BYTES, HostGeometryV1};
pub use host_theme::{HOST_THEME_V1_BYTES, HostThemeV1};
pub use line_rendition::LineRendition;
pub use width_profile::WidthProfile;

use charset::{CharsetSize, CharsetState, GraphicRegister, RectangleFillError};
pub use host_effect::{
    HOST_EFFECT_FRAME_V1_FORMAT_VERSION, HOST_EFFECT_FRAME_V2_FORMAT_VERSION,
    MAX_HOST_EFFECT_FRAME_V1_BYTES, MAX_HOST_EFFECT_FRAME_V2_BYTES,
    MAX_HOST_EFFECT_PAYLOAD_V2_BYTES, MAX_HOST_EFFECT_REPLY_V2_BYTES, MAX_HOST_EFFECTS_PER_CAPTURE,
};
pub use input::{
    CellPosition, InputEncodeError, InputEvent, Key, KeyEvent, KeypadKey, KittyFunctionalKey,
    KittyKey, KittyKeyEvent, KittyKeyEventType, KittyKeyEventValidationError, KittyModifiers,
    MAX_DEFAULT_MOUSE_CELL_COORDINATE, MAX_KITTY_ASSOCIATED_TEXT_BYTES,
    MAX_KITTY_ASSOCIATED_TEXT_SCALARS, MAX_MOUSE_PIXEL_COORDINATE, MAX_PASTE_BYTES,
    MAX_UTF8_MOUSE_CELL_COORDINATE, Modifiers, MouseAction, MouseButton, MouseButtons, MouseEvent,
    MousePosition, MouseWheelDirection, PasteEvent, PixelPosition, Win32KeyEvent,
};
use modes::{AttributeChangeExtent, KeyboardProtocolStack, Margins, ModeState};
pub use osc::{
    DefaultColorSlot, Hyperlink, HyperlinkParam, HyperlinkRef, OscBaseline, OscColor, OscState,
};
use osc::{OscError, TitleTarget};
use parser::{
    Action, CsiSequence, DcsSequence, EscSequence, OscSequence, PARAM_MISSING, ParameterSeparator,
    ParseError, ParserState, Sequence,
};
pub use render_frame::{
    MAX_RENDER_FRAME_V1_BYTES, MAX_RENDER_FRAME_V2_BYTES, MAX_RENDER_FRAME_V3_BYTES,
    MAX_RENDER_FRAME_V4_BYTES, RENDER_FRAME_V1_FORMAT_VERSION, RENDER_FRAME_V2_FORMAT_VERSION,
    RENDER_FRAME_V3_FORMAT_VERSION, RENDER_FRAME_V4_FORMAT_VERSION,
};
use screen::{Screen, ScrollCause};
use tabs::TabStops;
use terminal_graphics_motion::GraphicMotion;
use unicode_width::scalar_width;

pub const MAX_COLUMNS: u32 = 512;
pub const MAX_ROWS: u32 = 512;
pub const MAX_CELLS: usize = 262_144;
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_BROKER_TRANSACTION_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
/// Additive graphics profile; legacy C/JNI APIs keep their original 64 MiB admission limit.
pub const MAX_GRAPHICS_SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_GRAPHEME_BYTES: usize = 14;
pub const MAX_CAPTURE_EVENT_COUNT: usize = 256;
pub const MAX_CAPTURE_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
pub use diagnostic::MAX_DIAGNOSTIC_V1_BYTES;

/// Selects how output bytes in the C1 range are interpreted.
///
/// The default profile preserves TermLink's UTF-8-only behavior. The
/// standalone-C1 profile additionally accepts ECMA-48's one-byte C1 aliases,
/// but only at a UTF-8 scalar boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum OutputProfile {
    #[default]
    Utf8 = 0,
    Utf8WithStandaloneC1 = 1,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum CursorShape {
    #[default]
    Block = 0,
    Underline = 1,
    Bar = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum UnderlineStyle {
    #[default]
    None = 0,
    Single = 1,
    Double = 2,
    Curly = 3,
    Dotted = 4,
    Dashed = 5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Attributes {
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub underline_style: UnderlineStyle,
    pub blink: bool,
    pub inverse: bool,
    pub conceal: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub foreground: Color,
    pub background: Color,
    pub underline_color: Color,
}

impl Attributes {
    #[must_use]
    pub fn is_underlined(self) -> bool {
        self.underline_style != UnderlineStyle::None
    }
}

impl Default for Attributes {
    fn default() -> Self {
        Self {
            bold: false,
            faint: false,
            italic: false,
            underline_style: UnderlineStyle::None,
            blink: false,
            inverse: false,
            conceal: false,
            strikethrough: false,
            overline: false,
            foreground: Color::Default,
            background: Color::Default,
            underline_color: Color::Default,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cell {
    pub character: char,
    pub combining: String,
    pub width: u8,
    pub continuation: bool,
    pub attributes: Attributes,
    pub protected: bool,
    pub hyperlink: Option<HyperlinkRef>,
    /// Immutable downloaded glyph; never encoded as a Unicode private-use character.
    pub glyph: Option<drcs_bank::GlyphReference>,
}

impl Cell {
    pub(crate) fn blank(attributes: Attributes) -> Self {
        Self {
            character: ' ',
            combining: String::new(),
            width: 1,
            continuation: false,
            attributes,
            protected: false,
            hyperlink: None,
            glyph: None,
        }
    }

    pub(crate) fn lead(character: char, width: u8, attributes: Attributes) -> Self {
        Self::lead_with_presentation(character, width, attributes, false, None)
    }

    pub(crate) fn lead_with_presentation(
        character: char,
        width: u8,
        attributes: Attributes,
        protected: bool,
        hyperlink: Option<HyperlinkRef>,
    ) -> Self {
        Self {
            character,
            combining: String::new(),
            width,
            continuation: false,
            attributes,
            protected,
            hyperlink,
            glyph: None,
        }
    }

    pub(crate) fn continuation_with_presentation(
        attributes: Attributes,
        protected: bool,
        hyperlink: Option<HyperlinkRef>,
    ) -> Self {
        Self {
            character: ' ',
            combining: String::new(),
            width: 0,
            continuation: true,
            attributes,
            protected,
            hyperlink,
            glyph: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn continuation(attributes: Attributes) -> Self {
        Self::continuation_with_presentation(attributes, false, None)
    }

    pub(crate) fn continuation_from(lead: &Self) -> Self {
        Self::continuation_with_presentation(lead.attributes, lead.protected, lead.hyperlink)
    }

    #[must_use]
    pub fn text(&self) -> String {
        if self.continuation {
            String::new()
        } else {
            let mut text = String::with_capacity(self.grapheme_bytes());
            text.push(self.character);
            text.push_str(&self.combining);
            text
        }
    }

    pub(crate) fn grapheme_bytes(&self) -> usize {
        self.character.len_utf8() + self.combining.len()
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::blank(Attributes::default())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Utf8Decoder {
    pending: [u8; 4],
    len: u8,
    expected: u8,
}

impl Utf8Decoder {
    fn clear(&mut self) {
        *self = Self::default();
    }
}

/// A bounded UTF-8 continuation tracker for non-rendered control-string
/// payloads. Rendered Ground text continues to use `Utf8Decoder`, which also
/// owns the scalar bytes needed for decoding.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct StringUtf8Boundary {
    remaining: u8,
}

impl StringUtf8Boundary {
    fn clear(&mut self) {
        self.remaining = 0;
    }

    /// Observes one physical payload byte and returns whether that byte began
    /// at a UTF-8 boundary. A continuation expected by the preceding lead byte
    /// always wins over standalone-C1 recognition.
    fn observe(&mut self, byte: u8) -> bool {
        if self.remaining != 0 {
            if byte & 0xc0 == 0x80 {
                self.remaining -= 1;
                return false;
            }
            self.clear();
        }

        self.remaining = utf8_sequence_length(byte)
            .map(|length| length - 1)
            .unwrap_or(0);
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RepeatSource {
    character: char,
    combining: String,
    width: u8,
    glyph: Option<drcs_bank::GlyphReference>,
}

impl RepeatSource {
    fn new(character: char) -> Self {
        Self {
            character,
            combining: String::new(),
            width: scalar_width(character),
            glyph: None,
        }
    }

    fn append(&mut self, character: char, profile: WidthProfile) -> Result<(), TerminalError> {
        if self.character.len_utf8() + self.combining.len() + character.len_utf8()
            > profile.max_grapheme_bytes()
        {
            return Err(TerminalError::Unsupported("grapheme size limit"));
        }
        self.combining
            .try_reserve_exact(character.len_utf8())
            .map_err(|_| TerminalError::LimitExceeded)?;
        self.combining.push(character);
        self.width = profile.grapheme_width(self.character, &self.combining);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Terminal {
    columns: u32,
    rows: u32,
    main: Screen,
    alternate: Screen,
    pages: pages::PageStore,
    host_geometry: HostGeometryV1,
    shell_integration: ShellIntegration,
    host_signals: HostSignalsV1,
    graphics: Option<graphics_state::GraphicsState>,
    kitty_transfer: kitty_graphics::TransferDecoder,
    broker_replay: Option<BrokerReplay>,
    legacy_projection: Option<legacy_projection::Projection>,
    active_alternate: bool,
    cursor_visible: bool,
    cursor_shape: CursorShape,
    cursor_blinking: bool,
    modes: ModeState,
    main_keyboard_protocol: KeyboardProtocolStack,
    alternate_keyboard_protocol: KeyboardProtocolStack,
    margins: Margins,
    tabs: TabStops,
    charset: CharsetState,
    user_preferred_charset: charset::CharsetDesignation,
    main_saved_charset: CharsetState,
    alternate_saved_charset: CharsetState,
    osc: OscState,
    next_source_sequence: u64,
    logical_time_micros: u64,
    synchronized_output_deadline_micros: Option<u64>,
    output_profile: OutputProfile,
    parser: ParserState,
    utf8: Utf8Decoder,
    string_utf8: StringUtf8Boundary,
    repeat_source: Option<RepeatSource>,
    sgr_stack: sgr_stack::SgrStack,
    macros: macros::MacroStore,
    macro_execution: macros::MacroExecution,
    failed_closed: bool,
    diagnostics: diagnostic::OutputDiagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event<'a> {
    HostTheme {
        source_sequence: u64,
        bytes: &'a [u8],
    },
    HostGeometry {
        source_sequence: u64,
        geometry: HostGeometryV1,
    },
    Output {
        source_sequence: u64,
        bytes: &'a [u8],
    },
    Resize {
        source_sequence: u64,
        columns: u32,
        rows: u32,
    },
    Advance {
        source_sequence: u64,
        micros: u64,
    },
    Gap {
        source_sequence: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BrokerReplay {
    captures: Vec<Arc<kitty_graphics::BrokerCapture>>,
    next_ordinal: u16,
    pending: Option<kitty_graphics::BrokerRequest>,
}

impl Event<'_> {
    fn source_sequence(self) -> u64 {
        match self {
            Self::HostTheme {
                source_sequence, ..
            }
            | Self::Output {
                source_sequence, ..
            }
            | Self::Resize {
                source_sequence, ..
            }
            | Self::Advance {
                source_sequence, ..
            }
            | Self::Gap { source_sequence } => source_sequence,
            Self::HostGeometry {
                source_sequence, ..
            } => source_sequence,
        }
    }
}

struct EffectContext<'a> {
    send_c1: bool,
    capture: &'a mut dyn host_effect::CaptureSink,
    source_sequence: u64,
    source_offset_end: u32,
}

impl EffectContext<'_> {
    fn reborrow(&mut self) -> EffectContext<'_> {
        EffectContext {
            send_c1: self.send_c1,
            capture: &mut *self.capture,
            source_sequence: self.source_sequence,
            source_offset_end: self.source_offset_end,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalError {
    InvalidDimensions,
    LimitExceeded,
    ProtocolResourceLimit(&'static str),
    SequenceMismatch {
        expected: u64,
        actual: u64,
    },
    SequenceExhausted,
    FailedClosed,
    HostEffectVersionRequired,
    BrokerCaptureRequired,
    RenderFrameVersionRequired,
    Unsupported(&'static str),
    UnsupportedPayload {
        reason: &'static str,
        offset: usize,
        byte: Option<u8>,
    },
    UnsupportedDecodedPayload {
        domain: &'static str,
        reason: &'static str,
        offset: usize,
        byte: u8,
    },
    Gap,
    SnapshotNotEligible,
    PresentationDeferred,
    MalformedSnapshot,
    IncompatibleSnapshot,
    TimeOverflow,
}

impl Display for TerminalError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDimensions => formatter.write_str("invalid terminal dimensions"),
            Self::LimitExceeded => formatter.write_str("terminal limit exceeded"),
            Self::ProtocolResourceLimit(reason) => formatter.write_str(reason),
            Self::UnsupportedPayload { reason, offset, byte } => {
                write!(formatter, "{reason}; retained payload byte {offset}: ")?;
                match byte {
                    Some(byte @ 0x20..=0x7e) => write!(formatter, "0x{byte:02X} U+{byte:04X} '{}'", char::from(*byte)),
                    Some(byte) => write!(formatter, "0x{byte:02X}"),
                    None => formatter.write_str("end of payload"),
                }
            }
            Self::UnsupportedDecodedPayload { domain, reason, offset, byte } => {
                write!(formatter, "{reason}; decoded {domain} byte {offset}: 0x{byte:02X} (not an original stream offset)")
            }
            Self::SequenceMismatch { expected, actual } => {
                write!(
                    formatter,
                    "source sequence mismatch: expected {expected}, got {actual}"
                )
            }
            Self::SequenceExhausted => formatter.write_str("source sequence exhausted"),
            Self::FailedClosed => formatter.write_str("terminal has failed closed"),
            Self::HostEffectVersionRequired => {
                formatter.write_str("host effect frame V2 is required for this terminal reply")
            }
            Self::BrokerCaptureRequired => formatter.write_str("Kitty broker capture is required"),
            Self::RenderFrameVersionRequired => formatter
                .write_str("a newer render frame version is required for this terminal profile or presentation state"),
            Self::Unsupported(feature) => {
                write!(formatter, "unsupported terminal input: {feature}")
            }
            Self::Gap => formatter.write_str("source gap encountered"),
            Self::SnapshotNotEligible => {
                formatter.write_str("snapshot requires a complete parser and UTF-8 boundary")
            }
            Self::PresentationDeferred => {
                formatter.write_str("presentation is deferred by synchronized output")
            }
            Self::MalformedSnapshot => formatter.write_str("malformed terminal snapshot"),
            Self::IncompatibleSnapshot => formatter.write_str("incompatible terminal snapshot"),
            Self::TimeOverflow => formatter.write_str("logical terminal time overflow"),
        }
    }
}

impl Display for InputEncodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TerminalFailedClosed => formatter.write_str("terminal has failed closed"),
            Self::Win32InputModeDisabled => {
                formatter.write_str("Win32 input event requires DEC private mode 9001")
            }
            Self::UnsupportedVt52Key(key) => write!(formatter, "key {key:?} has no VT52 encoding"),
            Self::KittyKeyEventRequired => formatter.write_str(
                "legacy key event is ambiguous while Kitty keyboard enhancements are active",
            ),
            Self::InvalidKittyFlags { flags } => {
                write!(formatter, "invalid Kitty keyboard flags 0x{flags:02x}")
            }
            Self::KittyAssociatedTextTooLarge {
                actual_bytes,
                maximum_bytes,
                actual_scalars,
                maximum_scalars,
            } => write!(
                formatter,
                "Kitty associated text is {actual_bytes} bytes/{actual_scalars} scalars; maximum is {maximum_bytes} bytes/{maximum_scalars} scalars"
            ),
            Self::KittyTextContainsControl(character) => write!(
                formatter,
                "Kitty key text contains control character U+{:04X}",
                u32::from(*character)
            ),
            Self::InvalidKittyKeyEvent(reason) => {
                write!(formatter, "invalid Kitty key event: {reason:?}")
            }
            Self::UnsupportedKittyLegacyEncoding { key, modifiers } => write!(
                formatter,
                "Kitty key {key:?} with modifiers {modifiers:?} has no lossless legacy encoding"
            ),
            Self::EmptyPaste => formatter.write_str("paste payload is empty"),
            Self::PasteTooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "paste payload is {actual} bytes; maximum is {maximum}"
                )
            }
            Self::UnsupportedModifiers { key, modifiers } => {
                write!(
                    formatter,
                    "unsupported modifiers {modifiers:?} for key {key:?}"
                )
            }
            Self::UnsupportedControlCharacter(character) => {
                write!(
                    formatter,
                    "unsupported control character U+{:04X}",
                    u32::from(*character)
                )
            }
            Self::InvalidMouseButtonState {
                action,
                buttons_held,
            } => write!(
                formatter,
                "mouse action {action:?} is inconsistent with post-event button state {buttons_held:?}"
            ),
            Self::MissingMousePixelPosition => {
                formatter.write_str("pixel mouse reporting requires a pixel position")
            }
            Self::MouseCellOutOfBounds {
                position,
                columns,
                rows,
            } => write!(
                formatter,
                "mouse cell ({}, {}) is outside terminal dimensions {}x{}",
                position.column, position.row, columns, rows
            ),
            Self::MouseCoordinateOutOfRange { value, maximum } => write!(
                formatter,
                "mouse coordinate {value} exceeds the encoding maximum {maximum}"
            ),
            Self::AllocationFailed => formatter.write_str("input encoding allocation failed"),
        }
    }
}

impl Error for InputEncodeError {}

impl Error for TerminalError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerminalOptions {
    pub history_capacity_rows: u32,
}

/// A fully evaluated source transaction that still holds the original
/// terminal unmodified. Publishing consumes the owned host-effect frame and
/// commits the candidate only after the publisher reports success.
pub struct PreparedApplyWithHostEffectsV1<'a> {
    terminal: &'a mut Terminal,
    candidate: Option<Terminal>,
    host_effect_frame_v1: Vec<u8>,
}

/// A fully evaluated source transaction carrying a canonical Host Effect
/// Frame V2. V2 supports multiple replies at one source offset and the
/// bounded query replies that cannot be represented by V1.
pub struct PreparedApplyWithHostEffectsV2<'a> {
    terminal: &'a mut Terminal,
    candidate: Option<Terminal>,
    host_effect_frame_v2: Vec<u8>,
    legacy_projection: Vec<u8>,
}

/// A bounded single-Output candidate. Captures are replayed through the original parser;
/// no state or host effect reaches the live terminal before `finish().commit_with(...)`.
pub struct PreparedBrokerOutputV1<'a> {
    terminal: &'a mut Terminal,
    source_sequence: u64,
    output: Vec<u8>,
    captures: Vec<Arc<kitty_graphics::BrokerCapture>>,
    capture_bytes: usize,
    pending: Option<kitty_graphics::BrokerRequest>,
    ready: Option<(Terminal, Vec<u8>)>,
    /// Diagnostic produced by the immutable replay candidate when parsing or
    /// broker replay is rejected.  The live terminal remains unchanged while
    /// this transaction is pending, so the candidate evidence must travel
    /// with the preparation instead of being read from `self.terminal`.
    diagnostic_v3: Option<String>,
}

/// A broker preparation failure together with the diagnostic captured from
/// the discarded candidate, when the parser produced one.
///
/// The live terminal is never modified by a failed preparation.  Callers that
/// expose the failure to a host should prefer the diagnostic when present and
/// fall back to the stable `TerminalError` text for validation failures that
/// occur before a parser candidate exists.
#[derive(Debug)]
pub struct BrokerPrepareError {
    error: TerminalError,
    diagnostic_v3: Option<String>,
}

impl BrokerPrepareError {
    #[must_use]
    pub fn into_parts(self) -> (TerminalError, Option<String>) {
        (self.error, self.diagnostic_v3)
    }
}

impl From<TerminalError> for BrokerPrepareError {
    fn from(error: TerminalError) -> Self {
        Self {
            error,
            diagnostic_v3: None,
        }
    }
}

impl PreparedApplyWithHostEffectsV2<'_> {
    pub fn take_legacy_projection(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.legacy_projection)
    }
    #[must_use]
    pub fn host_effect_frame_v2(&self) -> &[u8] {
        &self.host_effect_frame_v2
    }

    pub fn commit_with<E>(
        mut self,
        publisher: impl FnOnce(Vec<u8>) -> Result<(), E>,
    ) -> Result<(), E> {
        let frame = std::mem::take(&mut self.host_effect_frame_v2);
        publisher(frame)?;
        *self.terminal = self
            .candidate
            .take()
            .expect("prepared apply candidate is present");
        Ok(())
    }

    pub fn publish<E>(self, publisher: impl FnOnce(Vec<u8>) -> Result<(), E>) -> Result<(), E> {
        self.commit_with(publisher)
    }
}

impl<'a> PreparedBrokerOutputV1<'a> {
    pub fn request(&self) -> Option<&kitty_graphics::BrokerRequest> {
        self.pending.as_ref()
    }

    /// Returns the most recent parser diagnostic captured while rebuilding the
    /// immutable broker candidate.  This is intentionally separate from the
    /// live terminal diagnostics: a rejected candidate must never overwrite
    /// committed state.
    #[must_use]
    pub fn failure_diagnostic_v3(&self) -> Option<&str> {
        self.diagnostic_v3.as_deref()
    }

    /// A failed supply leaves the request pending, so the caller may retry or cancel.
    pub fn supply(&mut self, capture: kitty_graphics::BrokerCapture) -> Result<(), TerminalError> {
        let Some(request) = &self.pending else {
            return Err(TerminalError::Unsupported(
                "Kitty broker has no pending request",
            ));
        };
        if capture.request != *request {
            return Err(TerminalError::Unsupported(
                "Kitty broker capture request identity mismatch",
            ));
        }
        let bytes = match &capture.outcome {
            kitty_graphics::BrokerOutcome::Bytes(bytes) => bytes.len(),
            kitty_graphics::BrokerOutcome::Failure(_) => 0,
        };
        if bytes > kitty_graphics::MAX_BROKER_CAPTURE_BYTES
            || bytes > MAX_BROKER_TRANSACTION_BYTES - self.capture_bytes
        {
            return Err(TerminalError::ProtocolResourceLimit(
                "Kitty broker transaction exceeds 32 MiB",
            ));
        }
        self.captures
            .try_reserve(1)
            .map_err(|_| TerminalError::LimitExceeded)?;
        self.captures.push(Arc::new(capture));
        let result = self.rebuild_with_diagnostic();
        match result {
            Ok(()) => {
                self.capture_bytes += bytes;
                Ok(())
            }
            Err(failure) => {
                self.captures.pop();
                self.diagnostic_v3 = failure.diagnostic_v3;
                Err(failure.error)
            }
        }
    }

    /// Only a fully resolved transaction can expose a publishable HostEffect frame.
    pub fn finish(self) -> Result<PreparedApplyWithHostEffectsV2<'a>, TerminalError> {
        let (candidate, frame) = self.ready.ok_or(TerminalError::BrokerCaptureRequired)?;
        Ok(PreparedApplyWithHostEffectsV2 {
            terminal: self.terminal,
            candidate: Some(candidate),
            host_effect_frame_v2: frame,
            legacy_projection: Vec::new(),
        })
    }

    fn rebuild_with_diagnostic(&mut self) -> Result<(), BrokerRebuildError> {
        // shortcut: replay is bounded to 64 requests per 1 MiB Output; use resumable parsing if this becomes hot.
        let mut candidate = self.terminal.clone();
        candidate.diagnostics.failure = None;
        candidate.diagnostics.failure_v2 = None;
        candidate.diagnostics.expanded_failure_v2 = None;
        candidate.macro_execution = macros::MacroExecution::default();
        candidate.broker_replay = Some(BrokerReplay {
            captures: self.captures.clone(),
            next_ordinal: 0,
            pending: None,
        });
        let mut effects = host_effect::CaptureV2::default();
        let result = candidate.apply_output(&self.output, self.source_sequence, Some(&mut effects));
        let replay = candidate
            .broker_replay
            .take()
            .expect("broker replay installed");
        match result {
            Err(TerminalError::BrokerCaptureRequired) => {
                self.pending = replay.pending;
                self.ready = None;
                self.diagnostic_v3 = None;
                Ok(())
            }
            Err(error) => Err(BrokerRebuildError {
                diagnostic_v3: candidate.failure_diagnostic_v3(),
                error,
            }),
            Ok(()) => {
                if replay.next_ordinal as usize != self.captures.len() {
                    return Err(BrokerRebuildError {
                        diagnostic_v3: candidate.failure_diagnostic_v3(),
                        error: TerminalError::Unsupported(
                            "Kitty broker capture count exceeds parsed requests",
                        ),
                    });
                }
                candidate.main.canonicalize_cursor_cells();
                candidate.alternate.canonicalize_cursor_cells();
                for page in candidate.pages.screens_mut() {
                    page.canonicalize_cursor_cells();
                }
                candidate.next_source_sequence += 1;
                candidate.macro_execution = macros::MacroExecution::default();
                let frame = match effects.encode_v2() {
                    Ok(frame) => frame,
                    Err(error) => {
                        return Err(BrokerRebuildError {
                            diagnostic_v3: candidate.failure_diagnostic_v3(),
                            error,
                        });
                    }
                };
                self.ready = Some((candidate, frame));
                self.pending = None;
                self.diagnostic_v3 = None;
                Ok(())
            }
        }
    }
}

struct BrokerRebuildError {
    error: TerminalError,
    diagnostic_v3: Option<String>,
}

impl PreparedApplyWithHostEffectsV1<'_> {
    /// Borrows the canonical Host Effect Frame V1 before publication.
    #[must_use]
    pub fn host_effect_frame_v1(&self) -> &[u8] {
        &self.host_effect_frame_v1
    }

    /// Gives the publisher ownership of the canonical frame. The original
    /// terminal is committed only when the publisher returns `Ok`.
    pub fn commit_with<E>(
        mut self,
        publisher: impl FnOnce(Vec<u8>) -> Result<(), E>,
    ) -> Result<(), E> {
        let frame = std::mem::take(&mut self.host_effect_frame_v1);
        publisher(frame)?;
        *self.terminal = self
            .candidate
            .take()
            .expect("prepared apply candidate is present");
        Ok(())
    }

    /// Alias for [`Self::commit_with`] emphasizing the publish-before-commit
    /// authority boundary.
    pub fn publish<E>(self, publisher: impl FnOnce(Vec<u8>) -> Result<(), E>) -> Result<(), E> {
        self.commit_with(publisher)
    }
}

impl Terminal {
    pub fn new(columns: u32, rows: u32) -> Result<Self, TerminalError> {
        Self::new_with_sequence(columns, rows, 0)
    }

    pub fn new_with_sequence(
        columns: u32,
        rows: u32,
        initial_source_sequence: u64,
    ) -> Result<Self, TerminalError> {
        Self::new_with_options(
            columns,
            rows,
            initial_source_sequence,
            TerminalOptions::default(),
        )
    }

    pub fn new_with_options(
        columns: u32,
        rows: u32,
        initial_source_sequence: u64,
        options: TerminalOptions,
    ) -> Result<Self, TerminalError> {
        Self::new_with_output_profile(
            columns,
            rows,
            initial_source_sequence,
            options,
            OutputProfile::Utf8,
        )
    }

    pub fn new_with_output_profile(
        columns: u32,
        rows: u32,
        initial_source_sequence: u64,
        options: TerminalOptions,
        output_profile: OutputProfile,
    ) -> Result<Self, TerminalError> {
        validate_dimensions(columns, rows)?;
        if initial_source_sequence >= u64::MAX - 1 {
            return Err(TerminalError::SequenceExhausted);
        }
        Ok(Self {
            columns,
            rows,
            main: Screen::new_with_history_capacity(
                columns,
                rows,
                options.history_capacity_rows,
                MAX_CELLS,
            ),
            alternate: Screen::new(columns, rows),
            pages: pages::PageStore::default(),
            host_geometry: HostGeometryV1::default(),
            shell_integration: ShellIntegration::default(),
            host_signals: HostSignalsV1::default(),
            graphics: None,
            kitty_transfer: kitty_graphics::TransferDecoder::default(),
            broker_replay: None,
            legacy_projection: None,
            active_alternate: false,
            cursor_visible: true,
            cursor_shape: CursorShape::Block,
            cursor_blinking: true,
            modes: ModeState::default(),
            main_keyboard_protocol: KeyboardProtocolStack::default(),
            alternate_keyboard_protocol: KeyboardProtocolStack::default(),
            margins: Margins::full(columns, rows),
            tabs: TabStops::new(columns),
            charset: CharsetState::default(),
            user_preferred_charset: charset::CharsetDesignation::LATIN1,
            main_saved_charset: CharsetState::default(),
            alternate_saved_charset: CharsetState::default(),
            osc: OscState::default(),
            next_source_sequence: initial_source_sequence,
            logical_time_micros: 0,
            synchronized_output_deadline_micros: None,
            output_profile,
            parser: ParserState::Ground,
            utf8: Utf8Decoder::default(),
            string_utf8: StringUtf8Boundary::default(),
            repeat_source: None,
            sgr_stack: sgr_stack::SgrStack::default(),
            macros: macros::MacroStore::default(),
            macro_execution: macros::MacroExecution::default(),
            failed_closed: false,
            diagnostics: diagnostic::OutputDiagnostics::default(),
        })
    }

    /// Width rules are fixed at creation; existing constructors retain the legacy profile.
    pub fn new_with_profiles(
        columns: u32,
        rows: u32,
        initial_source_sequence: u64,
        options: TerminalOptions,
        output_profile: OutputProfile,
        width_profile: WidthProfile,
    ) -> Result<Self, TerminalError> {
        let mut terminal = Self::new_with_output_profile(
            columns,
            rows,
            initial_source_sequence,
            options,
            output_profile,
        )?;
        terminal.main.width_profile = width_profile;
        terminal.alternate.width_profile = width_profile;
        terminal.modes.grapheme_clustering = width_profile == WidthProfile::Unicode16;
        Ok(terminal)
    }

    #[must_use]
    pub fn width_profile(&self) -> WidthProfile {
        self.main.width_profile
    }

    pub fn apply_event(&mut self, event: Event<'_>) -> Result<(), TerminalError> {
        self.apply_events_atomic(std::slice::from_ref(&event))
    }

    /// Applies a contiguous source batch as one transaction.
    ///
    /// A successful batch commits every event after cloning terminal state
    /// once. Any failure discards the complete candidate state. As with a
    /// single event, a gap, a forward sequence discontinuity, or unsupported
    /// deterministic input latches the original terminal failed-closed while
    /// preserving all other pre-batch state.
    pub fn apply_events_atomic(&mut self, events: &[Event<'_>]) -> Result<(), TerminalError> {
        if self.failed_closed {
            return Err(TerminalError::FailedClosed);
        }
        self.diagnostics.failure = None;
        self.diagnostics.failure_v2 = None;
        self.diagnostics.expanded_failure_v2 = None;
        if events.is_empty() {
            return Ok(());
        }
        match self.build_candidate::<host_effect::Capture>(events, None) {
            Ok(candidate) => {
                *self = candidate;
                Ok(())
            }
            Err((error, poison)) => {
                if poison {
                    self.failed_closed = true;
                }
                Err(error)
            }
        }
    }

    /// Prepares one event while capturing canonical host effects without
    /// changing the terminal until the returned transaction is published.
    pub fn prepare_event_with_host_effects_v1<'a>(
        &'a mut self,
        event: Event<'_>,
    ) -> Result<PreparedApplyWithHostEffectsV1<'a>, TerminalError> {
        self.prepare_events_atomic_with_host_effects_v1(std::slice::from_ref(&event))
    }

    /// Prepares up to 256 source events and captures their PTY replies in
    /// a canonical Host Effect Frame V1. Output payloads may total at most
    /// 4 MiB. These capture-only limits do not apply to the legacy discard API.
    pub fn prepare_events_atomic_with_host_effects_v1<'a>(
        &'a mut self,
        events: &[Event<'_>],
    ) -> Result<PreparedApplyWithHostEffectsV1<'a>, TerminalError> {
        if self.failed_closed {
            return Err(TerminalError::FailedClosed);
        }
        self.diagnostics.failure = None;
        self.diagnostics.failure_v2 = None;
        self.diagnostics.expanded_failure_v2 = None;
        if events.len() > MAX_CAPTURE_EVENT_COUNT {
            return Err(TerminalError::LimitExceeded);
        }
        let mut output_bytes = 0_usize;
        for event in events {
            if let Event::Output { bytes, .. } = event {
                output_bytes = output_bytes
                    .checked_add(bytes.len())
                    .ok_or(TerminalError::LimitExceeded)?;
                if output_bytes > MAX_CAPTURE_OUTPUT_BYTES {
                    return Err(TerminalError::LimitExceeded);
                }
            }
        }

        let mut capture = host_effect::Capture::default();
        let candidate = match self.build_candidate(events, Some(&mut capture)) {
            Ok(candidate) => candidate,
            Err((error, poison)) => {
                if poison {
                    self.failed_closed = true;
                }
                return Err(error);
            }
        };
        let host_effect_frame_v1 = capture.encode_v1()?;
        Ok(PreparedApplyWithHostEffectsV1 {
            terminal: self,
            candidate: Some(candidate),
            host_effect_frame_v1,
        })
    }

    /// Prepares one event while capturing a canonical Host Effect Frame V2.
    pub fn prepare_event_with_host_effects_v2<'a>(
        &'a mut self,
        event: Event<'_>,
    ) -> Result<PreparedApplyWithHostEffectsV2<'a>, TerminalError> {
        self.prepare_events_atomic_with_host_effects_v2(std::slice::from_ref(&event))
    }

    /// Replays a single bounded Output through the real parser after each immutable broker
    /// capture. Holding the preparation prevents the live baseline from changing meanwhile.
    pub fn prepare_output_with_broker_v1(
        &mut self,
        source_sequence: u64,
        bytes: &[u8],
    ) -> Result<PreparedBrokerOutputV1<'_>, TerminalError> {
        self.prepare_output_with_broker_captures_v1(source_sequence, bytes, &[])
    }

    /// Diagnostic-preserving variant of [`Self::prepare_output_with_broker_v1`].
    ///
    /// The compatibility method above intentionally keeps its historical error
    /// type. FFI callers that need the parser-produced TL-DIAG/3 evidence use
    /// this additive entry point instead.
    pub fn prepare_output_with_broker_v1_with_diagnostic(
        &mut self,
        source_sequence: u64,
        bytes: &[u8],
    ) -> Result<PreparedBrokerOutputV1<'_>, BrokerPrepareError> {
        self.prepare_output_with_broker_captures_v1_with_diagnostic(source_sequence, bytes, &[])
    }

    /// Replays one Output once with a bounded prefix of immutable captures. The parser verifies
    /// each supplied request in source order and returns the next request if the prefix is short.
    pub fn prepare_output_with_broker_captures_v1(
        &mut self,
        source_sequence: u64,
        bytes: &[u8],
        captures: &[kitty_graphics::BrokerCapture],
    ) -> Result<PreparedBrokerOutputV1<'_>, TerminalError> {
        self.prepare_output_with_broker_captures_v1_with_diagnostic(
            source_sequence,
            bytes,
            captures,
        )
        .map_err(|failure| failure.into_parts().0)
    }

    /// Diagnostic-preserving variant of
    /// [`Self::prepare_output_with_broker_captures_v1`]. The candidate replay
    /// owns any parser evidence so a rejected transaction never mutates the
    /// live terminal's diagnostics.
    pub fn prepare_output_with_broker_captures_v1_with_diagnostic(
        &mut self,
        source_sequence: u64,
        bytes: &[u8],
        captures: &[kitty_graphics::BrokerCapture],
    ) -> Result<PreparedBrokerOutputV1<'_>, BrokerPrepareError> {
        if self.failed_closed {
            return Err(TerminalError::FailedClosed.into());
        }
        if source_sequence != self.next_source_sequence {
            return Err(TerminalError::SequenceMismatch {
                expected: self.next_source_sequence,
                actual: source_sequence,
            }
            .into());
        }
        if source_sequence >= u64::MAX - 1 {
            return Err(TerminalError::SequenceExhausted.into());
        }
        if bytes.len() > MAX_OUTPUT_BYTES {
            return Err(TerminalError::LimitExceeded.into());
        }
        if captures.len() > usize::from(kitty_graphics::MAX_BROKER_REQUESTS_PER_OUTPUT) {
            return Err(TerminalError::ProtocolResourceLimit(
                "Kitty broker request count exceeds 64 per Output",
            )
            .into());
        }
        let mut capture_bytes = 0;
        for capture in captures {
            let bytes = match &capture.outcome {
                kitty_graphics::BrokerOutcome::Bytes(bytes) => bytes.len(),
                kitty_graphics::BrokerOutcome::Failure(_) => 0,
            };
            if bytes > kitty_graphics::MAX_BROKER_CAPTURE_BYTES
                || bytes > MAX_BROKER_TRANSACTION_BYTES - capture_bytes
            {
                return Err(TerminalError::ProtocolResourceLimit(
                    "Kitty broker transaction exceeds 32 MiB",
                )
                .into());
            }
            capture_bytes += bytes;
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(bytes.len())
            .map_err(|_| BrokerPrepareError::from(TerminalError::LimitExceeded))?;
        output.extend_from_slice(bytes);
        let mut owned_captures = Vec::new();
        owned_captures
            .try_reserve_exact(captures.len())
            .map_err(|_| BrokerPrepareError::from(TerminalError::LimitExceeded))?;
        owned_captures.extend(captures.iter().cloned().map(Arc::new));
        let mut preparation = PreparedBrokerOutputV1 {
            terminal: self,
            source_sequence,
            output,
            captures: owned_captures,
            capture_bytes,
            pending: None,
            ready: None,
            diagnostic_v3: None,
        };
        preparation
            .rebuild_with_diagnostic()
            .map_err(|failure| BrokerPrepareError {
                error: failure.error,
                diagnostic_v3: failure.diagnostic_v3,
            })?;
        Ok(preparation)
    }

    /// Prepares up to 256 source events using the V2 host-effect profile.
    /// Publication must succeed before the candidate terminal is committed.
    pub fn prepare_events_atomic_with_host_effects_v2<'a>(
        &'a mut self,
        events: &[Event<'_>],
    ) -> Result<PreparedApplyWithHostEffectsV2<'a>, TerminalError> {
        self.prepare_effects_v2(events, false)
    }

    /// Display-only normalized bytes, generated by the same parser and transaction as the state.
    /// BEL/DECPS/OSC 9/52 never reach an outer terminal. Macros expand through the authority.
    pub fn prepare_event_with_legacy_projection<'a>(
        &'a mut self,
        event: Event<'_>,
    ) -> Result<PreparedApplyWithHostEffectsV2<'a>, TerminalError> {
        self.prepare_effects_v2(std::slice::from_ref(&event), true)
    }

    fn prepare_effects_v2<'a>(
        &'a mut self,
        events: &[Event<'_>],
        project: bool,
    ) -> Result<PreparedApplyWithHostEffectsV2<'a>, TerminalError> {
        if self.failed_closed {
            return Err(TerminalError::FailedClosed);
        }
        self.diagnostics.failure = None;
        self.diagnostics.failure_v2 = None;
        self.diagnostics.expanded_failure_v2 = None;
        validate_capture_events(events)?;

        let mut capture = host_effect::CaptureV2::default();
        let result = self.build_projected_candidate(events, Some(&mut capture), project);
        let mut candidate = match result {
            Ok(candidate) => candidate,
            Err((error, poison)) => {
                if poison {
                    self.failed_closed = true;
                }
                return Err(error);
            }
        };
        let host_effect_frame_v2 = capture.encode_v2()?;
        let legacy_projection = candidate
            .legacy_projection
            .take()
            .map_or_else(Vec::new, |p| p.bytes);
        Ok(PreparedApplyWithHostEffectsV2 {
            terminal: self,
            candidate: Some(candidate),
            host_effect_frame_v2,
            legacy_projection,
        })
    }

    fn build_candidate<C: host_effect::CaptureSink>(
        &mut self,
        events: &[Event<'_>],
        capture: Option<&mut C>,
    ) -> Result<Self, (TerminalError, bool)> {
        self.build_projected_candidate(events, capture, false)
    }

    fn build_projected_candidate<C: host_effect::CaptureSink>(
        &mut self,
        events: &[Event<'_>],
        mut capture: Option<&mut C>,
        project: bool,
    ) -> Result<Self, (TerminalError, bool)> {
        let mut candidate = self.clone();
        candidate.legacy_projection = project.then(legacy_projection::Projection::default);
        candidate.diagnostics.failure = None;
        candidate.diagnostics.failure_v2 = None;
        candidate.diagnostics.expanded_failure_v2 = None;
        candidate.macro_execution = macros::MacroExecution::default();
        for &event in events {
            let actual = event.source_sequence();
            if matches!(event, Event::Gap { .. }) {
                return Err((TerminalError::Gap, true));
            }
            if actual > candidate.next_source_sequence {
                return Err((
                    TerminalError::SequenceMismatch {
                        expected: candidate.next_source_sequence,
                        actual,
                    },
                    true,
                ));
            }
            if actual < candidate.next_source_sequence {
                return Err((
                    TerminalError::SequenceMismatch {
                        expected: candidate.next_source_sequence,
                        actual,
                    },
                    false,
                ));
            }
            if candidate.next_source_sequence >= u64::MAX - 1 {
                return Err((TerminalError::SequenceExhausted, false));
            }

            let result = match event {
                Event::Output { bytes, .. } => {
                    candidate.macro_execution.expanded_bytes = 0;
                    candidate.apply_output(bytes, actual, capture.as_deref_mut())
                }
                Event::Resize { columns, rows, .. } => candidate.resize(columns, rows),
                Event::HostGeometry { geometry, .. } => geometry.validate().map(|()| {
                    candidate.host_geometry = geometry;
                }),
                Event::HostTheme { bytes, .. } => candidate.apply_host_theme(
                    bytes,
                    capture
                        .as_deref_mut()
                        .map(|value| value as &mut dyn host_effect::CaptureSink),
                ),
                Event::Advance { micros, .. } => candidate.advance(micros),
                Event::Gap { .. } => unreachable!("gap handled before transaction"),
            };
            match result {
                Ok(()) => {
                    // Cursor coordinates are durable state, while row edits,
                    // scrolling, and reflow can move a wide-cell continuation
                    // underneath a previously saved coordinate. Canonicalize
                    // both screens at every event boundary so any immediately
                    // exported checkpoint is accepted by the strict reader.
                    candidate.main.canonicalize_cursor_cells();
                    candidate.alternate.canonicalize_cursor_cells();
                    for page in candidate.pages.screens_mut() {
                        page.canonicalize_cursor_cells();
                    }
                    candidate.next_source_sequence += 1;
                }
                Err(
                    error @ (TerminalError::Unsupported(_)
                    | TerminalError::UnsupportedPayload { .. }
                    | TerminalError::UnsupportedDecodedPayload { .. }),
                ) => {
                    self.diagnostics.failure = candidate.diagnostics.failure.take();
                    self.diagnostics.failure_v2 = candidate.diagnostics.failure_v2.take();
                    self.diagnostics.expanded_failure_v2 =
                        candidate.diagnostics.expanded_failure_v2.take();
                    return Err((error, true));
                }
                Err(error @ TerminalError::HostEffectVersionRequired) => {
                    return Err((error, false));
                }
                Err(error) => {
                    self.diagnostics.failure = candidate.diagnostics.failure.take();
                    self.diagnostics.failure_v2 = candidate.diagnostics.failure_v2.take();
                    self.diagnostics.expanded_failure_v2 =
                        candidate.diagnostics.expanded_failure_v2.take();
                    return Err((error, false));
                }
            }
        }
        candidate.macro_execution = macros::MacroExecution::default();
        Ok(candidate)
    }

    pub fn export_snapshot_v2(&self) -> Result<Vec<u8>, TerminalError> {
        self.ensure_presentation_eligible()?;
        snapshot::export(self)
    }

    /// Bounded, ASCII-only evidence of a failed output operation, readable even
    /// after failed-closed. Never execute this text as terminal control data.
    #[must_use]
    pub fn failure_diagnostic_v1(&self) -> Option<&str> {
        self.diagnostics.failure.as_deref()
    }

    /// V2 adds physical detection coordinates and build/profile identity. The
    /// host supplies its role and live/replay context; the core cannot infer them.
    #[must_use]
    pub fn failure_diagnostic_v2(&self) -> Option<String> {
        self.diagnostics.failure_v2.as_ref().map(|text| {
            format!(
                "{text}core_impact={}\n",
                if self.failed_closed {
                    "failed_closed"
                } else {
                    "operation_rejected"
                }
            )
        })
    }

    /// V3 adds separately labeled macro-byte coordinates. Ordinary output keeps
    /// its V2 encoding; V1/V2 consumers still receive the physical invocation.
    #[must_use]
    pub fn failure_diagnostic_v3(&self) -> Option<String> {
        let original = self.failure_diagnostic_v2()?;
        let Some(expanded) = &self.diagnostics.expanded_failure_v2 else {
            return Some(original);
        };
        let mut text = original.replacen("TL-DIAG/2\n", "TL-DIAG/3\n", 1);
        text.push_str(if expanded.origins.is_some() {
            "expanded_source=mixed\n"
        } else {
            "expanded_source=macro\n"
        });
        if let Some(origins) = &expanded.origins {
            text.push_str(origins);
        }
        for line in expanded.text.lines().skip(1) {
            text.push_str("expanded_");
            text.push_str(line);
            text.push('\n');
        }
        text.push_str(if self.failed_closed {
            "expanded_core_impact=failed_closed\n"
        } else {
            "expanded_core_impact=operation_rejected\n"
        });
        debug_assert!(text.len() <= diagnostic::MAX_DIAGNOSTIC_V3_BYTES);
        Some(text)
    }

    pub fn import_snapshot_v2(bytes: &[u8]) -> Result<Self, TerminalError> {
        snapshot::import(bytes)
    }

    /// Exports the active visible screen as canonical renderer-only bytes.
    /// The result is not a restorable terminal snapshot.
    pub fn export_render_frame_v1(&self) -> Result<Vec<u8>, TerminalError> {
        render_frame::export(self)
    }

    /// Exports the active visible screen, including cursor shape and blink
    /// policy, as canonical renderer-only V2 bytes. The result is not a
    /// restorable terminal snapshot.
    pub fn export_render_frame_v2(&self) -> Result<Vec<u8>, TerminalError> {
        render_frame::export_v2(self)
    }

    /// Exports the active visible screen with the extended rendition model as
    /// canonical renderer-only V3 bytes. The result is not restorable state.
    pub fn export_render_frame_v3(&self) -> Result<Vec<u8>, TerminalError> {
        render_frame::export_v3(self)
    }

    /// Renderer-only text frame with an explicit frozen width profile.
    pub fn export_render_frame_v4(&self) -> Result<Vec<u8>, TerminalError> {
        render_frame::export_v4(self)
    }

    /// Returns the retained active-screen scrollback row count and a
    /// runtime-only monotonic count of rows captured into primary scrollback.
    /// Alternate-screen applications intentionally expose no local history.
    #[must_use]
    pub fn scrollback_info(&self) -> (u32, u64) {
        if self.active_alternate || self.pages.visible != 1 {
            (0, self.main.scrollback_generation_rows())
        } else {
            (
                self.main.viewport_top(),
                self.main.scrollback_generation_rows(),
            )
        }
    }

    /// Exports a renderer-only V3 window `offset_rows` above the live tail.
    /// Requests beyond the oldest retained row are clamped to that boundary.
    pub fn export_render_frame_v3_at_scrollback(
        &self,
        offset_rows: u32,
    ) -> Result<Vec<u8>, TerminalError> {
        let (available_rows, _) = self.scrollback_info();
        let offset_rows = offset_rows.min(available_rows);
        if offset_rows == 0 {
            return render_frame::export_v3(self);
        }
        render_frame::export_v3_at_scrollback(self, offset_rows)
    }

    /// Exports a V4 window above the live tail, clamped to retained history.
    pub fn export_render_frame_v4_at_scrollback(
        &self,
        offset_rows: u32,
    ) -> Result<Vec<u8>, TerminalError> {
        let offset_rows = offset_rows.min(self.scrollback_info().0);
        if offset_rows == 0 {
            render_frame::export_v4(self)
        } else {
            render_frame::export_v4_at_scrollback(self, offset_rows)
        }
    }

    /// Encodes one semantic keyboard or paste event using the terminal's
    /// current input modes. This operation does not mutate terminal state.
    pub fn encode_input(&self, event: InputEvent<'_>) -> Result<Vec<u8>, InputEncodeError> {
        if self.failed_closed {
            return Err(InputEncodeError::TerminalFailedClosed);
        }
        if let InputEvent::Mouse(mouse) = event
            && (mouse.position.cell.column >= self.columns || mouse.position.cell.row >= self.rows)
        {
            return Err(InputEncodeError::MouseCellOutOfBounds {
                position: mouse.position.cell,
                columns: self.columns,
                rows: self.rows,
            });
        }
        if let InputEvent::Mouse(mouse) = event
            && self.active_alternate
            && self.modes.alternate_scroll
            && self.modes.mouse_tracking == modes::MouseTracking::None
        {
            return input::encode_alternate_scroll(
                mouse,
                &self.modes,
                self.active_keyboard_protocol().flags(),
            );
        }
        input::encode_input(event, &self.modes, self.active_keyboard_protocol().flags())
    }

    #[must_use]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.columns, self.rows)
    }

    #[must_use]
    pub fn next_source_sequence(&self) -> u64 {
        self.next_source_sequence
    }

    #[must_use]
    pub fn logical_time_micros(&self) -> u64 {
        self.logical_time_micros
    }

    /// Returns the logical-time deadline for an active DEC synchronized-output
    /// transaction. `None` means presentation is not deferred.
    #[must_use]
    pub fn synchronized_output_deadline_micros(&self) -> Option<u64> {
        self.synchronized_output_deadline_micros
    }

    /// Returns whether presentation is blocked by an incomplete parser or
    /// UTF-8 boundary.
    #[must_use]
    pub fn is_parser_incomplete(&self) -> bool {
        !self.parser.is_ground()
            || self.utf8.len != 0
            || self.string_utf8.remaining != 0
            || self.kitty_transfer.is_pending()
    }

    #[must_use]
    pub const fn output_profile(&self) -> OutputProfile {
        self.output_profile
    }

    #[must_use]
    pub fn is_alternate_screen_active(&self) -> bool {
        self.active_alternate
    }

    #[must_use]
    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    #[must_use]
    pub fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }

    #[must_use]
    pub fn cursor_blinking(&self) -> bool {
        self.cursor_blinking
    }

    #[must_use]
    pub fn cursor(&self) -> (u32, u32) {
        let screen = self.active_screen();
        (screen.cursor_row, screen.cursor_column)
    }

    #[must_use]
    pub fn cell(&self, row: u32, column: u32) -> Option<Cell> {
        if row >= self.rows || column >= self.columns {
            return None;
        }
        let screen = self.active_screen();
        Some(screen.cells[screen.visible_cell_index(row, column)].clone())
    }

    #[must_use]
    pub fn is_failed_closed(&self) -> bool {
        self.failed_closed
    }

    #[must_use]
    pub const fn osc_state(&self) -> &OscState {
        &self.osc
    }

    fn active_screen(&self) -> &Screen {
        if self.active_alternate {
            &self.alternate
        } else {
            self.pages.screen(&self.main, self.pages.active)
        }
    }

    fn active_screen_mut(&mut self) -> &mut Screen {
        if self.active_alternate {
            &mut self.alternate
        } else {
            self.pages.screen_mut(&mut self.main, self.pages.active)
        }
    }

    fn active_keyboard_protocol(&self) -> &KeyboardProtocolStack {
        if self.active_alternate {
            &self.alternate_keyboard_protocol
        } else {
            &self.main_keyboard_protocol
        }
    }

    fn active_keyboard_protocol_mut(&mut self) -> &mut KeyboardProtocolStack {
        if self.active_alternate {
            &mut self.alternate_keyboard_protocol
        } else {
            &mut self.main_keyboard_protocol
        }
    }

    fn apply_output<C: host_effect::CaptureSink>(
        &mut self,
        bytes: &[u8],
        source_sequence: u64,
        mut capture: Option<&mut C>,
    ) -> Result<(), TerminalError> {
        if bytes.len() > MAX_OUTPUT_BYTES {
            return Err(TerminalError::LimitExceeded);
        }
        for (offset, &byte) in bytes.iter().enumerate() {
            let effect = capture.as_deref_mut().map(|capture| EffectContext {
                send_c1: self.modes.send_c1 && !self.modes.vt52,
                capture,
                source_sequence,
                source_offset_end: u32::try_from(offset + 1).expect("output offset fits u32"),
            });
            let sensitive = !self.parser.is_public_control_payload()
                && (self.parser.is_string_payload()
                    || matches!(
                        self.parser,
                        ParserState::OscEscape(_)
                            | ParserState::DcsEscape(_)
                            | ParserState::ApcEscape(_)
                            | ParserState::IgnoredStringEscape(_)
                    ));
            let restart = (self.parser.is_ground() && self.utf8.len == 0)
                || (byte == 0x1b && !self.parser.is_string_state());
            if let Some(physical) = &mut self.diagnostics.physical_trace {
                physical.observe(byte, source_sequence, offset as u32, restart, sensitive);
            }
            self.diagnostics.trace.observe(
                byte,
                source_sequence,
                offset as u32,
                restart,
                sensitive,
            );
            if let Err(error) = self.process_byte(byte, effect) {
                self.diagnostics.expanded_failure_v2 = self.diagnostics.trace.expanded_diagnostic(
                    &error,
                    self.output_profile,
                    self.width_profile(),
                );
                let trace = if self.diagnostics.expanded_failure_v2.is_some() {
                    self.diagnostics
                        .physical_trace
                        .as_ref()
                        .unwrap_or(&self.diagnostics.trace)
                } else {
                    &self.diagnostics.trace
                };
                self.diagnostics.failure = trace.diagnostic(&error);
                self.diagnostics.failure_v2 =
                    trace.diagnostic_v2(&error, self.output_profile, self.width_profile());
                return Err(error);
            }
            if self.parser.is_ground() && self.utf8.len == 0 {
                self.diagnostics.trace.clear();
                self.diagnostics.physical_trace = None;
            }
        }
        if self.graphics.is_some() {
            let cell_resources = self.cell_graphic_resources();
            if let Some(graphics) = &mut self.graphics {
                graphics.collect_unused(&cell_resources)?;
            }
        }
        Ok(())
    }

    fn resize(&mut self, columns: u32, rows: u32) -> Result<(), TerminalError> {
        validate_dimensions(columns, rows)?;
        if self.pages.screens().count() * columns as usize * rows as usize > pages::MAX_EXTRA_CELLS
        {
            return Err(TerminalError::ProtocolResourceLimit(
                "DEC page resize exceeds 32768 additional cells",
            ));
        }
        for page in 2..=6 {
            if self.pages.extra[usize::from(page - 2)].is_some() {
                self.with_page_graphics_motion(page, GraphicMotion::Reflow, |screen| {
                    screen.resize_crop(columns, rows);
                    Ok(())
                })?;
            }
        }
        if !self.active_alternate {
            let crop = self.modes.left_right_margin;
            self.with_page_graphics_motion(1, GraphicMotion::Reflow, |screen| {
                if crop {
                    screen.resize_crop(columns, rows);
                    Ok(())
                } else {
                    screen.resize_reflow(columns, rows)
                }
            })?;
        }
        self.with_page_graphics_motion(7, GraphicMotion::Reflow, |screen| {
            screen.resize_crop(columns, rows);
            Ok(())
        })?;
        self.margins.resize(columns, rows);
        debug_assert!(self.margins.validate(columns, rows));
        let resized_tabs = self.tabs.resize(columns);
        debug_assert!(resized_tabs);
        self.columns = columns;
        self.rows = rows;
        self.revalidate_active_wrap_pending();
        Ok(())
    }

    fn advance(&mut self, micros: u64) -> Result<(), TerminalError> {
        self.logical_time_micros = self
            .logical_time_micros
            .checked_add(micros)
            .ok_or(TerminalError::TimeOverflow)?;
        if self
            .synchronized_output_deadline_micros
            .is_some_and(|deadline| self.logical_time_micros >= deadline)
        {
            self.synchronized_output_deadline_micros = None;
        }
        Ok(())
    }

    fn ensure_presentation_eligible(&self) -> Result<(), TerminalError> {
        if self.failed_closed {
            return Err(TerminalError::FailedClosed);
        }
        if self.is_parser_incomplete() {
            return Err(TerminalError::SnapshotNotEligible);
        }
        if self.synchronized_output_deadline_micros.is_some() {
            return Err(TerminalError::PresentationDeferred);
        }
        Ok(())
    }

    fn process_byte(
        &mut self,
        byte: u8,
        mut effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        if self.utf8.len != 0 && byte & 0xc0 != 0x80 {
            // A removed bell/macro/string must not join previously emitted partial UTF-8
            // with later bytes in the outer renderer. ST is an inert ground-state barrier.
            if let Some(projection) = &mut self.legacy_projection {
                projection.push(b"\x1b\\")?;
            }
            self.utf8.clear();
            self.process_character('\u{fffd}')?;
        }

        let at_utf8_boundary = if self.modes.iso2022 {
            self.string_utf8.clear();
            true
        } else if self.parser.is_string_payload() {
            self.string_utf8.observe(byte)
        } else {
            self.string_utf8.clear();
            !(self.parser.is_ground() && self.utf8.len != 0 && byte & 0xc0 == 0x80)
        };

        let receive_c1 = match self.modes.receive_c1 {
            1 => true,
            2 => false,
            _ => self.output_profile == OutputProfile::Utf8WithStandaloneC1,
        };
        if receive_c1 && !self.modes.vt52 && at_utf8_boundary && (0x80..=0x9f).contains(&byte) {
            self.diagnostics.trace.mark_standalone_c1();
            // One physical C1 byte is two virtual parser bytes. Both retain
            // the same source key while each resulting action is dispatched
            // in order; the enclosing candidate transaction remains atomic.
            let first_effect = effect.as_mut().map(EffectContext::reborrow);
            self.process_parser_byte(0x1b, first_effect)?;
            if matches!(self.parser, ParserState::Escape(_)) {
                self.diagnostics.trace.restart_escape();
            }
            return self.process_parser_byte(byte - 0x40, effect);
        }

        self.process_parser_byte(byte, effect)
    }

    fn process_parser_byte(
        &mut self,
        byte: u8,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        if byte == 0x7f && self.parser.is_ground() {
            if let Some(mapped) = self.charset.map_delete_metadata(
                self.modes.national_replacement_character_set_mode,
                self.user_preferred_charset,
            ) {
                if let Some(projection) = &mut self.legacy_projection {
                    projection.push(&[0x7f])?;
                }
                self.process_mapped_graphic(mapped)?;
            }
            return Ok(());
        }
        if self.modes.vt52 && byte == 0x1b {
            self.parser = ParserState::Vt52Escape;
            self.repeat_source = None;
            return Ok(());
        }
        let was_ground = self.parser.is_ground();
        let fresh_escape = matches!(
            self.parser,
            ParserState::OscEscape(_)
                | ParserState::DcsEscape(_)
                | ParserState::ApcEscape(_)
                | ParserState::IgnoredStringEscape(_)
        ) && !matches!(byte, 0 | 0x18 | 0x1a | 0x7f | b'\\')
            && (byte != 7 || matches!(self.parser, ParserState::ApcEscape(_)))
            && !(matches!(self.parser, ParserState::IgnoredStringEscape(_))
                && byte < 0x20
                && byte != 0x1b);
        let parsed = if self.graphics.is_some() {
            self.parser.advance_with_graphics(byte, true)
        } else {
            self.parser.advance(byte)
        };
        let action = match parsed {
            Ok(action) => action,
            Err(error) => {
                if fresh_escape {
                    self.diagnostics.trace.restart_escape();
                }
                return Err(map_parse_error(error));
            }
        };
        let completes_string = matches!(
            action,
            Action::Sequence(Sequence::Dcs(_) | Sequence::Osc(_) | Sequence::Apc(_))
        );
        if fresh_escape && !completes_string {
            self.diagnostics.trace.restart_escape();
        }
        if !was_ground && self.parser.is_ground() && matches!(action, Action::None) {
            self.repeat_source = None;
        }
        self.dispatch_action(action, effect)?;
        if fresh_escape && completes_string {
            self.diagnostics.trace.restart_escape();
        }
        Ok(())
    }

    fn dispatch_action(
        &mut self,
        action: Action,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        let empty_repeat = matches!(&action, Action::Sequence(Sequence::Csi(csi)) if Self::is_repeat_sequence(csi))
            && self.repeat_source.is_none();
        if !empty_repeat && let Some(projection) = &mut self.legacy_projection {
            projection.observe(&action)?;
        }
        match action {
            Action::Vt52 { command, address } => self.dispatch_vt52(command, address, effect),
            Action::None => Ok(()),
            Action::Print(byte) => self.process_print_byte(byte),
            Action::Execute(byte) => {
                self.repeat_source = None;
                self.execute_control(byte)?;
                if byte == 0x07 {
                    host_signals::bell(effect)?;
                }
                self.revalidate_active_wrap_pending();
                Ok(())
            }
            Action::Sequence(sequence) => match sequence {
                Sequence::Csi(sequence) if Self::is_repeat_sequence(&sequence) => {
                    self.dispatch_csi(sequence, effect)
                }
                Sequence::Esc(sequence) => {
                    self.repeat_source = None;
                    self.dispatch_escape(sequence)?;
                    if self.active_screen().has_line_renditions() {
                        self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
                            screen.finish_line_edit()
                        })?;
                    }
                    self.revalidate_active_wrap_pending();
                    Ok(())
                }
                Sequence::Csi(sequence) => {
                    self.repeat_source = None;
                    self.dispatch_csi(sequence, effect)?;
                    if self.active_screen().has_line_renditions() {
                        let reset = self.modes.left_right_margin;
                        self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
                            if reset {
                                screen.reset_visible_line_renditions();
                            }
                            screen.finish_line_edit()
                        })?;
                    }
                    self.revalidate_active_wrap_pending();
                    Ok(())
                }
                Sequence::Osc(sequence) => {
                    self.repeat_source = None;
                    self.dispatch_osc(&sequence, effect)
                }
                Sequence::Dcs(sequence) => {
                    self.repeat_source = None;
                    self.dispatch_dcs(sequence, effect)
                }
                Sequence::Apc(sequence) => {
                    self.repeat_source = None;
                    self.dispatch_kitty(sequence.bytes(), effect)
                }
            },
        }
    }

    fn is_repeat_sequence(csi: &CsiSequence) -> bool {
        csi.final_byte() == b'b'
            && csi.private_prefix().is_none()
            && csi.intermediates().is_empty()
            && !csi.has_subparameters()
            && csi.parameters().len() <= 1
    }

    fn process_print_byte(&mut self, byte: u8) -> Result<(), TerminalError> {
        if self.modes.iso2022 && byte >= 0xa0 {
            let mapped = self.charset.map_gr_metadata(
                byte,
                self.modes.national_replacement_character_set_mode,
                self.user_preferred_charset,
            )?;
            return self.process_mapped_graphic(mapped);
        }
        if self.utf8.len == 0 {
            if (b' '..=b'~').contains(&byte) {
                let mapped = self
                    .charset
                    .map_graphic_metadata(
                        byte,
                        self.modes.national_replacement_character_set_mode,
                        self.user_preferred_charset,
                    )
                    .map_err(|_| TerminalError::Unsupported("charset graphic mapping"))?;
                self.process_mapped_graphic(mapped)?;
            } else if byte <= 0x7f {
                self.process_character(char::from(byte))?;
            } else if let Some(expected) = utf8_sequence_length(byte) {
                self.utf8.pending[0] = byte;
                self.utf8.len = 1;
                self.utf8.expected = expected;
            } else {
                self.process_character('\u{fffd}')?;
            }
            return Ok(());
        }

        debug_assert_eq!(byte & 0xc0, 0x80);
        let index = usize::from(self.utf8.len);
        self.utf8.pending[index] = byte;
        self.utf8.len += 1;
        if self.utf8.len == self.utf8.expected {
            let len = usize::from(self.utf8.len);
            let decoded = std::str::from_utf8(&self.utf8.pending[..len])
                .ok()
                .and_then(|text| text.chars().next())
                .unwrap_or('\u{fffd}');
            self.utf8.clear();
            self.process_character(decoded)?;
        }
        Ok(())
    }

    fn execute_control(&mut self, byte: u8) -> Result<(), TerminalError> {
        match byte {
            // ENQ is ignored; BEL is emitted by the caller through the versioned capture.
            0x05 | 0x07 => {}
            b'\r' => self.carriage_return_active(),
            b'\n' | 0x0b | 0x0c => {
                if self.modes.newline_mode {
                    self.carriage_return_active();
                }
                self.line_feed_active()?;
            }
            // Software flow control is handled outside the mirror state engine.
            0x11 | 0x13 => {}
            0x08 => self.active_screen_mut().backspace(),
            0x0e => self.charset.shift_out(),
            0x0f => self.charset.shift_in(),
            b'\t' => {
                let column = self.active_screen().cursor_column;
                let row = self.active_screen().cursor_row;
                let target = self
                    .tabs
                    .next(column, 1)
                    .min(self.forward_tab_limit(row, column));
                self.active_screen_mut().position_cursor(row, target);
            }
            _ => return Err(TerminalError::Unsupported("C0 control")),
        }
        Ok(())
    }

    fn process_character(&mut self, character: char) -> Result<(), TerminalError> {
        if character.is_control() {
            return Err(TerminalError::Unsupported("C1 or Unicode control"));
        }
        self.print_terminal_character(character)?;
        self.update_repeat_source(character)
    }

    fn print_terminal_character(&mut self, character: char) -> Result<(), TerminalError> {
        self.print_terminal_character_with_join(character, true)
    }

    fn print_terminal_character_with_join(
        &mut self,
        character: char,
        join_previous: bool,
    ) -> Result<(), TerminalError> {
        self.print_terminal_graphic(character, None, join_previous)
    }

    fn print_terminal_graphic(
        &mut self,
        character: char,
        glyph: Option<drcs_bank::GlyphReference>,
        join_previous: bool,
    ) -> Result<(), TerminalError> {
        let modes = self.modes;
        let margins = self.margins;
        let (left, right) = self.scrolling_horizontal_bounds();
        let hyperlink = self.osc.current_hyperlink();
        let revision = self.next_source_sequence + 1;
        // Controls end the streaming cluster. Zero-width marks may still attach
        // to a preceding visible base (for example across an SGR change).
        let clustering = modes.grapheme_clustering && self.repeat_source.is_some();
        self.with_graphics_motion(GraphicMotion::Observed, |screen| {
            screen.print_graphic_in_rectangle(
                character,
                glyph,
                modes.insert_mode,
                modes.auto_wrap,
                margins.top,
                margins.bottom,
                left,
                right,
                hyperlink,
                ScrollCause::Output(revision),
                clustering,
                join_previous,
            )
        })?;
        Ok(())
    }

    fn update_repeat_source(&mut self, character: char) -> Result<(), TerminalError> {
        let profile = self.width_profile();
        if profile == WidthProfile::Unicode16 || self.graphics.is_some() {
            let (left, right) = self.scrolling_horizontal_bounds();
            let screen = self.active_screen();
            self.repeat_source = screen
                .previous_lead_index(self.modes.auto_wrap, left, right)
                .map(|index| {
                    let cell = &screen.cells[index];
                    RepeatSource {
                        character: cell.character,
                        combining: cell.combining.clone(),
                        width: cell.width,
                        glyph: cell.glyph,
                    }
                });
            return Ok(());
        }
        let width = scalar_width(character);
        let extends_joined = width == 2
            && self
                .repeat_source
                .as_ref()
                .and_then(|source| source.combining.chars().last())
                == Some('\u{200d}');
        if width == 0 || extends_joined {
            if let Some(source) = &mut self.repeat_source {
                source.append(character, profile)?;
            }
        } else {
            self.repeat_source = Some(RepeatSource::new(character));
        }
        Ok(())
    }

    fn repeat_last_grapheme(&mut self, count: u16) -> Result<(), TerminalError> {
        let Some(source) = self.repeat_source.clone() else {
            return Ok(());
        };
        for _ in 0..count {
            if source.glyph.is_some() {
                self.print_terminal_graphic(source.character, source.glyph, false)?;
                continue;
            }
            if self.width_profile() == WidthProfile::Unicode16 {
                self.repeat_source = None;
                self.print_terminal_character_with_join(source.character, false)?;
                self.update_repeat_source(source.character)?;
            } else {
                self.print_terminal_character(source.character)?;
            }
            for character in source.combining.chars() {
                self.print_terminal_character(character)?;
                if self.width_profile() == WidthProfile::Unicode16 {
                    self.update_repeat_source(character)?;
                }
            }
        }
        self.repeat_source = None;
        Ok(())
    }

    fn dispatch_escape(&mut self, sequence: EscSequence) -> Result<(), TerminalError> {
        if sequence.intermediates() == b" " && sequence.final_byte() == b'7' {
            self.modes.receive_c1 = 1;
            return Ok(());
        }
        if sequence.intermediates() == b" " && matches!(sequence.final_byte(), b'F' | b'G') {
            self.modes.send_c1 = sequence.final_byte() == b'G';
            return Ok(());
        }
        if sequence.intermediates() == b"%" {
            match sequence.final_byte() {
                b'@' => {
                    self.modes.iso2022 = true;
                    self.modes.receive_c1 = 1;
                }
                b'G' | b'8' => {
                    self.modes.iso2022 = false;
                    self.modes.receive_c1 = 2;
                }
                _ => return Err(TerminalError::Unsupported("coding system designation")),
            }
            self.utf8.clear();
            self.string_utf8.clear();
            return Ok(());
        }
        if sequence.intermediates() == b"#" {
            let rendition = match sequence.final_byte() {
                b'3' => LineRendition::DoubleHeightTop,
                b'4' => LineRendition::DoubleHeightBottom,
                b'5' => LineRendition::SingleWidth,
                b'6' => LineRendition::DoubleWidth,
                b'8' => {
                    self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
                        screen.alignment_pattern()
                    })?;
                    self.modes.origin_mode = false;
                    self.modes.left_right_margin = false;
                    self.margins = Margins::full(self.columns, self.rows);
                    return Ok(());
                }
                _ => return Err(TerminalError::Unsupported("DEC line rendition selector")),
            };
            if !self.modes.left_right_margin {
                self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
                    screen.set_line_rendition(rendition)
                })?;
            }
            return Ok(());
        }
        if let Some(&intermediate) = sequence.intermediates().first() {
            let (register, size) = match intermediate {
                b'(' => (GraphicRegister::G0, CharsetSize::Characters94),
                b')' => (GraphicRegister::G1, CharsetSize::Characters94),
                b'*' => (GraphicRegister::G2, CharsetSize::Characters94),
                b'+' => (GraphicRegister::G3, CharsetSize::Characters94),
                b'-' => (GraphicRegister::G1, CharsetSize::Characters96),
                b'.' => (GraphicRegister::G2, CharsetSize::Characters96),
                b'/' => (GraphicRegister::G3, CharsetSize::Characters96),
                _ => return Err(TerminalError::Unsupported("escape sequence intermediates")),
            };
            return self
                .charset
                .designate_with_soft(
                    register,
                    size,
                    &sequence.intermediates()[1..],
                    sequence.final_byte(),
                    self.graphics.is_some(),
                )
                .map_err(|_| TerminalError::Unsupported("charset designation"));
        }
        match sequence.final_byte() {
            b'6' | b'9' => {
                let margins = self.margins;
                let (left, right) = self.scrolling_horizontal_bounds();
                self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
                    screen.horizontal_index(
                        sequence.final_byte() == b'9',
                        margins.top,
                        margins.bottom,
                        left,
                        right,
                    );
                    Ok(())
                })?;
            }
            b'7' => self.save_active_cursor(),
            b'8' => self.restore_active_cursor(),
            b'D' => self.line_feed_active()?,
            b'E' => {
                self.carriage_return_active();
                self.line_feed_active()?;
            }
            b'H' => {
                let column = self.active_screen().cursor_column;
                let set = self.tabs.set_current(column);
                debug_assert!(set);
            }
            b'M' => self.reverse_index_active()?,
            b'N' => self.charset.single_shift_2(),
            b'O' => self.charset.single_shift_3(),
            b'n' => self.charset.invoke_gl(GraphicRegister::G2),
            b'o' => self.charset.invoke_gl(GraphicRegister::G3),
            b'~' => self.charset.invoke_gr(GraphicRegister::G1),
            b'}' => self.charset.invoke_gr(GraphicRegister::G2),
            b'|' => self.charset.invoke_gr(GraphicRegister::G3),
            b'=' => self.modes.set_keypad_application(true),
            b'>' => self.modes.set_keypad_application(false),
            b'c' => self.hard_reset_model(),
            _ => return Err(TerminalError::Unsupported("escape sequence")),
        }
        Ok(())
    }

    fn dispatch_csi(
        &mut self,
        csi: CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        let is_rectangle_rendition = csi.private_prefix().is_none()
            && csi.intermediates() == b"$"
            && matches!(csi.final_byte(), b'r' | b't');
        if csi.has_subparameters()
            && (csi.final_byte() != b'm'
                || csi.private_prefix().is_some()
                || !csi.intermediates().is_empty())
            && !is_rectangle_rendition
        {
            return Err(TerminalError::Unsupported("CSI subparameters"));
        }
        if !csi.intermediates().is_empty() {
            return self.dispatch_intermediate_csi(&csi, effect);
        }
        if let Some(prefix) = csi.private_prefix() {
            return match prefix {
                b'?' if csi.final_byte() == b'W' => {
                    require_max_parameters(&csi, 1, "DECST8C parameters")?;
                    if csi.parameter(0, 5) != 5 {
                        return Err(TerminalError::Unsupported("DECST8C selector"));
                    }
                    self.tabs.hard_reset();
                    Ok(())
                }
                b'?' if csi.final_byte() == b'm' => self.dispatch_xterm_modifier_query(&csi),
                b'?' if csi.final_byte() == b'n' => self.dispatch_dec_device_status(&csi, effect),
                b'?' if csi.final_byte() == b'u' => {
                    self.dispatch_keyboard_protocol_query(&csi, effect)
                }
                b'=' if csi.final_byte() == b'u' => self.dispatch_keyboard_protocol_set(&csi),
                b'>' if csi.final_byte() == b'u' => self.dispatch_keyboard_protocol_push(&csi),
                b'<' if csi.final_byte() == b'u' => self.dispatch_keyboard_protocol_pop(&csi),
                b'=' if csi.final_byte() == b'c' => {
                    if csi.parameters().len() <= 1 && csi.parameter(0, 0) == 0 {
                        Self::emit_host_reply_v2(effect, b"\x1bP!|00000000\x1b\\")
                    } else {
                        Ok(())
                    }
                }
                b'?' => self.dispatch_private_mode(&csi, effect),
                b'>' => self.dispatch_xterm_private_csi(&csi, effect),
                _ => Err(TerminalError::Unsupported("private CSI prefix")),
            };
        }
        match csi.final_byte() {
            b'@' => {
                let margins = self.margins;
                let (left, right) = self.scrolling_horizontal_bounds();
                self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
                    screen.insert_characters_in_rectangle(
                        nonzero_parameter(&csi, 0),
                        margins.top,
                        margins.bottom,
                        left,
                        right,
                    );
                    Ok(())
                })?;
            }
            b'A' => self.move_cursor(-i64::from(nonzero_parameter(&csi, 0)), 0),
            b'B' | b'e' => self.move_cursor(i64::from(nonzero_parameter(&csi, 0)), 0),
            b'C' | b'a' => self.move_cursor(0, i64::from(nonzero_parameter(&csi, 0))),
            b'D' => self.move_cursor(0, -i64::from(nonzero_parameter(&csi, 0))),
            b'E' => {
                self.move_cursor_to_line(i64::from(nonzero_parameter(&csi, 0)));
            }
            b'F' => {
                self.move_cursor_to_line(-i64::from(nonzero_parameter(&csi, 0)));
            }
            b'G' | b'`' => {
                let row = self.active_screen().cursor_row;
                let column = self.absolute_column(nonzero_parameter(&csi, 0));
                self.active_screen_mut().position_cursor(row, column);
            }
            b'I' => {
                let current = self.active_screen().cursor_column;
                let row = self.active_screen().cursor_row;
                let column = self
                    .tabs
                    .next(current, u32::from(nonzero_parameter(&csi, 0)))
                    .min(self.forward_tab_limit(row, current));
                self.active_screen_mut().position_cursor(row, column);
            }
            b'L' => {
                let margins = self.margins;
                let (left, right) = self.scrolling_horizontal_bounds();
                self.with_graphics_motion(GraphicMotion::Observed, |screen| {
                    screen.insert_lines_in_rectangle(
                        nonzero_parameter(&csi, 0),
                        margins.top,
                        margins.bottom,
                        left,
                        right,
                    );
                    Ok(())
                })?;
            }
            b'M' => {
                let margins = self.margins;
                let (left, right) = self.scrolling_horizontal_bounds();
                self.with_graphics_motion(GraphicMotion::Observed, |screen| {
                    screen.delete_lines_in_rectangle(
                        nonzero_parameter(&csi, 0),
                        margins.top,
                        margins.bottom,
                        left,
                        right,
                    );
                    Ok(())
                })?;
            }
            b'P' => {
                let margins = self.margins;
                let (left, right) = self.scrolling_horizontal_bounds();
                self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
                    screen.delete_characters_in_rectangle(
                        nonzero_parameter(&csi, 0),
                        margins.top,
                        margins.bottom,
                        left,
                        right,
                    );
                    Ok(())
                })?;
            }
            b'S' => {
                let margins = self.margins;
                let (left, right) = self.scrolling_horizontal_bounds();
                let revision = self.next_source_sequence + 1;
                self.with_graphics_motion(GraphicMotion::Observed, |screen| {
                    screen.scroll_up_rectangle_with_cause(
                        nonzero_parameter(&csi, 0),
                        margins.top,
                        margins.bottom,
                        left,
                        right,
                        ScrollCause::Output(revision),
                    );
                    Ok(())
                })?;
            }
            b'T' if csi.parameters().len() <= 1 => {
                let margins = self.margins;
                let (left, right) = self.scrolling_horizontal_bounds();
                self.with_graphics_motion(GraphicMotion::Observed, |screen| {
                    screen.scroll_down_rectangle(
                        nonzero_parameter(&csi, 0),
                        margins.top,
                        margins.bottom,
                        left,
                        right,
                    );
                    Ok(())
                })?;
            }
            b'U' | b'V' => {
                require_max_parameters(&csi, 1, "page movement parameters")?;
                let count = u32::from(nonzero_parameter(&csi, 0));
                let active = u32::from(self.active_page());
                self.move_page(if csi.final_byte() == b'U' {
                    active.saturating_add(count)
                } else {
                    active.saturating_sub(count)
                })?;
                let row = self.absolute_row(1);
                let column = self.absolute_column(1);
                self.active_screen_mut().position_cursor(row, column);
            }
            b'X' => {
                self.erase_characters_with_graphics(nonzero_parameter(&csi, 0))?;
            }
            b'Z' => {
                let current = self.active_screen().cursor_column;
                let row = self.active_screen().cursor_row;
                let column = self
                    .tabs
                    .previous(current, u32::from(nonzero_parameter(&csi, 0)))
                    .max(self.backward_tab_limit(row, current));
                self.active_screen_mut().position_cursor(row, column);
            }
            b'b' if csi.parameters().len() <= 1 => {
                self.repeat_last_grapheme(nonzero_parameter(&csi, 0))?
            }
            b'c' if matches!(csi.parameters(), [] | [0] | [PARAM_MISSING]) => {
                Self::emit_host_reply(effect, b"\x1b[?1;2c")?;
            }
            b'c' => {} // Optional read-only DA selectors do not mutate terminal state.
            b'x' => self.dispatch_terminal_parameters(&csi, effect)?,
            b'd' => {
                let row = self.absolute_row(nonzero_parameter(&csi, 0));
                let column = self.active_screen().cursor_column;
                self.active_screen_mut().position_cursor(row, column);
            }
            b'H' | b'f' => {
                let row = self.absolute_row(nonzero_parameter(&csi, 0));
                let column = self.absolute_column(nonzero_parameter(&csi, 1));
                self.active_screen_mut().position_cursor(row, column);
            }
            b'g' => match csi.parameter(0, 0) {
                0 => {
                    let column = self.active_screen().cursor_column;
                    let cleared = self.tabs.clear_current(column);
                    debug_assert!(cleared);
                }
                3 => self.tabs.clear_all(),
                _ => return Err(TerminalError::Unsupported("tab clear mode")),
            },
            b'h' | b'l' => self.dispatch_ansi_mode(&csi)?,
            b'J' => self.erase_display_with_graphics(csi.parameter(0, 0))?,
            b'K' => self.erase_line_with_graphics(csi.parameter(0, 0))?,
            b'm' => self.apply_sgr(&csi)?,
            b'n' => self.dispatch_device_status(&csi, effect)?,
            b'r' => self.set_vertical_margins(&csi)?,
            b's' if self.modes.left_right_margin => self.set_horizontal_margins(&csi)?,
            b's' if csi.parameters().is_empty() => self.save_active_cursor(),
            b't' => self.dispatch_window_manipulation(&csi, effect)?,
            b'u' if csi.parameters().is_empty() => self.restore_active_cursor(),
            _ => return Err(TerminalError::Unsupported("CSI command")),
        }
        Ok(())
    }

    fn dispatch_xterm_modifier_keys(&mut self, csi: &CsiSequence) -> Result<(), TerminalError> {
        match csi.parameters() {
            [] | [4] | [4, PARAM_MISSING] => {
                self.modes.reset_modify_other_keys();
                Ok(())
            }
            [4, parameter] if self.modes.set_modify_other_keys(*parameter) => Ok(()),
            _ => Err(TerminalError::Unsupported("XTMODKEYS parameters")),
        }
    }

    fn dispatch_keyboard_protocol_set(&mut self, csi: &CsiSequence) -> Result<(), TerminalError> {
        let (flags, mode) = match csi.parameters() {
            [flags] if *flags != PARAM_MISSING => (*flags, 1),
            [flags, mode] if *flags != PARAM_MISSING => {
                (*flags, if *mode == PARAM_MISSING { 1 } else { *mode })
            }
            _ => return Err(TerminalError::Unsupported("Kitty keyboard set parameters")),
        };
        let stack = self.active_keyboard_protocol_mut();
        match mode {
            1 => stack.replace(flags),
            2 => stack.union(flags),
            3 => stack.subtract(flags),
            _ => return Err(TerminalError::Unsupported("Kitty keyboard set mode")),
        }
        Ok(())
    }

    fn dispatch_keyboard_protocol_query(
        &self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        if !csi.parameters().is_empty() {
            return Err(TerminalError::Unsupported(
                "Kitty keyboard query parameters",
            ));
        }
        let mut reply = [0_u8; 8];
        reply[..3].copy_from_slice(b"\x1b[?");
        let mut len = 3;
        append_decimal(
            &mut reply,
            &mut len,
            u32::from(self.active_keyboard_protocol().flags()),
        );
        reply[len] = b'u';
        len += 1;
        Self::emit_host_reply_v2(effect, &reply[..len])
    }

    fn dispatch_keyboard_protocol_push(&mut self, csi: &CsiSequence) -> Result<(), TerminalError> {
        let flags = match csi.parameters() {
            [] => 0,
            [flags] if *flags != PARAM_MISSING => *flags,
            _ => return Err(TerminalError::Unsupported("Kitty keyboard push parameters")),
        };
        self.active_keyboard_protocol_mut().push(flags);
        Ok(())
    }

    fn dispatch_keyboard_protocol_pop(&mut self, csi: &CsiSequence) -> Result<(), TerminalError> {
        let count = match csi.parameters() {
            [] => 1,
            [count] if *count != PARAM_MISSING => *count,
            _ => return Err(TerminalError::Unsupported("Kitty keyboard pop parameters")),
        };
        self.active_keyboard_protocol_mut().pop(count);
        Ok(())
    }

    fn dispatch_xterm_private_csi(
        &mut self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        match csi.final_byte() {
            b'm' => self.dispatch_xterm_modifier_keys(csi),
            b'c' if matches!(csi.parameters(), [] | [0] | [PARAM_MISSING]) => {
                Self::emit_host_reply(effect, b"\x1b[>0;0;0c")
            }
            b'q' if matches!(csi.parameters(), [] | [0] | [PARAM_MISSING]) => {
                Self::emit_host_reply_v2(effect, b"\x1bP>|TermLink(1)\x1b\\")
            }
            b'c' | b'q' => Ok(()),
            _ => Err(TerminalError::Unsupported("private CSI prefix")),
        }
    }

    fn dispatch_xterm_modifier_query(&self, csi: &CsiSequence) -> Result<(), TerminalError> {
        // XTQMODKEYS is likewise a reply-only query for implemented resource
        // 4. Accepting it does not claim that this state mirror has injected
        // the response into an authoritative PTY.
        if csi.parameters() == [4] {
            Ok(())
        } else {
            Err(TerminalError::Unsupported("XTQMODKEYS parameters"))
        }
    }

    fn dispatch_device_status(
        &self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        match csi.parameters() {
            [5] => Self::emit_host_reply(effect, b"\x1b[0n"),
            [6] => self.emit_cursor_position_report(effect, false),
            _ => Ok(()), // Unsupported read-only selectors have no reply or state change.
        }
    }

    fn dispatch_dec_device_status(
        &self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        match csi.parameters() {
            [5] => Self::emit_host_reply(effect, b"\x1b[?0n"),
            [6] => self.emit_cursor_position_report(effect, true),
            [15] => Self::emit_host_reply_v2(effect, b"\x1b[?13n"),
            [25] => Self::emit_host_reply_v2(effect, b"\x1b[?23n"),
            [26] => Self::emit_host_reply_v2(effect, b"\x1b[?27;0;0;5n"),
            [53] | [55] => Self::emit_host_reply_v2(effect, b"\x1b[?53n"),
            [56] => Self::emit_host_reply_v2(effect, b"\x1b[?57;0n"),
            [62] => Self::emit_host_reply_v2(
                effect,
                format!("\x1b[{}*{{", (macros::MAX_BYTES - self.macros.used()) / 16).as_bytes(),
            ),
            [63] | [63, _] => Self::emit_host_reply_v2(
                effect,
                format!(
                    "\x1bP{}!~{:04X}\x1b\\",
                    csi.parameter(1, 0),
                    self.macros.checksum()
                )
                .as_bytes(),
            ),
            [75] => Self::emit_host_reply_v2(effect, b"\x1b[?70n"),
            [85] => Self::emit_host_reply_v2(effect, b"\x1b[?83n"),
            [996] => self.report_theme(effect),
            _ => Ok(()),
        }
    }

    fn emit_cursor_position_report(
        &self,
        effect: Option<EffectContext<'_>>,
        dec_private: bool,
    ) -> Result<(), TerminalError> {
        let screen = self.active_screen();
        let (top, bottom) = self.vertical_cursor_bounds();
        let row = screen.cursor_row.clamp(top, bottom - 1) - top + 1;
        let (left, right) = self.horizontal_cursor_bounds();
        let column = screen.cursor_column.clamp(left, right - 1) - left + 1;
        if dec_private && self.pages.has_state() {
            return Self::emit_host_reply_v2(
                effect,
                format!("\x1b[?{row};{column};{}R", self.active_page()).as_bytes(),
            );
        }
        let mut reply = [0_u8; 32];
        reply[..2].copy_from_slice(b"\x1b[");
        let mut len = 2;
        if dec_private {
            reply[len] = b'?';
            len += 1;
        }
        append_decimal(&mut reply, &mut len, row);
        reply[len] = b';';
        len += 1;
        append_decimal(&mut reply, &mut len, column);
        reply[len] = b'R';
        len += 1;
        Self::emit_host_reply(effect, &reply[..len])
    }

    fn emit_host_reply(
        effect: Option<EffectContext<'_>>,
        payload: &[u8],
    ) -> Result<(), TerminalError> {
        if let Some(effect) = effect {
            let encoded = if effect.send_c1 {
                Some(traditional::encode_c1(payload)?)
            } else {
                None
            };
            effect.capture.push_reply(
                effect.source_sequence,
                effect.source_offset_end,
                encoded.as_deref().unwrap_or(payload),
                effect.send_c1,
            )?;
        }
        Ok(())
    }

    fn emit_host_reply_v2(
        effect: Option<EffectContext<'_>>,
        payload: &[u8],
    ) -> Result<(), TerminalError> {
        if let Some(effect) = effect {
            let encoded = if effect.send_c1 {
                Some(traditional::encode_c1(payload)?)
            } else {
                None
            };
            effect.capture.push_reply(
                effect.source_sequence,
                effect.source_offset_end,
                encoded.as_deref().unwrap_or(payload),
                true,
            )?;
        }
        Ok(())
    }

    fn emit_host_reply_v2_ref(
        effect: Option<&mut EffectContext<'_>>,
        payload: &[u8],
    ) -> Result<(), TerminalError> {
        if let Some(effect) = effect {
            let encoded = if effect.send_c1 {
                Some(traditional::encode_c1(payload)?)
            } else {
                None
            };
            effect.capture.push_reply(
                effect.source_sequence,
                effect.source_offset_end,
                encoded.as_deref().unwrap_or(payload),
                true,
            )?;
        }
        Ok(())
    }

    fn dispatch_intermediate_csi(
        &mut self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        if csi.intermediates() == b"$" && csi.final_byte() == b'p' {
            return self.dispatch_mode_query(csi, effect);
        }
        if csi.private_prefix().is_some() {
            return Err(TerminalError::Unsupported("private intermediate CSI"));
        }
        match (csi.intermediates(), csi.final_byte()) {
            (b",", b'~') => return host_signals::sound(csi, effect),
            (b"$" | b"*", b'|') => {
                require_max_parameters(csi, 1, "page size parameters")?;
                let columns_requested = csi.intermediates() == b"$";
                let default = if columns_requested { 80 } else { 24 };
                let value = match csi.parameter(0, default) {
                    0 => default,
                    value => value,
                };
                if value > 512 {
                    self.diagnostics.trace.reject_parameter(0);
                    return Err(TerminalError::Unsupported("page size exceeds 512 cells"));
                }
                // Only a later ordered Resize, after the host has changed its PTY,
                // may alter geometry. Replay and mirrors never perform host actions.
                if let Some(effect) = effect {
                    effect.capture.push_resize(
                        effect.source_sequence,
                        effect.source_offset_end,
                        if columns_requested { value } else { 0 },
                        if columns_requested { 0 } else { value },
                    )?;
                }
            }
            (b"*", b'z') => return self.invoke_macro(csi, effect),
            (b",", b'|') => {
                require_max_parameters(csi, 3, "DECAC parameters")?;
                for (index, limit, reason) in [
                    (0, 2, "DECAC item"),
                    (1, 257, "DECAC foreground color index"),
                    (2, 257, "DECAC background color index"),
                ] {
                    if csi.parameter(index, 0) > limit {
                        self.diagnostics.trace.reject_parameter(index);
                        return Err(TerminalError::Unsupported(reason));
                    }
                }
                self.osc
                    .assign_colors(
                        csi.parameter(0, 1).max(1),
                        csi.parameter(1, 0),
                        csi.parameter(2, 0),
                    )
                    .map_err(map_osc_error)?;
            }
            (b"&", b'u') => {
                require_max_parameters(csi, 1, "DECRQUPSS parameters")?;
                if csi.parameter(0, 0) == 0 {
                    self.report_user_preference(effect)?;
                }
            }
            (b"$", b'u') => self.report_terminal_state(csi, effect)?,
            (b"$", b'w') => self.report_presentation_state(csi, effect)?,
            (b"#", b'{' | b'p') => self.push_rendition(csi),
            (b"#", b'}' | b'q') => self.pop_rendition(csi)?,
            (b" ", b'P' | b'Q' | b'R') => {
                require_max_parameters(csi, 1, "page movement parameters")?;
                let count = u32::from(nonzero_parameter(csi, 0));
                let active = u32::from(self.active_page());
                let page = match csi.final_byte() {
                    b'P' => count,
                    b'Q' => active.saturating_add(count),
                    _ => active.saturating_sub(count),
                };
                self.move_page(page)?;
            }
            (b"\"", b'v') => {
                if csi.parameters().is_empty() {
                    let reply = format!(
                        "\x1b[{};{};1;1;{}\"w",
                        self.rows,
                        self.columns,
                        self.visible_page()
                    );
                    Self::emit_host_reply_v2(effect, reply.as_bytes())?;
                }
            }
            (b"*", b'y') => self.dispatch_rectangle_checksum(csi, effect)?,
            (b"!", b'p') if csi.parameters().is_empty() => self.soft_reset_model(),
            (b" ", b'q') => {
                self.set_cursor_style(csi)?;
            }
            (b"'", b'}') => {
                require_max_parameters(csi, 1, "DECIC parameters")?;
                let margins = self.margins;
                let (left, right) = self.scrolling_horizontal_bounds();
                self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
                    screen.insert_columns_in_rectangle(
                        nonzero_parameter(csi, 0),
                        margins.top,
                        margins.bottom,
                        left,
                        right,
                    );
                    Ok(())
                })?;
            }
            (b"'", b'~') => {
                require_max_parameters(csi, 1, "DECDC parameters")?;
                let margins = self.margins;
                let (left, right) = self.scrolling_horizontal_bounds();
                self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
                    screen.delete_columns_in_rectangle(
                        nonzero_parameter(csi, 0),
                        margins.top,
                        margins.bottom,
                        left,
                        right,
                    );
                    Ok(())
                })?;
            }
            // Vim emits this legacy character-encoding selection marker on
            // ConPTY startup. TermLink's output contract is always UTF-8, so
            // the exact default selector is an explicit state no-op. Other
            // `% m` forms remain unsupported rather than being generalized
            // into a blanket intermediate-sequence ignore rule.
            (b"%", b'm') if csi.parameters().len() <= 1 && csi.parameter(0, 0) == 0 => {}
            (b"$", b'z') => {
                require_max_parameters(csi, 4, "DECERA parameters")?;
                if let Some((top, left, bottom, right)) = self.rectangle_coordinates(csi, 0) {
                    self.with_graphics_motion(
                        GraphicMotion::EraseSixel {
                            top,
                            left,
                            bottom,
                            right,
                        },
                        |screen| {
                            screen.erase_rectangle(top, left, bottom, right);
                            Ok(())
                        },
                    )?;
                }
            }
            (b"$", b'{') => {
                require_max_parameters(csi, 4, "DECSERA parameters")?;
                if let Some((top, left, bottom, right)) = self.rectangle_coordinates(csi, 0) {
                    self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
                        screen.selective_erase_rectangle(top, left, bottom, right)
                    })?;
                }
            }
            (b"$", b'x') => {
                require_max_parameters(csi, 5, "DECFRA parameters")?;
                let pch = csi.parameters().first().copied().unwrap_or(PARAM_MISSING);
                let mapped = match self.charset.rectangle_fill_metadata(
                    u32::from(pch),
                    self.modes.national_replacement_character_set_mode,
                    self.user_preferred_charset,
                ) {
                    Ok(character) => character,
                    Err(RectangleFillError::InvalidPch(_)) => return Ok(()),
                    Err(RectangleFillError::Unsupported(_)) => {
                        return Err(TerminalError::Unsupported("DECFRA character set"));
                    }
                };
                let (character, glyph) = self.resolve_graphic(mapped);
                if let Some((top, left, bottom, right)) = self.rectangle_coordinates(csi, 1) {
                    let hyperlink = self.osc.current_hyperlink();
                    self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
                        screen.fill_rectangle_with_glyph(
                            character, glyph, top, left, bottom, right, hyperlink,
                        )
                    })?;
                }
            }
            (b"$", b'v') => {
                self.copy_page_rectangle(csi)?;
            }
            (b"$", b'r') => self.change_rectangle_rendition(csi, false)?,
            (b"$", b't') => self.change_rectangle_rendition(csi, true)?,
            (b"*", b'x') => {
                require_max_parameters(csi, 1, "DECSACE parameters")?;
                self.modes.attribute_change_extent = match csi.parameter(0, 0) {
                    0 | 1 => AttributeChangeExtent::Stream,
                    2 => AttributeChangeExtent::Rectangle,
                    _ => return Ok(()),
                };
            }
            (b"\"", b'q') => {
                require_max_parameters(csi, 1, "DECSCA parameters")?;
                match csi.parameter(0, 0) {
                    0 | 2 => self.active_screen_mut().set_protected(false),
                    1 => self.active_screen_mut().set_protected(true),
                    _ => return Err(TerminalError::Unsupported("DECSCA parameter")),
                }
            }
            _ => return Err(TerminalError::Unsupported("CSI intermediates")),
        }
        Ok(())
    }

    fn dispatch_mode_query(
        &self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        let parameter = match csi.parameters() {
            [parameter] if *parameter != PARAM_MISSING => *parameter,
            _ => return Err(TerminalError::Unsupported("DECRQM parameters")),
        };
        let private = match csi.private_prefix() {
            None => false,
            Some(b'?') => true,
            Some(_) => return Err(TerminalError::Unsupported("DECRQM private prefix")),
        };
        let setting = if private {
            match parameter {
                2 => Some(!self.modes.vt52),
                25 => Some(self.cursor_visible),
                47 | 1047 | 1049 => Some(self.active_alternate),
                64 => Some(self.pages.coupled),
                80 => self
                    .graphics
                    .as_ref()
                    .map(|graphics| graphics.sixel_scrolling()),
                2026 => Some(self.synchronized_output_deadline_micros.is_some()),
                // Keep legacy mode bits snapshot-compatible, but do not advertise
                // full grapheme segmentation or theme reports we cannot produce.
                2027 if self.width_profile() == WidthProfile::Unicode16 => {
                    Some(self.modes.grapheme_clustering)
                }
                2031 if self.osc.theme_scheme() != 0 => Some(self.modes.color_scheme_reporting),
                1048 | 2027 | 2031 => None,
                _ => self.modes.query_private_mode(parameter),
            }
        } else if parameter == 25 {
            Some(self.cursor_visible)
        } else {
            self.modes.query_ansi_mode(parameter)
        };
        let reply = query_reply::build_decrpm_reply(
            parameter,
            query_reply::ModeReportStatus::from_setting(setting),
            private,
        )
        .map_err(map_query_reply_error)?;
        Self::emit_host_reply_v2(effect, reply.as_bytes())
    }

    fn set_cursor_style(&mut self, csi: &CsiSequence) -> Result<(), TerminalError> {
        let parameter = match csi.parameters() {
            [] | [PARAM_MISSING] => 0,
            [parameter] => *parameter,
            _ => return Err(TerminalError::Unsupported("DECSCUSR parameters")),
        };
        let (shape, blinking) = match parameter {
            0 | 1 => (CursorShape::Block, true),
            2 => (CursorShape::Block, false),
            3 => (CursorShape::Underline, true),
            4 => (CursorShape::Underline, false),
            5 => (CursorShape::Bar, true),
            6 => (CursorShape::Bar, false),
            _ => return Err(TerminalError::Unsupported("DECSCUSR parameter")),
        };
        self.cursor_shape = shape;
        self.cursor_blinking = blinking;
        Ok(())
    }

    fn rectangle_coordinates(
        &self,
        csi: &CsiSequence,
        offset: usize,
    ) -> Option<(u32, u32, u32, u32)> {
        let (row_base, row_limit) = self.vertical_cursor_bounds();
        let (column_base, column_limit) = self.horizontal_cursor_bounds();
        let row_span = row_limit - row_base;
        let column_span = column_limit - column_base;
        let top = u32::from(dec_parameter(csi, offset, 1)).min(row_span);
        let left = u32::from(dec_parameter(csi, offset + 1, 1)).min(column_span);
        let bottom = u32::from(dec_parameter(
            csi,
            offset + 2,
            u16::try_from(row_span).expect("row span fits u16"),
        ))
        .min(row_span);
        let right = u32::from(dec_parameter(
            csi,
            offset + 3,
            u16::try_from(column_span).expect("column span fits u16"),
        ))
        .min(column_span);
        (top <= bottom && left <= right).then_some((
            row_base + top.saturating_sub(1),
            column_base + left.saturating_sub(1),
            row_base + bottom,
            column_base + right,
        ))
    }

    fn rectangle_origin_coordinates(&self, csi: &CsiSequence, offset: usize) -> (u32, u32) {
        let (row_base, row_limit) = self.vertical_cursor_bounds();
        let (column_base, column_limit) = self.horizontal_cursor_bounds();
        let row_span = row_limit - row_base;
        let column_span = column_limit - column_base;
        let row = u32::from(dec_parameter(csi, offset, 1)).min(row_span);
        let column = u32::from(dec_parameter(csi, offset + 1, 1)).min(column_span);
        (
            row_base + row.saturating_sub(1),
            column_base + column.saturating_sub(1),
        )
    }

    fn change_rectangle_rendition(
        &mut self,
        csi: &CsiSequence,
        reverse: bool,
    ) -> Result<(), TerminalError> {
        if csi.parameters().len() > 16 {
            return Err(TerminalError::Unsupported("rectangle rendition parameters"));
        }
        require_rectangle_coordinate_separators(csi)?;
        if csi.parameters().len() <= 4 {
            return Ok(());
        }
        let operations = parse_sgr_operations(csi, 4, &mut self.diagnostics.trace)?;
        let Some((top, left, bottom, right)) = self.rectangle_coordinates(csi, 0) else {
            return Ok(());
        };
        let rectangle_extent =
            self.modes.attribute_change_extent == AttributeChangeExtent::Rectangle;
        let retain_graphics_padding = self.graphics.is_some();
        self.with_graphics_motion(GraphicMotion::Preserve, |screen| {
            screen.transform_rectangle_attributes_with_extent(
                top,
                left,
                bottom,
                right,
                rectangle_extent,
                |attributes| {
                    if reverse {
                        apply_reverse_sgr_operations(attributes, &operations)
                    } else {
                        apply_sgr_operations(attributes, &operations)
                    }
                },
            );
            if retain_graphics_padding {
                screen.retain_rectangle_rendition_padding(
                    top,
                    left,
                    bottom,
                    right,
                    rectangle_extent,
                );
            }
            Ok(())
        })?;
        Ok(())
    }

    fn dispatch_private_mode(
        &mut self,
        csi: &CsiSequence,
        mut effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        let enabled = match csi.final_byte() {
            b'h' => true,
            b'l' => false,
            _ => return Err(TerminalError::Unsupported("private CSI command")),
        };
        if csi.parameters().is_empty() {
            return Err(TerminalError::Unsupported("empty private mode"));
        }
        for (index, &parameter) in csi.parameters().iter().enumerate() {
            match parameter {
                2 => self.set_ansi_mode(enabled),
                2031 => {
                    let changed = self.modes.color_scheme_reporting != enabled;
                    self.modes.color_scheme_reporting = enabled;
                    if changed && enabled {
                        self.report_theme(effect.as_mut().map(EffectContext::reborrow))?;
                    }
                }
                25 => self.cursor_visible = enabled,
                80 if self.graphics.is_some() => {
                    self.graphics.as_mut().unwrap().set_sixel_scrolling(enabled)
                }
                2026 if enabled => {
                    self.synchronized_output_deadline_micros =
                        Some(self.logical_time_micros.saturating_add(1_000_000));
                }
                2026 => self.synchronized_output_deadline_micros = None,
                47 => self.set_alternate_buffer(enabled)?,
                64 => {
                    self.pages.coupled = enabled;
                    if enabled {
                        self.pages.visible = self.pages.active;
                    }
                }
                1047 => self.set_alternate_buffer_and_clear_on_exit(enabled)?,
                1048 if enabled => self.save_active_cursor(),
                1048 => self.restore_active_cursor(),
                1049 => self.set_saved_alternate_buffer(enabled)?,
                69 => {
                    let changed = self.modes.set_private_mode(parameter, enabled);
                    debug_assert!(changed);
                    self.margins.reset_horizontal(self.columns);
                    self.revalidate_active_wrap_pending();
                }
                _ if self.modes.set_private_mode(parameter, enabled) => {
                    if parameter == 6 {
                        self.home_cursor();
                    } else if parameter == 7 && !enabled {
                        self.active_screen_mut().cancel_pending_wrap();
                    }
                }
                _ => {
                    self.diagnostics.trace.reject_parameter(index);
                    return Err(TerminalError::Unsupported("private mode"));
                }
            }
        }
        Ok(())
    }

    fn dispatch_ansi_mode(&mut self, csi: &CsiSequence) -> Result<(), TerminalError> {
        let enabled = csi.final_byte() == b'h';
        if csi.parameters().is_empty() {
            return Err(TerminalError::Unsupported("empty ANSI mode"));
        }
        for (index, &parameter) in csi.parameters().iter().enumerate() {
            match parameter {
                // ConPTY emits the non-private form while synchronizing legacy
                // console cursor visibility. Treat it as the DECTCEM alias
                // used by the same captured stream rather than poisoning the
                // deterministic mirror.
                25 => self.cursor_visible = enabled,
                PARAM_MISSING => {
                    self.diagnostics.trace.reject_parameter(index);
                    return Err(TerminalError::Unsupported("ANSI mode"));
                }
                _ if self.modes.set_ansi_mode(parameter, enabled) => {}
                _ => {
                    self.diagnostics.trace.reject_parameter(index);
                    return Err(TerminalError::Unsupported("ANSI mode"));
                }
            }
        }
        Ok(())
    }

    fn dispatch_window_manipulation(
        &mut self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        if matches!(csi.parameter(0, 0), 14..=16 | 18 | 19) {
            return self.report_geometry(csi, effect);
        }
        require_max_parameters(csi, 3, "window manipulation parameters")?;
        // Modern OpenConsole begins its VT stream with the window-restore hint.
        // Window visibility belongs to the outer UI; it has no durable cell state
        // or reply. Recognize only this exact operation, not arbitrary window ops.
        if csi.parameters() == [1] {
            return Ok(());
        }
        let save = match csi.parameter(0, 0) {
            22 => true,
            23 => false,
            _ => return Err(TerminalError::Unsupported("window manipulation")),
        };
        let target = match csi.parameter(1, 0) {
            0 => TitleTarget::Both,
            1 => TitleTarget::Icon,
            2 => TitleTarget::Window,
            _ => return Err(TerminalError::Unsupported("window title target")),
        };
        let slot = csi.parameter(2, 0);
        if slot > 10 {
            return Err(TerminalError::Unsupported("window title stack slot"));
        }
        let result = if save {
            self.osc.save_titles(target, slot)
        } else {
            self.osc.restore_titles(target, slot)
        };
        match result {
            Ok(_) => Ok(()),
            Err(OscError::AllocationFailed) => Err(TerminalError::LimitExceeded),
            Err(OscError::LimitExceeded { .. }) => {
                Err(TerminalError::Unsupported("title stack limit"))
            }
            Err(_) => Err(TerminalError::Unsupported(
                "malformed title stack operation",
            )),
        }
    }

    fn dispatch_osc(
        &mut self,
        sequence: &OscSequence,
        mut effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        if sequence.selector() == 9 {
            return self.host_signals.apply_progress(sequence.payload());
        }
        if sequence.selector() == 52 {
            return clipboard::write(sequence.payload(), effect);
        }
        if matches!(sequence.selector(), 7 | 133) {
            return self.apply_shell_annotation(u32::from(sequence.selector()), sequence.payload());
        }
        // DOCS selects ISO-8859-1 for control-string text. G-set translation
        // applies only to screen glyphs, never titles or hyperlink metadata.
        let decoded = if self.modes.iso2022 && matches!(sequence.selector(), 0 | 1 | 2 | 8 | 21) {
            let mut text = String::new();
            text.try_reserve(sequence.payload().len() * 2)
                .map_err(|_| TerminalError::LimitExceeded)?;
            text.extend(sequence.payload().iter().copied().map(char::from));
            Some(text)
        } else {
            None
        };
        let (_, queries) = self
            .osc
            .apply_with_color_queries(
                sequence.selector(),
                decoded.as_deref().map_or(sequence.payload(), str::as_bytes),
            )
            .map_err(map_osc_error)?;
        for query in queries {
            let reply = match query {
                osc::OscColorQueryResult::Palette {
                    index,
                    color: Some(color),
                } => query_reply::build_osc_palette_color_reply(index, color),
                osc::OscColorQueryResult::DefaultColor {
                    slot,
                    color: Some(color),
                } => query_reply::build_osc_default_color_reply(slot, color),
                osc::OscColorQueryResult::Palette { color: None, .. }
                | osc::OscColorQueryResult::DefaultColor { color: None, .. } => continue,
            }
            .map_err(map_query_reply_error)?;
            Self::emit_host_reply_v2_ref(effect.as_mut(), reply.as_bytes())?;
        }
        Ok(())
    }

    fn dispatch_dcs(
        &mut self,
        sequence: DcsSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        debug_assert_eq!(sequence.private_prefix(), None);
        debug_assert!(!sequence.has_subparameters());
        if self.graphics.is_some()
            && sequence.intermediates().is_empty()
            && sequence.final_byte() == b'{'
        {
            let download = drcs::decode(sequence.parameters(), sequence.payload())?;
            let cell_resources = self.cell_graphic_resources();
            if let Some(graphics) = &mut self.graphics {
                graphics.install_font(download, &cell_resources)?;
            }
            return Ok(());
        }
        if self.graphics.is_some()
            && sequence.intermediates().is_empty()
            && sequence.final_byte() == b'q'
        {
            return self.dispatch_sixel(&sequence);
        }
        if sequence.intermediates() == b"!" && sequence.final_byte() == b'z' {
            return self.define_macro(&sequence);
        }
        if sequence.intermediates() == b"!" && sequence.final_byte() == b'u' {
            return self.assign_user_preference(&sequence);
        }
        if sequence.intermediates() == b"$" && matches!(sequence.final_byte(), b'p' | b't') {
            return self.restore_report(&sequence);
        }
        if sequence.intermediates() == b"$" && sequence.final_byte() == b'q' {
            if !sequence.parameters().is_empty() {
                return Err(TerminalError::Unsupported("DCS query parameters"));
            }
            if let Some(item) = sequence.payload().strip_suffix(b",|") {
                let item = match item {
                    b"" | b"0" | b"1" => 1,
                    b"2" => 2,
                    _ => 0,
                };
                let reply = if item == 0 {
                    "\x1bP0$r\x1b\\".to_string()
                } else {
                    let aliases = self.osc.color_aliases();
                    let offset = (item - 1) * 2;
                    format!(
                        "\x1bP1$r{};{};{},|\x1b\\",
                        item,
                        aliases[offset].unwrap_or(256),
                        aliases[offset + 1].unwrap_or(257)
                    )
                };
                return Self::emit_host_reply_v2(effect, reply.as_bytes());
            }
            let screen = self.active_screen();
            let state = query_reply::DecrqssState::new(
                screen.attributes,
                self.margins,
                self.cursor_shape,
                self.cursor_blinking,
                screen.protected,
                self.modes.attribute_change_extent,
            );
            let reply = query_reply::build_decrqss_reply(sequence.payload(), state)
                .map_err(map_query_reply_error)?;
            return Self::emit_host_reply_v2(effect, reply.as_bytes());
        }
        if sequence.intermediates() == b"+" && sequence.final_byte() == b'q' {
            if !sequence.parameters().is_empty() {
                return Err(TerminalError::Unsupported("DCS query parameters"));
            }
            let reply = query_reply::build_xtgettcap_replies(
                sequence.payload(),
                query_reply::XtgettcapModes {
                    newline_mode: self.modes.newline_mode,
                    cursor_keys_application: self.modes.cursor_keys_application,
                    keypad_application: self.modes.keypad_application,
                    modify_other_keys: self.modes.modify_other_keys,
                },
            )
            .map_err(map_query_reply_error)?;
            return Self::emit_host_reply_v2(effect, reply.as_bytes());
        }
        let feature = match (sequence.intermediates(), sequence.final_byte()) {
            (b"", b'q') => "SIXEL",
            (b"", b'|') => "DECUDK",
            (b"", b'{') => "DECDLD/DRCS",
            (b"!", b'z') => "DECDMAC",
            (b"!", b'u') => "DECAUPSS",
            (b"$", b'p') => "DECRSTS",
            (b"$", b't') => "DECRSPS",
            _ => "stateful DCS",
        };
        Err(TerminalError::Unsupported(feature))
    }

    fn line_feed_active(&mut self) -> Result<(), TerminalError> {
        let margins = self.margins;
        let (left, right) = self.scrolling_horizontal_bounds();
        let revision = self.next_source_sequence + 1;
        self.with_graphics_motion(GraphicMotion::Observed, |screen| {
            screen.line_feed_in_rectangle_with_scroll_cause(
                margins.top,
                margins.bottom,
                left,
                right,
                ScrollCause::Output(revision),
            );
            Ok(())
        })
    }

    fn scrolling_horizontal_bounds(&self) -> (u32, u32) {
        if self.modes.left_right_margin {
            (self.margins.left, self.margins.right)
        } else {
            (0, self.columns)
        }
    }

    fn carriage_return_active(&mut self) {
        let screen = self.active_screen();
        let row = screen.cursor_row;
        let column = screen.cursor_column;
        let (left, _) = self.scrolling_horizontal_bounds();
        let in_vertical_margins = row >= self.margins.top && row < self.margins.bottom;
        let target = if in_vertical_margins && column >= left {
            left
        } else {
            0
        };
        self.active_screen_mut().position_cursor(row, target);
    }

    fn reverse_index_active(&mut self) -> Result<(), TerminalError> {
        let margins = self.margins;
        let (left, right) = self.scrolling_horizontal_bounds();
        self.with_graphics_motion(GraphicMotion::Observed, |screen| {
            screen.reverse_index_in_rectangle(margins.top, margins.bottom, left, right);
            Ok(())
        })
    }

    fn save_active_cursor(&mut self) {
        let origin_mode = self.modes.origin_mode;
        let auto_wrap = self.modes.auto_wrap;
        self.active_screen_mut().save_cursor(origin_mode, auto_wrap);
        if self.active_alternate {
            self.alternate_saved_charset = self.charset;
        } else {
            self.main_saved_charset = self.charset;
            self.pages.saved = self.pages.active;
        }
    }

    fn restore_active_cursor(&mut self) {
        if !self.active_alternate {
            self.pages.active = self.pages.saved;
            if self.pages.coupled {
                self.pages.visible = self.pages.active;
            }
        }
        let (origin_mode, auto_wrap) = self.active_screen_mut().restore_cursor();
        self.modes.origin_mode = origin_mode;
        self.modes.auto_wrap = auto_wrap;
        self.charset = if self.active_alternate {
            self.alternate_saved_charset
        } else {
            self.main_saved_charset
        };
        self.revalidate_active_wrap_pending();
    }

    fn expected_wrap_column(&self, row: u32) -> u32 {
        if self.modes.left_right_margin && row >= self.margins.top && row < self.margins.bottom {
            self.margins
                .right
                .min(self.active_screen().line_columns(row))
                - 1
        } else {
            self.active_screen().line_columns(row) - 1
        }
    }

    fn revalidate_active_wrap_pending(&mut self) {
        let cursor_boundary = self.expected_wrap_column(self.active_screen().cursor_row);
        let saved_boundary = self.expected_wrap_column(self.active_screen().saved.row);
        let screen = self.active_screen_mut();
        screen.wrap_pending &= screen.cursor_column == cursor_boundary;
        screen.saved.wrap_pending &= screen.saved.column == saved_boundary;
    }

    fn vertical_cursor_bounds(&self) -> (u32, u32) {
        if self.modes.origin_mode {
            (self.margins.top, self.margins.bottom)
        } else {
            (0, self.rows)
        }
    }

    fn horizontal_cursor_bounds(&self) -> (u32, u32) {
        if self.modes.origin_mode && self.modes.left_right_margin {
            (self.margins.left, self.margins.right)
        } else {
            (0, self.columns)
        }
    }

    fn absolute_row(&self, one_based: u16) -> u32 {
        let (top, bottom) = self.vertical_cursor_bounds();
        top.saturating_add(u32::from(one_based.saturating_sub(1)))
            .min(bottom - 1)
    }

    fn absolute_column(&self, one_based: u16) -> u32 {
        let (left, right) = self.horizontal_cursor_bounds();
        left.saturating_add(u32::from(one_based.saturating_sub(1)))
            .min(right - 1)
    }

    fn move_cursor(&mut self, row_delta: i64, column_delta: i64) {
        let screen = self.active_screen();
        let cursor_row = screen.cursor_row;
        let cursor_column = screen.cursor_column;
        let mut row = i64::from(cursor_row)
            .saturating_add(row_delta)
            .clamp(0, i64::from(self.rows - 1));
        let mut column = i64::from(cursor_column)
            .saturating_add(column_delta)
            .clamp(0, i64::from(self.columns - 1));
        let (left, right) = self.scrolling_horizontal_bounds();
        let column_in_margins = cursor_column >= left && cursor_column < right;
        if self.modes.origin_mode || column_in_margins {
            if self.modes.origin_mode || cursor_row >= self.margins.top {
                row = row.max(i64::from(self.margins.top));
            }
            if self.modes.origin_mode || cursor_row < self.margins.bottom {
                row = row.min(i64::from(self.margins.bottom - 1));
            }
        }
        let target_row = u32::try_from(row).expect("clamped row");
        let row_in_margins = target_row >= self.margins.top && target_row < self.margins.bottom;
        if self.modes.origin_mode || row_in_margins {
            if self.modes.origin_mode || cursor_column >= left {
                column = column.max(i64::from(left));
            }
            if self.modes.origin_mode || cursor_column < right {
                column = column.min(i64::from(right - 1));
            }
        }
        self.active_screen_mut()
            .position_cursor(target_row, u32::try_from(column).expect("clamped column"));
    }

    fn move_cursor_to_line(&mut self, row_delta: i64) {
        let original_column = self.active_screen().cursor_column;
        self.move_cursor(row_delta, 0);
        let row = self.active_screen().cursor_row;
        let (left, right) = self.scrolling_horizontal_bounds();
        let target = if self.modes.origin_mode
            || (row >= self.margins.top
                && row < self.margins.bottom
                && original_column >= left
                && original_column < right)
        {
            left
        } else {
            0
        };
        self.active_screen_mut().position_cursor(row, target);
    }

    fn forward_tab_limit(&self, row: u32, column: u32) -> u32 {
        let (_, right) = self.scrolling_horizontal_bounds();
        if row >= self.margins.top && row < self.margins.bottom && column < right {
            right - 1
        } else {
            self.columns - 1
        }
    }

    fn backward_tab_limit(&self, row: u32, column: u32) -> u32 {
        let (left, _) = self.scrolling_horizontal_bounds();
        if row >= self.margins.top && row < self.margins.bottom && column >= left {
            left
        } else {
            0
        }
    }

    fn home_cursor(&mut self) {
        let (top, _) = self.vertical_cursor_bounds();
        let (left, _) = self.horizontal_cursor_bounds();
        self.active_screen_mut().position_cursor(top, left);
    }

    fn set_vertical_margins(&mut self, csi: &CsiSequence) -> Result<(), TerminalError> {
        if csi.parameters().len() > 2 {
            return Err(TerminalError::Unsupported("scroll margin parameters"));
        }
        let top_parameter = dec_parameter(csi, 0, 1);
        let bottom_parameter =
            dec_parameter(csi, 1, u16::try_from(self.rows).expect("rows fit u16"));
        if top_parameter >= bottom_parameter {
            return Ok(());
        }
        let top = u32::from(top_parameter - 1);
        let bottom = u32::from(bottom_parameter);
        if self.margins.set_vertical(top, bottom, self.rows) {
            self.home_cursor();
            self.revalidate_active_wrap_pending();
        }
        Ok(())
    }

    fn set_horizontal_margins(&mut self, csi: &CsiSequence) -> Result<(), TerminalError> {
        if csi.parameters().len() > 2 {
            return Err(TerminalError::Unsupported("left-right margin parameters"));
        }
        let left_parameter = dec_parameter(csi, 0, 1);
        let right_parameter = dec_parameter(
            csi,
            1,
            u16::try_from(self.columns).expect("columns fit u16"),
        );
        if left_parameter >= right_parameter {
            return Ok(());
        }
        let left = u32::from(left_parameter - 1);
        let right = u32::from(right_parameter);
        if self.margins.set_horizontal(left, right, self.columns) {
            self.home_cursor();
            self.revalidate_active_wrap_pending();
        }
        Ok(())
    }

    fn apply_sgr(&mut self, csi: &CsiSequence) -> Result<(), TerminalError> {
        let operations = parse_sgr_operations(csi, 0, &mut self.diagnostics.trace)?;
        let mut attributes = self.active_screen().attributes;
        apply_sgr_operations(&mut attributes, &operations);
        self.active_screen_mut().attributes = attributes;
        Ok(())
    }

    fn copy_cursor_presentation(source: &Screen, target: &mut Screen, wrap_column: u32) {
        let cursor_row = source.cursor_row.min(target.rows - 1);
        let cursor_column = source.cursor_column.min(target.columns - 1);
        let wrap_pending = source.wrap_pending;
        let attributes = source.attributes;
        let protected = source.protected;
        target.position_cursor(cursor_row, cursor_column);
        target.wrap_pending = wrap_pending && target.cursor_column == wrap_column;
        target.attributes = attributes;
        target.set_protected(protected);
    }

    fn switch_to_alternate(&mut self) {
        if self.active_alternate {
            return;
        }
        let wrap_column = self.expected_wrap_column(self.active_screen().cursor_row);
        Self::copy_cursor_presentation(
            self.pages.screen(&self.main, self.pages.active),
            &mut self.alternate,
            wrap_column,
        );
        self.active_alternate = true;
        self.revalidate_active_wrap_pending();
    }

    fn switch_to_main(&mut self) -> Result<(), TerminalError> {
        if !self.active_alternate {
            return Ok(());
        }
        if self.main.columns != self.columns || self.main.rows != self.rows {
            let (columns, rows, crop) = (self.columns, self.rows, self.modes.left_right_margin);
            self.with_page_graphics_motion(1, GraphicMotion::Reflow, |screen| {
                if crop {
                    screen.resize_crop(columns, rows);
                    Ok(())
                } else {
                    screen.resize_reflow(columns, rows)
                }
            })?;
        }
        let wrap_column = self.expected_wrap_column(self.alternate.cursor_row);
        Self::copy_cursor_presentation(
            &self.alternate,
            self.pages.screen_mut(&mut self.main, self.pages.active),
            wrap_column,
        );
        self.active_alternate = false;
        self.revalidate_active_wrap_pending();
        Ok(())
    }

    fn clear_alternate(&mut self) {
        self.alternate.reset();
        let cell_resources = self.cell_graphic_resources();
        if let Some(graphics) = &mut self.graphics {
            graphics
                .retain_placements(|p| p.anchor.page != 7, &cell_resources)
                .expect("committed cells have valid graphic references");
        }
        self.alternate_saved_charset = CharsetState::default();
    }

    fn set_alternate_buffer(&mut self, enabled: bool) -> Result<(), TerminalError> {
        if enabled {
            self.switch_to_alternate();
            Ok(())
        } else {
            self.switch_to_main()
        }
    }

    fn set_alternate_buffer_and_clear_on_exit(
        &mut self,
        enabled: bool,
    ) -> Result<(), TerminalError> {
        if enabled {
            self.switch_to_alternate();
            return Ok(());
        }
        if self.active_alternate {
            self.switch_to_main()?;
            self.clear_alternate();
        }
        Ok(())
    }

    fn set_saved_alternate_buffer(&mut self, enabled: bool) -> Result<(), TerminalError> {
        if enabled && !self.active_alternate {
            let attributes = self.active_screen().attributes;
            let protected = self.active_screen().protected;
            self.save_active_cursor();
            self.clear_alternate();
            self.alternate.attributes = attributes;
            self.alternate.set_protected(protected);
            self.active_alternate = true;
        } else if !enabled && self.active_alternate {
            self.switch_to_main()?;
            self.clear_alternate();
            self.restore_active_cursor();
        }
        Ok(())
    }

    fn soft_reset_model(&mut self) {
        // DECSTR resets the active cursor's presentation, not its position,
        // cells, history, tabs, alternate-screen selection or host input modes.
        self.cursor_visible = true;
        self.modes.insert_mode = false;
        self.modes.origin_mode = false;
        self.modes.auto_wrap = true;
        self.modes.cursor_keys_application = false;
        self.modes.keypad_application = false;
        self.modes.left_right_margin = false;
        self.pages.coupled = true;
        self.pages.visible = self.pages.active;
        self.margins = Margins::full(self.columns, self.rows);
        self.charset.hard_reset();
        if self.active_alternate {
            self.alternate_saved_charset = self.charset;
        } else {
            self.main_saved_charset = self.charset;
            self.pages.saved = self.pages.active;
        }
        let screen = self.active_screen_mut();
        screen.attributes = Attributes::default();
        screen.set_protected(false);
        screen.saved = screen::SavedCursor::default();
        screen.cancel_pending_wrap();
    }

    fn hard_reset_model(&mut self) {
        let history_capacity_rows = self.main.requested_history_rows();
        let width_profile = self.width_profile();
        self.main = Screen::new_with_history_capacity(
            self.columns,
            self.rows,
            history_capacity_rows,
            MAX_CELLS,
        );
        self.alternate = Screen::new(self.columns, self.rows);
        self.pages = pages::PageStore::default();
        self.main.width_profile = width_profile;
        self.alternate.width_profile = width_profile;
        self.active_alternate = false;
        self.cursor_visible = true;
        self.cursor_shape = CursorShape::Block;
        self.cursor_blinking = true;
        self.modes.hard_reset();
        self.user_preferred_charset = charset::CharsetDesignation::LATIN1;
        self.modes.grapheme_clustering = width_profile == WidthProfile::Unicode16;
        self.main_keyboard_protocol = KeyboardProtocolStack::default();
        self.alternate_keyboard_protocol = KeyboardProtocolStack::default();
        self.margins = Margins::full(self.columns, self.rows);
        self.tabs.hard_reset();
        self.charset.hard_reset();
        self.main_saved_charset = CharsetState::default();
        self.alternate_saved_charset = CharsetState::default();
        self.osc.reset_to_host_baseline();
        self.synchronized_output_deadline_micros = None;
        self.parser = ParserState::Ground;
        self.utf8.clear();
        self.string_utf8.clear();
        self.repeat_source = None;
        self.sgr_stack = sgr_stack::SgrStack::default();
        self.macros = macros::MacroStore::default();
        self.shell_integration.reset();
        self.host_signals = HostSignalsV1::default();
        if let Some(graphics) = &mut self.graphics {
            graphics
                .reset(&[])
                .expect("empty graphics references cannot fail collection");
        }
        self.kitty_transfer.abort();
        if self.macro_execution.depth != 0 {
            self.macro_execution.cancelled = true;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColorTarget {
    Foreground,
    Background,
    Underline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SgrOperation {
    Reset,
    Bold,
    Faint,
    ClearIntensity,
    Italic(bool),
    Underline {
        style: UnderlineStyle,
        reverse_target: Option<UnderlineStyle>,
    },
    Blink(bool),
    Inverse(bool),
    Conceal(bool),
    Strikethrough(bool),
    Overline(bool),
    Color(ColorTarget, Color),
}

fn parse_sgr_operations(
    csi: &CsiSequence,
    start: usize,
    trace: &mut diagnostic::SequenceTrace,
) -> Result<Vec<SgrOperation>, TerminalError> {
    let parameters = csi.parameters();
    let separators = csi.parameter_separators();
    let mut operations = Vec::new();
    operations
        .try_reserve(parameters.len().saturating_sub(start).max(1))
        .map_err(|_| TerminalError::LimitExceeded)?;
    if parameters.is_empty() && start == 0 {
        operations.push(SgrOperation::Reset);
        return Ok(operations);
    }

    let mut index = start;
    while index < parameters.len() {
        let mut group_end = index;
        while separators.get(group_end) == Some(&ParameterSeparator::Colon) {
            group_end += 1;
        }
        if group_end != index {
            operations.push(
                parse_colon_sgr_operation(&parameters[index..=group_end]).map_err(
                    |(error, offset)| {
                        trace.reject_parameter(index + offset);
                        error
                    },
                )?,
            );
            index = group_end + 1;
            continue;
        }

        let parameter = if parameters[index] == PARAM_MISSING {
            0
        } else {
            parameters[index]
        };
        let operation = match parameter {
            0 => SgrOperation::Reset,
            1 => SgrOperation::Bold,
            2 => SgrOperation::Faint,
            3 => SgrOperation::Italic(true),
            4 => SgrOperation::Underline {
                style: UnderlineStyle::Single,
                reverse_target: Some(UnderlineStyle::Single),
            },
            5 | 6 => SgrOperation::Blink(true),
            7 => SgrOperation::Inverse(true),
            8 => SgrOperation::Conceal(true),
            9 => SgrOperation::Strikethrough(true),
            21 => SgrOperation::Underline {
                style: UnderlineStyle::Double,
                reverse_target: Some(UnderlineStyle::Double),
            },
            22 => SgrOperation::ClearIntensity,
            23 => SgrOperation::Italic(false),
            24 => SgrOperation::Underline {
                style: UnderlineStyle::None,
                reverse_target: Some(UnderlineStyle::Single),
            },
            25 => SgrOperation::Blink(false),
            27 => SgrOperation::Inverse(false),
            28 => SgrOperation::Conceal(false),
            29 => SgrOperation::Strikethrough(false),
            30..=37 => SgrOperation::Color(
                ColorTarget::Foreground,
                Color::Indexed(u8::try_from(parameter - 30).expect("basic foreground")),
            ),
            38 | 48 | 58 => {
                let (color, end) =
                    parse_semicolon_extended_color(csi, index).map_err(|(error, index)| {
                        trace.reject_parameter(index);
                        error
                    })?;
                index = end;
                SgrOperation::Color(color_target(parameter), color)
            }
            39 => SgrOperation::Color(ColorTarget::Foreground, Color::Default),
            40..=47 => SgrOperation::Color(
                ColorTarget::Background,
                Color::Indexed(u8::try_from(parameter - 40).expect("basic background")),
            ),
            49 => SgrOperation::Color(ColorTarget::Background, Color::Default),
            53 => SgrOperation::Overline(true),
            55 => SgrOperation::Overline(false),
            59 => SgrOperation::Color(ColorTarget::Underline, Color::Default),
            90..=97 => SgrOperation::Color(
                ColorTarget::Foreground,
                Color::Indexed(u8::try_from(parameter - 90 + 8).expect("bright foreground")),
            ),
            100..=107 => SgrOperation::Color(
                ColorTarget::Background,
                Color::Indexed(u8::try_from(parameter - 100 + 8).expect("bright background")),
            ),
            _ => {
                trace.reject_parameter(index);
                return Err(TerminalError::Unsupported("SGR parameter"));
            }
        };
        operations.push(operation);
        index += 1;
    }
    Ok(operations)
}

fn parse_colon_sgr_operation(group: &[u16]) -> Result<SgrOperation, (TerminalError, usize)> {
    match group {
        [4, value @ 0..=5] => {
            let style = underline_style(*value);
            Ok(SgrOperation::Underline {
                style,
                reverse_target: (style != UnderlineStyle::None).then_some(style),
            })
        }
        [selector @ (38 | 48 | 58), ..] => Ok(SgrOperation::Color(
            color_target(*selector),
            parse_colon_extended_color(group)?,
        )),
        [4, value, ..] if *value > 5 => Err((TerminalError::Unsupported("SGR underline style"), 1)),
        [4, _, _, ..] => Err((TerminalError::Unsupported("SGR underline group length"), 2)),
        _ => Err((TerminalError::Unsupported("SGR colon group"), 0)),
    }
}

fn underline_style(value: u16) -> UnderlineStyle {
    match value {
        0 => UnderlineStyle::None,
        1 => UnderlineStyle::Single,
        2 => UnderlineStyle::Double,
        3 => UnderlineStyle::Curly,
        4 => UnderlineStyle::Dotted,
        5 => UnderlineStyle::Dashed,
        _ => unreachable!("underline style was range checked"),
    }
}

fn color_target(selector: u16) -> ColorTarget {
    match selector {
        38 => ColorTarget::Foreground,
        48 => ColorTarget::Background,
        58 => ColorTarget::Underline,
        _ => unreachable!("extended color selector was validated"),
    }
}

fn apply_sgr_operations(attributes: &mut Attributes, operations: &[SgrOperation]) {
    for &operation in operations {
        match operation {
            SgrOperation::Reset => *attributes = Attributes::default(),
            SgrOperation::Bold => attributes.bold = true,
            SgrOperation::Faint => attributes.faint = true,
            SgrOperation::ClearIntensity => {
                attributes.bold = false;
                attributes.faint = false;
            }
            SgrOperation::Italic(value) => attributes.italic = value,
            SgrOperation::Underline { style, .. } => attributes.underline_style = style,
            SgrOperation::Blink(value) => attributes.blink = value,
            SgrOperation::Inverse(value) => attributes.inverse = value,
            SgrOperation::Conceal(value) => attributes.conceal = value,
            SgrOperation::Strikethrough(value) => attributes.strikethrough = value,
            SgrOperation::Overline(value) => attributes.overline = value,
            SgrOperation::Color(target, color) => match target {
                ColorTarget::Foreground => attributes.foreground = color,
                ColorTarget::Background => attributes.background = color,
                ColorTarget::Underline => attributes.underline_color = color,
            },
        }
    }
}

fn apply_reverse_sgr_operations(attributes: &mut Attributes, operations: &[SgrOperation]) {
    for &operation in operations {
        match operation {
            SgrOperation::Reset => {
                attributes.bold = !attributes.bold;
                attributes.faint = !attributes.faint;
                attributes.italic = !attributes.italic;
                toggle_underline(attributes, UnderlineStyle::Single);
                attributes.blink = !attributes.blink;
                attributes.inverse = !attributes.inverse;
                attributes.conceal = !attributes.conceal;
                attributes.strikethrough = !attributes.strikethrough;
                attributes.overline = !attributes.overline;
            }
            SgrOperation::Bold => attributes.bold = !attributes.bold,
            SgrOperation::Faint => attributes.faint = !attributes.faint,
            SgrOperation::ClearIntensity => {
                attributes.bold = !attributes.bold;
                attributes.faint = !attributes.faint;
            }
            SgrOperation::Italic(_) => attributes.italic = !attributes.italic,
            SgrOperation::Underline { reverse_target, .. } => {
                if let Some(target) = reverse_target {
                    toggle_underline(attributes, target);
                }
            }
            SgrOperation::Blink(_) => attributes.blink = !attributes.blink,
            SgrOperation::Inverse(_) => attributes.inverse = !attributes.inverse,
            SgrOperation::Conceal(_) => attributes.conceal = !attributes.conceal,
            SgrOperation::Strikethrough(_) => {
                attributes.strikethrough = !attributes.strikethrough;
            }
            SgrOperation::Overline(_) => attributes.overline = !attributes.overline,
            SgrOperation::Color(_, _) => {}
        }
    }
}

fn toggle_underline(attributes: &mut Attributes, target: UnderlineStyle) {
    attributes.underline_style = match attributes.underline_style {
        UnderlineStyle::None => target,
        current if current == target => UnderlineStyle::None,
        current => current,
    };
}

fn parse_semicolon_extended_color(
    csi: &CsiSequence,
    index: usize,
) -> Result<(Color, usize), (TerminalError, usize)> {
    let parameters = csi.parameters();
    let mode = parameters.get(index + 1).copied().ok_or((
        TerminalError::Unsupported("incomplete extended color"),
        index + 1,
    ))?;
    let end = match mode {
        5 => index + 2,
        2 => index + 4,
        _ => return Err((TerminalError::Unsupported("extended color mode"), index + 1)),
    };
    if parameters.get(end).is_none() {
        return Err((
            TerminalError::Unsupported("incomplete extended color"),
            parameters.len(),
        ));
    }
    if let Some(offset) = csi.parameter_separators()
        [index..=end.min(csi.parameter_separators().len() - 1)]
        .iter()
        .position(|separator| *separator == ParameterSeparator::Colon)
    {
        return Err((
            TerminalError::Unsupported("malformed extended color grouping"),
            index + offset + 1,
        ));
    }

    let color = match mode {
        5 => Color::Indexed(parse_color_component(
            parameters[index + 2],
            "indexed color",
            index + 2,
        )?),
        2 => Color::Rgb(
            parse_color_component(parameters[index + 2], "RGB color", index + 2)?,
            parse_color_component(parameters[index + 3], "RGB color", index + 3)?,
            parse_color_component(parameters[index + 4], "RGB color", index + 4)?,
        ),
        _ => unreachable!("extended color mode was validated"),
    };
    Ok((color, end))
}

fn parse_colon_extended_color(group: &[u16]) -> Result<Color, (TerminalError, usize)> {
    let color = match group {
        [38 | 48 | 58, 5, value] => {
            Color::Indexed(parse_color_component(*value, "indexed color", 2)?)
        }
        [38 | 48 | 58, 2, color_space, red, green, blue]
            if *color_space == PARAM_MISSING || *color_space == 0 =>
        {
            Color::Rgb(
                parse_color_component(*red, "RGB color", 3)?,
                parse_color_component(*green, "RGB color", 4)?,
                parse_color_component(*blue, "RGB color", 5)?,
            )
        }
        [38 | 48 | 58, 2, _, _, _, _] => {
            return Err((TerminalError::Unsupported("SGR color space"), 2));
        }
        [_, mode, ..] if !matches!(mode, 2 | 5) => {
            return Err((TerminalError::Unsupported("extended color mode"), 1));
        }
        [_, 5, ..] => {
            return Err((
                TerminalError::Unsupported("malformed SGR colon color"),
                if group.len() > 3 { 3 } else { group.len() },
            ));
        }
        _ => {
            return Err((
                TerminalError::Unsupported("malformed SGR colon color"),
                if group.len() > 6 { 6 } else { group.len() },
            ));
        }
    };
    Ok(color)
}

fn parse_color_component(
    value: u16,
    feature: &'static str,
    index: usize,
) -> Result<u8, (TerminalError, usize)> {
    u8::try_from(value).map_err(|_| (TerminalError::Unsupported(feature), index))
}

fn validate_dimensions(columns: u32, rows: u32) -> Result<(), TerminalError> {
    if columns == 0 || rows == 0 {
        return Err(TerminalError::InvalidDimensions);
    }
    if columns > MAX_COLUMNS || rows > MAX_ROWS || cell_count(columns, rows) > MAX_CELLS {
        return Err(TerminalError::LimitExceeded);
    }
    Ok(())
}

fn validate_capture_events(events: &[Event<'_>]) -> Result<(), TerminalError> {
    if events.len() > MAX_CAPTURE_EVENT_COUNT {
        return Err(TerminalError::LimitExceeded);
    }
    let mut output_bytes = 0_usize;
    for event in events {
        if let Event::Output { bytes, .. } = event {
            output_bytes = output_bytes
                .checked_add(bytes.len())
                .ok_or(TerminalError::LimitExceeded)?;
            if output_bytes > MAX_CAPTURE_OUTPUT_BYTES {
                return Err(TerminalError::LimitExceeded);
            }
        }
    }
    Ok(())
}

fn map_parse_error(error: ParseError) -> TerminalError {
    match error {
        ParseError::OscPayloadAllocation
        | ParseError::DcsQueryPayloadAllocation
        | ParseError::ApcAllocation => TerminalError::LimitExceeded,
        ParseError::ApcHeaderLimit | ParseError::ApcPayloadLimit => {
            TerminalError::ProtocolResourceLimit(error.description())
        }
        other => TerminalError::Unsupported(other.description()),
    }
}

fn map_osc_error(error: OscError) -> TerminalError {
    match error {
        OscError::AllocationFailed => TerminalError::LimitExceeded,
        OscError::LimitExceeded { .. } => TerminalError::Unsupported("OSC state limit"),
        _ => TerminalError::Unsupported("malformed OSC state"),
    }
}

fn map_query_reply_error(error: query_reply::QueryReplyError) -> TerminalError {
    match error {
        query_reply::QueryReplyError::AllocationFailed
        | query_reply::QueryReplyError::LimitExceeded { .. } => TerminalError::LimitExceeded,
        query_reply::QueryReplyError::Unsupported(reason) => TerminalError::Unsupported(reason),
    }
}

fn cell_count(columns: u32, rows: u32) -> usize {
    usize::try_from(u64::from(columns) * u64::from(rows)).expect("cell count fits usize")
}

fn cell_index(columns: u32, row: u32, column: u32) -> usize {
    usize::try_from(u64::from(row) * u64::from(columns) + u64::from(column))
        .expect("cell index fits usize")
}

fn nonzero_parameter(csi: &CsiSequence, index: usize) -> u16 {
    csi.parameter(index, 1).max(1)
}

fn dec_parameter(csi: &CsiSequence, index: usize, default: u16) -> u16 {
    match csi.parameters().get(index).copied() {
        None | Some(0 | PARAM_MISSING) => default,
        Some(value) => value,
    }
}

fn require_max_parameters(
    csi: &CsiSequence,
    maximum: usize,
    feature: &'static str,
) -> Result<(), TerminalError> {
    if csi.parameters().len() > maximum {
        Err(TerminalError::Unsupported(feature))
    } else {
        Ok(())
    }
}

fn require_rectangle_coordinate_separators(csi: &CsiSequence) -> Result<(), TerminalError> {
    if csi
        .parameter_separators()
        .iter()
        .take(4)
        .any(|separator| *separator != ParameterSeparator::Semicolon)
    {
        Err(TerminalError::Unsupported(
            "rectangle rendition coordinate separator",
        ))
    } else {
        Ok(())
    }
}

fn append_decimal(output: &mut [u8], offset: &mut usize, value: u32) {
    let mut digits = [0_u8; 10];
    let mut start = digits.len();
    let mut remaining = value;
    loop {
        start -= 1;
        digits[start] = b'0' + u8::try_from(remaining % 10).expect("decimal digit fits u8");
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    let count = digits.len() - start;
    output[*offset..*offset + count].copy_from_slice(&digits[start..]);
    *offset += count;
}

fn utf8_sequence_length(first: u8) -> Option<u8> {
    match first {
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod color_extensions_tests;

#[cfg(test)]
mod traditional_tests;
