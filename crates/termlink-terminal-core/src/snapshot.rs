use super::sgr_stack::{
    CAPACITY as SGR_STACK_CAPACITY, SELECTORS as SGR_STACK_SELECTORS, SavedRendition, SgrStack,
};
use std::cmp::Ordering;

use super::charset::{
    CharsetDesignation, CharsetSize, CharsetState, GraphicCharset, GraphicRegister,
    NationalReplacementCharset,
};
use super::crc32c;
use super::drcs_bank::GlyphReference;
use super::modes::{
    AttributeChangeExtent, KITTY_KEYBOARD_STACK_CAPACITY, KITTY_KEYBOARD_SUPPORTED_FLAGS,
    KeyboardProtocolStack, Margins, ModeState, ModifyOtherKeys, MouseEncoding, MouseTracking,
};
use super::osc::{HyperlinkRef, MAX_OSC_SNAPSHOT_V2_BYTES, OscCodecError, OscState};
use super::screen::{
    MAX_HISTORY_ROWS, ROW_FLAG_HEAD_TRUNCATED, RowMetadata, SavedCursor, Screen, StoragePolicy,
};
use super::tabs::TabStops;
#[cfg(test)]
use super::unicode_width::scalar_width;
use super::{
    Attributes, Cell, Color, CursorShape, MAX_CELLS, MAX_ROWS, MAX_SNAPSHOT_BYTES, OutputProfile,
    ParserState, RepeatSource, StringUtf8Boundary, Terminal, TerminalError, UnderlineStyle,
    Utf8Decoder, validate_dimensions,
};

const MAGIC: &[u8; 8] = b"TLSNP2S\0";
const MAJOR: u16 = 2;
const MINOR_V2_0: u16 = 0;
const MINOR_V2_1: u16 = 1;
const MINOR_V2_2: u16 = 2;
const MINOR_V2_3: u16 = 3;
const MINOR_V2_4: u16 = 4;
const MINOR_V2_5: u16 = 5;
const MINOR_V2_6: u16 = 6;
const MINOR_V2_7: u16 = 7;
const MINOR_V2_8: u16 = 8;
const MINOR_V2_9: u16 = 9;
const MINOR_V2_10: u16 = 10;
const MINOR_V2_11: u16 = 11;
const MINOR_V2_12: u16 = 12;
const MINOR_V2_13: u16 = 13;
const MINOR_V2_14: u16 = 14;
const MINOR_V2_15: u16 = 15;
const MINOR_V2_16: u16 = 16;
const MINOR_V2_17: u16 = 17;
const MINOR_V2_18: u16 = 18;
const MINOR_V2_19: u16 = 19;
const MINOR_V2_20: u16 = 20;
const MINOR_V2_21: u16 = 21;
const MINOR_V2_22: u16 = 22;
const HEADER_SIZE: usize = 104;
const DIRECTORY_ENTRY_SIZE: usize = 32;
const SECTION_COUNT: usize = 12;
#[cfg(test)]
const SECTION_COUNT_V2_6: usize = 13;
const MAX_LEGACY_KNOWN_SECTIONS: usize = 20;
const MAX_KNOWN_SECTIONS: usize = MAX_LEGACY_KNOWN_SECTIONS + 1;
const MAX_SECTIONS: usize = 64;
const CRITICAL: u16 = 1;
const REQUIRED_FEATURES_V2_0: u128 = 0x18ff;
const HISTORY_ANCHORS_FEATURE: u128 = 0x2000;
const REP_SOURCE_FEATURE: u128 = 0x4000;
const CURSOR_STYLE_FEATURE: u128 = 0x8000;
const EXTENDED_SGR_FEATURE: u128 = 0x1_0000;
const WIN32_INPUT_MODE_FEATURE: u128 = 0x2_0000;
const KEYBOARD_PROTOCOL_STATE_FEATURE: u128 = 0x4_0000;
const NATIONAL_REPLACEMENT_CHARSET_FEATURE: u128 = 0x8_0000;
const STANDALONE_C1_OUTPUT_PROFILE_FEATURE: u128 = 0x10_0000;
const MODERN_PRIVATE_MODES_FEATURE: u128 = 0x20_0000;
const RETAINED_PRIMARY_SCROLLBACK_FEATURE: u128 = 0x40_0000;
const SGR_STACK_FEATURE: u128 = 0x80_0000;
const ALTERNATE_SCROLL_FEATURE: u128 = 0x100_0000;
const UNICODE16_FEATURE: u128 = 0x200_0000;
const LINE_RENDITION_FEATURE: u128 = 0x400_0000;
const SHELL_INTEGRATION_FEATURE: u128 = 0x2_0000_0000;
const GRAPHICS_FEATURE: u128 = 0x8_0000_0000;
const HOST_SIGNALS_FEATURE: u128 = 0x4_0000_0000;
const HOST_THEME_FEATURE: u128 = 0x1_0000_0000;
const HOST_GEOMETRY_FEATURE: u128 = 0x8000_0000;
const PAGES_FEATURE: u128 = 0x4000_0000;
const MACROS_FEATURE: u128 = 0x2000_0000;
const COLOR_EXTENSIONS_FEATURE: u128 = 0x800_0000;
const TRADITIONAL_MODES_FEATURE: u128 = 0x1000_0000;
const NEW_FEATURES_V2_11: u128 = SGR_STACK_FEATURE | ALTERNATE_SCROLL_FEATURE;
const OPTIONAL_FEATURES_V2_11: u128 =
    OPTIONAL_FEATURES_V2_10 | HISTORY_ANCHORS_FEATURE | RETAINED_PRIMARY_SCROLLBACK_FEATURE;
const OPTIONAL_FEATURES_V2_5: u128 =
    HISTORY_ANCHORS_FEATURE | REP_SOURCE_FEATURE | CURSOR_STYLE_FEATURE | EXTENDED_SGR_FEATURE;
const REQUIRED_FEATURES_V2_5: u128 = REQUIRED_FEATURES_V2_0 | WIN32_INPUT_MODE_FEATURE;
const OPTIONAL_FEATURES_V2_6: u128 = HISTORY_ANCHORS_FEATURE
    | REP_SOURCE_FEATURE
    | CURSOR_STYLE_FEATURE
    | EXTENDED_SGR_FEATURE
    | WIN32_INPUT_MODE_FEATURE;
const REQUIRED_FEATURES_V2_6: u128 = REQUIRED_FEATURES_V2_0 | KEYBOARD_PROTOCOL_STATE_FEATURE;
const OPTIONAL_FEATURES_V2_7: u128 = HISTORY_ANCHORS_FEATURE
    | REP_SOURCE_FEATURE
    | CURSOR_STYLE_FEATURE
    | EXTENDED_SGR_FEATURE
    | WIN32_INPUT_MODE_FEATURE
    | KEYBOARD_PROTOCOL_STATE_FEATURE;
const REQUIRED_FEATURES_V2_7: u128 = REQUIRED_FEATURES_V2_0 | NATIONAL_REPLACEMENT_CHARSET_FEATURE;
const OPTIONAL_FEATURES_V2_8: u128 = HISTORY_ANCHORS_FEATURE
    | REP_SOURCE_FEATURE
    | CURSOR_STYLE_FEATURE
    | EXTENDED_SGR_FEATURE
    | WIN32_INPUT_MODE_FEATURE
    | KEYBOARD_PROTOCOL_STATE_FEATURE
    | NATIONAL_REPLACEMENT_CHARSET_FEATURE;
const REQUIRED_FEATURES_V2_8: u128 = REQUIRED_FEATURES_V2_0 | STANDALONE_C1_OUTPUT_PROFILE_FEATURE;
const OPTIONAL_FEATURES_V2_9: u128 = HISTORY_ANCHORS_FEATURE
    | REP_SOURCE_FEATURE
    | CURSOR_STYLE_FEATURE
    | EXTENDED_SGR_FEATURE
    | WIN32_INPUT_MODE_FEATURE
    | KEYBOARD_PROTOCOL_STATE_FEATURE
    | NATIONAL_REPLACEMENT_CHARSET_FEATURE
    | STANDALONE_C1_OUTPUT_PROFILE_FEATURE;
const REQUIRED_FEATURES_V2_9: u128 = REQUIRED_FEATURES_V2_0 | MODERN_PRIVATE_MODES_FEATURE;
const OPTIONAL_FEATURES_V2_10: u128 = REP_SOURCE_FEATURE
    | CURSOR_STYLE_FEATURE
    | EXTENDED_SGR_FEATURE
    | WIN32_INPUT_MODE_FEATURE
    | KEYBOARD_PROTOCOL_STATE_FEATURE
    | NATIONAL_REPLACEMENT_CHARSET_FEATURE
    | STANDALONE_C1_OUTPUT_PROFILE_FEATURE
    | MODERN_PRIVATE_MODES_FEATURE;
const REQUIRED_FEATURES_V2_10: u128 = REQUIRED_FEATURES_V2_1 | RETAINED_PRIMARY_SCROLLBACK_FEATURE;
const REQUIRED_FEATURES_V2_1: u128 = REQUIRED_FEATURES_V2_0 | HISTORY_ANCHORS_FEATURE;
const REQUIRED_FEATURES_V2_2: u128 = REQUIRED_FEATURES_V2_0 | REP_SOURCE_FEATURE;
const REQUIRED_FEATURES_V2_2_HISTORY: u128 = REQUIRED_FEATURES_V2_1 | REP_SOURCE_FEATURE;
const REQUIRED_FEATURES_V2_3: u128 = REQUIRED_FEATURES_V2_0 | CURSOR_STYLE_FEATURE;
const REQUIRED_FEATURES_V2_3_HISTORY: u128 = REQUIRED_FEATURES_V2_1 | CURSOR_STYLE_FEATURE;
const REQUIRED_FEATURES_V2_3_REPEAT: u128 = REQUIRED_FEATURES_V2_2 | CURSOR_STYLE_FEATURE;
const REQUIRED_FEATURES_V2_3_HISTORY_REPEAT: u128 =
    REQUIRED_FEATURES_V2_2_HISTORY | CURSOR_STYLE_FEATURE;
const REQUIRED_FEATURES_V2_4: u128 = REQUIRED_FEATURES_V2_0 | EXTENDED_SGR_FEATURE;
const REQUIRED_FEATURES_V2_4_HISTORY: u128 = REQUIRED_FEATURES_V2_1 | EXTENDED_SGR_FEATURE;
const REQUIRED_FEATURES_V2_4_REPEAT: u128 = REQUIRED_FEATURES_V2_2 | EXTENDED_SGR_FEATURE;
const REQUIRED_FEATURES_V2_4_HISTORY_REPEAT: u128 =
    REQUIRED_FEATURES_V2_2_HISTORY | EXTENDED_SGR_FEATURE;
const REQUIRED_FEATURES_V2_4_CURSOR: u128 = REQUIRED_FEATURES_V2_3 | EXTENDED_SGR_FEATURE;
const REQUIRED_FEATURES_V2_4_HISTORY_CURSOR: u128 =
    REQUIRED_FEATURES_V2_3_HISTORY | EXTENDED_SGR_FEATURE;
const REQUIRED_FEATURES_V2_4_REPEAT_CURSOR: u128 =
    REQUIRED_FEATURES_V2_3_REPEAT | EXTENDED_SGR_FEATURE;
const REQUIRED_FEATURES_V2_4_HISTORY_REPEAT_CURSOR: u128 =
    REQUIRED_FEATURES_V2_3_HISTORY_REPEAT | EXTENDED_SGR_FEATURE;

// The two screens are independently capped at MAX_CELLS. The mandatory blank
// grapheme and the four screen/saved-cursor renditions can add entries that do
// not occur in either cell array.
const MAX_GRAPHEME_TABLE_ENTRIES: usize = 2 * MAX_CELLS + crate::pages::MAX_EXTRA_CELLS + 2;
const MAX_RENDITION_TABLE_ENTRIES: usize =
    2 * MAX_CELLS + crate::pages::MAX_EXTRA_CELLS + 15 + SGR_STACK_CAPACITY;
const GRAPHEME_WIDTH_WORDS: usize = MAX_GRAPHEME_TABLE_ENTRIES.div_ceil(u64::BITS as usize);
const RENDITION_USE_WORDS: usize = MAX_RENDITION_TABLE_ENTRIES.div_ceil(u64::BITS as usize);
const ENVELOPE_BYTES: usize = HEADER_SIZE + MAX_LEGACY_KNOWN_SECTIONS * DIRECTORY_ENTRY_SIZE;
const MAX_GRAPHEME_SECTION_BYTES: usize = 16 + (MAX_GRAPHEME_TABLE_ENTRIES - 1) * 52;
const MAX_RENDITION_SECTION_BYTES: usize = 8 + MAX_RENDITION_TABLE_ENTRIES * 48;
const MAX_LEGACY_ROWS_SECTION_BYTES: usize = 8 + MAX_ROWS as usize * 24 + MAX_CELLS * 16;
const MAX_PRIMARY_ROWS_SECTION_BYTES: usize =
    16 + (MAX_ROWS as usize + MAX_HISTORY_ROWS as usize) * 32 + MAX_CELLS * 16;

const MAX_PAGES_SECTION_BYTES: usize =
    16 + 5 * (8 + 64 + 8 + MAX_ROWS as usize * 24) + crate::pages::MAX_EXTRA_CELLS * 16;

// Every payload bound below is independent and conservative. There is one
// pre-section alignment per directory entry and one final alignment.
const MAX_CANONICAL_SNAPSHOT_BYTES: usize = ENVELOPE_BYTES
    + 32 // core
    + 200 // at most twenty-four mode records
    + 24 // margins
    + 72 // tab stops
    + 24 // two bounded Kitty keyboard-protocol stacks
    + 8 + SGR_STACK_CAPACITY * 8
    + crate::macros::MAX_SNAPSHOT_BYTES
    + MAX_PAGES_SECTION_BYTES
    + crate::HOST_GEOMETRY_V1_BYTES
    + crate::host_theme::THEME_STATE_BYTES
    + crate::MAX_SHELL_STATE_BYTES
    + crate::HOST_SIGNALS_V1_BYTES
    + 52 // charset records and optional preference
    + MAX_OSC_SNAPSHOT_V2_BYTES
    + MAX_GRAPHEME_SECTION_BYTES
    + MAX_RENDITION_SECTION_BYTES
    + 64 // primary metadata
    + MAX_PRIMARY_ROWS_SECTION_BYTES
    + 64 // alternate metadata
    + MAX_LEGACY_ROWS_SECTION_BYTES
    + (MAX_LEGACY_KNOWN_SECTIONS + 1) * 7;
const _: () = assert!(MAX_CANONICAL_SNAPSHOT_BYTES <= MAX_SNAPSHOT_BYTES);
const MAX_CANONICAL_GRAPHICS_SNAPSHOT_BYTES: usize = MAX_CANONICAL_SNAPSHOT_BYTES
    + 39
    + crate::graphics_state::MAX_GRAPHICS_STATE_BYTES
    + 20 * (MAX_GRAPHEME_TABLE_ENTRIES - 1)
    + 36; // Three charset records grow from 12 to 24 bytes.
const _: () = assert!(MAX_CANONICAL_GRAPHICS_SNAPSHOT_BYTES <= crate::MAX_GRAPHICS_SNAPSHOT_BYTES);

const CORE_STATE: u32 = 0x0000_0001;
const MODE_BANK: u32 = 0x0000_0002;
const MARGINS: u32 = 0x0000_0003;
const TAB_STOPS: u32 = 0x0000_0004;
const KEYBOARD_PROTOCOL_STATE: u32 = 0x0000_0008;
const MACROS_STATE: u32 = 0x0000_0018;
const SGR_STACK_STATE: u32 = 0x0000_0010;
const CHARSET_STATE: u32 = 0x0000_0020;
const OSC_STATE: u32 = 0x0000_0040;
const GRAPHEME_TABLE: u32 = 0x0000_0100;
const RENDITION_TABLE: u32 = 0x0000_0101;
const PRIMARY_META: u32 = 0x0000_0200;
const PRIMARY_ROWS: u32 = 0x0000_0201;
const ALTERNATE_META: u32 = 0x0000_0210;
const ALTERNATE_ROWS: u32 = 0x0000_0211;
const PAGES_STATE: u32 = 0x0000_0300;
const HOST_GEOMETRY_STATE: u32 = 0x0000_0400;
const HOST_THEME_STATE: u32 = 0x0000_0401;
const SHELL_INTEGRATION_STATE: u32 = 0x0000_0402;
const HOST_SIGNALS_STATE: u32 = 0x0000_0403;
const GRAPHICS_STATE: u32 = 0x0000_0500;

const ALL_TAGS: [u32; MAX_KNOWN_SECTIONS] = [
    CORE_STATE,
    MODE_BANK,
    MARGINS,
    TAB_STOPS,
    KEYBOARD_PROTOCOL_STATE,
    SGR_STACK_STATE,
    MACROS_STATE,
    CHARSET_STATE,
    OSC_STATE,
    GRAPHEME_TABLE,
    RENDITION_TABLE,
    PRIMARY_META,
    PRIMARY_ROWS,
    ALTERNATE_META,
    ALTERNATE_ROWS,
    PAGES_STATE,
    HOST_GEOMETRY_STATE,
    HOST_THEME_STATE,
    SHELL_INTEGRATION_STATE,
    HOST_SIGNALS_STATE,
    GRAPHICS_STATE,
];

#[derive(Clone, Copy, Debug, Default)]
struct SectionDesc {
    tag: u32,
    flags: u16,
    version: u16,
    offset: usize,
    length: usize,
    logical_count: u32,
}

#[derive(Clone, Copy)]
struct ScreenMeta {
    columns: u32,
    visible_rows: u32,
    stored_rows: u32,
    viewport_top: u32,
    cursor_row: u32,
    cursor_column: u32,
    cursor_flags: u32,
    attributes_id: u32,
    saved_row: u32,
    saved_column: u32,
    saved_flags: u32,
    saved_attributes_id: u32,
    next_logical_line_id: u64,
    requested_history_rows: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnapshotVersion {
    V2_22 {
        features: u128,
    },
    V2_21 {
        features: u128,
    },
    V2_20 {
        features: u128,
    },
    V2_19 {
        features: u128,
    },
    V2_18 {
        features: u128,
    },
    V2_17 {
        features: u128,
    },
    V2_16 {
        features: u128,
    },
    V2_15 {
        features: u128,
    },
    V2_14 {
        features: u128,
    },
    V2_13 {
        features: u128,
    },
    V2_12 {
        features: u128,
    },
    V2_11 {
        features: u128,
    },
    V2_0,
    V2_1,
    V2_2 {
        history: bool,
    },
    V2_3 {
        history: bool,
        repeat: bool,
    },
    V2_4 {
        history: bool,
        repeat: bool,
        cursor: bool,
    },
    V2_5 {
        history: bool,
        repeat: bool,
        cursor: bool,
        extended_sgr: bool,
    },
    V2_6 {
        history: bool,
        repeat: bool,
        cursor: bool,
        extended_sgr: bool,
        win32_input: bool,
    },
    V2_7 {
        history: bool,
        repeat: bool,
        cursor: bool,
        extended_sgr: bool,
        win32_input: bool,
        keyboard_protocol: bool,
    },
    V2_8 {
        history: bool,
        repeat: bool,
        cursor: bool,
        extended_sgr: bool,
        win32_input: bool,
        keyboard_protocol: bool,
        national_replacement: bool,
    },
    V2_9 {
        history: bool,
        repeat: bool,
        cursor: bool,
        extended_sgr: bool,
        win32_input: bool,
        keyboard_protocol: bool,
        national_replacement: bool,
        standalone_c1: bool,
    },
    V2_10 {
        repeat: bool,
        cursor: bool,
        extended_sgr: bool,
        win32_input: bool,
        keyboard_protocol: bool,
        national_replacement: bool,
        standalone_c1: bool,
        modern_private_modes: bool,
    },
}

impl SnapshotVersion {
    fn for_terminal(terminal: &Terminal) -> Self {
        let legacy = Self::for_legacy_terminal(terminal);
        let extensions = if terminal.sgr_stack.is_empty() {
            0
        } else {
            SGR_STACK_FEATURE
        } | if terminal.modes.alternate_scroll {
            ALTERNATE_SCROLL_FEATURE
        } else {
            0
        };
        let base = if terminal.main.has_line_renditions()
            || terminal.alternate.has_line_renditions()
            || terminal.pages.screens().any(Screen::has_line_renditions)
        {
            Self::V2_13 {
                features: legacy.required_features()
                    | extensions
                    | LINE_RENDITION_FEATURE
                    | if terminal.width_profile() == crate::WidthProfile::Unicode16 {
                        UNICODE16_FEATURE
                    } else {
                        0
                    },
            }
        } else if terminal.width_profile() == crate::WidthProfile::Unicode16 {
            Self::V2_12 {
                features: legacy.required_features() | extensions | UNICODE16_FEATURE,
            }
        } else if extensions == 0 {
            legacy
        } else {
            Self::V2_11 {
                features: legacy.required_features() | extensions,
            }
        };
        let base = if terminal.osc.has_extended_colors() {
            Self::V2_14 {
                features: base.required_features() | COLOR_EXTENSIONS_FEATURE,
            }
        } else {
            base
        };
        let base = if terminal.modes.has_traditional_modes()
            || terminal.charset.has_extended()
            || terminal.main_saved_charset.has_extended()
            || terminal.alternate_saved_charset.has_extended()
            || terminal.user_preferred_charset != CharsetDesignation::LATIN1
        {
            Self::V2_15 {
                features: base.required_features() | TRADITIONAL_MODES_FEATURE,
            }
        } else {
            base
        };
        let base = if terminal.macros.is_empty() {
            base
        } else {
            Self::V2_16 {
                features: base.required_features() | MACROS_FEATURE,
            }
        };
        let base = if terminal.pages.has_state() {
            Self::V2_17 {
                features: base.required_features() | PAGES_FEATURE,
            }
        } else {
            base
        };
        let base = if terminal.host_geometry != crate::HostGeometryV1::default() {
            Self::V2_18 {
                features: base.required_features() | HOST_GEOMETRY_FEATURE,
            }
        } else {
            base
        };
        let base = if terminal.osc.theme_scheme() != 0 {
            Self::V2_19 {
                features: base.required_features() | HOST_THEME_FEATURE,
            }
        } else {
            base
        };
        let base = if terminal.shell_integration.has_state() {
            Self::V2_20 {
                features: base.required_features() | SHELL_INTEGRATION_FEATURE,
            }
        } else {
            base
        };
        let base = if terminal.host_signals != crate::HostSignalsV1::default() {
            Self::V2_21 {
                features: base.required_features() | HOST_SIGNALS_FEATURE,
            }
        } else {
            base
        };
        if terminal.graphics.is_some() {
            Self::V2_22 {
                features: base.required_features() | GRAPHICS_FEATURE | HISTORY_ANCHORS_FEATURE,
            }
        } else {
            base
        }
    }

    fn for_legacy_terminal(terminal: &Terminal) -> Self {
        let history = terminal.main.requested_history_rows() != 0;
        let repeat = terminal.repeat_source.is_some();
        let cursor_style = terminal.cursor_shape != CursorShape::Block || !terminal.cursor_blinking;
        let extended_sgr = terminal_has_extended_sgr(terminal);
        let keyboard_protocol = !terminal.main_keyboard_protocol.is_empty()
            || !terminal.alternate_keyboard_protocol.is_empty();
        let national_replacement = terminal.modes.national_replacement_character_set_mode
            || charset_has_national_replacement(terminal.charset)
            || charset_has_national_replacement(terminal.main_saved_charset)
            || charset_has_national_replacement(terminal.alternate_saved_charset)
            || matches!(
                terminal.user_preferred_charset.charset,
                GraphicCharset::NationalReplacement(_)
            );
        let modern_private_modes =
            terminal.modes.grapheme_clustering || terminal.modes.color_scheme_reporting;
        if history && terminal.main.viewport_top() != 0 {
            Self::V2_10 {
                repeat,
                cursor: cursor_style,
                extended_sgr,
                win32_input: terminal.modes.win32_input_mode,
                keyboard_protocol,
                national_replacement,
                standalone_c1: terminal.output_profile == OutputProfile::Utf8WithStandaloneC1,
                modern_private_modes,
            }
        } else if modern_private_modes {
            Self::V2_9 {
                history,
                repeat,
                cursor: cursor_style,
                extended_sgr,
                win32_input: terminal.modes.win32_input_mode,
                keyboard_protocol,
                national_replacement,
                standalone_c1: terminal.output_profile == OutputProfile::Utf8WithStandaloneC1,
            }
        } else if terminal.output_profile == OutputProfile::Utf8WithStandaloneC1 {
            Self::V2_8 {
                history,
                repeat,
                cursor: cursor_style,
                extended_sgr,
                win32_input: terminal.modes.win32_input_mode,
                keyboard_protocol,
                national_replacement,
            }
        } else if national_replacement {
            Self::V2_7 {
                history,
                repeat,
                cursor: cursor_style,
                extended_sgr,
                win32_input: terminal.modes.win32_input_mode,
                keyboard_protocol,
            }
        } else if keyboard_protocol {
            Self::V2_6 {
                history,
                repeat,
                cursor: cursor_style,
                extended_sgr,
                win32_input: terminal.modes.win32_input_mode,
            }
        } else if terminal.modes.win32_input_mode {
            Self::V2_5 {
                history,
                repeat,
                cursor: cursor_style,
                extended_sgr,
            }
        } else if extended_sgr {
            Self::V2_4 {
                history,
                repeat,
                cursor: cursor_style,
            }
        } else if cursor_style {
            Self::V2_3 { history, repeat }
        } else if repeat {
            Self::V2_2 { history }
        } else if history {
            Self::V2_1
        } else {
            Self::V2_0
        }
    }

    fn has_history(self) -> bool {
        if let Self::V2_11 { features }
        | Self::V2_12 { features }
        | Self::V2_13 { features }
        | Self::V2_14 { features }
        | Self::V2_15 { features }
        | Self::V2_16 { features }
        | Self::V2_17 { features }
        | Self::V2_22 { features }
        | Self::V2_21 { features }
        | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } = self
        {
            return features & HISTORY_ANCHORS_FEATURE != 0;
        }
        matches!(
            self,
            Self::V2_1
                | Self::V2_2 { history: true }
                | Self::V2_3 { history: true, .. }
                | Self::V2_4 { history: true, .. }
                | Self::V2_5 { history: true, .. }
                | Self::V2_6 { history: true, .. }
                | Self::V2_7 { history: true, .. }
                | Self::V2_8 { history: true, .. }
                | Self::V2_9 { history: true, .. }
                | Self::V2_10 { .. }
        )
    }

    fn has_repeat_source(self) -> bool {
        if let Self::V2_11 { features }
        | Self::V2_12 { features }
        | Self::V2_13 { features }
        | Self::V2_14 { features }
        | Self::V2_15 { features }
        | Self::V2_16 { features }
        | Self::V2_17 { features }
        | Self::V2_22 { features }
        | Self::V2_21 { features }
        | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } = self
        {
            return features & REP_SOURCE_FEATURE != 0;
        }
        matches!(
            self,
            Self::V2_2 { .. }
                | Self::V2_3 { repeat: true, .. }
                | Self::V2_4 { repeat: true, .. }
                | Self::V2_5 { repeat: true, .. }
                | Self::V2_6 { repeat: true, .. }
                | Self::V2_7 { repeat: true, .. }
                | Self::V2_8 { repeat: true, .. }
                | Self::V2_9 { repeat: true, .. }
                | Self::V2_10 { repeat: true, .. }
        )
    }

    fn has_cursor_style(self) -> bool {
        if let Self::V2_11 { features }
        | Self::V2_12 { features }
        | Self::V2_13 { features }
        | Self::V2_14 { features }
        | Self::V2_15 { features }
        | Self::V2_16 { features }
        | Self::V2_17 { features }
        | Self::V2_22 { features }
        | Self::V2_21 { features }
        | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } = self
        {
            return features & CURSOR_STYLE_FEATURE != 0;
        }
        matches!(
            self,
            Self::V2_3 { .. }
                | Self::V2_4 { cursor: true, .. }
                | Self::V2_5 { cursor: true, .. }
                | Self::V2_6 { cursor: true, .. }
                | Self::V2_7 { cursor: true, .. }
                | Self::V2_8 { cursor: true, .. }
                | Self::V2_9 { cursor: true, .. }
                | Self::V2_10 { cursor: true, .. }
        )
    }

    fn has_extended_sgr(self) -> bool {
        if let Self::V2_11 { features }
        | Self::V2_12 { features }
        | Self::V2_13 { features }
        | Self::V2_14 { features }
        | Self::V2_15 { features }
        | Self::V2_16 { features }
        | Self::V2_17 { features }
        | Self::V2_22 { features }
        | Self::V2_21 { features }
        | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } = self
        {
            return features & EXTENDED_SGR_FEATURE != 0;
        }
        matches!(
            self,
            Self::V2_4 { .. }
                | Self::V2_5 {
                    extended_sgr: true,
                    ..
                }
                | Self::V2_6 {
                    extended_sgr: true,
                    ..
                }
                | Self::V2_7 {
                    extended_sgr: true,
                    ..
                }
                | Self::V2_8 {
                    extended_sgr: true,
                    ..
                }
                | Self::V2_9 {
                    extended_sgr: true,
                    ..
                }
                | Self::V2_10 {
                    extended_sgr: true,
                    ..
                }
        )
    }

    fn has_win32_input_mode(self) -> bool {
        if let Self::V2_11 { features }
        | Self::V2_12 { features }
        | Self::V2_13 { features }
        | Self::V2_14 { features }
        | Self::V2_15 { features }
        | Self::V2_16 { features }
        | Self::V2_17 { features }
        | Self::V2_22 { features }
        | Self::V2_21 { features }
        | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } = self
        {
            return features & WIN32_INPUT_MODE_FEATURE != 0;
        }
        matches!(
            self,
            Self::V2_5 { .. }
                | Self::V2_6 {
                    win32_input: true,
                    ..
                }
                | Self::V2_7 {
                    win32_input: true,
                    ..
                }
                | Self::V2_8 {
                    win32_input: true,
                    ..
                }
                | Self::V2_9 {
                    win32_input: true,
                    ..
                }
                | Self::V2_10 {
                    win32_input: true,
                    ..
                }
        )
    }

    fn has_keyboard_protocol_state(self) -> bool {
        if let Self::V2_11 { features }
        | Self::V2_12 { features }
        | Self::V2_13 { features }
        | Self::V2_14 { features }
        | Self::V2_15 { features }
        | Self::V2_16 { features }
        | Self::V2_17 { features }
        | Self::V2_22 { features }
        | Self::V2_21 { features }
        | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } = self
        {
            return features & KEYBOARD_PROTOCOL_STATE_FEATURE != 0;
        }
        matches!(
            self,
            Self::V2_6 { .. }
                | Self::V2_7 {
                    keyboard_protocol: true,
                    ..
                }
                | Self::V2_8 {
                    keyboard_protocol: true,
                    ..
                }
                | Self::V2_9 {
                    keyboard_protocol: true,
                    ..
                }
                | Self::V2_10 {
                    keyboard_protocol: true,
                    ..
                }
        )
    }

    fn has_national_replacement_charset(self) -> bool {
        if let Self::V2_11 { features }
        | Self::V2_12 { features }
        | Self::V2_13 { features }
        | Self::V2_14 { features }
        | Self::V2_15 { features }
        | Self::V2_16 { features }
        | Self::V2_17 { features }
        | Self::V2_22 { features }
        | Self::V2_21 { features }
        | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } = self
        {
            return features & NATIONAL_REPLACEMENT_CHARSET_FEATURE != 0;
        }
        matches!(
            self,
            Self::V2_7 { .. }
                | Self::V2_8 {
                    national_replacement: true,
                    ..
                }
                | Self::V2_9 {
                    national_replacement: true,
                    ..
                }
                | Self::V2_10 {
                    national_replacement: true,
                    ..
                }
        )
    }

    fn has_standalone_c1_output_profile(self) -> bool {
        if let Self::V2_11 { features }
        | Self::V2_12 { features }
        | Self::V2_13 { features }
        | Self::V2_14 { features }
        | Self::V2_15 { features }
        | Self::V2_16 { features }
        | Self::V2_17 { features }
        | Self::V2_22 { features }
        | Self::V2_21 { features }
        | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } = self
        {
            return features & STANDALONE_C1_OUTPUT_PROFILE_FEATURE != 0;
        }
        matches!(
            self,
            Self::V2_8 { .. }
                | Self::V2_9 {
                    standalone_c1: true,
                    ..
                }
                | Self::V2_10 {
                    standalone_c1: true,
                    ..
                }
        )
    }

    fn has_modern_private_modes(self) -> bool {
        if let Self::V2_11 { features }
        | Self::V2_12 { features }
        | Self::V2_13 { features }
        | Self::V2_14 { features }
        | Self::V2_15 { features }
        | Self::V2_16 { features }
        | Self::V2_17 { features }
        | Self::V2_22 { features }
        | Self::V2_21 { features }
        | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } = self
        {
            return features & MODERN_PRIVATE_MODES_FEATURE != 0;
        }
        matches!(
            self,
            Self::V2_9 { .. }
                | Self::V2_10 {
                    modern_private_modes: true,
                    ..
                }
        )
    }

    fn has_retained_primary_scrollback(self) -> bool {
        if let Self::V2_11 { features }
        | Self::V2_12 { features }
        | Self::V2_13 { features }
        | Self::V2_14 { features }
        | Self::V2_15 { features }
        | Self::V2_16 { features }
        | Self::V2_17 { features }
        | Self::V2_22 { features }
        | Self::V2_21 { features }
        | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } = self
        {
            return features & RETAINED_PRIMARY_SCROLLBACK_FEATURE != 0;
        }
        matches!(self, Self::V2_10 { .. })
    }

    fn v2_5_from_features(features: u128) -> Option<Self> {
        if features & !OPTIONAL_FEATURES_V2_5 != REQUIRED_FEATURES_V2_5 {
            return None;
        }
        Some(Self::V2_5 {
            history: features & HISTORY_ANCHORS_FEATURE != 0,
            repeat: features & REP_SOURCE_FEATURE != 0,
            cursor: features & CURSOR_STYLE_FEATURE != 0,
            extended_sgr: features & EXTENDED_SGR_FEATURE != 0,
        })
    }

    fn v2_6_from_features(features: u128) -> Option<Self> {
        if features & !OPTIONAL_FEATURES_V2_6 != REQUIRED_FEATURES_V2_6 {
            return None;
        }
        Some(Self::V2_6 {
            history: features & HISTORY_ANCHORS_FEATURE != 0,
            repeat: features & REP_SOURCE_FEATURE != 0,
            cursor: features & CURSOR_STYLE_FEATURE != 0,
            extended_sgr: features & EXTENDED_SGR_FEATURE != 0,
            win32_input: features & WIN32_INPUT_MODE_FEATURE != 0,
        })
    }

    fn v2_7_from_features(features: u128) -> Option<Self> {
        if features & !OPTIONAL_FEATURES_V2_7 != REQUIRED_FEATURES_V2_7 {
            return None;
        }
        Some(Self::V2_7 {
            history: features & HISTORY_ANCHORS_FEATURE != 0,
            repeat: features & REP_SOURCE_FEATURE != 0,
            cursor: features & CURSOR_STYLE_FEATURE != 0,
            extended_sgr: features & EXTENDED_SGR_FEATURE != 0,
            win32_input: features & WIN32_INPUT_MODE_FEATURE != 0,
            keyboard_protocol: features & KEYBOARD_PROTOCOL_STATE_FEATURE != 0,
        })
    }

    fn v2_8_from_features(features: u128) -> Option<Self> {
        if features & !OPTIONAL_FEATURES_V2_8 != REQUIRED_FEATURES_V2_8 {
            return None;
        }
        Some(Self::V2_8 {
            history: features & HISTORY_ANCHORS_FEATURE != 0,
            repeat: features & REP_SOURCE_FEATURE != 0,
            cursor: features & CURSOR_STYLE_FEATURE != 0,
            extended_sgr: features & EXTENDED_SGR_FEATURE != 0,
            win32_input: features & WIN32_INPUT_MODE_FEATURE != 0,
            keyboard_protocol: features & KEYBOARD_PROTOCOL_STATE_FEATURE != 0,
            national_replacement: features & NATIONAL_REPLACEMENT_CHARSET_FEATURE != 0,
        })
    }

    fn v2_9_from_features(features: u128) -> Option<Self> {
        if features & !OPTIONAL_FEATURES_V2_9 != REQUIRED_FEATURES_V2_9 {
            return None;
        }
        Some(Self::V2_9 {
            history: features & HISTORY_ANCHORS_FEATURE != 0,
            repeat: features & REP_SOURCE_FEATURE != 0,
            cursor: features & CURSOR_STYLE_FEATURE != 0,
            extended_sgr: features & EXTENDED_SGR_FEATURE != 0,
            win32_input: features & WIN32_INPUT_MODE_FEATURE != 0,
            keyboard_protocol: features & KEYBOARD_PROTOCOL_STATE_FEATURE != 0,
            national_replacement: features & NATIONAL_REPLACEMENT_CHARSET_FEATURE != 0,
            standalone_c1: features & STANDALONE_C1_OUTPUT_PROFILE_FEATURE != 0,
        })
    }

    fn v2_10_from_features(features: u128) -> Option<Self> {
        if features & !OPTIONAL_FEATURES_V2_10 != REQUIRED_FEATURES_V2_10 {
            return None;
        }
        Some(Self::V2_10 {
            repeat: features & REP_SOURCE_FEATURE != 0,
            cursor: features & CURSOR_STYLE_FEATURE != 0,
            extended_sgr: features & EXTENDED_SGR_FEATURE != 0,
            win32_input: features & WIN32_INPUT_MODE_FEATURE != 0,
            keyboard_protocol: features & KEYBOARD_PROTOCOL_STATE_FEATURE != 0,
            national_replacement: features & NATIONAL_REPLACEMENT_CHARSET_FEATURE != 0,
            standalone_c1: features & STANDALONE_C1_OUTPUT_PROFILE_FEATURE != 0,
            modern_private_modes: features & MODERN_PRIVATE_MODES_FEATURE != 0,
        })
    }

    fn minor(self) -> u16 {
        match self {
            Self::V2_13 { .. } => MINOR_V2_13,
            Self::V2_14 { .. } => MINOR_V2_14,
            Self::V2_15 { .. } => MINOR_V2_15,
            Self::V2_16 { .. } => MINOR_V2_16,
            Self::V2_17 { .. } => MINOR_V2_17,
            Self::V2_22 { .. } => MINOR_V2_22,
            Self::V2_21 { .. } => MINOR_V2_21,
            Self::V2_20 { .. } => MINOR_V2_20,
            Self::V2_19 { .. } => MINOR_V2_19,
            Self::V2_18 { .. } => MINOR_V2_18,
            Self::V2_12 { .. } => MINOR_V2_12,
            Self::V2_11 { .. } => MINOR_V2_11,
            Self::V2_0 => MINOR_V2_0,
            Self::V2_1 => MINOR_V2_1,
            Self::V2_2 { .. } => MINOR_V2_2,
            Self::V2_3 { .. } => MINOR_V2_3,
            Self::V2_4 { .. } => MINOR_V2_4,
            Self::V2_5 { .. } => MINOR_V2_5,
            Self::V2_6 { .. } => MINOR_V2_6,
            Self::V2_7 { .. } => MINOR_V2_7,
            Self::V2_8 { .. } => MINOR_V2_8,
            Self::V2_9 { .. } => MINOR_V2_9,
            Self::V2_10 { .. } => MINOR_V2_10,
        }
    }

    fn required_features(self) -> u128 {
        match self {
            Self::V2_11 { features }
            | Self::V2_12 { features }
            | Self::V2_13 { features }
            | Self::V2_14 { features }
            | Self::V2_15 { features }
            | Self::V2_16 { features }
            | Self::V2_17 { features }
            | Self::V2_22 { features }
            | Self::V2_21 { features }
            | Self::V2_20 { features }
            | Self::V2_19 { features }
            | Self::V2_18 { features } => features,
            Self::V2_0 => REQUIRED_FEATURES_V2_0,
            Self::V2_1 => REQUIRED_FEATURES_V2_1,
            Self::V2_2 { history: false } => REQUIRED_FEATURES_V2_2,
            Self::V2_2 { history: true } => REQUIRED_FEATURES_V2_2_HISTORY,
            Self::V2_3 {
                history: false,
                repeat: false,
            } => REQUIRED_FEATURES_V2_3,
            Self::V2_3 {
                history: true,
                repeat: false,
            } => REQUIRED_FEATURES_V2_3_HISTORY,
            Self::V2_3 {
                history: false,
                repeat: true,
            } => REQUIRED_FEATURES_V2_3_REPEAT,
            Self::V2_3 {
                history: true,
                repeat: true,
            } => REQUIRED_FEATURES_V2_3_HISTORY_REPEAT,
            Self::V2_4 {
                history: false,
                repeat: false,
                cursor: false,
            } => REQUIRED_FEATURES_V2_4,
            Self::V2_4 {
                history: true,
                repeat: false,
                cursor: false,
            } => REQUIRED_FEATURES_V2_4_HISTORY,
            Self::V2_4 {
                history: false,
                repeat: true,
                cursor: false,
            } => REQUIRED_FEATURES_V2_4_REPEAT,
            Self::V2_4 {
                history: true,
                repeat: true,
                cursor: false,
            } => REQUIRED_FEATURES_V2_4_HISTORY_REPEAT,
            Self::V2_4 {
                history: false,
                repeat: false,
                cursor: true,
            } => REQUIRED_FEATURES_V2_4_CURSOR,
            Self::V2_4 {
                history: true,
                repeat: false,
                cursor: true,
            } => REQUIRED_FEATURES_V2_4_HISTORY_CURSOR,
            Self::V2_4 {
                history: false,
                repeat: true,
                cursor: true,
            } => REQUIRED_FEATURES_V2_4_REPEAT_CURSOR,
            Self::V2_4 {
                history: true,
                repeat: true,
                cursor: true,
            } => REQUIRED_FEATURES_V2_4_HISTORY_REPEAT_CURSOR,
            Self::V2_5 {
                history,
                repeat,
                cursor,
                extended_sgr,
            } => {
                REQUIRED_FEATURES_V2_0
                    | WIN32_INPUT_MODE_FEATURE
                    | if history { HISTORY_ANCHORS_FEATURE } else { 0 }
                    | if repeat { REP_SOURCE_FEATURE } else { 0 }
                    | if cursor { CURSOR_STYLE_FEATURE } else { 0 }
                    | if extended_sgr {
                        EXTENDED_SGR_FEATURE
                    } else {
                        0
                    }
            }
            Self::V2_7 {
                history,
                repeat,
                cursor,
                extended_sgr,
                win32_input,
                keyboard_protocol,
            } => {
                REQUIRED_FEATURES_V2_7
                    | if history { HISTORY_ANCHORS_FEATURE } else { 0 }
                    | if repeat { REP_SOURCE_FEATURE } else { 0 }
                    | if cursor { CURSOR_STYLE_FEATURE } else { 0 }
                    | if extended_sgr {
                        EXTENDED_SGR_FEATURE
                    } else {
                        0
                    }
                    | if win32_input {
                        WIN32_INPUT_MODE_FEATURE
                    } else {
                        0
                    }
                    | if keyboard_protocol {
                        KEYBOARD_PROTOCOL_STATE_FEATURE
                    } else {
                        0
                    }
            }
            Self::V2_8 {
                history,
                repeat,
                cursor,
                extended_sgr,
                win32_input,
                keyboard_protocol,
                national_replacement,
            } => {
                REQUIRED_FEATURES_V2_8
                    | if history { HISTORY_ANCHORS_FEATURE } else { 0 }
                    | if repeat { REP_SOURCE_FEATURE } else { 0 }
                    | if cursor { CURSOR_STYLE_FEATURE } else { 0 }
                    | if extended_sgr {
                        EXTENDED_SGR_FEATURE
                    } else {
                        0
                    }
                    | if win32_input {
                        WIN32_INPUT_MODE_FEATURE
                    } else {
                        0
                    }
                    | if keyboard_protocol {
                        KEYBOARD_PROTOCOL_STATE_FEATURE
                    } else {
                        0
                    }
                    | if national_replacement {
                        NATIONAL_REPLACEMENT_CHARSET_FEATURE
                    } else {
                        0
                    }
            }
            Self::V2_9 {
                history,
                repeat,
                cursor,
                extended_sgr,
                win32_input,
                keyboard_protocol,
                national_replacement,
                standalone_c1,
            } => {
                REQUIRED_FEATURES_V2_9
                    | if history { HISTORY_ANCHORS_FEATURE } else { 0 }
                    | if repeat { REP_SOURCE_FEATURE } else { 0 }
                    | if cursor { CURSOR_STYLE_FEATURE } else { 0 }
                    | if extended_sgr {
                        EXTENDED_SGR_FEATURE
                    } else {
                        0
                    }
                    | if win32_input {
                        WIN32_INPUT_MODE_FEATURE
                    } else {
                        0
                    }
                    | if keyboard_protocol {
                        KEYBOARD_PROTOCOL_STATE_FEATURE
                    } else {
                        0
                    }
                    | if national_replacement {
                        NATIONAL_REPLACEMENT_CHARSET_FEATURE
                    } else {
                        0
                    }
                    | if standalone_c1 {
                        STANDALONE_C1_OUTPUT_PROFILE_FEATURE
                    } else {
                        0
                    }
            }
            Self::V2_10 {
                repeat,
                cursor,
                extended_sgr,
                win32_input,
                keyboard_protocol,
                national_replacement,
                standalone_c1,
                modern_private_modes,
            } => {
                REQUIRED_FEATURES_V2_10
                    | if repeat { REP_SOURCE_FEATURE } else { 0 }
                    | if cursor { CURSOR_STYLE_FEATURE } else { 0 }
                    | if extended_sgr {
                        EXTENDED_SGR_FEATURE
                    } else {
                        0
                    }
                    | if win32_input {
                        WIN32_INPUT_MODE_FEATURE
                    } else {
                        0
                    }
                    | if keyboard_protocol {
                        KEYBOARD_PROTOCOL_STATE_FEATURE
                    } else {
                        0
                    }
                    | if national_replacement {
                        NATIONAL_REPLACEMENT_CHARSET_FEATURE
                    } else {
                        0
                    }
                    | if standalone_c1 {
                        STANDALONE_C1_OUTPUT_PROFILE_FEATURE
                    } else {
                        0
                    }
                    | if modern_private_modes {
                        MODERN_PRIVATE_MODES_FEATURE
                    } else {
                        0
                    }
            }
            Self::V2_6 {
                history,
                repeat,
                cursor,
                extended_sgr,
                win32_input,
            } => {
                REQUIRED_FEATURES_V2_6
                    | if history { HISTORY_ANCHORS_FEATURE } else { 0 }
                    | if repeat { REP_SOURCE_FEATURE } else { 0 }
                    | if cursor { CURSOR_STYLE_FEATURE } else { 0 }
                    | if extended_sgr {
                        EXTENDED_SGR_FEATURE
                    } else {
                        0
                    }
                    | if win32_input {
                        WIN32_INPUT_MODE_FEATURE
                    } else {
                        0
                    }
            }
        }
    }

    fn has_line_renditions(self) -> bool {
        self.required_features() & LINE_RENDITION_FEATURE != 0
    }

    fn width_profile(self) -> crate::WidthProfile {
        if self.required_features() & UNICODE16_FEATURE != 0 {
            crate::WidthProfile::Unicode16
        } else {
            crate::WidthProfile::LegacyV1
        }
    }

    fn section_count(self) -> usize {
        self.tags().count()
    }

    fn has_sgr_stack(self) -> bool {
        matches!(self, Self::V2_11 { features } | Self::V2_12 { features } | Self::V2_13 { features } | Self::V2_14 { features } | Self::V2_15 { features } | Self::V2_16 { features } | Self::V2_17 { features }
        | Self::V2_22 { features } | Self::V2_21 { features } | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } if features & SGR_STACK_FEATURE != 0)
    }

    fn has_alternate_scroll(self) -> bool {
        matches!(self, Self::V2_11 { features } | Self::V2_12 { features } | Self::V2_13 { features } | Self::V2_14 { features } | Self::V2_15 { features } | Self::V2_16 { features } | Self::V2_17 { features }
        | Self::V2_22 { features } | Self::V2_21 { features } | Self::V2_20 { features }
        | Self::V2_19 { features }
        | Self::V2_18 { features } if features & ALTERNATE_SCROLL_FEATURE != 0)
    }

    fn tags(self) -> impl Iterator<Item = u32> {
        ALL_TAGS.into_iter().filter(move |&tag| {
            (tag != KEYBOARD_PROTOCOL_STATE || self.has_keyboard_protocol_state())
                && (tag != GRAPHICS_STATE || self.required_features() & GRAPHICS_FEATURE != 0)
                && (tag != HOST_SIGNALS_STATE
                    || self.required_features() & HOST_SIGNALS_FEATURE != 0)
                && (tag != SHELL_INTEGRATION_STATE
                    || self.required_features() & SHELL_INTEGRATION_FEATURE != 0)
                && (tag != HOST_THEME_STATE || self.required_features() & HOST_THEME_FEATURE != 0)
                && (tag != SGR_STACK_STATE || self.has_sgr_stack())
                && (tag != MACROS_STATE || self.required_features() & MACROS_FEATURE != 0)
                && (tag != PAGES_STATE || self.required_features() & PAGES_FEATURE != 0)
                && (tag != HOST_GEOMETRY_STATE
                    || self.required_features() & HOST_GEOMETRY_FEATURE != 0)
        })
    }

    fn section_version(self, tag: u32) -> u16 {
        if self.required_features() & GRAPHICS_FEATURE != 0 {
            match tag {
                GRAPHEME_TABLE => return 2,
                CHARSET_STATE => return 4,
                _ => {}
            }
        }
        if self.required_features() & TRADITIONAL_MODES_FEATURE != 0 && tag == CHARSET_STATE {
            return 3;
        }
        if self.required_features() & TRADITIONAL_MODES_FEATURE != 0 && tag == MODE_BANK {
            return 6;
        }
        if self.has_retained_primary_scrollback() && matches!(tag, PRIMARY_META | PRIMARY_ROWS) {
            3
        } else if self.has_alternate_scroll() && tag == MODE_BANK {
            5
        } else if self.has_modern_private_modes() && tag == MODE_BANK {
            4
        } else if self.has_national_replacement_charset() && tag == MODE_BANK {
            3
        } else if (self.has_national_replacement_charset() && tag == CHARSET_STATE)
            || (self.has_win32_input_mode() && tag == MODE_BANK)
            || (self.has_extended_sgr() && tag == RENDITION_TABLE)
        {
            2
        } else if self.has_cursor_style() && tag == CORE_STATE {
            3
        } else if (self.has_repeat_source() && tag == CORE_STATE)
            || (self.has_history()
                && matches!(
                    tag,
                    PRIMARY_META | PRIMARY_ROWS | ALTERNATE_META | ALTERNATE_ROWS
                ))
        {
            2
        } else {
            1
        }
    }
}

fn charset_has_national_replacement(charset: CharsetState) -> bool {
    [
        GraphicRegister::G0,
        GraphicRegister::G1,
        GraphicRegister::G2,
        GraphicRegister::G3,
    ]
    .into_iter()
    .any(|register| {
        matches!(
            charset.designation(register).charset,
            GraphicCharset::NationalReplacement(_)
        )
    })
}

fn attributes_need_extended_sgr(attributes: Attributes) -> bool {
    attributes.faint
        || attributes.overline
        || !matches!(
            attributes.underline_style,
            UnderlineStyle::None | UnderlineStyle::Single
        )
        || attributes.underline_color != Color::Default
}

fn terminal_has_extended_sgr(terminal: &Terminal) -> bool {
    [
        terminal.main.attributes,
        terminal.main.saved.attributes,
        terminal.alternate.attributes,
        terminal.alternate.saved.attributes,
    ]
    .into_iter()
    .chain(
        terminal
            .pages
            .screens()
            .flat_map(|page| [page.attributes, page.saved.attributes]),
    )
    .chain(terminal.sgr_stack.entries().map(|entry| entry.attributes))
    .chain(
        terminal
            .main
            .cells
            .iter()
            .chain(terminal.alternate.visible_cells())
            .chain(
                terminal
                    .pages
                    .screens()
                    .flat_map(|page| page.visible_cells()),
            )
            .map(|cell| cell.attributes),
    )
    .any(attributes_need_extended_sgr)
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct GraphemeRef<'a> {
    character: char,
    combining: &'a str,
    width: u8,
    glyph: Option<GlyphReference>,
}

impl GraphemeRef<'_> {
    fn compare(self, other: Self) -> Ordering {
        self.character
            .cmp(&other.character)
            .then_with(|| self.combining.as_bytes().cmp(other.combining.as_bytes()))
            .then_with(|| self.width.cmp(&other.width))
            .then_with(|| self.glyph.cmp(&other.glyph))
    }

    fn byte_len(self) -> usize {
        self.character.len_utf8() + self.combining.len()
    }
}

struct GraphemeOwned {
    character: char,
    combining: String,
    width: u8,
    glyph: Option<GlyphReference>,
}

pub(super) fn export(terminal: &Terminal) -> Result<Vec<u8>, TerminalError> {
    terminal.ensure_presentation_eligible()?;
    terminal.validate_graphics_anchors()?;
    if terminal.main.width_profile != terminal.alternate.width_profile {
        return Err(TerminalError::MalformedSnapshot);
    }
    terminal.main.validate()?;
    terminal.alternate.validate()?;
    terminal
        .pages
        .validate(terminal.columns, terminal.rows, terminal.width_profile())?;
    if terminal.main.viewport_top() != 0
        && terminal
            .main
            .row_metadata
            .iter()
            .any(|metadata| metadata.sealed_revision > terminal.next_source_sequence)
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    if !terminal.margins.validate(terminal.columns, terminal.rows) {
        return Err(TerminalError::MalformedSnapshot);
    }
    validate_margin_mode_consistency(terminal.modes, terminal.margins)?;
    validate_active_screen_wrap_state(terminal.active_screen(), terminal.modes, terminal.margins)?;
    if terminal.repeat_source.as_ref().is_some_and(|source| {
        !terminal
            .width_profile()
            .valid_grapheme(source.character, &source.combining, source.width)
    }) {
        return Err(TerminalError::MalformedSnapshot);
    }
    if terminal
        .main
        .cells
        .iter()
        .chain(terminal.alternate.visible_cells())
        .chain(
            terminal
                .pages
                .screens()
                .flat_map(|page| page.visible_cells()),
        )
        .filter_map(|cell| cell.hyperlink)
        .any(|reference| terminal.osc.hyperlink(reference).is_none())
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    encode_terminal(terminal)
}

#[cfg(test)]
pub(super) fn export_unchecked_for_test(terminal: &Terminal) -> Result<Vec<u8>, TerminalError> {
    encode_terminal(terminal)
}

#[cfg(test)]
pub(super) fn section_payload_offset_for_test(bytes: &[u8], index: usize) -> usize {
    let base = HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + 8;
    usize::try_from(u64::from_le_bytes(
        bytes[base..base + 8].try_into().expect("directory offset"),
    ))
    .expect("snapshot offset fits usize")
}

#[cfg(test)]
pub(super) fn refresh_crc_for_test(bytes: &mut [u8]) {
    let payload_crc = crc32c::checksum(&bytes[HEADER_SIZE..]);
    bytes[96..100].copy_from_slice(&payload_crc.to_le_bytes());
    let header_crc = crc32c::checksum(&bytes[..100]);
    bytes[100..104].copy_from_slice(&header_crc.to_le_bytes());
}

fn encode_terminal(terminal: &Terminal) -> Result<Vec<u8>, TerminalError> {
    if terminal.alternate.requested_history_rows() != 0 {
        return Err(TerminalError::MalformedSnapshot);
    }
    let version = SnapshotVersion::for_terminal(terminal);
    if version.required_features() & GRAPHICS_FEATURE == 0
        && [
            terminal.charset,
            terminal.main_saved_charset,
            terminal.alternate_saved_charset,
        ]
        .into_iter()
        .flat_map(|state| {
            [
                GraphicRegister::G0,
                GraphicRegister::G1,
                GraphicRegister::G2,
                GraphicRegister::G3,
            ]
            .map(|register| state.designation(register))
        })
        .chain([terminal.user_preferred_charset])
        .any(|designation| designation.charset == GraphicCharset::Soft)
    {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    let section_count = version.section_count();
    let graphemes = collect_graphemes(terminal, version)?;
    let renditions = collect_renditions(terminal, version)?;
    let osc = terminal
        .osc
        .encode_snapshot_v2()
        .map_err(map_osc_export_error)?;
    let mut writer = Writer::new();
    if version.required_features() & GRAPHICS_FEATURE != 0 {
        writer.limit = crate::MAX_GRAPHICS_SNAPSHOT_BYTES;
    }
    writer.zeros(HEADER_SIZE + section_count * DIRECTORY_ENTRY_SIZE);
    writer.align8();
    let mut descs = [SectionDesc::default(); MAX_KNOWN_SECTIONS];
    let mut index = 0;

    macro_rules! section {
        ($tag:expr, $body:expr) => {{
            writer.align8();
            let offset = writer.len();
            let logical_count: u32 = $body;
            descs[index] = SectionDesc {
                tag: $tag,
                flags: CRITICAL,
                version: version.section_version($tag),
                offset,
                length: writer.len().saturating_sub(offset),
                logical_count,
            };
            index += 1;
        }};
    }

    section!(CORE_STATE, {
        encode_core(&mut writer, terminal, &graphemes)?;
        1
    });
    section!(MODE_BANK, encode_modes(&mut writer, terminal.modes));
    section!(MARGINS, {
        encode_margins(&mut writer, terminal.margins);
        1
    });
    section!(TAB_STOPS, {
        encode_tabs(&mut writer, &terminal.tabs, terminal.columns);
        512
    });
    if version.has_keyboard_protocol_state() {
        section!(
            KEYBOARD_PROTOCOL_STATE,
            encode_keyboard_protocol_state(&mut writer, terminal,)
        );
    }
    if version.has_sgr_stack() {
        section!(SGR_STACK_STATE, {
            let count = terminal.sgr_stack.entries().count() as u32;
            writer.u32(count);
            writer.u32(0);
            for entry in terminal.sgr_stack.entries() {
                writer.u32(entry.mask);
                writer.u32(rendition_id(&renditions, entry.attributes, version));
            }
            count
        });
    }
    if version.required_features() & MACROS_FEATURE != 0 {
        section!(MACROS_STATE, {
            let count = terminal.macros.entries().count() as u32;
            writer.u32(count);
            writer.u32(terminal.macros.used() as u32);
            for (id, bytes) in terminal.macros.entries() {
                writer.u32(id as u32);
                writer.u32(bytes.len() as u32);
                writer.bytes(bytes);
            }
            count
        });
    }
    section!(CHARSET_STATE, {
        encode_charsets(&mut writer, terminal);
        3
    });
    section!(OSC_STATE, {
        writer.bytes(&osc);
        1
    });
    section!(GRAPHEME_TABLE, {
        encode_graphemes(&mut writer, &graphemes, version);
        graphemes.len() as u32
    });
    section!(RENDITION_TABLE, {
        encode_renditions(&mut writer, &renditions, version);
        renditions.len() as u32
    });
    section!(PRIMARY_META, {
        encode_screen_meta(&mut writer, &terminal.main, &renditions, version, true);
        1
    });
    section!(
        PRIMARY_ROWS,
        encode_rows(
            &mut writer,
            &terminal.main,
            &graphemes,
            &renditions,
            version,
            true,
        )
    );
    section!(ALTERNATE_META, {
        encode_screen_meta(
            &mut writer,
            &terminal.alternate,
            &renditions,
            version,
            false,
        );
        1
    });
    section!(
        ALTERNATE_ROWS,
        encode_rows(
            &mut writer,
            &terminal.alternate,
            &graphemes,
            &renditions,
            version,
            false,
        )
    );
    if version.required_features() & PAGES_FEATURE != 0 {
        section!(PAGES_STATE, {
            writer.u8(terminal.pages.active);
            writer.u8(terminal.pages.visible);
            writer.u8(terminal.pages.saved);
            writer.u8(u8::from(terminal.pages.coupled));
            let count = terminal.pages.screens().count() as u32;
            writer.u32(count);
            writer.u64(0);
            for (index, page) in terminal.pages.extra.iter().enumerate() {
                if let Some(page) = page {
                    writer.u32(index as u32 + 2);
                    let size_at = writer.len();
                    writer.u32(0);
                    encode_screen_meta(&mut writer, page, &renditions, version, false);
                    let rows_at = writer.len();
                    encode_rows(&mut writer, page, &graphemes, &renditions, version, false);
                    writer.patch_u32(size_at, (writer.len() - rows_at) as u32);
                }
            }
            count
        });
    }
    if version.required_features() & HOST_GEOMETRY_FEATURE != 0 {
        section!(HOST_GEOMETRY_STATE, {
            writer.bytes(&terminal.host_geometry.encode());
            1
        });
    }
    if version.required_features() & HOST_THEME_FEATURE != 0 {
        section!(HOST_THEME_STATE, {
            writer.bytes(&terminal.osc.encode_theme_state());
            1
        });
    }
    if version.required_features() & SHELL_INTEGRATION_FEATURE != 0 {
        section!(SHELL_INTEGRATION_STATE, {
            writer.bytes(&terminal.shell_integration.encode()?);
            1
        });
    }
    if version.required_features() & HOST_SIGNALS_FEATURE != 0 {
        section!(HOST_SIGNALS_STATE, {
            writer.bytes(&terminal.host_signals.encode());
            1
        });
    }
    if version.required_features() & GRAPHICS_FEATURE != 0 {
        section!(GRAPHICS_STATE, {
            writer.bytes(
                &terminal
                    .graphics
                    .as_ref()
                    .ok_or(TerminalError::MalformedSnapshot)?
                    .encode()?,
            );
            1
        });
    }
    debug_assert_eq!(index, section_count);
    writer.align8();
    writer.check()?;

    for (entry, desc) in descs.iter().take(section_count).enumerate() {
        let base = HEADER_SIZE + entry * DIRECTORY_ENTRY_SIZE;
        writer.patch_u32(base, desc.tag);
        writer.patch_u16(base + 4, desc.flags);
        writer.patch_u16(base + 6, desc.version);
        writer.patch_u64(base + 8, desc.offset as u64);
        writer.patch_u64(base + 16, desc.length as u64);
        writer.patch_u32(base + 24, desc.logical_count);
    }
    writer.patch_bytes(0, MAGIC);
    writer.patch_u16(8, MAJOR);
    writer.patch_u16(10, version.minor());
    writer.patch_u16(12, version.minor());
    writer.patch_u16(14, HEADER_SIZE as u16);
    writer.patch_u16(16, DIRECTORY_ENTRY_SIZE as u16);
    writer.patch_u64(24, writer.len() as u64);
    writer.patch_u64(32, HEADER_SIZE as u64);
    writer.patch_u32(40, section_count as u32);
    writer.patch_u32(44, 1);
    writer.patch_u32(48, terminal.width_profile() as u32);
    writer.patch_u32(52, 1);
    writer.patch_u32(56, 1);
    writer.patch_u32(60, 1);
    let required_features = version.required_features();
    writer.patch_u64(64, required_features as u64);
    writer.patch_u64(72, (required_features >> 64) as u64);
    let payload_crc = crc32c::checksum(&writer.as_slice()[HEADER_SIZE..]);
    writer.patch_u32(96, payload_crc);
    let header_crc = crc32c::checksum(&writer.as_slice()[..100]);
    writer.patch_u32(100, header_crc);
    writer.finish()
}

fn collect_graphemes(
    terminal: &Terminal,
    version: SnapshotVersion,
) -> Result<Vec<GraphemeRef<'_>>, TerminalError> {
    let main_cells = if version.has_retained_primary_scrollback() {
        terminal.main.cells.as_slice()
    } else {
        terminal.main.visible_cells()
    };
    let capacity = main_cells
        .len()
        .checked_add(terminal.alternate.visible_cells().len())
        .and_then(|v| {
            v.checked_add(
                terminal
                    .pages
                    .screens()
                    .map(|s| s.cells.len())
                    .sum::<usize>(),
            )
        })
        .and_then(|v| v.checked_add(1 + usize::from(terminal.repeat_source.is_some())))
        .ok_or(TerminalError::LimitExceeded)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| TerminalError::LimitExceeded)?;
    values.push(GraphemeRef {
        character: ' ',
        combining: "",
        width: 1,
        glyph: None,
    });
    if let Some(source) = &terminal.repeat_source {
        values.push(GraphemeRef {
            character: source.character,
            combining: &source.combining,
            width: source.width,
            glyph: source.glyph,
        });
    }
    for cell in main_cells
        .iter()
        .chain(terminal.alternate.visible_cells())
        .chain(
            terminal
                .pages
                .screens()
                .flat_map(|page| page.visible_cells()),
        )
    {
        if !cell.continuation {
            values.push(GraphemeRef {
                character: cell.character,
                combining: &cell.combining,
                width: cell.width,
                glyph: cell.glyph,
            });
        }
    }
    values.sort_unstable_by(|a, b| a.compare(*b));
    values.dedup();
    if version.required_features() & GRAPHICS_FEATURE == 0
        && values.iter().any(|value| value.glyph.is_some())
    {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    Ok(values)
}

fn collect_renditions(
    terminal: &Terminal,
    version: SnapshotVersion,
) -> Result<Vec<Attributes>, TerminalError> {
    let main_cells = if version.has_retained_primary_scrollback() {
        terminal.main.cells.as_slice()
    } else {
        terminal.main.visible_cells()
    };
    let capacity = main_cells
        .len()
        .checked_add(terminal.alternate.visible_cells().len())
        .and_then(|v| {
            v.checked_add(
                terminal
                    .pages
                    .screens()
                    .map(|s| s.cells.len())
                    .sum::<usize>(),
            )
        })
        .and_then(|v| {
            v.checked_add(
                5 + 2 * terminal.pages.screens().count() + terminal.sgr_stack.entries().count(),
            )
        })
        .ok_or(TerminalError::LimitExceeded)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| TerminalError::LimitExceeded)?;
    values.extend([
        Attributes::default(),
        terminal.main.attributes,
        terminal.main.saved.attributes,
        terminal.alternate.attributes,
        terminal.alternate.saved.attributes,
    ]);
    for cell in main_cells
        .iter()
        .chain(terminal.alternate.visible_cells())
        .chain(
            terminal
                .pages
                .screens()
                .flat_map(|page| page.visible_cells()),
        )
    {
        values.push(cell.attributes);
    }
    values.extend(
        terminal
            .pages
            .screens()
            .flat_map(|page| [page.attributes, page.saved.attributes]),
    );
    values.extend(terminal.sgr_stack.entries().map(|entry| entry.attributes));
    values.sort_unstable_by_key(|a| rendition_bytes(*a, version));
    values.dedup();
    Ok(values)
}

fn encode_core(
    writer: &mut Writer,
    terminal: &Terminal,
    graphemes: &[GraphemeRef<'_>],
) -> Result<(), TerminalError> {
    writer.u32(terminal.columns);
    writer.u32(terminal.rows);
    writer.u64(terminal.next_source_sequence);
    writer.u64(terminal.logical_time_micros);
    writer.u8(u8::from(terminal.active_alternate));
    writer.u8(u8::from(terminal.cursor_visible));
    let mut flags = u16::from(terminal.repeat_source.is_some());
    if !terminal.cursor_blinking {
        flags |= 1 << 1;
    }
    flags |= (terminal.cursor_shape as u16) << 2;
    writer.u16(flags);
    if let Some(source) = &terminal.repeat_source {
        let id = grapheme_ref_id(
            graphemes,
            GraphemeRef {
                character: source.character,
                combining: &source.combining,
                width: source.width,
                glyph: source.glyph,
            },
        );
        if id == u32::MAX {
            return Err(TerminalError::MalformedSnapshot);
        }
        writer.u32(id);
    } else {
        writer.u32(0);
    }
    Ok(())
}

fn mode_parameter_tracking(value: MouseTracking) -> u16 {
    match value {
        MouseTracking::None => 0,
        MouseTracking::X10 => 9,
        MouseTracking::Normal => 1000,
        MouseTracking::ButtonEvent => 1002,
        MouseTracking::AnyEvent => 1003,
    }
}

fn mode_parameter_encoding(value: MouseEncoding) -> u16 {
    match value {
        MouseEncoding::Default => 0,
        MouseEncoding::Utf8 => 1005,
        MouseEncoding::Sgr => 1006,
        MouseEncoding::Urxvt => 1015,
        MouseEncoding::SgrPixels => 1016,
    }
}

fn encode_modes(writer: &mut Writer, modes: ModeState) -> u32 {
    let mut records = [
        (4, 1, modes.vt52),
        (4, 5, modes.send_c1),
        (4, 2, modes.iso2022),
        (4, 3, modes.receive_c1 == 1),
        (4, 4, modes.receive_c1 == 2),
        (0, 4, modes.insert_mode),
        (0, 20, modes.newline_mode),
        (1, 1, modes.cursor_keys_application),
        (1, 5, modes.reverse_video),
        (1, 6, modes.origin_mode),
        (1, 7, modes.auto_wrap),
        (1, 42, modes.national_replacement_character_set_mode),
        (1, 66, modes.keypad_application),
        (1, 69, modes.left_right_margin),
        (1, 1004, modes.focus_reporting),
        (1, 1007, modes.alternate_scroll),
        (1, 2004, modes.bracketed_paste),
        (1, 2027, modes.grapheme_clustering),
        (1, 2031, modes.color_scheme_reporting),
        (1, 9001, modes.win32_input_mode),
        (
            1,
            mode_parameter_tracking(modes.mouse_tracking),
            modes.mouse_tracking != MouseTracking::None,
        ),
        (
            1,
            mode_parameter_encoding(modes.mouse_encoding),
            modes.mouse_encoding != MouseEncoding::Default,
        ),
        (
            2,
            match modes.attribute_change_extent {
                AttributeChangeExtent::Stream => 1,
                AttributeChangeExtent::Rectangle => 2,
            },
            true,
        ),
        (
            3,
            modes.modify_other_keys.parameter(),
            modes.modify_other_keys != ModifyOtherKeys::Disabled,
        ),
    ];
    records.sort_unstable();
    let keep = |namespace: u8, mode: u16, value: bool| {
        mode != 0 && (value || (namespace == 1 && mode == 7))
    };
    let count = records.iter().filter(|&&(n, m, v)| keep(n, m, v)).count() as u32;
    writer.u32(count);
    writer.u32(0);
    for (namespace, mode, value) in records {
        if keep(namespace, mode, value) {
            writer.u16(mode);
            writer.u8(namespace);
            writer.u8(u8::from(value));
            writer.u32(0);
        }
    }
    count
}

fn encode_margins(writer: &mut Writer, margins: Margins) {
    writer.u32(margins.top);
    writer.u32(margins.bottom);
    writer.u32(margins.left);
    writer.u32(margins.right);
    writer.u32(
        u32::from(margins.vertical_follows_screen)
            | (u32::from(margins.horizontal_follows_screen) << 1),
    );
    writer.u32(0);
}

fn encode_tabs(writer: &mut Writer, tabs: &TabStops, columns: u32) {
    writer.u32(columns);
    writer.u32(8);
    for word in tabs.words() {
        writer.u64(*word);
    }
}

fn encode_keyboard_protocol_state(writer: &mut Writer, terminal: &Terminal) -> u32 {
    let main = terminal.main_keyboard_protocol.as_slice();
    let alternate = terminal.alternate_keyboard_protocol.as_slice();
    writer.u8(u8::try_from(main.len()).expect("bounded Kitty main stack length fits u8"));
    writer.u8(u8::try_from(alternate.len()).expect("bounded Kitty alternate stack length fits u8"));
    writer.u16(0);
    writer.u32(0);
    writer.bytes(main);
    writer.bytes(alternate);
    u32::try_from(main.len() + alternate.len()).expect("bounded Kitty entry count fits u32")
}

fn encode_charsets(writer: &mut Writer, terminal: &Terminal) {
    let version = SnapshotVersion::for_terminal(terminal);
    let graphics = version.required_features() & GRAPHICS_FEATURE != 0;
    writer.u32(3);
    writer.u32(if graphics { 24 } else { 12 });
    encode_charset(writer, terminal.charset, graphics);
    encode_charset(writer, terminal.main_saved_charset, graphics);
    encode_charset(writer, terminal.alternate_saved_charset, graphics);
    if version.required_features() & TRADITIONAL_MODES_FEATURE != 0 {
        if graphics {
            encode_raw_designation(writer, terminal.user_preferred_charset);
        } else {
            writer.u8(charset_wire_id(terminal.user_preferred_charset));
            writer.u8(0);
            writer.u16(0);
        }
        writer.u32(0);
    }
}

fn encode_raw_designation(writer: &mut Writer, designation: CharsetDesignation) {
    writer.u8(match designation.size {
        CharsetSize::Characters94 => 94,
        CharsetSize::Characters96 => 96,
    });
    let bytes = designation.designator();
    writer.bytes(bytes);
    writer.zeros(3 - bytes.len());
}

fn charset_wire_id(designation: CharsetDesignation) -> u8 {
    match designation.charset {
        GraphicCharset::Soft => unreachable!("soft designations require graphics charset records"),
        GraphicCharset::Extended(set) => 15 + set.id(),
        GraphicCharset::UserPreferred => {
            if designation.size == CharsetSize::Characters94 {
                31
            } else {
                32
            }
        }
        GraphicCharset::Ascii => 0,
        GraphicCharset::DecSpecialGraphics => 1,
        GraphicCharset::IsoLatin1Supplemental => 2,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::British) => 3,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::Dutch) => 4,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::Finnish) => 5,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::French) => 6,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::FrenchCanadian) => 7,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::German) => 8,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::Italian) => 9,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::NorwegianDanish) => 10,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::Portuguese) => 11,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::Spanish) => 12,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::Swedish) => 13,
        GraphicCharset::NationalReplacement(NationalReplacementCharset::Swiss) => 14,
    }
}

fn encode_charset(writer: &mut Writer, charset: CharsetState, graphics: bool) {
    for register in [
        GraphicRegister::G0,
        GraphicRegister::G1,
        GraphicRegister::G2,
        GraphicRegister::G3,
    ] {
        if graphics {
            encode_raw_designation(writer, charset.designation(register));
        } else {
            writer.u8(charset_wire_id(charset.designation(register)));
        }
    }
    writer.u8(match charset.gl() {
        GraphicRegister::G0 => 0,
        GraphicRegister::G1 => 1,
        GraphicRegister::G2 => 2,
        GraphicRegister::G3 => 3,
    });
    writer.u8(match charset.gr() {
        GraphicRegister::G0 => 0,
        GraphicRegister::G1 => 1,
        GraphicRegister::G2 => 2,
        GraphicRegister::G3 => 3,
    });
    writer.u8(match charset.pending_single_shift() {
        None => 0,
        Some(GraphicRegister::G2) => 2,
        Some(GraphicRegister::G3) => 3,
        Some(GraphicRegister::G0 | GraphicRegister::G1) => unreachable!(),
    });
    writer.u8(0);
    writer.u32(0);
}

fn encode_graphemes(writer: &mut Writer, values: &[GraphemeRef<'_>], version: SnapshotVersion) {
    writer.u32(values.len() as u32);
    writer.u32(0);
    for value in values {
        writer.u16(value.byte_len() as u16);
        writer.u8(value.width);
        writer.u8(u8::from(value.glyph.is_some()));
        if let Some(glyph) = value.glyph {
            debug_assert!(version.required_features() & GRAPHICS_FEATURE != 0);
            writer.u64(glyph.resource);
            writer.u32(glyph.x);
            writer.u32(glyph.width);
            writer.u32(glyph.height);
        }
        let mut scalar = [0; 4];
        writer.bytes(value.character.encode_utf8(&mut scalar).as_bytes());
        writer.bytes(value.combining.as_bytes());
        writer.align4();
    }
}

fn encode_renditions(writer: &mut Writer, values: &[Attributes], version: SnapshotVersion) {
    writer.u32(values.len() as u32);
    writer.u32(48);
    for value in values {
        writer.bytes(&rendition_bytes(*value, version));
    }
}

fn rendition_bytes(attributes: Attributes, version: SnapshotVersion) -> [u8; 48] {
    let mut bytes = [0; 48];
    let flags = u32::from(attributes.bold)
        | (u32::from(attributes.blink) << 1)
        | (u32::from(attributes.italic) << 2)
        | (u32::from(attributes.inverse) << 3)
        | (u32::from(attributes.conceal) << 4)
        | (u32::from(attributes.strikethrough) << 5)
        | (u32::from(attributes.faint && version.has_extended_sgr()) << 6)
        | (u32::from(attributes.overline && version.has_extended_sgr()) << 7);
    bytes[..4].copy_from_slice(&flags.to_le_bytes());
    bytes[4] = if version.has_extended_sgr() {
        underline_style_byte(attributes.underline_style)
    } else {
        u8::from(attributes.underline_style == UnderlineStyle::Single)
    };
    encode_color_spec(&mut bytes[8..16], attributes.foreground);
    encode_color_spec(&mut bytes[16..24], attributes.background);
    if version.has_extended_sgr() {
        encode_color_spec(&mut bytes[24..32], attributes.underline_color);
    }
    bytes
}

fn underline_style_byte(style: UnderlineStyle) -> u8 {
    match style {
        UnderlineStyle::None => 0,
        UnderlineStyle::Single => 1,
        UnderlineStyle::Double => 2,
        UnderlineStyle::Curly => 3,
        UnderlineStyle::Dotted => 4,
        UnderlineStyle::Dashed => 5,
    }
}

fn encode_color_spec(bytes: &mut [u8], color: Color) {
    let (kind, value) = match color {
        Color::Default => (0, 0),
        Color::Indexed(i) => (1, u32::from(i)),
        Color::Rgb(r, g, b) => (2, (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)),
    };
    bytes[0] = kind;
    bytes[4..8].copy_from_slice(&value.to_le_bytes());
}

fn rendition_id(values: &[Attributes], value: Attributes, version: SnapshotVersion) -> u32 {
    let key = rendition_bytes(value, version);
    values
        .binary_search_by_key(&key, |a| rendition_bytes(*a, version))
        .map_or(u32::MAX, |i| i as u32)
}

fn grapheme_id(values: &[GraphemeRef<'_>], cell: &Cell) -> u32 {
    grapheme_ref_id(
        values,
        GraphemeRef {
            character: cell.character,
            combining: &cell.combining,
            width: cell.width,
            glyph: cell.glyph,
        },
    )
}

fn grapheme_ref_id(values: &[GraphemeRef<'_>], key: GraphemeRef<'_>) -> u32 {
    values
        .binary_search_by(|v| v.compare(key))
        .map_or(u32::MAX, |i| i as u32)
}

fn encode_screen_meta(
    writer: &mut Writer,
    screen: &Screen,
    renditions: &[Attributes],
    version: SnapshotVersion,
    primary: bool,
) {
    let retained_primary = primary && version.has_retained_primary_scrollback();
    writer.u32(screen.columns);
    writer.u32(screen.rows);
    writer.u32(if retained_primary {
        screen.stored_rows()
    } else {
        screen.rows
    });
    writer.u32(if retained_primary {
        screen.viewport_top()
    } else {
        0
    });
    writer.u32(screen.cursor_row);
    writer.u32(screen.cursor_column);
    writer.u32(u32::from(screen.wrap_pending) | (u32::from(screen.protected) << 1));
    writer.u32(rendition_id(renditions, screen.attributes, version));
    writer.u32(screen.saved.row);
    writer.u32(screen.saved.column);
    writer.u32(
        u32::from(screen.saved.wrap_pending)
            | (u32::from(screen.saved.origin_mode) << 1)
            | (u32::from(screen.saved.auto_wrap) << 2)
            | (u32::from(screen.saved.protected) << 3),
    );
    writer.u32(rendition_id(renditions, screen.saved.attributes, version));
    if version.has_history() {
        writer.u64(screen.next_logical_line_id);
        writer.u64(if primary {
            u64::from(screen.requested_history_rows().min(MAX_HISTORY_ROWS))
        } else {
            0
        });
    } else {
        writer.u64(0);
        writer.u64(0);
    }
}

fn encode_rows(
    writer: &mut Writer,
    screen: &Screen,
    graphemes: &[GraphemeRef<'_>],
    renditions: &[Attributes],
    version: SnapshotVersion,
    primary: bool,
) -> u32 {
    let width = screen.columns as usize;
    let retained_primary = primary && version.has_retained_primary_scrollback();
    let (available, stored_start, cells) = if retained_primary {
        (screen.stored_rows() as usize, 0, screen.cells.as_slice())
    } else if version.has_history() {
        (
            screen.rows as usize,
            screen.viewport_top() as usize,
            screen.visible_cells(),
        )
    } else {
        (
            (screen.cells.len() / width)
                .min(screen.soft_wrapped.len())
                .min(screen.meaningful_end.len()),
            0,
            screen.cells.as_slice(),
        )
    };
    writer.u32(available as u32);
    writer.u32(0);
    if retained_primary {
        writer.u64(screen.scrollback_generation_rows());
    }
    for row in 0..available {
        let stored_row = stored_start + row;
        let metadata = if version.has_history() {
            screen.row_metadata[stored_row]
        } else {
            RowMetadata::default()
        };
        let head_truncated = version.has_history()
            && row == 0
            && (metadata.is_head_truncated()
                || screen.row_metadata[..stored_start]
                    .iter()
                    .any(|candidate| candidate.logical_line_id == metadata.logical_line_id));
        writer.u64(if version.has_history() {
            metadata.logical_line_id
        } else {
            0
        });
        writer.u32(if version.has_history() {
            metadata.logical_cell_offset
        } else {
            0
        });
        writer.u32(screen.meaningful_end[stored_row]);
        writer.u32(
            u32::from(screen.soft_wrapped[stored_row])
                | ((screen.line_renditions[stored_row] as u32) << 2)
                | if version.has_history() {
                    u32::from(head_truncated) << 1
                } else {
                    0
                },
        );
        writer.u32(count_runs(cells, width, row) as u32);
        if retained_primary {
            writer.u64(metadata.sealed_revision);
        }
        let mut column = 0;
        while column < width {
            let cell = &cells[row * width + column];
            if cell.continuation {
                column += 1;
                continue;
            }
            let gid = grapheme_id(graphemes, cell);
            let rid = rendition_id(renditions, cell.attributes, version);
            let metadata = cell_metadata(cell);
            let step = usize::from(cell.width.max(1));
            let mut repeat = 1;
            let mut next = column + step;
            while next < width {
                let candidate = &cells[row * width + next];
                if candidate.continuation
                    || grapheme_id(graphemes, candidate) != gid
                    || rendition_id(renditions, candidate.attributes, version) != rid
                    || cell_metadata(candidate) != metadata
                {
                    break;
                }
                repeat += 1;
                next += step;
            }
            writer.u32(gid);
            writer.u32(rid);
            writer.u32(repeat);
            writer.u32(metadata);
            column = next;
        }
    }
    available as u32
}

fn count_runs(cells: &[Cell], width: usize, row: usize) -> usize {
    let mut count = 0;
    let mut column = 0;
    while column < width {
        let cell = &cells[row * width + column];
        if cell.continuation {
            column += 1;
            continue;
        }
        count += 1;
        let step = usize::from(cell.width.max(1));
        let mut next = column + step;
        while next < width {
            let candidate = &cells[row * width + next];
            if candidate.continuation
                || candidate.character != cell.character
                || candidate.combining != cell.combining
                || candidate.width != cell.width
                || candidate.glyph != cell.glyph
                || candidate.attributes != cell.attributes
                || candidate.protected != cell.protected
                || candidate.hyperlink != cell.hyperlink
            {
                break;
            }
            next += step;
        }
        column = next;
    }
    count
}

fn cell_metadata(cell: &Cell) -> u32 {
    u32::from(cell.protected) | (cell.hyperlink.map_or(0, HyperlinkRef::get) << 1)
}

fn map_osc_export_error(error: OscCodecError) -> TerminalError {
    match error {
        OscCodecError::Malformed => TerminalError::MalformedSnapshot,
        OscCodecError::LimitExceeded | OscCodecError::AllocationFailed => {
            TerminalError::LimitExceeded
        }
    }
}

fn map_osc_import_error(error: OscCodecError) -> TerminalError {
    match error {
        OscCodecError::AllocationFailed => TerminalError::LimitExceeded,
        OscCodecError::Malformed | OscCodecError::LimitExceeded => TerminalError::MalformedSnapshot,
    }
}

pub(super) fn import(bytes: &[u8]) -> Result<Terminal, TerminalError> {
    let preflight = preflight_a(bytes)?;
    preflight_b(bytes, &preflight)?;
    decode_terminal(bytes, &preflight)
}

/// Borrowed canonical sections for the additive state-delta codec. The same
/// preflights as import run before exposing any slice; pixels remain borrowed.
pub(crate) fn delta_view(bytes: &[u8]) -> Result<DeltaSnapshotView<'_>, TerminalError> {
    let preflight = preflight_a(bytes)?;
    preflight_b(bytes, &preflight)?;
    if !matches!(preflight.version, SnapshotVersion::V2_22 { .. }) {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    let mut sections = Vec::new();
    sections
        .try_reserve_exact(MAX_KNOWN_SECTIONS)
        .map_err(|_| TerminalError::LimitExceeded)?;
    for desc in preflight.descs.iter().flatten() {
        sections.push((desc.tag, &bytes[desc.offset..desc.offset + desc.length]));
    }
    let sequence = scan_core(section(bytes, &preflight, CORE_STATE)?, preflight.version)?.sequence;
    Ok(DeltaSnapshotView {
        envelope: &bytes[..HEADER_SIZE + sections.len() * DIRECTORY_ENTRY_SIZE],
        sections,
        sequence,
        width_profile: preflight.version.width_profile(),
        output_profile: if preflight.version.has_standalone_c1_output_profile() {
            OutputProfile::Utf8WithStandaloneC1
        } else {
            OutputProfile::Utf8
        },
    })
}

pub(crate) struct DeltaSnapshotView<'a> {
    pub envelope: &'a [u8],
    pub sections: Vec<(u32, &'a [u8])>,
    pub sequence: u64,
    pub width_profile: crate::WidthProfile,
    pub output_profile: OutputProfile,
}

struct Preflight {
    descs: [Option<SectionDesc>; MAX_SECTIONS],
    version: SnapshotVersion,
}

#[derive(Clone, Copy)]
struct CoreState {
    columns: u32,
    rows: u32,
    sequence: u64,
    time: u64,
    active_alternate: bool,
    cursor_visible: bool,
    cursor_shape: CursorShape,
    cursor_blinking: bool,
    repeat_grapheme_id: Option<u32>,
}

struct GraphemeWidths {
    wide: [u64; GRAPHEME_WIDTH_WORDS],
}

struct GraphemeUses {
    used: [u64; GRAPHEME_WIDTH_WORDS],
}

struct RenditionUses {
    used: [u64; RENDITION_USE_WORDS],
}

struct RowScanTables<'a> {
    grapheme_count: usize,
    rendition_count: usize,
    hyperlink_count: usize,
    widths: &'a GraphemeWidths,
    grapheme_uses: &'a mut GraphemeUses,
    rendition_uses: &'a mut RenditionUses,
}

impl GraphemeUses {
    fn new() -> Self {
        Self {
            used: [0; GRAPHEME_WIDTH_WORDS],
        }
    }

    fn set(&mut self, index: usize) {
        self.used[index / u64::BITS as usize] |= 1 << (index % u64::BITS as usize);
    }

    fn get(&self, index: usize) -> bool {
        self.used[index / u64::BITS as usize] & (1 << (index % u64::BITS as usize)) != 0
    }
}

impl RenditionUses {
    fn new() -> Self {
        Self {
            used: [0; RENDITION_USE_WORDS],
        }
    }

    fn set(&mut self, index: usize) {
        self.used[index / u64::BITS as usize] |= 1 << (index % u64::BITS as usize);
    }

    fn get(&self, index: usize) -> bool {
        self.used[index / u64::BITS as usize] & (1 << (index % u64::BITS as usize)) != 0
    }
}

impl GraphemeWidths {
    fn new() -> Self {
        Self {
            wide: [0; GRAPHEME_WIDTH_WORDS],
        }
    }

    fn set(&mut self, index: usize, width: u8) {
        if width == 2 {
            self.wide[index / u64::BITS as usize] |= 1 << (index % u64::BITS as usize);
        }
    }

    fn get(&self, index: usize) -> u64 {
        1 + ((self.wide[index / u64::BITS as usize] >> (index % u64::BITS as usize)) & 1)
    }
}

fn preflight_a(bytes: &[u8]) -> Result<Preflight, TerminalError> {
    let graphics_version = bytes.get(10..12) == Some(&MINOR_V2_22.to_le_bytes());
    if bytes.len()
        > if graphics_version {
            crate::MAX_GRAPHICS_SNAPSHOT_BYTES
        } else {
            MAX_SNAPSHOT_BYTES
        }
    {
        return Err(TerminalError::LimitExceeded);
    }
    if bytes.len() < HEADER_SIZE {
        return Err(TerminalError::MalformedSnapshot);
    }
    if &bytes[..8] != MAGIC {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    let mut header = Reader::new(bytes);
    header.take(8)?;
    let major = header.u16()?;
    let minor = header.u16()?;
    let min_reader_minor = header.u16()?;
    if major != MAJOR {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    if minor != min_reader_minor
        || !matches!(
            minor,
            MINOR_V2_0
                | MINOR_V2_1
                | MINOR_V2_2
                | MINOR_V2_3
                | MINOR_V2_4
                | MINOR_V2_5
                | MINOR_V2_6
                | MINOR_V2_7
                | MINOR_V2_8
                | MINOR_V2_9
                | MINOR_V2_10
                | MINOR_V2_11
                | MINOR_V2_12
                | MINOR_V2_13
                | MINOR_V2_14
                | MINOR_V2_15
                | MINOR_V2_16
                | MINOR_V2_17
                | MINOR_V2_18
                | MINOR_V2_19
                | MINOR_V2_20
                | MINOR_V2_21
                | MINOR_V2_22
        )
    {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    if usize::from(header.u16()?) != HEADER_SIZE
        || usize::from(header.u16()?) != DIRECTORY_ENTRY_SIZE
        || header.u16()? != 0
        || header.u32()? != 0
    {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    let total = usize::try_from(header.u64()?).map_err(|_| TerminalError::MalformedSnapshot)?;
    if total != bytes.len() || header.u64()? != HEADER_SIZE as u64 {
        return Err(TerminalError::MalformedSnapshot);
    }
    let count = header.u32()? as usize;
    if !(SECTION_COUNT..=MAX_KNOWN_SECTIONS).contains(&count) || count > MAX_SECTIONS {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    if header.u32()? != 1 {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    let declared_width = header.u32()?;
    if !matches!(declared_width, 1 | 2)
        || header.u32()? != 1
        || header.u32()? != 1
        || header.u32()? != 1
    {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    let features = u128::from(header.u64()?) | (u128::from(header.u64()?) << 64);
    if declared_width
        != if features & UNICODE16_FEATURE != 0 {
            2
        } else {
            1
        }
    {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    let version = match (minor, features) {
        (MINOR_V2_22, features)
            if features
                & !(OPTIONAL_FEATURES_V2_11
                    | NEW_FEATURES_V2_11
                    | UNICODE16_FEATURE
                    | LINE_RENDITION_FEATURE
                    | COLOR_EXTENSIONS_FEATURE
                    | TRADITIONAL_MODES_FEATURE
                    | MACROS_FEATURE
                    | PAGES_FEATURE
                    | HOST_GEOMETRY_FEATURE
                    | HOST_THEME_FEATURE
                    | SHELL_INTEGRATION_FEATURE
                    | HOST_SIGNALS_FEATURE
                    | GRAPHICS_FEATURE)
                == REQUIRED_FEATURES_V2_0
                && features & GRAPHICS_FEATURE != 0
                && features & HISTORY_ANCHORS_FEATURE != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_22 { features }
        }
        (MINOR_V2_21, features)
            if features
                & !(OPTIONAL_FEATURES_V2_11
                    | NEW_FEATURES_V2_11
                    | UNICODE16_FEATURE
                    | LINE_RENDITION_FEATURE
                    | COLOR_EXTENSIONS_FEATURE
                    | TRADITIONAL_MODES_FEATURE
                    | MACROS_FEATURE
                    | PAGES_FEATURE
                    | HOST_GEOMETRY_FEATURE
                    | HOST_THEME_FEATURE
                    | SHELL_INTEGRATION_FEATURE
                    | HOST_SIGNALS_FEATURE)
                == REQUIRED_FEATURES_V2_0
                && features & HOST_SIGNALS_FEATURE != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_21 { features }
        }
        (MINOR_V2_20, features)
            if features
                & !(OPTIONAL_FEATURES_V2_11
                    | NEW_FEATURES_V2_11
                    | UNICODE16_FEATURE
                    | LINE_RENDITION_FEATURE
                    | COLOR_EXTENSIONS_FEATURE
                    | TRADITIONAL_MODES_FEATURE
                    | MACROS_FEATURE
                    | PAGES_FEATURE
                    | HOST_GEOMETRY_FEATURE
                    | HOST_THEME_FEATURE
                    | SHELL_INTEGRATION_FEATURE)
                == REQUIRED_FEATURES_V2_0
                && features & SHELL_INTEGRATION_FEATURE != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_20 { features }
        }
        (MINOR_V2_19, features)
            if features
                & !(OPTIONAL_FEATURES_V2_11
                    | NEW_FEATURES_V2_11
                    | UNICODE16_FEATURE
                    | LINE_RENDITION_FEATURE
                    | COLOR_EXTENSIONS_FEATURE
                    | TRADITIONAL_MODES_FEATURE
                    | MACROS_FEATURE
                    | PAGES_FEATURE
                    | HOST_GEOMETRY_FEATURE
                    | HOST_THEME_FEATURE)
                == REQUIRED_FEATURES_V2_0
                && features & HOST_THEME_FEATURE != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_19 { features }
        }
        (MINOR_V2_18, features)
            if features
                & !(OPTIONAL_FEATURES_V2_11
                    | NEW_FEATURES_V2_11
                    | UNICODE16_FEATURE
                    | LINE_RENDITION_FEATURE
                    | COLOR_EXTENSIONS_FEATURE
                    | TRADITIONAL_MODES_FEATURE
                    | MACROS_FEATURE
                    | PAGES_FEATURE
                    | HOST_GEOMETRY_FEATURE)
                == REQUIRED_FEATURES_V2_0
                && features & HOST_GEOMETRY_FEATURE != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_18 { features }
        }
        (MINOR_V2_17, features)
            if features
                & !(OPTIONAL_FEATURES_V2_11
                    | NEW_FEATURES_V2_11
                    | UNICODE16_FEATURE
                    | LINE_RENDITION_FEATURE
                    | COLOR_EXTENSIONS_FEATURE
                    | TRADITIONAL_MODES_FEATURE
                    | MACROS_FEATURE
                    | PAGES_FEATURE)
                == REQUIRED_FEATURES_V2_0
                && features & PAGES_FEATURE != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_17 { features }
        }
        (MINOR_V2_16, features)
            if features
                & !(OPTIONAL_FEATURES_V2_11
                    | NEW_FEATURES_V2_11
                    | UNICODE16_FEATURE
                    | LINE_RENDITION_FEATURE
                    | COLOR_EXTENSIONS_FEATURE
                    | TRADITIONAL_MODES_FEATURE
                    | MACROS_FEATURE)
                == REQUIRED_FEATURES_V2_0
                && features & MACROS_FEATURE != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_16 { features }
        }
        (MINOR_V2_15, features)
            if features
                & !(OPTIONAL_FEATURES_V2_11
                    | NEW_FEATURES_V2_11
                    | UNICODE16_FEATURE
                    | LINE_RENDITION_FEATURE
                    | COLOR_EXTENSIONS_FEATURE
                    | TRADITIONAL_MODES_FEATURE)
                == REQUIRED_FEATURES_V2_0
                && features & TRADITIONAL_MODES_FEATURE != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_15 { features }
        }
        (MINOR_V2_14, features)
            if features
                & !(OPTIONAL_FEATURES_V2_11
                    | NEW_FEATURES_V2_11
                    | UNICODE16_FEATURE
                    | LINE_RENDITION_FEATURE
                    | COLOR_EXTENSIONS_FEATURE)
                == REQUIRED_FEATURES_V2_0
                && features & COLOR_EXTENSIONS_FEATURE != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_14 { features }
        }
        (MINOR_V2_13, features)
            if features
                & !(OPTIONAL_FEATURES_V2_11
                    | NEW_FEATURES_V2_11
                    | UNICODE16_FEATURE
                    | LINE_RENDITION_FEATURE)
                == REQUIRED_FEATURES_V2_0
                && features & LINE_RENDITION_FEATURE != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_13 { features }
        }
        (MINOR_V2_12, features)
            if features & !(OPTIONAL_FEATURES_V2_11 | NEW_FEATURES_V2_11 | UNICODE16_FEATURE)
                == REQUIRED_FEATURES_V2_0
                && features & UNICODE16_FEATURE != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_12 { features }
        }
        (MINOR_V2_11, features)
            if features & !(OPTIONAL_FEATURES_V2_11 | NEW_FEATURES_V2_11)
                == REQUIRED_FEATURES_V2_0
                && features & NEW_FEATURES_V2_11 != 0
                && (features & RETAINED_PRIMARY_SCROLLBACK_FEATURE == 0
                    || features & HISTORY_ANCHORS_FEATURE != 0) =>
        {
            SnapshotVersion::V2_11 { features }
        }
        (MINOR_V2_0, REQUIRED_FEATURES_V2_0) => SnapshotVersion::V2_0,
        (MINOR_V2_1, REQUIRED_FEATURES_V2_1) => SnapshotVersion::V2_1,
        (MINOR_V2_2, REQUIRED_FEATURES_V2_2) => SnapshotVersion::V2_2 { history: false },
        (MINOR_V2_2, REQUIRED_FEATURES_V2_2_HISTORY) => SnapshotVersion::V2_2 { history: true },
        (MINOR_V2_3, REQUIRED_FEATURES_V2_3) => SnapshotVersion::V2_3 {
            history: false,
            repeat: false,
        },
        (MINOR_V2_3, REQUIRED_FEATURES_V2_3_HISTORY) => SnapshotVersion::V2_3 {
            history: true,
            repeat: false,
        },
        (MINOR_V2_3, REQUIRED_FEATURES_V2_3_REPEAT) => SnapshotVersion::V2_3 {
            history: false,
            repeat: true,
        },
        (MINOR_V2_3, REQUIRED_FEATURES_V2_3_HISTORY_REPEAT) => SnapshotVersion::V2_3 {
            history: true,
            repeat: true,
        },
        (MINOR_V2_4, REQUIRED_FEATURES_V2_4) => SnapshotVersion::V2_4 {
            history: false,
            repeat: false,
            cursor: false,
        },
        (MINOR_V2_4, REQUIRED_FEATURES_V2_4_HISTORY) => SnapshotVersion::V2_4 {
            history: true,
            repeat: false,
            cursor: false,
        },
        (MINOR_V2_4, REQUIRED_FEATURES_V2_4_REPEAT) => SnapshotVersion::V2_4 {
            history: false,
            repeat: true,
            cursor: false,
        },
        (MINOR_V2_4, REQUIRED_FEATURES_V2_4_HISTORY_REPEAT) => SnapshotVersion::V2_4 {
            history: true,
            repeat: true,
            cursor: false,
        },
        (MINOR_V2_4, REQUIRED_FEATURES_V2_4_CURSOR) => SnapshotVersion::V2_4 {
            history: false,
            repeat: false,
            cursor: true,
        },
        (MINOR_V2_4, REQUIRED_FEATURES_V2_4_HISTORY_CURSOR) => SnapshotVersion::V2_4 {
            history: true,
            repeat: false,
            cursor: true,
        },
        (MINOR_V2_4, REQUIRED_FEATURES_V2_4_REPEAT_CURSOR) => SnapshotVersion::V2_4 {
            history: false,
            repeat: true,
            cursor: true,
        },
        (MINOR_V2_4, REQUIRED_FEATURES_V2_4_HISTORY_REPEAT_CURSOR) => SnapshotVersion::V2_4 {
            history: true,
            repeat: true,
            cursor: true,
        },
        (MINOR_V2_5, features) => SnapshotVersion::v2_5_from_features(features)
            .ok_or(TerminalError::IncompatibleSnapshot)?,
        (MINOR_V2_6, features) => SnapshotVersion::v2_6_from_features(features)
            .ok_or(TerminalError::IncompatibleSnapshot)?,
        (MINOR_V2_7, features) => SnapshotVersion::v2_7_from_features(features)
            .ok_or(TerminalError::IncompatibleSnapshot)?,
        (MINOR_V2_8, features) => SnapshotVersion::v2_8_from_features(features)
            .ok_or(TerminalError::IncompatibleSnapshot)?,
        (MINOR_V2_9, features) => SnapshotVersion::v2_9_from_features(features)
            .ok_or(TerminalError::IncompatibleSnapshot)?,
        (MINOR_V2_10, features) => SnapshotVersion::v2_10_from_features(features)
            .ok_or(TerminalError::IncompatibleSnapshot)?,
        _ => return Err(TerminalError::IncompatibleSnapshot),
    };
    if count != version.section_count() {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    if header.take(16)? != [0; 16] {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    let payload_crc = header.u32()?;
    let header_crc = header.u32()?;
    if crc32c::checksum(&bytes[..100]) != header_crc
        || crc32c::checksum(&bytes[HEADER_SIZE..]) != payload_crc
    {
        return Err(TerminalError::MalformedSnapshot);
    }

    let directory_end = HEADER_SIZE
        .checked_add(
            count
                .checked_mul(DIRECTORY_ENTRY_SIZE)
                .ok_or(TerminalError::MalformedSnapshot)?,
        )
        .ok_or(TerminalError::MalformedSnapshot)?;
    let mut expected_offset = align8(directory_end).ok_or(TerminalError::MalformedSnapshot)?;
    if expected_offset > bytes.len() {
        return Err(TerminalError::MalformedSnapshot);
    }
    if bytes[directory_end..expected_offset]
        .iter()
        .any(|b| *b != 0)
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut descs = [None; MAX_SECTIONS];
    let mut previous = 0;
    for (index, expected_tag) in version.tags().enumerate() {
        let base = HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE;
        let mut reader = Reader::new(&bytes[base..base + DIRECTORY_ENTRY_SIZE]);
        let tag = reader.u32()?;
        let flags = reader.u16()?;
        let section_version = reader.u16()?;
        let offset =
            usize::try_from(reader.u64()?).map_err(|_| TerminalError::MalformedSnapshot)?;
        let length =
            usize::try_from(reader.u64()?).map_err(|_| TerminalError::MalformedSnapshot)?;
        let logical_count = reader.u32()?;
        require_zero(reader.u32()?)?;
        if tag != expected_tag
            || (index != 0 && tag <= previous)
            || flags != CRITICAL
            || section_version != version.section_version(tag)
        {
            return Err(TerminalError::IncompatibleSnapshot);
        }
        if length == 0 || offset != expected_offset {
            return Err(TerminalError::MalformedSnapshot);
        }
        let fixed_count = match tag {
            CORE_STATE | MARGINS | OSC_STATE | PRIMARY_META | ALTERNATE_META => Some(1),
            TAB_STOPS => Some(512),
            CHARSET_STATE => Some(3),
            _ => None,
        };
        if fixed_count.is_some_and(|expected| logical_count != expected) {
            return Err(TerminalError::MalformedSnapshot);
        }
        let end = offset
            .checked_add(length)
            .ok_or(TerminalError::MalformedSnapshot)?;
        let padded = align8(end).ok_or(TerminalError::MalformedSnapshot)?;
        if padded > bytes.len() || bytes[end..padded].iter().any(|b| *b != 0) {
            return Err(TerminalError::MalformedSnapshot);
        }
        descs[index] = Some(SectionDesc {
            tag,
            flags,
            version: section_version,
            offset,
            length,
            logical_count,
        });
        expected_offset = padded;
        previous = tag;
    }
    if expected_offset != bytes.len() {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(Preflight { descs, version })
}

fn section<'a>(
    bytes: &'a [u8],
    preflight: &Preflight,
    tag: u32,
) -> Result<&'a [u8], TerminalError> {
    let desc = preflight
        .descs
        .iter()
        .flatten()
        .find(|d| d.tag == tag)
        .ok_or(TerminalError::IncompatibleSnapshot)?;
    Ok(&bytes[desc.offset..desc.offset + desc.length])
}

fn logical_count(preflight: &Preflight, tag: u32) -> Result<u32, TerminalError> {
    preflight
        .descs
        .iter()
        .flatten()
        .find(|desc| desc.tag == tag)
        .map(|desc| desc.logical_count)
        .ok_or(TerminalError::IncompatibleSnapshot)
}

fn first_u32(bytes: &[u8]) -> Result<u32, TerminalError> {
    Reader::new(bytes).u32()
}

fn decode_host_geometry(
    bytes: &[u8],
    p: &Preflight,
) -> Result<crate::HostGeometryV1, TerminalError> {
    if p.version.required_features() & HOST_GEOMETRY_FEATURE == 0 {
        return Ok(crate::HostGeometryV1::default());
    }
    if logical_count(p, HOST_GEOMETRY_STATE)? != 1 {
        return Err(TerminalError::MalformedSnapshot);
    }
    let value = crate::HostGeometryV1::decode(section(bytes, p, HOST_GEOMETRY_STATE)?)
        .map_err(|_| TerminalError::MalformedSnapshot)?;
    if value == crate::HostGeometryV1::default() {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(value)
}

fn decode_host_signals(bytes: &[u8], p: &Preflight) -> Result<crate::HostSignalsV1, TerminalError> {
    if p.version.required_features() & HOST_SIGNALS_FEATURE == 0 {
        return Ok(crate::HostSignalsV1::default());
    }
    if logical_count(p, HOST_SIGNALS_STATE)? != 1 {
        return Err(TerminalError::MalformedSnapshot);
    }
    let value = crate::HostSignalsV1::decode(section(bytes, p, HOST_SIGNALS_STATE)?)?;
    if value == crate::HostSignalsV1::default() {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(value)
}

fn decode_shell_state(
    bytes: &[u8],
    p: &Preflight,
) -> Result<crate::ShellIntegration, TerminalError> {
    if p.version.required_features() & SHELL_INTEGRATION_FEATURE == 0 {
        return Ok(crate::ShellIntegration::default());
    }
    if logical_count(p, SHELL_INTEGRATION_STATE)? != 1 {
        return Err(TerminalError::MalformedSnapshot);
    }
    let sequence = scan_core(section(bytes, p, CORE_STATE)?, p.version)?.sequence;
    let value =
        crate::ShellIntegration::decode(section(bytes, p, SHELL_INTEGRATION_STATE)?, sequence)?;
    if !value.has_state() {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(value)
}

fn preflight_b(bytes: &[u8], preflight: &Preflight) -> Result<(), TerminalError> {
    decode_host_signals(bytes, preflight)?;
    if preflight.version.required_features() & SHELL_INTEGRATION_FEATURE != 0 {
        let sequence =
            scan_core(section(bytes, preflight, CORE_STATE)?, preflight.version)?.sequence;
        if logical_count(preflight, SHELL_INTEGRATION_STATE)? != 1
            || !crate::ShellIntegration::validate(
                section(bytes, preflight, SHELL_INTEGRATION_STATE)?,
                sequence,
            )?
        {
            return Err(TerminalError::MalformedSnapshot);
        }
    }
    decode_host_geometry(bytes, preflight)?;
    if preflight.version.required_features() & MACROS_FEATURE != 0 {
        scan_macros(bytes, preflight)?;
    }
    let core = scan_core(section(bytes, preflight, CORE_STATE)?, preflight.version)?;
    let columns = core.columns;
    let rows = core.rows;
    let modes = decode_modes(section(bytes, preflight, MODE_BANK)?, preflight.version)?;
    if first_u32(section(bytes, preflight, MODE_BANK)?)? != logical_count(preflight, MODE_BANK)? {
        return Err(TerminalError::MalformedSnapshot);
    }
    let margins = decode_margins(section(bytes, preflight, MARGINS)?, columns, rows)?;
    validate_margin_mode_consistency(modes, margins)?;
    decode_tabs(section(bytes, preflight, TAB_STOPS)?, columns)?;
    if preflight.version.has_keyboard_protocol_state() {
        decode_keyboard_protocol_state(
            section(bytes, preflight, KEYBOARD_PROTOCOL_STATE)?,
            logical_count(preflight, KEYBOARD_PROTOCOL_STATE)?,
        )?;
    }
    let (charsets, preference) =
        decode_charsets(section(bytes, preflight, CHARSET_STATE)?, preflight.version)?;
    validate_national_replacement_profile(preflight.version, modes, charsets, preference)?;
    let hyperlink_count =
        OscState::snapshot_v2_hyperlink_count(section(bytes, preflight, OSC_STATE)?)
            .map_err(map_osc_import_error)?;
    let mut widths = GraphemeWidths::new();
    let resources = if preflight.version.required_features() & GRAPHICS_FEATURE != 0 {
        if logical_count(preflight, GRAPHICS_STATE)? != 1 {
            return Err(TerminalError::MalformedSnapshot);
        }
        Some(crate::graphics_state::GraphicsState::inspect_resources(
            section(bytes, preflight, GRAPHICS_STATE)?,
        )?)
    } else {
        None
    };
    let grapheme_count = scan_graphemes(
        section(bytes, preflight, GRAPHEME_TABLE)?,
        &mut widths,
        preflight.version,
        resources.as_ref(),
    )?;
    let mut grapheme_uses = GraphemeUses::new();
    if let Some(id) = core.repeat_grapheme_id {
        if id as usize >= grapheme_count {
            return Err(TerminalError::MalformedSnapshot);
        }
        grapheme_uses.set(id as usize);
    }
    let rendition_count = scan_renditions(
        section(bytes, preflight, RENDITION_TABLE)?,
        preflight.version,
    )?;
    let mut rendition_uses = RenditionUses::new();
    if preflight.version.has_sgr_stack() {
        for (_, id) in scan_sgr_stack(bytes, preflight, rendition_count)? {
            rendition_uses.set(id as usize);
        }
    }
    if grapheme_count as u32 != logical_count(preflight, GRAPHEME_TABLE)?
        || rendition_count as u32 != logical_count(preflight, RENDITION_TABLE)?
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    let primary = scan_meta(
        section(bytes, preflight, PRIMARY_META)?,
        rendition_count,
        preflight.version,
        true,
    )?;
    let alternate = scan_meta(
        section(bytes, preflight, ALTERNATE_META)?,
        rendition_count,
        preflight.version,
        false,
    )?;
    if preflight.version.has_retained_primary_scrollback() {
        for id in [
            primary.attributes_id,
            primary.saved_attributes_id,
            alternate.attributes_id,
            alternate.saved_attributes_id,
        ] {
            rendition_uses.set(id as usize);
        }
    }
    let mut row_tables = RowScanTables {
        grapheme_count,
        rendition_count,
        hyperlink_count,
        widths: &widths,
        grapheme_uses: &mut grapheme_uses,
        rendition_uses: &mut rendition_uses,
    };
    scan_rows(
        section(bytes, preflight, PRIMARY_ROWS)?,
        primary,
        &mut row_tables,
        preflight.version,
        true,
        core.sequence,
    )?;
    if primary.stored_rows != logical_count(preflight, PRIMARY_ROWS)?
        || alternate.stored_rows != logical_count(preflight, ALTERNATE_ROWS)?
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    scan_rows(
        section(bytes, preflight, ALTERNATE_ROWS)?,
        alternate,
        &mut row_tables,
        preflight.version,
        false,
        core.sequence,
    )?;
    let mut active_meta = if core.active_alternate {
        alternate
    } else {
        primary
    };
    if preflight.version.required_features() & PAGES_FEATURE != 0 {
        let pages = page_records(bytes, preflight)?;
        let mut cells = 0usize;
        for (id, meta_bytes, rows_bytes) in pages.entries {
            let meta = scan_meta(meta_bytes, rendition_count, preflight.version, false)?;
            if meta.columns != columns || meta.visible_rows != rows {
                return Err(TerminalError::MalformedSnapshot);
            }
            cells += meta.columns as usize * meta.visible_rows as usize;
            if cells > crate::pages::MAX_EXTRA_CELLS {
                return Err(TerminalError::MalformedSnapshot);
            }
            row_tables.rendition_uses.set(meta.attributes_id as usize);
            row_tables
                .rendition_uses
                .set(meta.saved_attributes_id as usize);
            scan_rows(
                rows_bytes,
                meta,
                &mut row_tables,
                preflight.version,
                false,
                core.sequence,
            )?;
            if !core.active_alternate && id == pages.active {
                active_meta = meta;
            }
        }
    } else if grapheme_count > 2 * MAX_CELLS + 2
        || rendition_count > 2 * MAX_CELLS + 5 + SGR_STACK_CAPACITY
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    if alternate.columns != columns
        || alternate.visible_rows != rows
        || (!core.active_alternate && (primary.columns != columns || primary.visible_rows != rows))
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    if !preflight.version.has_line_renditions() {
        validate_active_meta_wrap_state(active_meta, modes, margins)?;
    }
    if (preflight.version.has_retained_primary_scrollback()
        || preflight.version.has_repeat_source()
        || preflight.version.has_cursor_style()
        || preflight.version.required_features() & GRAPHICS_FEATURE != 0)
        && (1..grapheme_count).any(|id| !grapheme_uses.get(id))
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    if preflight.version.has_retained_primary_scrollback()
        && (1..rendition_count).any(|id| !rendition_uses.get(id))
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(())
}

struct PageRecords<'a> {
    active: u8,
    visible: u8,
    saved: u8,
    coupled: bool,
    entries: Vec<(u8, &'a [u8], &'a [u8])>,
}

fn page_records<'a>(
    bytes: &'a [u8],
    preflight: &Preflight,
) -> Result<PageRecords<'a>, TerminalError> {
    let bytes = section(bytes, preflight, PAGES_STATE)?;
    if bytes.len() > MAX_PAGES_SECTION_BYTES {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut reader = Reader::new(bytes);
    let active = reader.u8()?;
    let visible = reader.u8()?;
    let saved = reader.u8()?;
    let coupled = decode_bool(reader.u8()?)?;
    let count = reader.u32()?;
    if count >= u32::from(crate::pages::MAX_PAGES)
        || count != logical_count(preflight, PAGES_STATE)?
        || reader.u64()? != 0
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut entries = Vec::with_capacity(count as usize);
    let mut previous = 1;
    for _ in 0..count {
        let id = reader.u32()?;
        if id <= previous || id > u32::from(crate::pages::MAX_PAGES) {
            return Err(TerminalError::MalformedSnapshot);
        }
        let length = reader.u32()? as usize;
        let meta = reader.take(64)?;
        let rows = reader.take(length)?;
        entries.push((id as u8, meta, rows));
        previous = id;
    }
    for id in [active, visible, saved] {
        if id != 1 && !entries.iter().any(|(page, _, _)| *page == id) {
            return Err(TerminalError::MalformedSnapshot);
        }
    }
    if !reader.is_empty()
        || (coupled && active != visible)
        || (coupled && count == 0 && active == 1 && visible == 1 && saved == 1)
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(PageRecords {
        active,
        visible,
        saved,
        coupled,
        entries,
    })
}

fn scan_macros<'a>(
    bytes: &'a [u8],
    p: &Preflight,
) -> Result<Vec<(usize, &'a [u8])>, TerminalError> {
    let payload = section(bytes, p, MACROS_STATE)?;
    if payload.len() > crate::macros::MAX_SNAPSHOT_BYTES {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut reader = Reader::new(payload);
    let count = reader.u32()? as usize;
    let total = reader.u32()? as usize;
    if !(1..=crate::macros::CAPACITY).contains(&count)
        || count as u32 != logical_count(p, MACROS_STATE)?
        || total > crate::macros::MAX_BYTES
        || payload.len() != 8 + count * 8 + total
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| TerminalError::LimitExceeded)?;
    let mut previous = None;
    let mut used = 0usize;
    for _ in 0..count {
        let id = reader.u32()? as usize;
        let length = reader.u32()? as usize;
        if id >= crate::macros::CAPACITY
            || previous.is_some_and(|v| v >= id)
            || length == 0
            || length > total - used
        {
            return Err(TerminalError::MalformedSnapshot);
        }
        result.push((id, reader.take(length)?));
        used += length;
        previous = Some(id);
    }
    if used != total {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(result)
}

fn scan_sgr_stack(
    bytes: &[u8],
    p: &Preflight,
    rendition_count: usize,
) -> Result<impl Iterator<Item = (u32, u32)>, TerminalError> {
    let bytes = section(bytes, p, SGR_STACK_STATE)?;
    let mut reader = Reader::new(bytes);
    let count = reader.u32()? as usize;
    require_zero(reader.u32()?)?;
    if !(1..=SGR_STACK_CAPACITY).contains(&count)
        || count as u32 != logical_count(p, SGR_STACK_STATE)?
        || bytes.len() != 8 + count * 8
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut entries = [(0, 0); SGR_STACK_CAPACITY];
    for entry in &mut entries[..count] {
        let mask = reader.u32()?;
        let id = reader.u32()?;
        if mask & !SGR_STACK_SELECTORS != 0
            || (mask & 1 != 0 && mask != 1)
            || id as usize >= rendition_count
        {
            return Err(TerminalError::MalformedSnapshot);
        }
        *entry = (mask, id);
    }
    Ok(entries.into_iter().take(count))
}

fn scan_core(bytes: &[u8], version: SnapshotVersion) -> Result<CoreState, TerminalError> {
    if bytes.len() != 32 {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut r = Reader::new(bytes);
    let columns = r.u32()?;
    let rows = r.u32()?;
    validate_dimensions(columns, rows)?;
    let sequence = r.u64()?;
    if sequence == u64::MAX {
        return Err(TerminalError::MalformedSnapshot);
    }
    let time = r.u64()?;
    let alternate = decode_bool(r.u8()?)?;
    let visible = decode_bool(r.u8()?)?;
    let flags = r.u16()?;
    let repeat_grapheme_id = r.u32()?;
    let (cursor_shape, cursor_blinking) = if version.has_cursor_style() {
        if flags & !0x000f != 0 || flags & 0x000e == 0 {
            return Err(TerminalError::MalformedSnapshot);
        }
        let cursor_shape = match (flags >> 2) & 3 {
            0 => CursorShape::Block,
            1 => CursorShape::Underline,
            2 => CursorShape::Bar,
            _ => return Err(TerminalError::MalformedSnapshot),
        };
        (cursor_shape, flags & (1 << 1) == 0)
    } else {
        if flags & !1 != 0 {
            return Err(TerminalError::MalformedSnapshot);
        }
        (CursorShape::Block, true)
    };
    let repeat_grapheme_id = if version.has_repeat_source() {
        if flags & 1 == 0 {
            return Err(TerminalError::MalformedSnapshot);
        }
        Some(repeat_grapheme_id)
    } else {
        if flags & 1 != 0 {
            return Err(TerminalError::MalformedSnapshot);
        }
        require_zero(repeat_grapheme_id)?;
        None
    };
    Ok(CoreState {
        columns,
        rows,
        sequence,
        time,
        active_alternate: alternate,
        cursor_visible: visible,
        cursor_shape,
        cursor_blinking,
        repeat_grapheme_id,
    })
}

fn decode_modes(bytes: &[u8], version: SnapshotVersion) -> Result<ModeState, TerminalError> {
    let mut r = Reader::new(bytes);
    let count = r.u32()? as usize;
    require_zero(r.u32()?)?;
    if bytes.len()
        != 8 + count
            .checked_mul(8)
            .ok_or(TerminalError::MalformedSnapshot)?
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut result = ModeState::default();
    let mut previous = None;
    for _ in 0..count {
        let mode = r.u16()?;
        let namespace = r.u8()?;
        let flags = r.u8()?;
        require_zero(r.u32()?)?;
        let key = (namespace, mode);
        if previous.is_some_and(|v| v >= key) || flags & !1 != 0 {
            return Err(TerminalError::MalformedSnapshot);
        }
        let enabled = flags & 1 != 0;
        let accepted = match namespace {
            0 => result.set_ansi_mode(mode, enabled),
            1 => result.set_private_mode(mode, enabled),
            2 if enabled && matches!(mode, 1 | 2) => {
                result.attribute_change_extent = if mode == 2 {
                    AttributeChangeExtent::Rectangle
                } else {
                    AttributeChangeExtent::Stream
                };
                true
            }
            3 if enabled && result.modify_other_keys == ModifyOtherKeys::Disabled => {
                let Some(value) = ModifyOtherKeys::from_parameter(mode) else {
                    return Err(TerminalError::MalformedSnapshot);
                };
                if value == ModifyOtherKeys::Disabled {
                    return Err(TerminalError::MalformedSnapshot);
                }
                result.modify_other_keys = value;
                true
            }
            4 if enabled && version.required_features() & TRADITIONAL_MODES_FEATURE != 0 => {
                match mode {
                    1 => {
                        result.vt52 = true;
                        true
                    }
                    5 => {
                        result.send_c1 = true;
                        true
                    }
                    2 => {
                        result.iso2022 = true;
                        true
                    }
                    3 | 4 if result.receive_c1 == 0 => {
                        result.receive_c1 = (mode - 2) as u8;
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        if !accepted || (!enabled && !(namespace == 1 && mode == 7)) {
            return Err(TerminalError::MalformedSnapshot);
        }
        previous = Some(key);
    }
    if result.win32_input_mode != version.has_win32_input_mode() {
        return Err(TerminalError::MalformedSnapshot);
    }
    if result.alternate_scroll != version.has_alternate_scroll() {
        return Err(TerminalError::MalformedSnapshot);
    }
    if (result.grapheme_clustering || result.color_scheme_reporting)
        != version.has_modern_private_modes()
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    if result.national_replacement_character_set_mode && !version.has_national_replacement_charset()
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(result)
}

fn decode_margins(bytes: &[u8], columns: u32, rows: u32) -> Result<Margins, TerminalError> {
    if bytes.len() != 24 {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut r = Reader::new(bytes);
    let top = r.u32()?;
    let bottom = r.u32()?;
    let left = r.u32()?;
    let right = r.u32()?;
    let flags = r.u32()?;
    require_zero(r.u32()?)?;
    if flags & !3 != 0 {
        return Err(TerminalError::MalformedSnapshot);
    }
    let result = Margins {
        top,
        bottom,
        left,
        right,
        vertical_follows_screen: flags & 1 != 0,
        horizontal_follows_screen: flags & 2 != 0,
    };
    if !result.validate(columns, rows) {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(result)
}

fn validate_margin_mode_consistency(
    modes: ModeState,
    margins: Margins,
) -> Result<(), TerminalError> {
    if !modes.left_right_margin && !margins.horizontal_follows_screen {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(())
}

fn expected_wrap_column(row: u32, columns: u32, modes: ModeState, margins: Margins) -> u32 {
    if modes.left_right_margin && row >= margins.top && row < margins.bottom {
        margins.right - 1
    } else {
        columns - 1
    }
}

fn validate_active_meta_wrap_state(
    meta: ScreenMeta,
    modes: ModeState,
    margins: Margins,
) -> Result<(), TerminalError> {
    if (meta.cursor_flags & 1 != 0
        && meta.cursor_column
            != expected_wrap_column(meta.cursor_row, meta.columns, modes, margins))
        || (meta.saved_flags & 1 != 0
            && meta.saved_column
                != expected_wrap_column(meta.saved_row, meta.columns, modes, margins))
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(())
}

fn validate_active_screen_wrap_state(
    screen: &Screen,
    modes: ModeState,
    margins: Margins,
) -> Result<(), TerminalError> {
    if (screen.wrap_pending
        && screen.cursor_column
            != expected_wrap_column(
                screen.cursor_row,
                screen.line_columns(screen.cursor_row),
                modes,
                margins,
            ))
        || (screen.saved.wrap_pending
            && screen.saved.column
                != expected_wrap_column(
                    screen.saved.row,
                    screen.line_columns(screen.saved.row),
                    modes,
                    margins,
                ))
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(())
}

fn decode_tabs(bytes: &[u8], columns: u32) -> Result<TabStops, TerminalError> {
    if bytes.len() != 72 {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut r = Reader::new(bytes);
    if r.u32()? != columns || r.u32()? != 8 {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut words = [0; 8];
    for word in &mut words {
        *word = r.u64()?;
    }
    TabStops::from_words(columns, words).ok_or(TerminalError::MalformedSnapshot)
}

fn decode_keyboard_protocol_state(
    bytes: &[u8],
    logical_count: u32,
) -> Result<(KeyboardProtocolStack, KeyboardProtocolStack), TerminalError> {
    let mut r = Reader::new(bytes);
    let main_len = usize::from(r.u8()?);
    let alternate_len = usize::from(r.u8()?);
    require_zero(r.u16()?)?;
    require_zero(r.u32()?)?;
    if main_len > KITTY_KEYBOARD_STACK_CAPACITY || alternate_len > KITTY_KEYBOARD_STACK_CAPACITY {
        return Err(TerminalError::MalformedSnapshot);
    }
    let count = main_len
        .checked_add(alternate_len)
        .ok_or(TerminalError::MalformedSnapshot)?;
    if count == 0 || logical_count != count as u32 || bytes.len() != 8 + count {
        return Err(TerminalError::MalformedSnapshot);
    }
    let main_values = r.take(main_len)?;
    let alternate_values = r.take(alternate_len)?;
    if !r.is_empty()
        || main_values
            .iter()
            .chain(alternate_values)
            .any(|flags| flags & !KITTY_KEYBOARD_SUPPORTED_FLAGS != 0)
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    let main =
        KeyboardProtocolStack::from_slice(main_values).ok_or(TerminalError::MalformedSnapshot)?;
    let alternate = KeyboardProtocolStack::from_slice(alternate_values)
        .ok_or(TerminalError::MalformedSnapshot)?;
    Ok((main, alternate))
}

fn decode_charsets(
    bytes: &[u8],
    version: SnapshotVersion,
) -> Result<
    (
        (CharsetState, CharsetState, CharsetState),
        CharsetDesignation,
    ),
    TerminalError,
> {
    let extended = version.required_features() & TRADITIONAL_MODES_FEATURE != 0;
    let graphics = version.required_features() & GRAPHICS_FEATURE != 0;
    let record_size = if graphics { 24 } else { 12 };
    if bytes.len() != 8 + 3 * record_size + if extended { 8 } else { 0 } {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut r = Reader::new(bytes);
    if r.u32()? != 3 || r.u32()? as usize != record_size {
        return Err(TerminalError::MalformedSnapshot);
    }
    let charsets = (
        decode_charset(&mut r, version)?,
        decode_charset(&mut r, version)?,
        decode_charset(&mut r, version)?,
    );
    let preference = if extended {
        let value = if graphics {
            decode_raw_designation(&mut r)?
        } else {
            let value = decode_designation(r.u8()?, version)?;
            require_zero(r.u8()?)?;
            require_zero(r.u16()?)?;
            value
        };
        require_zero(r.u32()?)?;
        if value.charset == GraphicCharset::UserPreferred {
            return Err(TerminalError::MalformedSnapshot);
        }
        value
    } else {
        CharsetDesignation::LATIN1
    };
    Ok((charsets, preference))
}

fn decode_raw_designation(r: &mut Reader<'_>) -> Result<CharsetDesignation, TerminalError> {
    let size = match r.u8()? {
        94 => CharsetSize::Characters94,
        96 => CharsetSize::Characters96,
        _ => return Err(TerminalError::MalformedSnapshot),
    };
    let bytes = r.take(3)?;
    let length = bytes.iter().position(|&byte| byte == 0).unwrap_or(3);
    if bytes[length..].iter().any(|&byte| byte != 0) {
        return Err(TerminalError::MalformedSnapshot);
    }
    let (&final_byte, prefix) = bytes[..length]
        .split_last()
        .ok_or(TerminalError::MalformedSnapshot)?;
    CharsetDesignation::decode_with_soft(size, prefix, final_byte, true)
        .map_err(|_| TerminalError::MalformedSnapshot)
}

fn decode_designation(
    designator: u8,
    version: SnapshotVersion,
) -> Result<CharsetDesignation, TerminalError> {
    let (size, prefix, final_byte) = match designator {
        15..=30 if version.required_features() & TRADITIONAL_MODES_FEATURE != 0 => {
            let set = crate::legacy_charsets::ExtendedCharset::from_id(designator - 15)
                .ok_or(TerminalError::MalformedSnapshot)?;
            let (&final_byte, prefix) = set.designator().as_bytes().split_last().unwrap();
            return CharsetDesignation::decode_with_prefix(set.size(), prefix, final_byte)
                .map_err(|_| TerminalError::MalformedSnapshot);
        }
        31 | 32 if version.required_features() & TRADITIONAL_MODES_FEATURE != 0 => {
            return CharsetDesignation::decode_with_prefix(
                if designator == 31 {
                    CharsetSize::Characters94
                } else {
                    CharsetSize::Characters96
                },
                b"",
                b'<',
            )
            .map_err(|_| TerminalError::MalformedSnapshot);
        }
        0 => (CharsetSize::Characters94, &b""[..], b'B'),
        1 => (CharsetSize::Characters94, &b""[..], b'0'),
        2 => (CharsetSize::Characters96, &b""[..], b'A'),
        3 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b""[..], b'A')
        }
        4 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b""[..], b'4')
        }
        5 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b""[..], b'5')
        }
        6 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b""[..], b'R')
        }
        7 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b""[..], b'Q')
        }
        8 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b""[..], b'K')
        }
        9 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b""[..], b'Y')
        }
        10 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b""[..], b'E')
        }
        11 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b"%"[..], b'6')
        }
        12 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b""[..], b'Z')
        }
        13 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b""[..], b'7')
        }
        14 if version.has_national_replacement_charset() => {
            (CharsetSize::Characters94, &b""[..], b'=')
        }
        _ => return Err(TerminalError::MalformedSnapshot),
    };
    CharsetDesignation::decode_with_prefix(size, prefix, final_byte)
        .map_err(|_| TerminalError::MalformedSnapshot)
}

fn decode_charset(
    r: &mut Reader<'_>,
    version: SnapshotVersion,
) -> Result<CharsetState, TerminalError> {
    let mut designators = [CharsetDesignation::LATIN1; 4];
    for designation in &mut designators {
        *designation = if version.required_features() & GRAPHICS_FEATURE != 0 {
            decode_raw_designation(r)?
        } else {
            decode_designation(r.u8()?, version)?
        };
    }
    let gl = r.u8()?;
    let gr = r.u8()?;
    let shift = r.u8()?;
    require_zero(r.u8()?)?;
    require_zero(r.u32()?)?;
    let mut state = CharsetState::default();
    for (register, designator) in [
        GraphicRegister::G0,
        GraphicRegister::G1,
        GraphicRegister::G2,
        GraphicRegister::G3,
    ]
    .into_iter()
    .zip(designators)
    {
        state.set_designation(register, designator);
    }
    match gl {
        0 => state.shift_in(),
        1 => state.shift_out(),
        2 => state.invoke_gl(GraphicRegister::G2),
        3 => state.invoke_gl(GraphicRegister::G3),
        _ => return Err(TerminalError::MalformedSnapshot),
    }
    state.invoke_gr(match gr {
        0 => GraphicRegister::G0,
        1 => GraphicRegister::G1,
        2 => GraphicRegister::G2,
        3 => GraphicRegister::G3,
        _ => return Err(TerminalError::MalformedSnapshot),
    });
    match shift {
        0 => {}
        2 => state.single_shift_2(),
        3 => state.single_shift_3(),
        _ => return Err(TerminalError::MalformedSnapshot),
    }
    Ok(state)
}

fn validate_national_replacement_profile(
    version: SnapshotVersion,
    modes: ModeState,
    charsets: (CharsetState, CharsetState, CharsetState),
    preference: CharsetDesignation,
) -> Result<(), TerminalError> {
    let state_uses_national_replacement = modes.national_replacement_character_set_mode
        || charset_has_national_replacement(charsets.0)
        || charset_has_national_replacement(charsets.1)
        || charset_has_national_replacement(charsets.2)
        || matches!(preference.charset, GraphicCharset::NationalReplacement(_));
    let traditional = modes.has_traditional_modes()
        || charsets.0.has_extended()
        || charsets.1.has_extended()
        || charsets.2.has_extended()
        || preference != CharsetDesignation::LATIN1;
    if traditional != (version.required_features() & TRADITIONAL_MODES_FEATURE != 0) {
        return Err(TerminalError::MalformedSnapshot);
    }
    if state_uses_national_replacement != version.has_national_replacement_charset() {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(())
}

fn scan_graphemes(
    bytes: &[u8],
    widths: &mut GraphemeWidths,
    version: SnapshotVersion,
    resources: Option<&crate::graphics_resources::ResourceFrame<'_>>,
) -> Result<usize, TerminalError> {
    let mut r = Reader::new(bytes);
    let count = r.u32()? as usize;
    require_zero(r.u32()?)?;
    if count == 0 || count > MAX_GRAPHEME_TABLE_ENTRIES {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut previous: Option<GraphemeRef<'_>> = None;
    for index in 0..count {
        let value = read_grapheme(&mut r, version)?;
        if previous.is_some_and(|previous| previous.compare(value) != Ordering::Less)
            || (index == 0
                && (value.character != ' '
                    || !value.combining.is_empty()
                    || value.width != 1
                    || value.glyph.is_some()))
        {
            return Err(TerminalError::MalformedSnapshot);
        }
        if let Some(glyph) = value.glyph {
            let (width, height, format) = resources
                .and_then(|resources| resources.metadata(glyph.resource))
                .ok_or(TerminalError::MalformedSnapshot)?;
            if format != crate::graphics_resources::ImageFormat::Alpha8
                || glyph.width == 0
                || glyph.height == 0
                || glyph.height > height
                || glyph
                    .x
                    .checked_add(glyph.width)
                    .is_none_or(|right| right > width)
            {
                return Err(TerminalError::MalformedSnapshot);
            }
        }
        widths.set(index, value.width);
        previous = Some(value);
    }
    if !r.is_empty() {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(count)
}

fn read_grapheme<'a>(
    r: &mut Reader<'a>,
    version: SnapshotVersion,
) -> Result<GraphemeRef<'a>, TerminalError> {
    let length = r.u16()? as usize;
    let width = r.u8()?;
    let glyph = match r.u8()? {
        0 => None,
        1 if version.required_features() & GRAPHICS_FEATURE != 0 => Some(GlyphReference {
            resource: r.u64()?,
            x: r.u32()?,
            width: r.u32()?,
            height: r.u32()?,
        }),
        _ => return Err(TerminalError::MalformedSnapshot),
    };
    let profile = version.width_profile();
    if length == 0 || length > profile.max_grapheme_bytes() || !matches!(width, 1 | 2) {
        return Err(TerminalError::MalformedSnapshot);
    }
    let text =
        std::str::from_utf8(r.take(length)?).map_err(|_| TerminalError::MalformedSnapshot)?;
    let mut chars = text.chars();
    let character = chars.next().ok_or(TerminalError::MalformedSnapshot)?;
    let combining = chars.as_str();
    if !profile.valid_grapheme(character, combining, width)
        || (glyph.is_some() && (width != 1 || !combining.is_empty()))
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    r.zero_padding_to4()?;
    Ok(GraphemeRef {
        character,
        combining,
        width,
        glyph,
    })
}

fn scan_renditions(bytes: &[u8], version: SnapshotVersion) -> Result<usize, TerminalError> {
    let mut r = Reader::new(bytes);
    let count = r.u32()? as usize;
    if r.u32()? != 48
        || count == 0
        || count > MAX_RENDITION_TABLE_ENTRIES
        || bytes.len()
            != 8 + count
                .checked_mul(48)
                .ok_or(TerminalError::MalformedSnapshot)?
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut previous: Option<&[u8]> = None;
    let mut has_extended_sgr = false;
    for index in 0..count {
        let record = r.take(48)?;
        let attributes = decode_rendition_record(record, version)?;
        has_extended_sgr |= attributes_need_extended_sgr(attributes);
        if previous.is_some_and(|v| v >= record) || (index == 0 && record != [0; 48]) {
            return Err(TerminalError::MalformedSnapshot);
        }
        previous = Some(record);
    }
    if version.has_extended_sgr() != has_extended_sgr {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(count)
}

fn decode_rendition_record(
    record: &[u8],
    version: SnapshotVersion,
) -> Result<Attributes, TerminalError> {
    let flags = u32::from_le_bytes(
        record[..4]
            .try_into()
            .map_err(|_| TerminalError::MalformedSnapshot)?,
    );
    let allowed_flags = if version.has_extended_sgr() {
        0xff
    } else {
        0x3f
    };
    let maximum_style = if version.has_extended_sgr() { 5 } else { 1 };
    if flags & !allowed_flags != 0
        || record[4] > maximum_style
        || record[5..8].iter().any(|b| *b != 0)
        || if version.has_extended_sgr() {
            record[32..].iter().any(|b| *b != 0)
        } else {
            record[24..].iter().any(|b| *b != 0)
        }
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(Attributes {
        bold: flags & 1 != 0,
        faint: flags & 0x40 != 0,
        blink: flags & 2 != 0,
        italic: flags & 4 != 0,
        underline_style: decode_underline_style(record[4])?,
        inverse: flags & 8 != 0,
        conceal: flags & 0x10 != 0,
        strikethrough: flags & 0x20 != 0,
        overline: flags & 0x80 != 0,
        foreground: decode_color_spec(&record[8..16])?,
        background: decode_color_spec(&record[16..24])?,
        underline_color: if version.has_extended_sgr() {
            decode_color_spec(&record[24..32])?
        } else {
            Color::Default
        },
    })
}

fn decode_underline_style(value: u8) -> Result<UnderlineStyle, TerminalError> {
    match value {
        0 => Ok(UnderlineStyle::None),
        1 => Ok(UnderlineStyle::Single),
        2 => Ok(UnderlineStyle::Double),
        3 => Ok(UnderlineStyle::Curly),
        4 => Ok(UnderlineStyle::Dotted),
        5 => Ok(UnderlineStyle::Dashed),
        _ => Err(TerminalError::MalformedSnapshot),
    }
}

fn decode_color_spec(bytes: &[u8]) -> Result<Color, TerminalError> {
    if bytes[1..4].iter().any(|b| *b != 0) {
        return Err(TerminalError::MalformedSnapshot);
    }
    let value = u32::from_le_bytes(
        bytes[4..8]
            .try_into()
            .map_err(|_| TerminalError::MalformedSnapshot)?,
    );
    match bytes[0] {
        0 if value == 0 => Ok(Color::Default),
        1 if value <= 255 => Ok(Color::Indexed(value as u8)),
        2 if value <= 0x00ff_ffff => Ok(Color::Rgb(
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
        )),
        _ => Err(TerminalError::MalformedSnapshot),
    }
}

fn decode_meta(bytes: &[u8]) -> Result<ScreenMeta, TerminalError> {
    if bytes.len() != 64 {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut r = Reader::new(bytes);
    let result = ScreenMeta {
        columns: r.u32()?,
        visible_rows: r.u32()?,
        stored_rows: r.u32()?,
        viewport_top: r.u32()?,
        cursor_row: r.u32()?,
        cursor_column: r.u32()?,
        cursor_flags: r.u32()?,
        attributes_id: r.u32()?,
        saved_row: r.u32()?,
        saved_column: r.u32()?,
        saved_flags: r.u32()?,
        saved_attributes_id: r.u32()?,
        next_logical_line_id: r.u64()?,
        requested_history_rows: r.u64()?,
    };
    Ok(result)
}

fn scan_meta(
    bytes: &[u8],
    rendition_count: usize,
    version: SnapshotVersion,
    primary: bool,
) -> Result<ScreenMeta, TerminalError> {
    let meta = decode_meta(bytes)?;
    validate_dimensions(meta.columns, meta.visible_rows)?;
    let cells = (meta.stored_rows as usize)
        .checked_mul(meta.columns as usize)
        .ok_or(TerminalError::MalformedSnapshot)?;
    let retained_primary = primary && version.has_retained_primary_scrollback();
    let geometry_is_invalid = if retained_primary {
        meta.stored_rows <= meta.visible_rows
            || meta.viewport_top != meta.stored_rows - meta.visible_rows
            || u64::from(meta.viewport_top) > meta.requested_history_rows
    } else {
        meta.stored_rows != meta.visible_rows || meta.viewport_top != 0
    };
    if geometry_is_invalid
        || cells > MAX_CELLS
        || meta.cursor_row >= meta.visible_rows
        || meta.saved_row >= meta.visible_rows
        || meta.cursor_column >= meta.columns
        || meta.saved_column >= meta.columns
        || meta.cursor_flags & !3 != 0
        || meta.saved_flags & !15 != 0
        || meta.attributes_id as usize >= rendition_count
        || meta.saved_attributes_id as usize >= rendition_count
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    if version.has_history() {
        let minimum_history = u64::from(version.required_features() & GRAPHICS_FEATURE == 0);
        if (primary
            && !(minimum_history..=u64::from(MAX_HISTORY_ROWS))
                .contains(&meta.requested_history_rows))
            || (!primary && meta.requested_history_rows != 0)
            || meta.next_logical_line_id == 0
        {
            return Err(TerminalError::MalformedSnapshot);
        }
    } else if meta.next_logical_line_id != 0 || meta.requested_history_rows != 0 {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(meta)
}

fn scan_rows(
    bytes: &[u8],
    meta: ScreenMeta,
    tables: &mut RowScanTables<'_>,
    version: SnapshotVersion,
    primary: bool,
    checkpoint_sequence: u64,
) -> Result<(), TerminalError> {
    let mut r = Reader::new(bytes);
    let retained_primary = primary && version.has_retained_primary_scrollback();
    let rows = r.u32()?;
    require_zero(r.u32()?)?;
    let _scrollback_generation_rows = if retained_primary { r.u64()? } else { 0 };
    if rows != meta.stored_rows {
        return Err(TerminalError::MalformedSnapshot);
    }
    let mut previous_anchor: Option<(u64, u32, u32, bool, u64)> = None;
    for row in 0..rows {
        let logical_line_id = r.u64()?;
        let logical_cell_offset = r.u32()?;
        let meaningful = r.u32()?;
        let flags = r.u32()?;
        let runs = r.u32()?;
        let sealed_revision = if retained_primary { r.u64()? } else { 0 };
        let soft_wrapped = flags & 1 != 0;
        let head_truncated = flags & 2 != 0;
        let rendition_flags = if version.has_line_renditions() { 12 } else { 0 };
        let line_rendition = crate::LineRendition::from_wire((flags & rendition_flags) >> 2)
            .ok_or(TerminalError::MalformedSnapshot)?;
        if sealed_revision > checkpoint_sequence
            || meaningful > line_rendition.columns(meta.columns)
            || (line_rendition != crate::LineRendition::SingleWidth && soft_wrapped)
            || (line_rendition != crate::LineRendition::SingleWidth
                && previous_anchor.is_some_and(|(_, _, _, wrapped, _)| wrapped))
            || (row + 1 == rows && soft_wrapped)
            || if version.has_history() {
                flags & !(3 | rendition_flags) != 0
                    || logical_line_id >= meta.next_logical_line_id
                    || (row != 0 && head_truncated)
                    || match previous_anchor {
                        None => logical_cell_offset != 0 && !head_truncated,
                        Some((
                            previous_id,
                            previous_offset,
                            previous_meaningful,
                            true,
                            previous_sealed_revision,
                        )) => {
                            logical_line_id != previous_id
                                || Some(logical_cell_offset)
                                    != previous_offset.checked_add(previous_meaningful)
                                || sealed_revision != previous_sealed_revision
                        }
                        Some((_, _, _, false, _)) => logical_cell_offset != 0,
                    }
            } else {
                logical_line_id != 0
                    || logical_cell_offset != 0
                    || flags & !(1 | rendition_flags) != 0
            }
        {
            return Err(TerminalError::MalformedSnapshot);
        }
        previous_anchor = Some((
            logical_line_id,
            logical_cell_offset,
            meaningful,
            soft_wrapped,
            sealed_revision,
        ));
        for (cursor_row, cursor_column) in [
            (meta.cursor_row, meta.cursor_column),
            (meta.saved_row, meta.saved_column),
        ] {
            if meta.viewport_top + cursor_row == row
                && cursor_column >= line_rendition.columns(meta.columns)
            {
                return Err(TerminalError::MalformedSnapshot);
            }
        }
        let mut physical = 0_u64;
        let mut previous = None;
        for _ in 0..runs {
            let gid = r.u32()?;
            let rid = r.u32()?;
            let repeat = r.u32()?;
            let metadata = r.u32()?;
            let hyperlink =
                usize::try_from(metadata >> 1).map_err(|_| TerminalError::MalformedSnapshot)?;
            if gid as usize >= tables.grapheme_count
                || rid as usize >= tables.rendition_count
                || repeat == 0
                || hyperlink > tables.hyperlink_count
                || previous == Some((gid, rid, metadata))
            {
                return Err(TerminalError::MalformedSnapshot);
            }
            tables.grapheme_uses.set(gid as usize);
            tables.rendition_uses.set(rid as usize);
            let start = physical;
            let cell_width = tables.widths.get(gid as usize);
            physical = physical
                .checked_add(cell_width * u64::from(repeat))
                .ok_or(TerminalError::MalformedSnapshot)?;
            if physical > u64::from(meta.columns)
                || (physical > u64::from(meaningful) && start < u64::from(meta.columns) && gid != 0)
            {
                return Err(TerminalError::MalformedSnapshot);
            }
            if cell_width == 2 {
                for (target_row, target_column, target_wrap_pending) in [
                    (
                        meta.cursor_row,
                        meta.cursor_column,
                        meta.cursor_flags & 1 != 0,
                    ),
                    (meta.saved_row, meta.saved_column, meta.saved_flags & 1 != 0),
                ] {
                    let target = u64::from(target_column);
                    if meta.viewport_top + target_row == row
                        && target >= start
                        && target < physical
                        && (target - start) % 2 == 1
                        && !target_wrap_pending
                    {
                        return Err(TerminalError::MalformedSnapshot);
                    }
                }
            }
            previous = Some((gid, rid, metadata));
        }
        if physical != u64::from(meta.columns) {
            return Err(TerminalError::MalformedSnapshot);
        }
    }
    if !r.is_empty() {
        return Err(TerminalError::MalformedSnapshot);
    }
    Ok(())
}

fn decode_terminal(bytes: &[u8], p: &Preflight) -> Result<Terminal, TerminalError> {
    let core = scan_core(section(bytes, p, CORE_STATE)?, p.version)?;
    let columns = core.columns;
    let rows = core.rows;
    let modes = decode_modes(section(bytes, p, MODE_BANK)?, p.version)?;
    let margins = decode_margins(section(bytes, p, MARGINS)?, columns, rows)?;
    validate_margin_mode_consistency(modes, margins)?;
    let tabs = decode_tabs(section(bytes, p, TAB_STOPS)?, columns)?;
    let (main_keyboard_protocol, alternate_keyboard_protocol) =
        if p.version.has_keyboard_protocol_state() {
            decode_keyboard_protocol_state(
                section(bytes, p, KEYBOARD_PROTOCOL_STATE)?,
                logical_count(p, KEYBOARD_PROTOCOL_STATE)?,
            )?
        } else {
            (
                KeyboardProtocolStack::default(),
                KeyboardProtocolStack::default(),
            )
        };
    let (charsets, user_preferred_charset) =
        decode_charsets(section(bytes, p, CHARSET_STATE)?, p.version)?;
    validate_national_replacement_profile(p.version, modes, charsets, user_preferred_charset)?;
    let (charset, main_saved_charset, alternate_saved_charset) = charsets;
    let mut osc = OscState::decode_snapshot_v2(section(bytes, p, OSC_STATE)?)
        .map_err(map_osc_import_error)?;
    if p.version.required_features() & HOST_THEME_FEATURE != 0 {
        if logical_count(p, HOST_THEME_STATE)? != 1 {
            return Err(TerminalError::MalformedSnapshot);
        }
        osc.restore_theme_state(section(bytes, p, HOST_THEME_STATE)?)?;
    }
    if osc.has_extended_colors() != (p.version.required_features() & COLOR_EXTENSIONS_FEATURE != 0)
    {
        return Err(TerminalError::IncompatibleSnapshot);
    }
    let graphemes = decode_graphemes(section(bytes, p, GRAPHEME_TABLE)?, p.version)?;
    let renditions = decode_renditions(section(bytes, p, RENDITION_TABLE)?, p.version)?;
    let mut macros = crate::macros::MacroStore::default();
    if p.version.required_features() & MACROS_FEATURE != 0 {
        for (id, bytes) in scan_macros(bytes, p)? {
            let mut owned = Vec::new();
            owned
                .try_reserve_exact(bytes.len())
                .map_err(|_| TerminalError::LimitExceeded)?;
            owned.extend_from_slice(bytes);
            macros.install(id, owned, false)?;
        }
    }
    let mut sgr_stack = SgrStack::default();
    if p.version.has_sgr_stack() {
        for (mask, id) in scan_sgr_stack(bytes, p, renditions.len())? {
            sgr_stack.push(SavedRendition {
                mask,
                attributes: renditions[id as usize],
            });
        }
    }
    let main = decode_screen(
        section(bytes, p, PRIMARY_META)?,
        section(bytes, p, PRIMARY_ROWS)?,
        &graphemes,
        &renditions,
        p.version,
        true,
    )?;
    let alternate = decode_screen(
        section(bytes, p, ALTERNATE_META)?,
        section(bytes, p, ALTERNATE_ROWS)?,
        &graphemes,
        &renditions,
        p.version,
        false,
    )?;
    let mut pages = crate::pages::PageStore::default();
    if p.version.required_features() & PAGES_FEATURE != 0 {
        let records = page_records(bytes, p)?;
        pages.active = records.active;
        pages.visible = records.visible;
        pages.saved = records.saved;
        pages.coupled = records.coupled;
        for (id, meta, rows) in records.entries {
            pages.extra[usize::from(id - 2)] = Some(decode_screen(
                meta,
                rows,
                &graphemes,
                &renditions,
                p.version,
                false,
            )?);
        }
        pages.validate(columns, rows, p.version.width_profile())?;
        if !pages.has_state() {
            return Err(TerminalError::MalformedSnapshot);
        }
    }
    if main
        .row_metadata
        .iter()
        .chain(&alternate.row_metadata)
        .any(|metadata| metadata.sealed_revision > core.sequence)
    {
        return Err(TerminalError::MalformedSnapshot);
    }
    main.validate()?;
    alternate.validate()?;
    if core.active_alternate && !modes.left_right_margin {
        main.validate_reflow_target(columns)?;
    }
    let repeat_source = if let Some(id) = core.repeat_grapheme_id {
        let grapheme = &graphemes[id as usize];
        let mut combining = String::new();
        combining
            .try_reserve_exact(grapheme.combining.len())
            .map_err(|_| TerminalError::LimitExceeded)?;
        combining.push_str(&grapheme.combining);
        Some(RepeatSource {
            character: grapheme.character,
            combining,
            width: grapheme.width,
            glyph: grapheme.glyph,
        })
    } else {
        None
    };
    let graphics = if p.version.required_features() & GRAPHICS_FEATURE != 0 {
        if logical_count(p, GRAPHICS_STATE)? != 1 {
            return Err(TerminalError::MalformedSnapshot);
        }
        Some(crate::graphics_state::GraphicsState::decode_with_admission(
            section(bytes, p, GRAPHICS_STATE)?,
            |placements| {
                for page in 1..=7 {
                    if !placements
                        .iter()
                        .any(|placement| placement.anchor.page == page)
                    {
                        continue;
                    }
                    let screen = match page {
                        1 => Some(&main),
                        7 => Some(&alternate),
                        _ => pages.extra[usize::from(page - 2)].as_ref(),
                    }
                    .ok_or(TerminalError::MalformedSnapshot)?;
                    let index = crate::terminal_graphics::GraphicsAnchorIndex::new(screen)?;
                    for placement in placements
                        .iter()
                        .filter(|placement| placement.anchor.page == page)
                    {
                        if index
                            .locate(placement.anchor.logical_line, placement.anchor.cell_offset)
                            .is_none()
                        {
                            return Err(TerminalError::MalformedSnapshot);
                        }
                    }
                }
                Ok(())
            },
        )?)
    } else {
        None
    };
    let terminal = Terminal {
        columns,
        rows,
        main,
        alternate,
        pages,
        host_geometry: decode_host_geometry(bytes, p)?,
        shell_integration: decode_shell_state(bytes, p)?,
        host_signals: decode_host_signals(bytes, p)?,
        graphics,
        legacy_projection: None,
        kitty_transfer: crate::kitty_graphics::TransferDecoder::default(),
        broker_replay: None,
        active_alternate: core.active_alternate,
        cursor_visible: core.cursor_visible,
        cursor_shape: core.cursor_shape,
        cursor_blinking: core.cursor_blinking,
        modes,
        main_keyboard_protocol,
        alternate_keyboard_protocol,
        margins,
        tabs,
        charset,
        user_preferred_charset,
        main_saved_charset,
        alternate_saved_charset,
        osc,
        next_source_sequence: core.sequence,
        logical_time_micros: core.time,
        synchronized_output_deadline_micros: None,
        output_profile: if p.version.has_standalone_c1_output_profile() {
            OutputProfile::Utf8WithStandaloneC1
        } else {
            OutputProfile::Utf8
        },
        parser: ParserState::Ground,
        utf8: Utf8Decoder::default(),
        string_utf8: StringUtf8Boundary::default(),
        repeat_source,
        sgr_stack,
        macros,
        macro_execution: crate::macros::MacroExecution::default(),
        failed_closed: false,
        diagnostics: crate::diagnostic::OutputDiagnostics::default(),
    };
    validate_active_screen_wrap_state(terminal.active_screen(), terminal.modes, terminal.margins)?;
    terminal.validate_graphics_anchors()?;
    Ok(terminal)
}

fn decode_graphemes(
    bytes: &[u8],
    version: SnapshotVersion,
) -> Result<Vec<GraphemeOwned>, TerminalError> {
    let mut r = Reader::new(bytes);
    let count = r.u32()? as usize;
    r.u32()?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| TerminalError::LimitExceeded)?;
    for _ in 0..count {
        let value = read_grapheme(&mut r, version)?;
        let mut combining = String::new();
        combining
            .try_reserve_exact(value.combining.len())
            .map_err(|_| TerminalError::LimitExceeded)?;
        combining.push_str(value.combining);
        result.push(GraphemeOwned {
            character: value.character,
            combining,
            width: value.width,
            glyph: value.glyph,
        });
    }
    Ok(result)
}

fn decode_renditions(
    bytes: &[u8],
    version: SnapshotVersion,
) -> Result<Vec<Attributes>, TerminalError> {
    let mut r = Reader::new(bytes);
    let count = r.u32()? as usize;
    r.u32()?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| TerminalError::LimitExceeded)?;
    for _ in 0..count {
        result.push(decode_rendition_record(r.take(48)?, version)?);
    }
    Ok(result)
}

fn decode_screen(
    meta_bytes: &[u8],
    rows_bytes: &[u8],
    graphemes: &[GraphemeOwned],
    renditions: &[Attributes],
    version: SnapshotVersion,
    primary: bool,
) -> Result<Screen, TerminalError> {
    let meta = decode_meta(meta_bytes)?;
    let cell_count = (meta.stored_rows as usize)
        .checked_mul(meta.columns as usize)
        .ok_or(TerminalError::MalformedSnapshot)?;
    let mut cells = Vec::new();
    let mut line_renditions = Vec::new();
    let mut soft_wrapped = Vec::new();
    let mut meaningful_end = Vec::new();
    let mut row_metadata = Vec::new();
    cells
        .try_reserve_exact(cell_count)
        .map_err(|_| TerminalError::LimitExceeded)?;
    soft_wrapped
        .try_reserve_exact(meta.stored_rows as usize)
        .map_err(|_| TerminalError::LimitExceeded)?;
    meaningful_end
        .try_reserve_exact(meta.stored_rows as usize)
        .map_err(|_| TerminalError::LimitExceeded)?;
    row_metadata
        .try_reserve_exact(meta.stored_rows as usize)
        .map_err(|_| TerminalError::LimitExceeded)?;
    let mut r = Reader::new(rows_bytes);
    let rows = r.u32()?;
    require_zero(r.u32()?)?;
    let retained_primary = primary && version.has_retained_primary_scrollback();
    let scrollback_generation_rows = if retained_primary { r.u64()? } else { 0 };
    for _ in 0..rows {
        let logical_line_id = r.u64()?;
        let logical_cell_offset = r.u32()?;
        meaningful_end.push(r.u32()?);
        let flags = r.u32()?;
        let runs = r.u32()?;
        let sealed_revision = if retained_primary { r.u64()? } else { 0 };
        soft_wrapped.push(flags & 1 != 0);
        line_renditions.push(
            crate::LineRendition::from_wire((flags >> 2) & 3)
                .ok_or(TerminalError::MalformedSnapshot)?,
        );
        if version.has_history() {
            row_metadata.push(RowMetadata {
                logical_line_id,
                logical_cell_offset,
                sealed_revision,
                flags: if flags & 2 != 0 {
                    ROW_FLAG_HEAD_TRUNCATED
                } else {
                    0
                },
            });
        }
        for _ in 0..runs {
            let grapheme = &graphemes[r.u32()? as usize];
            let attributes = renditions[r.u32()? as usize];
            let repeat = r.u32()?;
            let metadata = r.u32()?;
            let protected = metadata & 1 != 0;
            let hyperlink = match metadata >> 1 {
                0 => None,
                reference => Some(HyperlinkRef::new(reference)),
            };
            for _ in 0..repeat {
                let mut combining = String::new();
                combining
                    .try_reserve_exact(grapheme.combining.len())
                    .map_err(|_| TerminalError::LimitExceeded)?;
                combining.push_str(&grapheme.combining);
                cells.push(Cell {
                    character: grapheme.character,
                    combining,
                    width: grapheme.width,
                    continuation: false,
                    attributes,
                    protected,
                    hyperlink,
                    glyph: grapheme.glyph,
                });
                if grapheme.width == 2 {
                    let continuation =
                        Cell::continuation_from(cells.last().expect("wide lead was just appended"));
                    cells.push(continuation);
                }
            }
        }
    }
    let (row_metadata, next_logical_line_id) = if version.has_history() {
        (row_metadata, meta.next_logical_line_id)
    } else {
        Screen::metadata_from_visible_rows(&soft_wrapped, &meaningful_end)
    };
    let requested_history_rows = if primary {
        u32::try_from(meta.requested_history_rows).map_err(|_| TerminalError::MalformedSnapshot)?
    } else {
        0
    };
    Ok(Screen {
        width_profile: version.width_profile(),
        columns: meta.columns,
        rows: meta.visible_rows,
        cells,
        soft_wrapped,
        line_renditions,
        meaningful_end,
        row_metadata,
        next_logical_line_id,
        scrollback_generation_rows,
        graphic_scrolls: None,
        graphic_reflow_anchors: Vec::new(),
        graphic_anchor_generation: 0,
        graphic_anchor_cache: None,
        storage: StoragePolicy::new(requested_history_rows, MAX_CELLS),
        cursor_row: meta.cursor_row,
        cursor_column: meta.cursor_column,
        wrap_pending: meta.cursor_flags & 1 != 0,
        attributes: renditions[meta.attributes_id as usize],
        protected: meta.cursor_flags & 2 != 0,
        saved: SavedCursor {
            row: meta.saved_row,
            column: meta.saved_column,
            wrap_pending: meta.saved_flags & 1 != 0,
            attributes: renditions[meta.saved_attributes_id as usize],
            protected: meta.saved_flags & 8 != 0,
            origin_mode: meta.saved_flags & 2 != 0,
            auto_wrap: meta.saved_flags & 4 != 0,
        },
    })
}

fn decode_bool(value: u8) -> Result<bool, TerminalError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(TerminalError::MalformedSnapshot),
    }
}

fn require_zero<T: Default + PartialEq>(value: T) -> Result<(), TerminalError> {
    if value == T::default() {
        Ok(())
    } else {
        Err(TerminalError::MalformedSnapshot)
    }
}

fn align8(value: usize) -> Option<usize> {
    value.checked_add(7).map(|v| v & !7)
}

struct Writer {
    bytes: Vec<u8>,
    limit: usize,
    failed: bool,
}

impl Writer {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            limit: MAX_SNAPSHOT_BYTES,
            failed: false,
        }
    }
    fn reserve(&mut self, count: usize) -> bool {
        if self.failed
            || self
                .bytes
                .len()
                .checked_add(count)
                .is_none_or(|v| v > self.limit)
            || self.bytes.try_reserve(count).is_err()
        {
            self.failed = true;
            false
        } else {
            true
        }
    }
    fn bytes(&mut self, values: &[u8]) {
        if self.reserve(values.len()) {
            self.bytes.extend_from_slice(values);
        }
    }
    fn zeros(&mut self, count: usize) {
        if self.reserve(count) {
            self.bytes.resize(self.bytes.len() + count, 0);
        }
    }
    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }
    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }
    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }
    fn align4(&mut self) {
        self.zeros((4 - self.len() % 4) % 4);
    }
    fn align8(&mut self) {
        self.zeros((8 - self.len() % 8) % 8);
    }
    fn len(&self) -> usize {
        self.bytes.len()
    }
    fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
    fn patch_bytes(&mut self, offset: usize, value: &[u8]) {
        self.bytes[offset..offset + value.len()].copy_from_slice(value);
    }
    fn patch_u16(&mut self, offset: usize, value: u16) {
        self.patch_bytes(offset, &value.to_le_bytes());
    }
    fn patch_u32(&mut self, offset: usize, value: u32) {
        self.patch_bytes(offset, &value.to_le_bytes());
    }
    fn patch_u64(&mut self, offset: usize, value: u64) {
        self.patch_bytes(offset, &value.to_le_bytes());
    }
    fn check(&self) -> Result<(), TerminalError> {
        if self.failed {
            Err(TerminalError::LimitExceeded)
        } else {
            Ok(())
        }
    }
    fn finish(self) -> Result<Vec<u8>, TerminalError> {
        if self.failed {
            Err(TerminalError::LimitExceeded)
        } else {
            Ok(self.bytes)
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], TerminalError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(TerminalError::MalformedSnapshot)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(TerminalError::MalformedSnapshot)?;
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, TerminalError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, TerminalError> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| TerminalError::MalformedSnapshot)?,
        ))
    }
    fn u32(&mut self) -> Result<u32, TerminalError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| TerminalError::MalformedSnapshot)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, TerminalError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| TerminalError::MalformedSnapshot)?,
        ))
    }
    fn zero_padding_to4(&mut self) -> Result<(), TerminalError> {
        let count = (4 - self.offset % 4) % 4;
        if self.take(count)?.iter().any(|b| *b != 0) {
            Err(TerminalError::MalformedSnapshot)
        } else {
            Ok(())
        }
    }
    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_snapshot_rejects_bad_ids_lengths_and_feature_claims() {
        let mut terminal = Terminal::new(8, 2).unwrap();
        terminal.macros.install(0, b"ABC".to_vec(), false).unwrap();
        terminal.macros.install(63, b"XYZ".to_vec(), false).unwrap();
        let bytes = export(&terminal).unwrap();
        let entry = directory_index_for_tag(&bytes, MACROS_STATE);
        let offset = directory_value(&bytes, entry, 8) as usize;
        for (relative, value) in [(0, 0u32), (0, 65), (4, 7), (8, 64), (12, u32::MAX), (19, 0)] {
            assert_mutation_error(&bytes, TerminalError::MalformedSnapshot, |copy| {
                copy[offset + relative..offset + relative + 4]
                    .copy_from_slice(&value.to_le_bytes());
            });
        }
        assert_mutation_error(&bytes, TerminalError::IncompatibleSnapshot, |copy| {
            copy[64..72]
                .copy_from_slice(&(u64_at(&bytes, 64) & !(MACROS_FEATURE as u64)).to_le_bytes());
        });
        let copy = import(&bytes).unwrap();
        assert_eq!(copy.macros, terminal.macros);
    }

    fn refresh_crc(bytes: &mut [u8]) {
        let payload = crc32c::checksum(&bytes[HEADER_SIZE..]);
        bytes[96..100].copy_from_slice(&payload.to_le_bytes());
        let header = crc32c::checksum(&bytes[..100]);
        bytes[100..104].copy_from_slice(&header.to_le_bytes());
    }

    #[test]
    fn pages_snapshot_checks_ids_geometry_counts_resources_and_feature() {
        let mut terminal = Terminal::new(12, 4).unwrap();
        terminal
            .apply_event(crate::Event::Output {
                source_sequence: 0,
                bytes: b"\x1b[2 P\x1b[31mA\x1b[4 P\x1b#6B\x1b[?64l\x1b[1 P",
            })
            .unwrap();
        let bytes = export(&terminal).unwrap();
        let restored = import(&bytes).unwrap();
        assert_eq!(export(&restored).unwrap(), bytes);
        let index = directory_index_for_tag(&bytes, PAGES_STATE);
        let offset = directory_value(&bytes, index, 8) as usize;
        for (position, value) in [
            (offset, 0),
            (offset + 1, 7),
            (offset + 2, 3),
            (offset + 3, 1),
            (offset + 3, 2),
        ] {
            assert_mutation_error(&bytes, TerminalError::MalformedSnapshot, |b| {
                b[position] = value
            });
        }
        for (position, value) in [
            (offset + 4, 6_u32),
            (offset + 8, 1),
            (offset + 16, 1),
            (offset + 20, u32::MAX),
            (offset + 24, 13),
            (offset + 24 + 12, 1),
        ] {
            assert_mutation_error(&bytes, TerminalError::MalformedSnapshot, |b| {
                b[position..position + 4].copy_from_slice(&value.to_le_bytes())
            });
        }
        assert_mutation_error(&bytes, TerminalError::IncompatibleSnapshot, |b| {
            let features = u64_at(b, 64) & !(PAGES_FEATURE as u64);
            b[64..72].copy_from_slice(&features.to_le_bytes());
        });
    }

    fn assert_mutation_error(
        bytes: &[u8],
        expected: TerminalError,
        mutate: impl FnOnce(&mut [u8]),
    ) {
        let mut malformed = bytes.to_vec();
        mutate(&mut malformed);
        refresh_crc(&mut malformed);
        assert_eq!(import(&malformed), Err(expected));
    }

    fn directory_value(bytes: &[u8], index: usize, field: usize) -> u64 {
        let start = HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + field;
        u64::from_le_bytes(bytes[start..start + 8].try_into().expect("u64 field"))
    }

    #[test]
    fn sgr_stack_snapshot_validates_count_mask_references_and_required_feature() {
        let mut terminal = Terminal::new(8, 2).unwrap();
        terminal
            .apply_event(crate::Event::Output {
                source_sequence: 0,
                bytes: b"\x1b[4:3m\x1b[#{\x1b[0m",
            })
            .unwrap();
        let bytes = export(&terminal).unwrap();
        assert_eq!(import(&bytes).unwrap(), terminal);
        let index = directory_index_for_tag(&bytes, SGR_STACK_STATE);
        let offset = directory_value(&bytes, index, 8) as usize;
        for (position, value) in [
            (offset, 0),
            (offset, 11),
            (offset + 4, 1),
            (offset + 8, 1 << 6),
            (offset + 8, 3),
            (offset + 12, u32::MAX),
        ] {
            let mut corrupt = bytes.clone();
            corrupt[position..position + 4].copy_from_slice(&value.to_le_bytes());
            refresh_crc(&mut corrupt);
            assert_eq!(import(&corrupt), Err(TerminalError::MalformedSnapshot));
        }
        let mut incompatible = bytes.clone();
        incompatible[64..72]
            .copy_from_slice(&(u64_at(&bytes, 64) & !(SGR_STACK_FEATURE as u64)).to_le_bytes());
        refresh_crc(&mut incompatible);
        assert_eq!(
            import(&incompatible),
            Err(TerminalError::IncompatibleSnapshot)
        );
        for history in [false, true] {
            for keyboard in [false, true] {
                let mut t = if history {
                    wrapped_history_terminal()
                } else {
                    Terminal::new(8, 2).unwrap()
                };
                if keyboard {
                    t.main_keyboard_protocol.push(7);
                }
                t.sgr_stack.push(SavedRendition {
                    attributes: Attributes::default(),
                    mask: 1,
                });
                let snapshot = export(&t).unwrap();
                assert_eq!(import(&snapshot).unwrap(), t);
                assert_eq!(export(&import(&snapshot).unwrap()).unwrap(), snapshot);
            }
        }
    }

    fn directory_index_for_tag(bytes: &[u8], tag: u32) -> usize {
        let count = u32_at(bytes, 40) as usize;
        (0..count)
            .find(|index| u32_at(bytes, HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE) == tag)
            .expect("snapshot directory tag")
    }

    fn set_directory_u16(bytes: &mut [u8], index: usize, field: usize, value: u16) {
        let start = HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + field;
        bytes[start..start + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn set_directory_u32(bytes: &mut [u8], index: usize, field: usize, value: u32) {
        let start = HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + field;
        bytes[start..start + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn set_directory_u64(bytes: &mut [u8], index: usize, field: usize, value: u64) {
        let start = HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + field;
        bytes[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn u16_at(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("u16 field"))
    }

    fn u32_at(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("u32 field"))
    }

    fn u64_at(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("u64 field"))
    }

    fn row_record_offsets(bytes: &[u8], directory_index: usize) -> Vec<usize> {
        let section = directory_value(bytes, directory_index, 8) as usize;
        let section_version = u16_at(
            bytes,
            HEADER_SIZE + directory_index * DIRECTORY_ENTRY_SIZE + 6,
        );
        let retained_primary = section_version == 3;
        let rows = u32_at(bytes, section) as usize;
        let mut offset = section + if retained_primary { 16 } else { 8 };
        let mut result = Vec::with_capacity(rows);
        for _ in 0..rows {
            result.push(offset);
            let runs = u32_at(bytes, offset + 20) as usize;
            offset += if retained_primary { 32 } else { 24 } + runs * 16;
        }
        assert_eq!(
            offset,
            section + directory_value(bytes, directory_index, 16) as usize
        );
        result
    }

    #[test]
    fn scaled_rows_reject_bad_flags_bounds_and_old_reader_before_install() {
        let mut terminal = Terminal::new(8, 2).unwrap();
        terminal
            .apply_event(crate::Event::Output {
                source_sequence: 0,
                bytes: b"\x1b[2;1H\x1b#6ABC",
            })
            .unwrap();
        let bytes = export(&terminal).unwrap();
        assert_eq!(u16_at(&bytes, 10), 13);
        let records = row_record_offsets(&bytes, 9);
        for (offset, value) in [
            (records[1] + 16, 16_u32),
            (records[1] + 16, 5),
            (records[1] + 12, 5),
            (records[0] + 16, 1),
        ] {
            let mut invalid = bytes.clone();
            invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            refresh_crc(&mut invalid);
            assert_eq!(import(&invalid), Err(TerminalError::MalformedSnapshot));
        }
        let mut old_reader = bytes;
        old_reader[10..12].copy_from_slice(&12_u16.to_le_bytes());
        old_reader[12..14].copy_from_slice(&12_u16.to_le_bytes());
        refresh_crc(&mut old_reader);
        assert_eq!(
            import(&old_reader),
            Err(TerminalError::IncompatibleSnapshot)
        );
    }

    fn wrapped_history_terminal() -> Terminal {
        let mut terminal = Terminal::new_with_options(
            4,
            2,
            0,
            crate::TerminalOptions {
                history_capacity_rows: 4,
            },
        )
        .expect("history terminal");
        terminal
            .apply_event(crate::Event::Output {
                source_sequence: 0,
                bytes: b"abcdefghi\x07",
            })
            .expect("wrapped output");
        assert_eq!(terminal.main.viewport_top(), 1);
        terminal
    }

    fn next_unique_narrow_scalar(next: &mut u32) -> char {
        loop {
            let value = *next;
            *next = next
                .checked_add(1)
                .expect("Unicode scalar search is bounded");
            if let Some(character) = char::from_u32(value)
                && character != ' '
                && !character.is_control()
                && scalar_width(character) == 1
            {
                return character;
            }
        }
    }

    fn fill_unique_cells(
        screen: &mut Screen,
        count: usize,
        next_scalar: &mut u32,
        next_rendition: &mut u32,
    ) {
        assert!(count <= screen.cells.len());
        for (index, cell) in screen.cells.iter_mut().take(count).enumerate() {
            cell.character = next_unique_narrow_scalar(next_scalar);
            let value = *next_rendition;
            *next_rendition = next_rendition
                .checked_add(1)
                .expect("rendition sequence is bounded");
            cell.attributes = Attributes {
                foreground: Color::Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8),
                ..Attributes::default()
            };
            let row = index / screen.columns as usize;
            let column = index % screen.columns as usize;
            screen.meaningful_end[row] = (column + 1) as u32;
        }
    }

    #[test]
    fn new_envelope_is_canonical_and_old_magic_is_rejected() {
        let terminal = Terminal::new(8, 2).expect("terminal");
        let bytes = export(&terminal).expect("snapshot");
        assert_eq!(&bytes[..8], MAGIC);
        assert_eq!(import(&bytes).expect("import"), terminal);
        assert_eq!(
            export(&import(&bytes).expect("import")).expect("export"),
            bytes
        );
        let mut old = bytes;
        old[..8].copy_from_slice(b"TLSNPV2\0");
        assert_eq!(import(&old), Err(TerminalError::IncompatibleSnapshot));
    }

    #[test]
    fn runtime_row_anchors_leave_v2_0_bytes_unchanged_and_rebuild_deterministically() {
        let mut terminal = Terminal::new(8, 2).expect("terminal");
        let baseline = export(&terminal).expect("baseline snapshot");
        assert_eq!(u16_at(&baseline, 10), MINOR_V2_0);
        assert_eq!(u16_at(&baseline, 12), MINOR_V2_0);
        assert_eq!(u64_at(&baseline, 64), REQUIRED_FEATURES_V2_0 as u64);
        for index in 0..SECTION_COUNT {
            assert_eq!(
                u16_at(&baseline, HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + 6),
                1
            );
        }
        for index in [8, 10] {
            let meta = directory_value(&baseline, index, 8) as usize;
            assert_eq!(&baseline[meta + 48..meta + 64], &[0; 16]);
        }

        terminal.main.row_metadata[0].logical_line_id = 100;
        terminal.main.row_metadata[0].sealed_revision = 23;
        terminal.main.row_metadata[0].flags = 1;
        terminal.main.row_metadata[1].logical_line_id = 101;
        terminal.main.next_logical_line_id = 900;
        terminal.main.validate().expect("runtime anchors are valid");

        assert_eq!(
            export(&terminal).expect("metadata-independent snapshot"),
            baseline
        );
        let restored = import(&baseline).expect("legacy metadata reconstruction");
        let (expected, next_logical_line_id) =
            Screen::metadata_from_visible_rows(&[false, false], &[0, 0]);
        assert_eq!(restored.main.row_metadata, expected);
        assert_eq!(restored.main.next_logical_line_id, next_logical_line_id);
        assert_eq!(restored.main.requested_history_rows(), 0);
        assert_eq!(export(&restored).expect("legacy re-export"), baseline);
    }

    #[test]
    fn retained_history_selects_v2_10_and_roundtrips_all_rows_canonically() {
        let terminal = wrapped_history_terminal();
        let snapshot = export(&terminal).expect("V2.10 snapshot");

        assert_eq!(u16_at(&snapshot, 10), MINOR_V2_10);
        assert_eq!(u16_at(&snapshot, 12), MINOR_V2_10);
        assert_eq!(u64_at(&snapshot, 64), REQUIRED_FEATURES_V2_10 as u64);
        for index in 0..SECTION_COUNT {
            let expected = match index {
                8 | 9 => 3,
                10 | 11 => 2,
                _ => 1,
            };
            assert_eq!(
                u16_at(&snapshot, HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + 6),
                expected,
                "section {index} version"
            );
        }

        let primary_meta = directory_value(&snapshot, 8, 8) as usize;
        let alternate_meta = directory_value(&snapshot, 10, 8) as usize;
        assert_eq!(
            u64_at(&snapshot, primary_meta + 48),
            terminal.main.next_logical_line_id
        );
        assert_eq!(u64_at(&snapshot, primary_meta + 56), 4);
        assert_eq!(
            u64_at(&snapshot, alternate_meta + 48),
            terminal.alternate.next_logical_line_id
        );
        assert_eq!(u64_at(&snapshot, alternate_meta + 56), 0);
        assert_eq!(u32_at(&snapshot, primary_meta + 8), 3);
        assert_eq!(u32_at(&snapshot, primary_meta + 12), 1);
        assert_eq!(directory_value(&snapshot, 9, 24), 3);
        assert_eq!(directory_value(&snapshot, 11, 24), 2);

        let restored = import(&snapshot).expect("V2.10 restore");
        assert_eq!(restored.main.viewport_top(), terminal.main.viewport_top());
        assert_eq!(restored.main.stored_rows(), terminal.main.stored_rows());
        assert_eq!(restored.main.requested_history_rows(), 4);
        assert_eq!(restored.main.cells, terminal.main.cells);
        assert_eq!(
            restored.main.next_logical_line_id,
            terminal.main.next_logical_line_id
        );
        assert_eq!(restored.main.row_metadata, terminal.main.row_metadata);
        assert_eq!(restored.scrollback_info(), terminal.scrollback_info());
        assert_eq!(
            export(&restored).expect("canonical V2.10 re-export"),
            snapshot
        );

        let mut continued = restored;
        continued
            .apply_event(crate::Event::Output {
                source_sequence: 1,
                bytes: b"jklm",
            })
            .expect("restored capacity captures later output");
        assert!(continued.main.viewport_top() > 0);
        assert!(continued.main.viewport_top() <= 4);
    }

    #[test]
    fn v2_10_preserves_a_partially_evicted_soft_wrapped_head() {
        let mut terminal = Terminal::new_with_options(
            4,
            2,
            0,
            crate::TerminalOptions {
                history_capacity_rows: 1,
            },
        )
        .expect("history terminal");
        terminal
            .apply_event(crate::Event::Output {
                source_sequence: 0,
                bytes: b"abcdefghijklm",
            })
            .expect("evict wrapped logical head");
        assert_eq!(terminal.main.viewport_top(), 1);
        assert!(terminal.main.row_metadata[0].is_head_truncated());
        assert!(terminal.main.row_metadata[0].logical_cell_offset > 0);

        let snapshot = export(&terminal).expect("V2.10 truncated-head snapshot");
        let restored = import(&snapshot).expect("restore truncated head");
        assert_eq!(restored.main.row_metadata, terminal.main.row_metadata);
        assert_eq!(restored.main.cells, terminal.main.cells);
        assert_eq!(restored.scrollback_info(), terminal.scrollback_info());
        assert_eq!(export(&restored).expect("canonical re-export"), snapshot);
    }

    #[test]
    fn repeat_source_selects_the_two_exact_v2_2_profiles_and_roundtrips() {
        for (history_capacity_rows, expected_features, expected_screen_version) in [
            (0, REQUIRED_FEATURES_V2_2, 1),
            (4, REQUIRED_FEATURES_V2_2_HISTORY, 2),
        ] {
            let mut terminal = Terminal::new_with_options(
                8,
                2,
                0,
                crate::TerminalOptions {
                    history_capacity_rows,
                },
            )
            .expect("terminal");
            terminal
                .apply_event(crate::Event::Output {
                    source_sequence: 0,
                    bytes: b"A",
                })
                .expect("repeat source");

            let snapshot = export(&terminal).expect("V2.2 snapshot");
            assert_eq!(u16_at(&snapshot, 10), MINOR_V2_2);
            assert_eq!(u16_at(&snapshot, 12), MINOR_V2_2);
            assert_eq!(u64_at(&snapshot, 64), expected_features as u64);
            assert_eq!(u16_at(&snapshot, HEADER_SIZE + 6), 2);
            for index in 8..=11 {
                assert_eq!(
                    u16_at(&snapshot, HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + 6),
                    expected_screen_version,
                    "screen section {index}"
                );
            }
            let core = directory_value(&snapshot, 0, 8) as usize;
            assert_eq!(u16_at(&snapshot, core + 26), 1);
            assert_eq!(u32_at(&snapshot, core + 28), 1);

            let restored = import(&snapshot).expect("V2.2 restore");
            assert_eq!(restored, terminal);
            assert_eq!(export(&restored).expect("V2.2 re-export"), snapshot);
        }
    }

    #[test]
    fn cursor_style_selects_all_four_v2_3_profiles_and_preserves_all_legacy_profiles() {
        let profiles = [
            (
                false,
                false,
                false,
                MINOR_V2_0,
                REQUIRED_FEATURES_V2_0,
                1,
                1,
            ),
            (true, false, false, MINOR_V2_1, REQUIRED_FEATURES_V2_1, 1, 2),
            (false, true, false, MINOR_V2_2, REQUIRED_FEATURES_V2_2, 2, 1),
            (
                true,
                true,
                false,
                MINOR_V2_2,
                REQUIRED_FEATURES_V2_2_HISTORY,
                2,
                2,
            ),
            (false, false, true, MINOR_V2_3, REQUIRED_FEATURES_V2_3, 3, 1),
            (
                true,
                false,
                true,
                MINOR_V2_3,
                REQUIRED_FEATURES_V2_3_HISTORY,
                3,
                2,
            ),
            (
                false,
                true,
                true,
                MINOR_V2_3,
                REQUIRED_FEATURES_V2_3_REPEAT,
                3,
                1,
            ),
            (
                true,
                true,
                true,
                MINOR_V2_3,
                REQUIRED_FEATURES_V2_3_HISTORY_REPEAT,
                3,
                2,
            ),
        ];
        for (history, repeat, styled, minor, features, core_version, screen_version) in profiles {
            let mut terminal = Terminal::new_with_options(
                8,
                2,
                0,
                crate::TerminalOptions {
                    history_capacity_rows: if history { 4 } else { 0 },
                },
            )
            .expect("profile terminal");
            if repeat {
                terminal.repeat_source = Some(RepeatSource::new('A'));
            }
            if styled {
                terminal.cursor_shape = CursorShape::Underline;
            }

            let snapshot = export(&terminal).expect("profile snapshot");
            terminal.synchronized_output_deadline_micros = Some(1_000_000);
            assert_eq!(export(&terminal), Err(TerminalError::PresentationDeferred));
            terminal.synchronized_output_deadline_micros = None;
            assert_eq!(
                export(&terminal).expect("profile bytes after inactive transition"),
                snapshot,
                "history={history} repeat={repeat} styled={styled}"
            );
            assert_eq!(u16_at(&snapshot, 10), minor);
            assert_eq!(u16_at(&snapshot, 12), minor);
            assert_eq!(u64_at(&snapshot, 64), features as u64);
            assert_eq!(u16_at(&snapshot, HEADER_SIZE + 6), core_version);
            for index in 8..=11 {
                assert_eq!(
                    u16_at(&snapshot, HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + 6),
                    screen_version,
                    "history={history} repeat={repeat} styled={styled} section={index}"
                );
            }
            let restored = import(&snapshot).expect("profile restore");
            assert_eq!(restored.synchronized_output_deadline_micros, None);
            assert_eq!(restored, terminal);
            assert_eq!(export(&restored).expect("profile closure"), snapshot);
        }
    }

    #[test]
    fn v2_3_core_flags_cover_every_cursor_style_and_reset_downgrades_canonically() {
        let default =
            export(&Terminal::new(8, 2).expect("default terminal")).expect("default V2.0 snapshot");
        for (shape, blinking, expected_flags) in [
            (CursorShape::Block, false, 0x0002),
            (CursorShape::Underline, true, 0x0004),
            (CursorShape::Underline, false, 0x0006),
            (CursorShape::Bar, true, 0x0008),
            (CursorShape::Bar, false, 0x000a),
        ] {
            let mut terminal = Terminal::new(8, 2).expect("terminal");
            terminal.cursor_shape = shape;
            terminal.cursor_blinking = blinking;
            let snapshot = export(&terminal).expect("V2.3 snapshot");
            let core = directory_value(&snapshot, 0, 8) as usize;
            assert_eq!(u16_at(&snapshot, core + 26), expected_flags);
            let restored = import(&snapshot).expect("V2.3 restore");
            assert_eq!(restored.cursor_shape, shape);
            assert_eq!(restored.cursor_blinking, blinking);
            assert_eq!(export(&restored).expect("V2.3 closure"), snapshot);

            terminal.cursor_shape = CursorShape::Block;
            terminal.cursor_blinking = true;
            assert_eq!(export(&terminal).expect("canonical downgrade"), default);
        }

        let mut terminal = Terminal::new(8, 2).expect("terminal");
        terminal.cursor_shape = CursorShape::Underline;
        let snapshot = export(&terminal).expect("V2.3 snapshot");
        let mut restored = import(&snapshot).expect("V2.3 restore");
        restored
            .apply_event(crate::Event::Output {
                source_sequence: 0,
                bytes: b"\x1b[0 q",
            })
            .expect("reset restored cursor style");
        let downgraded = export(&restored).expect("downgraded restored snapshot");
        assert_eq!(u16_at(&downgraded, 10), MINOR_V2_0);
        assert_eq!(u64_at(&downgraded, 64), REQUIRED_FEATURES_V2_0 as u64);
        assert_eq!(u16_at(&downgraded, HEADER_SIZE + 6), 1);
    }

    #[test]
    fn extended_sgr_selects_all_eight_exact_v2_4_profiles_and_roundtrips() {
        let profiles = [
            (false, false, false, REQUIRED_FEATURES_V2_4, 1, 1),
            (true, false, false, REQUIRED_FEATURES_V2_4_HISTORY, 1, 2),
            (false, true, false, REQUIRED_FEATURES_V2_4_REPEAT, 2, 1),
            (
                true,
                true,
                false,
                REQUIRED_FEATURES_V2_4_HISTORY_REPEAT,
                2,
                2,
            ),
            (false, false, true, REQUIRED_FEATURES_V2_4_CURSOR, 3, 1),
            (
                true,
                false,
                true,
                REQUIRED_FEATURES_V2_4_HISTORY_CURSOR,
                3,
                2,
            ),
            (
                false,
                true,
                true,
                REQUIRED_FEATURES_V2_4_REPEAT_CURSOR,
                3,
                1,
            ),
            (
                true,
                true,
                true,
                REQUIRED_FEATURES_V2_4_HISTORY_REPEAT_CURSOR,
                3,
                2,
            ),
        ];
        for (history, repeat, cursor, features, core_version, screen_version) in profiles {
            let mut terminal = Terminal::new_with_options(
                8,
                2,
                0,
                crate::TerminalOptions {
                    history_capacity_rows: if history { 4 } else { 0 },
                },
            )
            .expect("profile terminal");
            terminal.main.attributes.faint = true;
            if repeat {
                terminal.repeat_source = Some(RepeatSource::new('A'));
            }
            if cursor {
                terminal.cursor_shape = CursorShape::Bar;
            }

            let snapshot = export(&terminal).expect("V2.4 profile snapshot");
            assert_eq!(u16_at(&snapshot, 10), MINOR_V2_4);
            assert_eq!(u16_at(&snapshot, 12), MINOR_V2_4);
            assert_eq!(u64_at(&snapshot, 64), features as u64);
            assert_eq!(u16_at(&snapshot, HEADER_SIZE + 6), core_version);
            assert_eq!(
                u16_at(&snapshot, HEADER_SIZE + 7 * DIRECTORY_ENTRY_SIZE + 6),
                2
            );
            for index in 8..=11 {
                assert_eq!(
                    u16_at(&snapshot, HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + 6),
                    screen_version,
                    "history={history} repeat={repeat} cursor={cursor} section={index}"
                );
            }
            for index in [1, 2, 3, 4, 5, 6] {
                assert_eq!(
                    u16_at(&snapshot, HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + 6),
                    1,
                    "unchanged section {index}"
                );
            }

            let restored = import(&snapshot).expect("V2.4 profile restore");
            assert_eq!(restored, terminal);
            assert_eq!(export(&restored).expect("V2.4 profile closure"), snapshot);
        }
    }

    #[test]
    fn win32_input_mode_selects_all_sixteen_exact_v2_5_profiles_and_roundtrips() {
        for option_bits in 0_u8..16 {
            let history = option_bits & 1 != 0;
            let repeat = option_bits & 2 != 0;
            let cursor = option_bits & 4 != 0;
            let extended_sgr = option_bits & 8 != 0;
            let mut terminal = Terminal::new_with_options(
                8,
                2,
                0,
                crate::TerminalOptions {
                    history_capacity_rows: if history { 4 } else { 0 },
                },
            )
            .expect("V2.5 profile terminal");
            assert!(terminal.modes.set_private_mode(9001, true));
            if repeat {
                terminal.repeat_source = Some(RepeatSource::new('A'));
            }
            if cursor {
                terminal.cursor_shape = CursorShape::Bar;
            }
            if extended_sgr {
                terminal.main.attributes.faint = true;
            }

            let snapshot = export(&terminal).expect("V2.5 profile snapshot");
            let expected_features = REQUIRED_FEATURES_V2_5
                | if history { HISTORY_ANCHORS_FEATURE } else { 0 }
                | if repeat { REP_SOURCE_FEATURE } else { 0 }
                | if cursor { CURSOR_STYLE_FEATURE } else { 0 }
                | if extended_sgr {
                    EXTENDED_SGR_FEATURE
                } else {
                    0
                };
            assert_eq!(u16_at(&snapshot, 10), MINOR_V2_5);
            assert_eq!(u16_at(&snapshot, 12), MINOR_V2_5);
            assert_eq!(u64_at(&snapshot, 64), expected_features as u64);
            assert_eq!(u16_at(&snapshot, HEADER_SIZE + DIRECTORY_ENTRY_SIZE + 6), 2);
            assert_eq!(
                u16_at(&snapshot, HEADER_SIZE + 6),
                if cursor {
                    3
                } else if repeat {
                    2
                } else {
                    1
                }
            );
            assert_eq!(
                u16_at(&snapshot, HEADER_SIZE + 7 * DIRECTORY_ENTRY_SIZE + 6),
                if extended_sgr { 2 } else { 1 }
            );
            for index in 8..=11 {
                assert_eq!(
                    u16_at(&snapshot, HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + 6),
                    if history { 2 } else { 1 },
                    "option bits {option_bits:#06b}, screen section {index}"
                );
            }

            let mode_section = directory_value(&snapshot, 1, 8) as usize;
            let count = u32_at(&snapshot, mode_section) as usize;
            let win32_record = (0..count)
                .map(|index| mode_section + 8 + index * 8)
                .find(|&record| u16_at(&snapshot, record) == 9001 && snapshot[record + 2] == 1)
                .expect("canonical 9001 mode record");
            assert_eq!(snapshot[win32_record + 3], 1);

            let restored = import(&snapshot).expect("V2.5 profile restore");
            assert!(restored.modes.win32_input_mode);
            assert_eq!(restored, terminal);
            assert_eq!(export(&restored).expect("V2.5 profile closure"), snapshot);
        }
    }

    #[test]
    fn color_scheme_reporting_mode_selects_v2_9_roundtrips_and_downgrades() {
        let mut terminal = Terminal::new(8, 2).expect("terminal");
        assert!(terminal.modes.set_private_mode(2031, true));

        let snapshot = export(&terminal).expect("mode 2031 snapshot");
        assert_eq!(u16_at(&snapshot, 10), MINOR_V2_9);
        assert_eq!(u16_at(&snapshot, 12), MINOR_V2_9);
        assert_eq!(u64_at(&snapshot, 64), REQUIRED_FEATURES_V2_9 as u64);
        assert_eq!(u16_at(&snapshot, HEADER_SIZE + DIRECTORY_ENTRY_SIZE + 6), 4);

        let mode_section = directory_value(&snapshot, 1, 8) as usize;
        let count = u32_at(&snapshot, mode_section) as usize;
        let color_scheme_record = (0..count)
            .map(|index| mode_section + 8 + index * 8)
            .find(|&record| u16_at(&snapshot, record) == 2031 && snapshot[record + 2] == 1)
            .expect("canonical 2031 mode record");
        assert_eq!(snapshot[color_scheme_record + 3], 1);

        let mut restored = import(&snapshot).expect("mode 2031 restore");
        assert!(restored.modes.color_scheme_reporting);
        assert_eq!(export(&restored).expect("canonical re-export"), snapshot);

        assert!(restored.modes.set_private_mode(2031, false));
        let downgraded = export(&restored).expect("mode 2031 reset snapshot");
        assert_eq!(u16_at(&downgraded, 10), MINOR_V2_0);

        assert!(restored.modes.set_private_mode(2027, true));
        let grapheme_snapshot = export(&restored).expect("mode 2027 snapshot");
        assert_eq!(u16_at(&grapheme_snapshot, 10), MINOR_V2_9);
        let grapheme_restored = import(&grapheme_snapshot).expect("mode 2027 restore");
        assert!(grapheme_restored.modes.grapheme_clustering);
        assert!(!grapheme_restored.modes.color_scheme_reporting);
    }

    #[test]
    fn win32_input_mode_reset_and_hard_reset_downgrade_to_the_legacy_profile() {
        let mut terminal = Terminal::new(8, 2).expect("terminal");
        let baseline = export(&terminal).expect("legacy baseline");

        assert!(terminal.modes.set_private_mode(9001, true));
        let enabled = export(&terminal).expect("V2.5 snapshot");
        assert_eq!(u16_at(&enabled, 10), MINOR_V2_5);
        assert_eq!(u64_at(&enabled, 64), REQUIRED_FEATURES_V2_5 as u64);

        assert!(terminal.modes.set_private_mode(9001, false));
        assert_eq!(
            export(&terminal).expect("DEC mode reset snapshot"),
            baseline
        );

        assert!(terminal.modes.set_private_mode(9001, true));
        terminal.modes.hard_reset();
        assert_eq!(export(&terminal).expect("hard reset snapshot"), baseline);
    }

    #[test]
    fn win32_input_mode_profile_rejects_corruption_mixes_and_legacy_injection() {
        let mut terminal = Terminal::new(8, 2).expect("terminal");
        assert!(terminal.modes.set_private_mode(9001, true));
        let snapshot = export(&terminal).expect("V2.5 snapshot");
        let mode_section = directory_value(&snapshot, 1, 8) as usize;
        let count = u32_at(&snapshot, mode_section) as usize;
        let win32_record = (0..count)
            .map(|index| mode_section + 8 + index * 8)
            .find(|&record| u16_at(&snapshot, record) == 9001)
            .expect("9001 record");

        let mut corrupted = snapshot.clone();
        corrupted[win32_record + 3] ^= 1;
        assert_eq!(import(&corrupted), Err(TerminalError::MalformedSnapshot));

        let mut wrong_features = snapshot.clone();
        wrong_features[64..72].copy_from_slice(&(REQUIRED_FEATURES_V2_0 as u64).to_le_bytes());
        refresh_crc(&mut wrong_features);
        assert_eq!(
            import(&wrong_features),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let mut wrong_minor = snapshot.clone();
        wrong_minor[10..12].copy_from_slice(&MINOR_V2_4.to_le_bytes());
        wrong_minor[12..14].copy_from_slice(&MINOR_V2_4.to_le_bytes());
        refresh_crc(&mut wrong_minor);
        assert_eq!(
            import(&wrong_minor),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let mut wrong_section_version = snapshot.clone();
        set_directory_u16(&mut wrong_section_version, 1, 6, 1);
        refresh_crc(&mut wrong_section_version);
        assert_eq!(
            import(&wrong_section_version),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let mut missing_mode = snapshot.clone();
        missing_mode[win32_record..win32_record + 2].copy_from_slice(&1004_u16.to_le_bytes());
        refresh_crc(&mut missing_mode);
        assert_eq!(import(&missing_mode), Err(TerminalError::MalformedSnapshot));

        let mut noncanonical_mode_order = snapshot.clone();
        noncanonical_mode_order[win32_record..win32_record + 2]
            .copy_from_slice(&6_u16.to_le_bytes());
        refresh_crc(&mut noncanonical_mode_order);
        assert_eq!(
            import(&noncanonical_mode_order),
            Err(TerminalError::MalformedSnapshot)
        );

        for (feature, section_versions) in [
            (
                HISTORY_ANCHORS_FEATURE,
                &[(8, 2_u16), (9, 2), (10, 2), (11, 2)][..],
            ),
            (REP_SOURCE_FEATURE, &[(0, 2_u16)][..]),
            (CURSOR_STYLE_FEATURE, &[(0, 3_u16)][..]),
            (EXTENDED_SGR_FEATURE, &[(7, 2_u16)][..]),
        ] {
            let mut false_feature_claim = snapshot.clone();
            false_feature_claim[64..72]
                .copy_from_slice(&((REQUIRED_FEATURES_V2_5 | feature) as u64).to_le_bytes());
            for &(index, section_version) in section_versions {
                set_directory_u16(&mut false_feature_claim, index, 6, section_version);
            }
            refresh_crc(&mut false_feature_claim);
            assert_eq!(
                import(&false_feature_claim),
                Err(TerminalError::MalformedSnapshot),
                "false optional feature claim {feature:#x}"
            );
        }

        let legacy = export(&Terminal::new(8, 2).expect("legacy terminal")).expect("V2.0");
        let legacy_mode_section = directory_value(&legacy, 1, 8) as usize;
        let legacy_count = u32_at(&legacy, legacy_mode_section) as usize;
        let auto_wrap_record = (0..legacy_count)
            .map(|index| legacy_mode_section + 8 + index * 8)
            .find(|&record| {
                u16_at(&legacy, record) == 7 && legacy[record + 2] == 1 && legacy[record + 3] == 1
            })
            .expect("canonical auto-wrap record");
        let mut legacy_injection = legacy;
        legacy_injection[auto_wrap_record..auto_wrap_record + 2]
            .copy_from_slice(&9001_u16.to_le_bytes());
        legacy_injection[auto_wrap_record + 3] = 1;
        refresh_crc(&mut legacy_injection);
        assert_eq!(
            import(&legacy_injection),
            Err(TerminalError::MalformedSnapshot)
        );
    }

    #[test]
    fn extended_sgr_detection_covers_visible_cells_and_all_current_and_saved_attributes() {
        for position in 0..6 {
            let mut terminal = Terminal::new(4, 2).expect("terminal");
            let attributes = match position {
                0 => &mut terminal.main.attributes,
                1 => &mut terminal.main.saved.attributes,
                2 => &mut terminal.alternate.attributes,
                3 => &mut terminal.alternate.saved.attributes,
                4 => &mut terminal.main.cells[0].attributes,
                5 => &mut terminal.alternate.cells[0].attributes,
                _ => unreachable!(),
            };
            attributes.overline = true;
            let snapshot = export(&terminal).expect("extended state snapshot");
            assert_eq!(u16_at(&snapshot, 10), MINOR_V2_4, "position {position}");
        }

        let mut hidden = wrapped_history_terminal();
        assert!(hidden.main.viewport_top() > 0);
        hidden.main.cells[0].attributes.faint = true;
        let snapshot = export(&hidden).expect("hidden history is retained");
        assert_eq!(u16_at(&snapshot, 10), MINOR_V2_10);
        assert_eq!(
            u16_at(&snapshot, HEADER_SIZE + 7 * DIRECTORY_ENTRY_SIZE + 6),
            2
        );
    }

    #[test]
    fn v2_4_rendition_v2_encodes_all_fields_and_styles_with_canonical_closure() {
        let styles = [
            UnderlineStyle::None,
            UnderlineStyle::Single,
            UnderlineStyle::Double,
            UnderlineStyle::Curly,
            UnderlineStyle::Dotted,
            UnderlineStyle::Dashed,
        ];
        let mut terminal = Terminal::new(8, 2).expect("terminal");
        for (index, style) in styles.into_iter().enumerate() {
            terminal.main.cells[index].attributes = Attributes {
                underline_style: style,
                underline_color: Color::Indexed(index as u8),
                ..Attributes::default()
            };
        }
        terminal.main.meaningful_end[0] = styles.len() as u32;
        terminal.main.attributes = Attributes {
            bold: true,
            faint: true,
            italic: true,
            underline_style: UnderlineStyle::Dashed,
            blink: true,
            inverse: true,
            conceal: true,
            strikethrough: true,
            overline: true,
            foreground: Color::Indexed(231),
            background: Color::Rgb(0x12, 0x34, 0x56),
            underline_color: Color::Rgb(0x65, 0x43, 0x21),
        };

        let snapshot = export(&terminal).expect("extended rendition snapshot");
        let rendition_offset = directory_value(&snapshot, 7, 8) as usize;
        let rendition_count = u32_at(&snapshot, rendition_offset) as usize;
        let expected = rendition_bytes(
            terminal.main.attributes,
            SnapshotVersion::V2_4 {
                history: false,
                repeat: false,
                cursor: false,
            },
        );
        let records = &snapshot[rendition_offset + 8..rendition_offset + 8 + rendition_count * 48];
        assert!(records.chunks_exact(48).any(|record| record == expected));
        assert_eq!(u32_at(&expected, 0), 0xff);
        assert_eq!(expected[4], 5);
        assert_eq!(&expected[32..], &[0; 16]);

        let restored = import(&snapshot).expect("extended rendition restore");
        assert_eq!(restored, terminal);
        assert_eq!(
            export(&restored).expect("extended rendition closure"),
            snapshot
        );
    }

    #[test]
    fn legacy_rendition_v1_maps_boolean_underline_and_keeps_extended_defaults() {
        let mut terminal = Terminal::new(4, 2).expect("terminal");
        terminal.main.attributes.underline_style = UnderlineStyle::Single;
        terminal.main.attributes.foreground = Color::Indexed(7);
        let snapshot = export(&terminal).expect("legacy rendition snapshot");
        assert_eq!(u16_at(&snapshot, 10), MINOR_V2_0);
        assert_eq!(
            u16_at(&snapshot, HEADER_SIZE + 7 * DIRECTORY_ENTRY_SIZE + 6),
            1
        );
        let mut expected = [0; 48];
        expected[4] = 1;
        expected[8] = 1;
        expected[12..16].copy_from_slice(&7_u32.to_le_bytes());
        let rendition_offset = directory_value(&snapshot, 7, 8) as usize;
        let count = u32_at(&snapshot, rendition_offset) as usize;
        assert_eq!(count, 2);
        assert_eq!(
            &snapshot[rendition_offset + 8 + 48..rendition_offset + 8 + 96],
            expected
        );

        let restored = import(&snapshot).expect("legacy rendition restore");
        assert_eq!(
            restored.main.attributes.underline_style,
            UnderlineStyle::Single
        );
        assert!(!restored.main.attributes.faint);
        assert!(!restored.main.attributes.overline);
        assert_eq!(restored.main.attributes.underline_color, Color::Default);
        assert_eq!(restored, terminal);
        assert_eq!(
            export(&restored).expect("legacy rendition closure"),
            snapshot
        );
    }

    #[test]
    fn v2_4_rejects_malicious_renditions_and_profile_version_mixes() {
        let mut terminal = Terminal::new(4, 2).expect("terminal");
        terminal.main.attributes.faint = true;
        let snapshot = export(&terminal).expect("V2.4 snapshot");
        let rendition_table = directory_value(&snapshot, 7, 8) as usize;
        assert_eq!(u32_at(&snapshot, rendition_table), 2);
        let extended_record = rendition_table + 8 + 48;

        for (name, offset, value) in [
            ("flags", extended_record + 1, 1_u8),
            ("style", extended_record + 4, 6),
            ("header reserved", extended_record + 5, 1),
            ("foreground", extended_record + 8, 3),
            ("background", extended_record + 16, 3),
            ("underline color", extended_record + 24, 3),
            ("tail reserved", extended_record + 32, 1),
        ] {
            let mut malformed = snapshot.clone();
            malformed[offset] = value;
            refresh_crc(&mut malformed);
            assert_eq!(
                import(&malformed),
                Err(TerminalError::MalformedSnapshot),
                "{name}"
            );
        }

        let mut rendition_v1 = snapshot.clone();
        set_directory_u16(&mut rendition_v1, 7, 6, 1);
        refresh_crc(&mut rendition_v1);
        assert_eq!(
            import(&rendition_v1),
            Err(TerminalError::IncompatibleSnapshot)
        );

        for (index, section_version) in [(0, 2_u16), (8, 2), (9, 2), (10, 2), (11, 2)] {
            let mut wrong_section_version = snapshot.clone();
            set_directory_u16(&mut wrong_section_version, index, 6, section_version);
            refresh_crc(&mut wrong_section_version);
            assert_eq!(
                import(&wrong_section_version),
                Err(TerminalError::IncompatibleSnapshot),
                "section {index}"
            );
        }

        let mut legacy_features = snapshot.clone();
        legacy_features[64..72].copy_from_slice(&(REQUIRED_FEATURES_V2_0 as u64).to_le_bytes());
        refresh_crc(&mut legacy_features);
        assert_eq!(
            import(&legacy_features),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let mut legacy_minor = snapshot.clone();
        legacy_minor[10..12].copy_from_slice(&MINOR_V2_3.to_le_bytes());
        legacy_minor[12..14].copy_from_slice(&MINOR_V2_3.to_le_bytes());
        refresh_crc(&mut legacy_minor);
        assert_eq!(
            import(&legacy_minor),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let legacy = export(&Terminal::new(4, 2).expect("legacy terminal")).expect("V2.0");
        let mut upgraded_rendition = legacy.clone();
        set_directory_u16(&mut upgraded_rendition, 7, 6, 2);
        refresh_crc(&mut upgraded_rendition);
        assert_eq!(
            import(&upgraded_rendition),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let mut upgraded_profile = legacy;
        upgraded_profile[10..12].copy_from_slice(&MINOR_V2_4.to_le_bytes());
        upgraded_profile[12..14].copy_from_slice(&MINOR_V2_4.to_le_bytes());
        refresh_crc(&mut upgraded_profile);
        assert_eq!(
            import(&upgraded_profile),
            Err(TerminalError::IncompatibleSnapshot)
        );
    }

    #[test]
    fn v2_3_rejects_core_flags_gid_and_profile_version_mixes() {
        let mut terminal = Terminal::new_with_options(
            8,
            2,
            0,
            crate::TerminalOptions {
                history_capacity_rows: 4,
            },
        )
        .expect("terminal");
        terminal.cursor_shape = CursorShape::Bar;
        terminal.repeat_source = Some(RepeatSource::new('A'));
        let snapshot = export(&terminal).expect("V2.3 full profile");
        let core = directory_value(&snapshot, 0, 8) as usize;
        let grapheme_count = directory_value(&snapshot, 6, 24) as u32;

        for flags in [0_u16, 1, 0x000c, 0x0019] {
            let mut malformed = snapshot.clone();
            malformed[core + 26..core + 28].copy_from_slice(&flags.to_le_bytes());
            refresh_crc(&mut malformed);
            assert_eq!(
                import(&malformed),
                Err(TerminalError::MalformedSnapshot),
                "flags {flags:#06x}"
            );
        }

        let mut missing_repeat_bit = snapshot.clone();
        missing_repeat_bit[core + 26..core + 28].copy_from_slice(&0x0008_u16.to_le_bytes());
        refresh_crc(&mut missing_repeat_bit);
        assert_eq!(
            import(&missing_repeat_bit),
            Err(TerminalError::MalformedSnapshot)
        );

        let mut bad_gid = snapshot.clone();
        bad_gid[core + 28..core + 32].copy_from_slice(&grapheme_count.to_le_bytes());
        refresh_crc(&mut bad_gid);
        assert_eq!(import(&bad_gid), Err(TerminalError::MalformedSnapshot));

        for core_version in [1_u16, 2] {
            let mut incompatible = snapshot.clone();
            set_directory_u16(&mut incompatible, 0, 6, core_version);
            refresh_crc(&mut incompatible);
            assert_eq!(
                import(&incompatible),
                Err(TerminalError::IncompatibleSnapshot)
            );
        }

        for index in 8..=11 {
            let mut wrong_screen_version = snapshot.clone();
            set_directory_u16(&mut wrong_screen_version, index, 6, 1);
            refresh_crc(&mut wrong_screen_version);
            assert_eq!(
                import(&wrong_screen_version),
                Err(TerminalError::IncompatibleSnapshot),
                "screen section {index}"
            );
        }

        for features in [REQUIRED_FEATURES_V2_3, REQUIRED_FEATURES_V2_3_REPEAT] {
            let mut wrong_profile = snapshot.clone();
            wrong_profile[64..72].copy_from_slice(&(features as u64).to_le_bytes());
            refresh_crc(&mut wrong_profile);
            assert_eq!(
                import(&wrong_profile),
                Err(TerminalError::IncompatibleSnapshot),
                "features {features:#x}"
            );
        }

        let mut repeat_feature_removed = snapshot.clone();
        repeat_feature_removed[64..72]
            .copy_from_slice(&(REQUIRED_FEATURES_V2_3_HISTORY as u64).to_le_bytes());
        refresh_crc(&mut repeat_feature_removed);
        assert_eq!(
            import(&repeat_feature_removed),
            Err(TerminalError::MalformedSnapshot)
        );

        let mut unused_repeat_entry = snapshot.clone();
        unused_repeat_entry[64..72]
            .copy_from_slice(&(REQUIRED_FEATURES_V2_3_HISTORY as u64).to_le_bytes());
        unused_repeat_entry[core + 26..core + 28].copy_from_slice(&0x0008_u16.to_le_bytes());
        unused_repeat_entry[core + 28..core + 32].copy_from_slice(&0_u32.to_le_bytes());
        refresh_crc(&mut unused_repeat_entry);
        assert_eq!(
            import(&unused_repeat_entry),
            Err(TerminalError::MalformedSnapshot)
        );

        let mut styled = Terminal::new(8, 2).expect("styled terminal");
        styled.cursor_shape = CursorShape::Underline;
        let mut default_style_v2_3 = export(&styled).expect("styled V2.3");
        let styled_core = directory_value(&default_style_v2_3, 0, 8) as usize;
        default_style_v2_3[styled_core + 26..styled_core + 28]
            .copy_from_slice(&0_u16.to_le_bytes());
        refresh_crc(&mut default_style_v2_3);
        assert_eq!(
            import(&default_style_v2_3),
            Err(TerminalError::MalformedSnapshot)
        );

        let legacy =
            export(&Terminal::new(8, 2).expect("legacy terminal")).expect("legacy snapshot");
        let mut upgraded_core = legacy;
        set_directory_u16(&mut upgraded_core, 0, 6, 3);
        refresh_crc(&mut upgraded_core);
        assert_eq!(
            import(&upgraded_core),
            Err(TerminalError::IncompatibleSnapshot)
        );
    }

    #[test]
    fn repeat_source_grapheme_table_handles_blank_dedup_and_repeat_only_entries() {
        let mut blank = Terminal::new(4, 2).expect("terminal");
        blank
            .apply_event(crate::Event::Output {
                source_sequence: 0,
                bytes: b" ",
            })
            .expect("blank repeat source");
        let blank_snapshot = export(&blank).expect("blank V2.2");
        let blank_core = directory_value(&blank_snapshot, 0, 8) as usize;
        assert_eq!(directory_value(&blank_snapshot, 6, 24), 1);
        assert_eq!(u32_at(&blank_snapshot, blank_core + 28), 0);
        let blank_restored = import(&blank_snapshot).expect("blank restore");
        assert_eq!(
            export(&blank_restored).expect("blank closure"),
            blank_snapshot
        );

        let mut deduplicated = Terminal::new(4, 2).expect("terminal");
        deduplicated
            .apply_event(crate::Event::Output {
                source_sequence: 0,
                bytes: b"A",
            })
            .expect("visible repeat source");
        let deduplicated_snapshot = export(&deduplicated).expect("deduplicated V2.2");
        assert_eq!(directory_value(&deduplicated_snapshot, 6, 24), 2);

        let mut repeat_only = deduplicated;
        repeat_only.main.cells[0] = Cell::default();
        repeat_only.main.meaningful_end[0] = 0;
        repeat_only
            .main
            .validate()
            .expect("repeat-only source terminal");
        let repeat_only_snapshot = export(&repeat_only).expect("repeat-only V2.2");
        assert_eq!(directory_value(&repeat_only_snapshot, 6, 24), 2);
        let restored = import(&repeat_only_snapshot).expect("repeat-only restore");
        assert_eq!(restored.repeat_source, repeat_only.repeat_source);
        assert_eq!(
            export(&restored).expect("repeat-only closure"),
            repeat_only_snapshot
        );
    }

    #[test]
    fn v2_2_rejects_core_profile_reference_and_canonical_table_violations() {
        let mut terminal = Terminal::new(8, 2).expect("terminal");
        terminal
            .apply_event(crate::Event::Output {
                source_sequence: 0,
                bytes: b"A",
            })
            .expect("repeat source");
        terminal.main.cells[0] = Cell::default();
        terminal.main.meaningful_end[0] = 0;
        let snapshot = export(&terminal).expect("repeat-only V2.2");
        let core = directory_value(&snapshot, 0, 8) as usize;
        let grapheme_table = directory_value(&snapshot, 6, 8) as usize;
        let grapheme_count = directory_value(&snapshot, 6, 24) as u32;

        for flags in [0_u16, 2] {
            let mut malformed = snapshot.clone();
            malformed[core + 26..core + 28].copy_from_slice(&flags.to_le_bytes());
            refresh_crc(&mut malformed);
            assert_eq!(import(&malformed), Err(TerminalError::MalformedSnapshot));
        }

        let mut out_of_range = snapshot.clone();
        out_of_range[core + 28..core + 32].copy_from_slice(&grapheme_count.to_le_bytes());
        refresh_crc(&mut out_of_range);
        assert_eq!(import(&out_of_range), Err(TerminalError::MalformedSnapshot));

        let mut unused_entry = snapshot.clone();
        unused_entry[core + 28..core + 32].copy_from_slice(&0_u32.to_le_bytes());
        refresh_crc(&mut unused_entry);
        assert_eq!(import(&unused_entry), Err(TerminalError::MalformedSnapshot));

        let mut wrong_width = snapshot.clone();
        wrong_width[grapheme_table + 18] = 2;
        refresh_crc(&mut wrong_width);
        assert_eq!(import(&wrong_width), Err(TerminalError::MalformedSnapshot));

        let mut invalid_utf8 = snapshot.clone();
        invalid_utf8[grapheme_table + 20] = 0xff;
        refresh_crc(&mut invalid_utf8);
        assert_eq!(import(&invalid_utf8), Err(TerminalError::MalformedSnapshot));

        let mut core_v1 = snapshot.clone();
        set_directory_u16(&mut core_v1, 0, 6, 1);
        refresh_crc(&mut core_v1);
        assert_eq!(import(&core_v1), Err(TerminalError::IncompatibleSnapshot));

        let mut history_feature_with_v1_screens = snapshot.clone();
        history_feature_with_v1_screens[64..72]
            .copy_from_slice(&(REQUIRED_FEATURES_V2_2_HISTORY as u64).to_le_bytes());
        refresh_crc(&mut history_feature_with_v1_screens);
        assert_eq!(
            import(&history_feature_with_v1_screens),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let mut screen_v2_without_history = snapshot.clone();
        set_directory_u16(&mut screen_v2_without_history, 8, 6, 2);
        refresh_crc(&mut screen_v2_without_history);
        assert_eq!(
            import(&screen_v2_without_history),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let mut legacy_minor = snapshot;
        legacy_minor[10..12].copy_from_slice(&MINOR_V2_1.to_le_bytes());
        legacy_minor[12..14].copy_from_slice(&MINOR_V2_1.to_le_bytes());
        refresh_crc(&mut legacy_minor);
        assert_eq!(
            import(&legacy_minor),
            Err(TerminalError::IncompatibleSnapshot)
        );
    }

    #[test]
    fn single_row_head_truncated_with_zero_offset_and_maximum_next_id_is_canonical() {
        let terminal = Terminal::new_with_options(
            4,
            1,
            0,
            crate::TerminalOptions {
                history_capacity_rows: 1,
            },
        )
        .expect("terminal");
        let mut snapshot = export(&terminal).expect("V2.1 snapshot");
        let meta = directory_value(&snapshot, 8, 8) as usize;
        let record = row_record_offsets(&snapshot, 9)[0];
        snapshot[meta + 48..meta + 56].copy_from_slice(&u64::MAX.to_le_bytes());
        snapshot[record + 16..record + 20].copy_from_slice(&2_u32.to_le_bytes());
        refresh_crc(&mut snapshot);

        let restored = import(&snapshot).expect("valid truncated single row");
        assert_eq!(restored.main.row_metadata[0].logical_cell_offset, 0);
        assert!(restored.main.row_metadata[0].is_head_truncated());
        assert_eq!(restored.main.next_logical_line_id, u64::MAX);
        assert_eq!(
            export(&restored).expect("canonical truncated re-export"),
            snapshot
        );
    }

    #[test]
    fn v2_10_retains_hidden_cells_tables_and_row_metadata() {
        let mut terminal = wrapped_history_terminal();
        let history_rows = terminal.main.viewport_top() as usize;
        assert!(history_rows > 0);
        let hidden_character = next_unique_narrow_scalar(&mut 0xe000);
        let hidden_attributes = Attributes {
            foreground: Color::Rgb(0x12, 0x34, 0x56),
            background: Color::Rgb(0x65, 0x43, 0x21),
            ..Attributes::default()
        };
        terminal.main.cells[0].character = hidden_character;
        terminal.main.cells[0].attributes = hidden_attributes;
        terminal.main.cells[0].protected = true;
        terminal
            .main
            .validate()
            .expect("hidden fixture remains valid");
        assert!(
            terminal
                .main
                .visible_cells()
                .iter()
                .all(|cell| cell.character != hidden_character
                    && cell.attributes != hidden_attributes)
        );

        let snapshot = export(&terminal).expect("retained-history snapshot");
        let preflight = preflight_a(&snapshot).expect("preflight");
        let graphemes = decode_graphemes(
            section(&snapshot, &preflight, GRAPHEME_TABLE).expect("grapheme section"),
            preflight.version,
        )
        .expect("grapheme table");
        let renditions = decode_renditions(
            section(&snapshot, &preflight, RENDITION_TABLE).expect("rendition section"),
            preflight.version,
        )
        .expect("rendition table");
        assert!(
            graphemes
                .iter()
                .any(|grapheme| grapheme.character == hidden_character)
        );
        assert!(renditions.contains(&hidden_attributes));
        assert_eq!(
            directory_value(&snapshot, 9, 24),
            terminal.main.stored_rows() as u64
        );

        let records = row_record_offsets(&snapshot, 9);
        let first_stored = &terminal.main.row_metadata[0];
        assert_eq!(u64_at(&snapshot, records[0]), first_stored.logical_line_id);
        assert_eq!(
            u32_at(&snapshot, records[0] + 8),
            first_stored.logical_cell_offset
        );
        assert_eq!(
            u64_at(&snapshot, records[0] + 24),
            first_stored.sealed_revision
        );
        assert_eq!(u32_at(&snapshot, records[0] + 16) & 2, 0);
        assert_eq!(u32_at(&snapshot, records[1] + 16) & 2, 0);

        let restored = import(&snapshot).expect("restore retained history");
        assert_eq!(restored.main.cells, terminal.main.cells);
        assert_eq!(restored.main.row_metadata, terminal.main.row_metadata);
        assert_eq!(restored.scrollback_info(), terminal.scrollback_info());
        assert_eq!(export(&restored).expect("canonical re-export"), snapshot);
        assert!(snapshot.len() <= MAX_CANONICAL_SNAPSHOT_BYTES);
    }

    #[test]
    fn every_short_envelope_is_malformed_before_compatibility_checks() {
        let bytes = [0_u8; HEADER_SIZE];
        for length in 0..HEADER_SIZE {
            assert_eq!(
                import(&bytes[..length]),
                Err(TerminalError::MalformedSnapshot),
                "length {length}"
            );
        }
        assert_eq!(
            import(&bytes),
            Err(TerminalError::IncompatibleSnapshot),
            "a complete header with the wrong magic is incompatible"
        );
    }

    #[test]
    fn crc_catches_header_and_payload_changes() {
        let terminal = Terminal::new(8, 2).expect("terminal");
        let bytes = export(&terminal).expect("snapshot");
        let mut header = bytes.clone();
        header[100] ^= 1;
        assert_eq!(import(&header), Err(TerminalError::MalformedSnapshot));
        let mut payload = bytes;
        let core = directory_value(&payload, 0, 8) as usize;
        payload[core] ^= 1;
        assert_eq!(import(&payload), Err(TerminalError::MalformedSnapshot));
    }

    #[test]
    fn directory_unknown_sections_missing_required_flags_and_versions_are_conservative() {
        let terminal = Terminal::new(8, 2).expect("terminal");
        let bytes = export(&terminal).expect("snapshot");

        for flags in [CRITICAL, 0] {
            let mut unknown = bytes.clone();
            set_directory_u32(&mut unknown, 0, 0, 0x7fff_ffff);
            set_directory_u16(&mut unknown, 0, 4, flags);
            refresh_crc(&mut unknown);
            assert_eq!(
                import(&unknown),
                Err(TerminalError::IncompatibleSnapshot),
                "unknown sections are rejected regardless of criticality"
            );
        }

        let mut missing_required = bytes.clone();
        missing_required[40..44].copy_from_slice(&((SECTION_COUNT - 1) as u32).to_le_bytes());
        refresh_crc(&mut missing_required);
        assert_eq!(
            import(&missing_required),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let mut flags = bytes.clone();
        set_directory_u16(&mut flags, 0, 4, CRITICAL | 2);
        refresh_crc(&mut flags);
        assert_eq!(import(&flags), Err(TerminalError::IncompatibleSnapshot));

        let mut version = bytes;
        set_directory_u16(&mut version, 0, 6, 2);
        refresh_crc(&mut version);
        assert_eq!(import(&version), Err(TerminalError::IncompatibleSnapshot));
    }

    #[test]
    fn version_feature_and_screen_section_version_mixes_are_rejected() {
        let v2_0 = export(&Terminal::new(8, 2).expect("terminal")).expect("V2.0 snapshot");
        let v2_10 = export(&wrapped_history_terminal()).expect("V2.10 snapshot");

        let mut minor_without_reader = v2_10.clone();
        minor_without_reader[12..14].copy_from_slice(&MINOR_V2_0.to_le_bytes());
        refresh_crc(&mut minor_without_reader);
        assert_eq!(
            import(&minor_without_reader),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let mut reader_without_minor = v2_0.clone();
        reader_without_minor[12..14].copy_from_slice(&MINOR_V2_1.to_le_bytes());
        refresh_crc(&mut reader_without_minor);
        assert_eq!(
            import(&reader_without_minor),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let mut v2_10_without_feature = v2_10.clone();
        v2_10_without_feature[64..72]
            .copy_from_slice(&(REQUIRED_FEATURES_V2_0 as u64).to_le_bytes());
        refresh_crc(&mut v2_10_without_feature);
        assert_eq!(
            import(&v2_10_without_feature),
            Err(TerminalError::IncompatibleSnapshot)
        );

        let mut v2_0_with_feature = v2_0.clone();
        v2_0_with_feature[64..72].copy_from_slice(&(REQUIRED_FEATURES_V2_1 as u64).to_le_bytes());
        refresh_crc(&mut v2_0_with_feature);
        assert_eq!(
            import(&v2_0_with_feature),
            Err(TerminalError::IncompatibleSnapshot)
        );

        for index in [8, 9, 10, 11] {
            let mut legacy_screen_section = v2_10.clone();
            set_directory_u16(&mut legacy_screen_section, index, 6, 1);
            refresh_crc(&mut legacy_screen_section);
            assert_eq!(
                import(&legacy_screen_section),
                Err(TerminalError::IncompatibleSnapshot),
                "V2.1 screen section {index} downgraded"
            );

            let mut upgraded_legacy_section = v2_0.clone();
            set_directory_u16(&mut upgraded_legacy_section, index, 6, 2);
            refresh_crc(&mut upgraded_legacy_section);
            assert_eq!(
                import(&upgraded_legacy_section),
                Err(TerminalError::IncompatibleSnapshot),
                "V2.0 screen section {index} upgraded"
            );
        }

        let mut upgraded_core = v2_10;
        set_directory_u16(&mut upgraded_core, 0, 6, 2);
        refresh_crc(&mut upgraded_core);
        assert_eq!(
            import(&upgraded_core),
            Err(TerminalError::IncompatibleSnapshot)
        );
    }

    #[test]
    fn v2_10_rejects_unreferenced_tables_without_narrowing_legacy_imports() {
        let mut legacy = Terminal::new_with_options(
            8,
            2,
            0,
            crate::TerminalOptions {
                history_capacity_rows: 4,
            },
        )
        .expect("legacy history terminal");
        legacy
            .apply_event(crate::Event::Output {
                source_sequence: 0,
                bytes: b"A\x07",
            })
            .expect("legacy visible cell");
        legacy.main.cells[0].protected = true;
        let mut legacy_unused = export(&legacy).expect("V2.1 snapshot");
        assert_eq!(u16_at(&legacy_unused, 10), MINOR_V2_1);
        let legacy_record = row_record_offsets(&legacy_unused, 9)[0];
        assert_ne!(u32_at(&legacy_unused, legacy_record + 24), 0);
        legacy_unused[legacy_record + 24..legacy_record + 28].copy_from_slice(&0_u32.to_le_bytes());
        refresh_crc(&mut legacy_unused);
        import(&legacy_unused).expect("legacy acceptance remains unchanged");

        let v2_10 = export(&wrapped_history_terminal()).expect("base V2.10 snapshot");
        assert_eq!(u64_at(&v2_10, 64), REQUIRED_FEATURES_V2_10 as u64);
        let first_record = row_record_offsets(&v2_10, 9)[0];
        let mut unused_grapheme = v2_10;
        assert_ne!(u32_at(&unused_grapheme, first_record + 32), 0);
        unused_grapheme[first_record + 32..first_record + 36].copy_from_slice(&0_u32.to_le_bytes());
        refresh_crc(&mut unused_grapheme);
        let preflight = preflight_a(&unused_grapheme).expect("valid mutated envelope");
        assert_eq!(
            preflight_b(&unused_grapheme, &preflight),
            Err(TerminalError::MalformedSnapshot)
        );

        let mut styled = wrapped_history_terminal();
        styled.main.cells[0].attributes.faint = true;
        let mut unused_rendition = export(&styled).expect("extended V2.10 snapshot");
        let first_record = row_record_offsets(&unused_rendition, 9)[0];
        assert_ne!(u32_at(&unused_rendition, first_record + 36), 0);
        unused_rendition[first_record + 36..first_record + 40]
            .copy_from_slice(&0_u32.to_le_bytes());
        refresh_crc(&mut unused_rendition);
        let preflight = preflight_a(&unused_rendition).expect("valid mutated envelope");
        assert_eq!(
            preflight_b(&unused_rendition, &preflight),
            Err(TerminalError::MalformedSnapshot)
        );
    }

    #[test]
    fn v2_0_reserved_anchors_and_v2_10_malicious_history_are_rejected() {
        let v2_0 = export(&Terminal::new(8, 2).expect("terminal")).expect("V2.0 snapshot");
        let v2_0_meta = directory_value(&v2_0, 8, 8) as usize;
        let v2_0_records = row_record_offsets(&v2_0, 9);
        for (name, mutate) in [
            ("V2.0 next logical id", (v2_0_meta + 48, 8_usize, 1_u64)),
            ("V2.0 requested history", (v2_0_meta + 56, 8_usize, 1_u64)),
            ("V2.0 row logical id", (v2_0_records[0], 8_usize, 1_u64)),
            (
                "V2.0 row logical offset",
                (v2_0_records[0] + 8, 4_usize, 1_u64),
            ),
            ("V2.0 row head flag", (v2_0_records[0] + 16, 4_usize, 2_u64)),
        ] {
            let mut corrupted = v2_0.clone();
            let (offset, width, value) = mutate;
            corrupted[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
            refresh_crc(&mut corrupted);
            assert_eq!(
                import(&corrupted),
                Err(TerminalError::MalformedSnapshot),
                "{name}"
            );
        }

        let v2_10 = export(&wrapped_history_terminal()).expect("V2.10 snapshot");
        let primary_meta = directory_value(&v2_10, 8, 8) as usize;
        let alternate_meta = directory_value(&v2_10, 10, 8) as usize;
        let records = row_record_offsets(&v2_10, 9);

        for capacity in [0_u64, u64::from(MAX_HISTORY_ROWS) + 1] {
            let mut corrupted = v2_10.clone();
            corrupted[primary_meta + 56..primary_meta + 64]
                .copy_from_slice(&capacity.to_le_bytes());
            refresh_crc(&mut corrupted);
            assert_eq!(
                import(&corrupted),
                Err(TerminalError::MalformedSnapshot),
                "primary history capacity {capacity}"
            );
        }

        let mut alternate_capacity = v2_10.clone();
        alternate_capacity[alternate_meta + 56..alternate_meta + 64]
            .copy_from_slice(&1_u64.to_le_bytes());
        refresh_crc(&mut alternate_capacity);
        assert_eq!(
            import(&alternate_capacity),
            Err(TerminalError::MalformedSnapshot)
        );

        let mut zero_next_id = v2_10.clone();
        zero_next_id[primary_meta + 48..primary_meta + 56].copy_from_slice(&0_u64.to_le_bytes());
        refresh_crc(&mut zero_next_id);
        assert_eq!(import(&zero_next_id), Err(TerminalError::MalformedSnapshot));

        let mut reserved_flag = v2_10.clone();
        reserved_flag[records[0] + 16..records[0] + 20].copy_from_slice(&7_u32.to_le_bytes());
        refresh_crc(&mut reserved_flag);
        assert_eq!(
            import(&reserved_flag),
            Err(TerminalError::MalformedSnapshot)
        );

        let mut later_head = v2_10.clone();
        let second_flags = u32_at(&later_head, records[1] + 16) | 2;
        later_head[records[1] + 16..records[1] + 20].copy_from_slice(&second_flags.to_le_bytes());
        refresh_crc(&mut later_head);
        assert_eq!(import(&later_head), Err(TerminalError::MalformedSnapshot));

        let mut wrong_offset = v2_10.clone();
        let second_offset = u32_at(&wrong_offset, records[1] + 8).wrapping_add(1);
        wrong_offset[records[1] + 8..records[1] + 12].copy_from_slice(&second_offset.to_le_bytes());
        refresh_crc(&mut wrong_offset);
        assert_eq!(import(&wrong_offset), Err(TerminalError::MalformedSnapshot));

        for (name, offset, value) in [
            ("stored rows omit history", primary_meta + 8, 2_u32),
            ("wrong viewport top", primary_meta + 12, 0_u32),
        ] {
            let mut corrupted = v2_10.clone();
            corrupted[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            refresh_crc(&mut corrupted);
            assert_eq!(
                import(&corrupted),
                Err(TerminalError::MalformedSnapshot),
                "{name}"
            );
        }
    }

    #[test]
    fn v2_10_future_seal_is_rejected_by_allocation_free_preflight() {
        let mut future_seal = export(&wrapped_history_terminal()).expect("V2.10 snapshot");
        let first_record = row_record_offsets(&future_seal, 9)[0];
        future_seal[first_record + 24..first_record + 32].copy_from_slice(&u64::MAX.to_le_bytes());
        refresh_crc(&mut future_seal);

        let preflight = preflight_a(&future_seal).expect("valid future-seal envelope");
        assert_eq!(
            preflight_b(&future_seal, &preflight),
            Err(TerminalError::MalformedSnapshot)
        );
        assert_eq!(import(&future_seal), Err(TerminalError::MalformedSnapshot));
    }

    #[test]
    fn overlarge_requested_history_is_canonicalized_to_the_wire_limit() {
        let terminal = Terminal::new_with_options(
            8,
            2,
            0,
            crate::TerminalOptions {
                history_capacity_rows: u32::MAX,
            },
        )
        .expect("terminal");
        let snapshot = export(&terminal).expect("bounded V2.1 snapshot");
        let meta = directory_value(&snapshot, 8, 8) as usize;
        assert_eq!(u64_at(&snapshot, meta + 56), u64::from(MAX_HISTORY_ROWS));
        let restored = import(&snapshot).expect("bounded restore");
        assert_eq!(restored.main.requested_history_rows(), MAX_HISTORY_ROWS);
        assert_eq!(export(&restored).expect("bounded re-export"), snapshot);
    }

    #[test]
    fn section_lengths_reject_truncation_and_integer_overflow() {
        let terminal = Terminal::new(8, 2).expect("terminal");
        let bytes = export(&terminal).expect("snapshot");

        let mut truncated = bytes.clone();
        set_directory_u64(&mut truncated, 0, 16, 31);
        refresh_crc(&mut truncated);
        assert_eq!(import(&truncated), Err(TerminalError::MalformedSnapshot));

        let mut overflow = bytes;
        set_directory_u64(&mut overflow, 0, 16, u64::MAX);
        refresh_crc(&mut overflow);
        assert_eq!(import(&overflow), Err(TerminalError::MalformedSnapshot));
    }

    #[test]
    fn directory_overlap_duplicate_hole_and_nonzero_padding_are_rejected() {
        let terminal = Terminal::new(8, 2).expect("terminal");
        let bytes = export(&terminal).expect("snapshot");

        let mut duplicate = bytes.clone();
        duplicate[HEADER_SIZE + DIRECTORY_ENTRY_SIZE..HEADER_SIZE + DIRECTORY_ENTRY_SIZE + 4]
            .copy_from_slice(&CORE_STATE.to_le_bytes());
        refresh_crc(&mut duplicate);
        assert_eq!(import(&duplicate), Err(TerminalError::IncompatibleSnapshot));

        let mut noncanonical_order = bytes.clone();
        set_directory_u32(&mut noncanonical_order, 0, 0, MODE_BANK);
        set_directory_u32(&mut noncanonical_order, 1, 0, CORE_STATE);
        refresh_crc(&mut noncanonical_order);
        assert_eq!(
            import(&noncanonical_order),
            Err(TerminalError::IncompatibleSnapshot)
        );

        for delta in [-8_i64, 8] {
            let mut displaced = bytes.clone();
            let field = HEADER_SIZE + DIRECTORY_ENTRY_SIZE + 8;
            let original = directory_value(&displaced, 1, 8);
            let changed = original.checked_add_signed(delta).expect("bounded offset");
            displaced[field..field + 8].copy_from_slice(&changed.to_le_bytes());
            refresh_crc(&mut displaced);
            assert_eq!(import(&displaced), Err(TerminalError::MalformedSnapshot));
        }

        let mut padding = bytes;
        let charset_offset = directory_value(&padding, 4, 8) as usize;
        let charset_length = directory_value(&padding, 4, 16) as usize;
        padding[charset_offset + charset_length] = 1;
        refresh_crc(&mut padding);
        assert_eq!(import(&padding), Err(TerminalError::MalformedSnapshot));
    }

    #[test]
    fn unknown_feature_and_noncanonical_tables_and_runs_are_rejected() {
        let terminal = Terminal::new(8, 2).expect("terminal");
        let bytes = export(&terminal).expect("snapshot");

        let mut feature = bytes.clone();
        feature[66] |= 0x20;
        refresh_crc(&mut feature);
        assert_eq!(import(&feature), Err(TerminalError::IncompatibleSnapshot));

        let mut table = bytes.clone();
        let grapheme_offset = directory_value(&table, 6, 8) as usize;
        table[grapheme_offset + 12] = b'!';
        refresh_crc(&mut table);
        assert_eq!(import(&table), Err(TerminalError::MalformedSnapshot));

        let mut run = bytes;
        let rows_offset = directory_value(&run, 9, 8) as usize;
        run[rows_offset + 32..rows_offset + 36].copy_from_slice(&u32::MAX.to_le_bytes());
        refresh_crc(&mut run);
        assert_eq!(import(&run), Err(TerminalError::MalformedSnapshot));
    }

    #[test]
    fn modify_other_keys_mode_records_roundtrip_and_default_bytes_remain_canonical() {
        let mut terminal = Terminal::new(8, 2).expect("terminal");
        let default = export(&terminal).expect("default snapshot");
        terminal.modes.modify_other_keys = ModifyOtherKeys::Level1;
        terminal.modes.reset_modify_other_keys();
        assert_eq!(export(&terminal).expect("reset snapshot"), default);

        for mode in [
            ModifyOtherKeys::Level1,
            ModifyOtherKeys::Level2,
            ModifyOtherKeys::Level3,
        ] {
            terminal.modes.modify_other_keys = mode;
            let bytes = export(&terminal).expect("mode snapshot");
            let section = directory_value(&bytes, 1, 8) as usize;
            let count =
                u32::from_le_bytes(bytes[section..section + 4].try_into().expect("mode count"))
                    as usize;
            let record = (0..count)
                .map(|index| section + 8 + index * 8)
                .find(|&record| bytes[record + 2] == 3)
                .expect("modifyOtherKeys mode record");
            assert_eq!(
                u16::from_le_bytes(
                    bytes[record..record + 2]
                        .try_into()
                        .expect("mode parameter")
                ),
                mode.parameter()
            );
            assert_eq!(bytes[record + 3], 1);

            let restored = import(&bytes).expect("mode restore");
            assert_eq!(restored.modes.modify_other_keys, mode);
            assert_eq!(export(&restored).expect("canonical mode re-export"), bytes);
        }
    }

    #[test]
    fn modify_other_keys_mode_records_reject_invalid_and_duplicate_state() {
        let mut terminal = Terminal::new(8, 2).expect("terminal");
        terminal.modes.modify_other_keys = ModifyOtherKeys::Level2;
        let bytes = export(&terminal).expect("mode snapshot");
        let section = directory_value(&bytes, 1, 8) as usize;
        let count = u32::from_le_bytes(bytes[section..section + 4].try_into().expect("mode count"))
            as usize;
        let records: Vec<_> = (0..count).map(|index| section + 8 + index * 8).collect();
        let mode_record = records
            .iter()
            .copied()
            .find(|&record| bytes[record + 2] == 3)
            .expect("modifyOtherKeys record");
        let extent_record = records
            .iter()
            .copied()
            .find(|&record| bytes[record + 2] == 2)
            .expect("attribute extent record");

        for parameter in [0_u16, 4] {
            let mut invalid = bytes.clone();
            invalid[mode_record..mode_record + 2].copy_from_slice(&parameter.to_le_bytes());
            refresh_crc(&mut invalid);
            assert_eq!(import(&invalid), Err(TerminalError::MalformedSnapshot));
        }

        let mut disabled_record = bytes.clone();
        disabled_record[mode_record + 3] = 0;
        refresh_crc(&mut disabled_record);
        assert_eq!(
            import(&disabled_record),
            Err(TerminalError::MalformedSnapshot)
        );

        let mut duplicate = bytes;
        duplicate[extent_record..extent_record + 2].copy_from_slice(&1_u16.to_le_bytes());
        duplicate[extent_record + 2] = 3;
        refresh_crc(&mut duplicate);
        assert_eq!(import(&duplicate), Err(TerminalError::MalformedSnapshot));
    }

    #[test]
    fn run_metadata_rejects_reserved_high_bits_and_invalid_hyperlink_references() {
        let terminal = Terminal::new(8, 2).expect("terminal");
        let bytes = export(&terminal).expect("snapshot");
        let rows = directory_value(&bytes, 9, 8) as usize;

        let mut reserved = bytes.clone();
        reserved[rows + 44..rows + 48].copy_from_slice(&(1_u32 << 31).to_le_bytes());
        refresh_crc(&mut reserved);
        assert_eq!(import(&reserved), Err(TerminalError::MalformedSnapshot));

        let mut dangling_export = Terminal::new(8, 2).expect("terminal");
        dangling_export.main.cells[0].hyperlink = Some(HyperlinkRef::new(1));
        dangling_export.main.meaningful_end[0] = 1;
        assert_eq!(
            export(&dangling_export),
            Err(TerminalError::MalformedSnapshot)
        );

        let mut linked = Terminal::new(8, 2).expect("terminal");
        linked
            .apply_event(crate::Event::Output {
                source_sequence: 0,
                bytes: b"\x1b]8;;https://example.test\x1b\\x\x1b]8;;\x1b\\",
            })
            .expect("create one hyperlink");
        let mut out_of_range = export(&linked).expect("linked snapshot");
        let linked_rows = directory_value(&out_of_range, 9, 8) as usize;
        out_of_range[linked_rows + 44..linked_rows + 48]
            .copy_from_slice(&(2_u32 << 1).to_le_bytes());
        refresh_crc(&mut out_of_range);
        assert_eq!(import(&out_of_range), Err(TerminalError::MalformedSnapshot));
    }

    #[test]
    fn kitty_keyboard_state_selects_all_thirty_two_v2_6_profiles_and_downgrades_exactly() {
        for dimensions in 0_u8..32 {
            let history = dimensions & 1 != 0;
            let repeat = dimensions & 2 != 0;
            let cursor = dimensions & 4 != 0;
            let extended_sgr = dimensions & 8 != 0;
            let win32_input = dimensions & 16 != 0;
            let mut terminal = Terminal::new_with_options(
                8,
                2,
                0,
                crate::TerminalOptions {
                    history_capacity_rows: u32::from(history) * 4,
                },
            )
            .expect("profile terminal");
            if repeat {
                terminal.repeat_source = Some(RepeatSource::new('R'));
            }
            if cursor {
                terminal.cursor_shape = CursorShape::Underline;
            }
            if extended_sgr {
                terminal.main.attributes.faint = true;
            }
            terminal.modes.win32_input_mode = win32_input;

            let legacy = export(&terminal).expect("legacy profile");
            assert_eq!(u32_at(&legacy, 40), SECTION_COUNT as u32);
            terminal.main_keyboard_protocol.push(0);
            let snapshot = export(&terminal).expect("V2.6 profile");
            let expected_features = REQUIRED_FEATURES_V2_6
                | if history { HISTORY_ANCHORS_FEATURE } else { 0 }
                | if repeat { REP_SOURCE_FEATURE } else { 0 }
                | if cursor { CURSOR_STYLE_FEATURE } else { 0 }
                | if extended_sgr {
                    EXTENDED_SGR_FEATURE
                } else {
                    0
                }
                | if win32_input {
                    WIN32_INPUT_MODE_FEATURE
                } else {
                    0
                };
            assert_eq!(u16_at(&snapshot, 10), MINOR_V2_6, "dimensions {dimensions}");
            assert_eq!(u16_at(&snapshot, 12), MINOR_V2_6, "dimensions {dimensions}");
            assert_eq!(u64_at(&snapshot, 64), expected_features as u64);
            assert_eq!(u64_at(&snapshot, 72), 0);
            assert_eq!(u32_at(&snapshot, 40), SECTION_COUNT_V2_6 as u32);
            for (index, tag) in ALL_TAGS
                .into_iter()
                .filter(|tag| {
                    *tag != SGR_STACK_STATE
                        && *tag != MACROS_STATE
                        && *tag != PAGES_STATE
                        && *tag != HOST_GEOMETRY_STATE
                        && *tag != HOST_THEME_STATE
                        && *tag != SHELL_INTEGRATION_STATE
                        && *tag != HOST_SIGNALS_STATE
                        && *tag != GRAPHICS_STATE
                })
                .enumerate()
            {
                let base = HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE;
                assert_eq!(u32_at(&snapshot, base), tag);
                assert_eq!(u16_at(&snapshot, base + 4), CRITICAL);
                assert_eq!(
                    u16_at(&snapshot, base + 6),
                    SnapshotVersion::V2_6 {
                        history,
                        repeat,
                        cursor,
                        extended_sgr,
                        win32_input,
                    }
                    .section_version(tag)
                );
            }
            let restored = import(&snapshot).expect("V2.6 restore");
            assert_eq!(restored, terminal, "dimensions {dimensions}");
            assert_eq!(export(&restored).expect("canonical re-export"), snapshot);

            terminal.main_keyboard_protocol.pop(1);
            assert_eq!(
                export(&terminal).expect("downgraded profile"),
                legacy,
                "dimensions {dimensions}"
            );
        }
    }

    #[test]
    fn kitty_keyboard_snapshot_layout_preserves_both_stacks_oldest_to_top() {
        let mut terminal = Terminal::new(8, 2).expect("terminal");
        for flags in [1, 2, 3] {
            terminal.main_keyboard_protocol.push(flags);
        }
        for flags in [4, 5] {
            terminal.alternate_keyboard_protocol.push(flags);
        }
        let snapshot = export(&terminal).expect("V2.6 snapshot");
        let directory = HEADER_SIZE + 4 * DIRECTORY_ENTRY_SIZE;
        assert_eq!(u32_at(&snapshot, directory), KEYBOARD_PROTOCOL_STATE);
        assert_eq!(u16_at(&snapshot, directory + 6), 1);
        assert_eq!(directory_value(&snapshot, 4, 16), 13);
        assert_eq!(directory_value(&snapshot, 4, 24), 5);
        let payload = directory_value(&snapshot, 4, 8) as usize;
        assert_eq!(
            &snapshot[payload..payload + 13],
            &[3, 2, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5]
        );
        let restored = import(&snapshot).expect("restore both stacks");
        assert_eq!(restored.main_keyboard_protocol.as_slice(), &[1, 2, 3]);
        assert_eq!(restored.alternate_keyboard_protocol.as_slice(), &[4, 5]);
        assert_eq!(export(&restored).expect("canonical re-export"), snapshot);
    }

    #[test]
    fn kitty_keyboard_snapshot_rejects_profile_section_and_payload_mixes() {
        let mut terminal = Terminal::new(8, 2).expect("terminal");
        terminal.main_keyboard_protocol.push(1);
        terminal.alternate_keyboard_protocol.push(2);
        let snapshot = export(&terminal).expect("V2.6 snapshot");
        let keyboard = directory_value(&snapshot, 4, 8) as usize;

        for offset in [
            keyboard + 2,
            keyboard + 4,
            keyboard + 5,
            keyboard + 6,
            keyboard + 7,
        ] {
            assert_mutation_error(&snapshot, TerminalError::MalformedSnapshot, |bytes| {
                bytes[offset] = 1;
            });
        }
        assert_mutation_error(&snapshot, TerminalError::MalformedSnapshot, |bytes| {
            bytes[keyboard] = 9;
        });
        assert_mutation_error(&snapshot, TerminalError::MalformedSnapshot, |bytes| {
            bytes[keyboard + 1] = 9;
        });
        assert_mutation_error(&snapshot, TerminalError::MalformedSnapshot, |bytes| {
            bytes[keyboard + 8] = 0x20;
        });
        assert_mutation_error(&snapshot, TerminalError::MalformedSnapshot, |bytes| {
            set_directory_u32(bytes, 4, 24, 1);
        });
        assert_mutation_error(&snapshot, TerminalError::MalformedSnapshot, |bytes| {
            set_directory_u64(bytes, 4, 16, 9);
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            set_directory_u16(bytes, 4, 6, 2);
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            set_directory_u32(bytes, 4, 0, CHARSET_STATE);
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[40..44].copy_from_slice(&(SECTION_COUNT as u32).to_le_bytes());
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[10..12].copy_from_slice(&MINOR_V2_5.to_le_bytes());
            bytes[12..14].copy_from_slice(&MINOR_V2_5.to_le_bytes());
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[64..72].copy_from_slice(&(REQUIRED_FEATURES_V2_0 as u64).to_le_bytes());
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[64..72]
                .copy_from_slice(&((REQUIRED_FEATURES_V2_6 | (1_u128 << 63)) as u64).to_le_bytes());
        });

        let legacy =
            export(&Terminal::new(8, 2).expect("legacy terminal")).expect("legacy snapshot");
        assert_mutation_error(&legacy, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[40..44].copy_from_slice(&(SECTION_COUNT_V2_6 as u32).to_le_bytes());
        });
        assert_mutation_error(&legacy, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[64..72].copy_from_slice(
                &((REQUIRED_FEATURES_V2_0 | KEYBOARD_PROTOCOL_STATE_FEATURE) as u64).to_le_bytes(),
            );
        });
    }

    #[test]
    fn nrc_state_selects_all_sixty_four_v2_7_profiles_and_downgrades_exactly() {
        for dimensions in 0_u8..64 {
            let history = dimensions & 1 != 0;
            let repeat = dimensions & 2 != 0;
            let cursor = dimensions & 4 != 0;
            let extended_sgr = dimensions & 8 != 0;
            let win32_input = dimensions & 16 != 0;
            let keyboard_protocol = dimensions & 32 != 0;
            let mut terminal = Terminal::new_with_options(
                8,
                2,
                0,
                crate::TerminalOptions {
                    history_capacity_rows: u32::from(history) * 4,
                },
            )
            .expect("profile terminal");
            if repeat {
                terminal.repeat_source = Some(RepeatSource::new('R'));
            }
            if cursor {
                terminal.cursor_shape = CursorShape::Underline;
            }
            if extended_sgr {
                terminal.main.attributes.faint = true;
            }
            terminal.modes.win32_input_mode = win32_input;
            if keyboard_protocol {
                terminal.main_keyboard_protocol.push(1);
            }

            let legacy = export(&terminal).expect("legacy profile");
            terminal.modes.national_replacement_character_set_mode = true;
            let snapshot = export(&terminal).expect("V2.7 profile");
            let expected_features = REQUIRED_FEATURES_V2_7
                | if history { HISTORY_ANCHORS_FEATURE } else { 0 }
                | if repeat { REP_SOURCE_FEATURE } else { 0 }
                | if cursor { CURSOR_STYLE_FEATURE } else { 0 }
                | if extended_sgr {
                    EXTENDED_SGR_FEATURE
                } else {
                    0
                }
                | if win32_input {
                    WIN32_INPUT_MODE_FEATURE
                } else {
                    0
                }
                | if keyboard_protocol {
                    KEYBOARD_PROTOCOL_STATE_FEATURE
                } else {
                    0
                };
            assert_eq!(u16_at(&snapshot, 10), MINOR_V2_7, "dimensions {dimensions}");
            assert_eq!(u16_at(&snapshot, 12), MINOR_V2_7, "dimensions {dimensions}");
            assert_eq!(u64_at(&snapshot, 64), expected_features as u64);
            assert_eq!(u64_at(&snapshot, 72), 0);
            assert_eq!(
                u32_at(&snapshot, 40),
                if keyboard_protocol {
                    SECTION_COUNT_V2_6
                } else {
                    SECTION_COUNT
                } as u32
            );
            let version = SnapshotVersion::V2_7 {
                history,
                repeat,
                cursor,
                extended_sgr,
                win32_input,
                keyboard_protocol,
            };
            for (index, tag) in version.tags().enumerate() {
                let base = HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE;
                assert_eq!(u32_at(&snapshot, base), tag);
                assert_eq!(u16_at(&snapshot, base + 4), CRITICAL);
                assert_eq!(u16_at(&snapshot, base + 6), version.section_version(tag));
            }
            let restored = import(&snapshot).expect("V2.7 restore");
            assert_eq!(restored, terminal, "dimensions {dimensions}");
            assert_eq!(export(&restored).expect("canonical re-export"), snapshot);

            terminal.modes.national_replacement_character_set_mode = false;
            assert_eq!(
                export(&terminal).expect("downgraded profile"),
                legacy,
                "dimensions {dimensions}"
            );
        }
    }

    #[test]
    fn every_nrc_tag_roundtrips_and_active_or_saved_state_selects_v2_7() {
        let designators: &[(&[u8], u8)] = &[
            (b"", b'A'),
            (b"", b'4'),
            (b"", b'5'),
            (b"", b'R'),
            (b"", b'Q'),
            (b"", b'K'),
            (b"", b'Y'),
            (b"", b'E'),
            (b"%", b'6'),
            (b"", b'Z'),
            (b"", b'7'),
            (b"", b'='),
        ];
        for &(prefix, final_byte) in designators {
            let mut terminal = Terminal::new(8, 2).expect("terminal");
            terminal
                .charset
                .designate_with_prefix(
                    GraphicRegister::G0,
                    CharsetSize::Characters94,
                    prefix,
                    final_byte,
                )
                .expect("NRC designation");
            let snapshot = export(&terminal).expect("NRC snapshot");
            assert_eq!(u16_at(&snapshot, 10), MINOR_V2_7);
            let charset_offset = directory_value(&snapshot, 4, 8) as usize;
            assert!((3..=14).contains(&snapshot[charset_offset + 8]));
            let restored = import(&snapshot).expect("NRC restore");
            assert_eq!(restored, terminal);
            assert_eq!(export(&restored).expect("canonical re-export"), snapshot);
        }

        for position in 0..3 {
            let mut terminal = Terminal::new(8, 2).expect("terminal");
            {
                let charset = match position {
                    0 => &mut terminal.charset,
                    1 => &mut terminal.main_saved_charset,
                    2 => &mut terminal.alternate_saved_charset,
                    _ => unreachable!(),
                };
                charset
                    .designate(GraphicRegister::G1, CharsetSize::Characters94, b'A')
                    .expect("British designation");
            }
            let snapshot = export(&terminal).expect("saved NRC snapshot");
            assert_eq!(u16_at(&snapshot, 10), MINOR_V2_7, "position {position}");
            assert_eq!(import(&snapshot).expect("restore saved NRC"), terminal);
            match position {
                0 => terminal.charset.hard_reset(),
                1 => terminal.main_saved_charset.hard_reset(),
                2 => terminal.alternate_saved_charset.hard_reset(),
                _ => unreachable!(),
            }
            assert_eq!(
                u16_at(&export(&terminal).expect("downgraded snapshot"), 10),
                MINOR_V2_0,
                "position {position}"
            );
        }
    }

    #[test]
    fn nrc_snapshot_suffix_replay_is_self_closing() {
        let mut direct = Terminal::new(8, 2).expect("terminal");
        direct
            .charset
            .designate(GraphicRegister::G0, CharsetSize::Characters94, b'A')
            .expect("British designation");
        direct.modes.national_replacement_character_set_mode = true;
        let checkpoint = export(&direct).expect("NRC checkpoint");
        let mut restored = import(&checkpoint).expect("restore NRC checkpoint");
        let suffix = crate::Event::Output {
            source_sequence: 0,
            bytes: b"#",
        };
        direct.apply_event(suffix).expect("direct suffix");
        restored.apply_event(suffix).expect("restored suffix");
        assert_eq!(direct.main.cells[0].character, '£');
        assert_eq!(restored, direct);
        assert_eq!(
            export(&restored).expect("restored snapshot"),
            export(&direct).expect("direct snapshot")
        );
    }

    #[test]
    fn nrc_snapshot_rejects_feature_section_and_legacy_state_mixes() {
        let mut terminal = Terminal::new(8, 2).expect("terminal");
        terminal.modes.national_replacement_character_set_mode = true;
        let snapshot = export(&terminal).expect("V2.7 snapshot");

        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[10..12].copy_from_slice(&MINOR_V2_6.to_le_bytes());
            bytes[12..14].copy_from_slice(&MINOR_V2_6.to_le_bytes());
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[64..72].copy_from_slice(&(REQUIRED_FEATURES_V2_0 as u64).to_le_bytes());
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[64..72]
                .copy_from_slice(&((REQUIRED_FEATURES_V2_7 | 0x10_0000) as u64).to_le_bytes());
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            set_directory_u16(bytes, 1, 6, 2);
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            set_directory_u16(bytes, 4, 6, 1);
        });

        let mode_offset = directory_value(&snapshot, 1, 8) as usize;
        let mode_count = u32_at(&snapshot, mode_offset) as usize;
        let nrc_record = (0..mode_count)
            .map(|index| mode_offset + 8 + index * 8)
            .find(|&record| u16_at(&snapshot, record) == 42 && snapshot[record + 2] == 1)
            .expect("canonical ?42 record");
        assert_mutation_error(&snapshot, TerminalError::MalformedSnapshot, |bytes| {
            bytes[nrc_record..nrc_record + 2].copy_from_slice(&66_u16.to_le_bytes());
        });

        let legacy =
            export(&Terminal::new(8, 2).expect("legacy terminal")).expect("legacy snapshot");
        let legacy_mode = directory_value(&legacy, 1, 8) as usize;
        let legacy_count = u32_at(&legacy, legacy_mode) as usize;
        let auto_wrap_record = (0..legacy_count)
            .map(|index| legacy_mode + 8 + index * 8)
            .find(|&record| u16_at(&legacy, record) == 7 && legacy[record + 2] == 1)
            .expect("canonical auto-wrap record");
        assert_mutation_error(&legacy, TerminalError::MalformedSnapshot, |bytes| {
            bytes[auto_wrap_record..auto_wrap_record + 2].copy_from_slice(&42_u16.to_le_bytes());
        });
        let legacy_charset = directory_value(&legacy, 4, 8) as usize;
        assert_mutation_error(&legacy, TerminalError::MalformedSnapshot, |bytes| {
            bytes[legacy_charset + 8] = 3;
        });

        let mut designated = Terminal::new(8, 2).expect("designated terminal");
        designated
            .charset
            .designate(GraphicRegister::G0, CharsetSize::Characters94, b'A')
            .expect("British designation");
        let designated_snapshot = export(&designated).expect("designated V2.7 snapshot");
        let designated_charset = directory_value(&designated_snapshot, 4, 8) as usize;
        assert_mutation_error(
            &designated_snapshot,
            TerminalError::MalformedSnapshot,
            |bytes| bytes[designated_charset + 8] = 0,
        );
    }

    #[test]
    fn standalone_c1_profile_selects_all_one_hundred_twenty_eight_v2_8_profiles() {
        for dimensions in 0_u8..128 {
            let history = dimensions & 1 != 0;
            let repeat = dimensions & 2 != 0;
            let cursor = dimensions & 4 != 0;
            let extended_sgr = dimensions & 8 != 0;
            let win32_input = dimensions & 16 != 0;
            let keyboard_protocol = dimensions & 32 != 0;
            let national_replacement = dimensions & 64 != 0;
            let mut terminal = Terminal::new_with_output_profile(
                8,
                2,
                0,
                crate::TerminalOptions {
                    history_capacity_rows: u32::from(history) * 4,
                },
                OutputProfile::Utf8WithStandaloneC1,
            )
            .expect("C1 profile terminal");
            if repeat {
                terminal.repeat_source = Some(RepeatSource::new('R'));
            }
            if cursor {
                terminal.cursor_shape = CursorShape::Underline;
            }
            if extended_sgr {
                terminal.main.attributes.faint = true;
            }
            terminal.modes.win32_input_mode = win32_input;
            if keyboard_protocol {
                terminal.main_keyboard_protocol.push(1);
            }
            terminal.modes.national_replacement_character_set_mode = national_replacement;

            let snapshot = export(&terminal).expect("V2.8 profile");
            let expected_features = REQUIRED_FEATURES_V2_8
                | if history { HISTORY_ANCHORS_FEATURE } else { 0 }
                | if repeat { REP_SOURCE_FEATURE } else { 0 }
                | if cursor { CURSOR_STYLE_FEATURE } else { 0 }
                | if extended_sgr {
                    EXTENDED_SGR_FEATURE
                } else {
                    0
                }
                | if win32_input {
                    WIN32_INPUT_MODE_FEATURE
                } else {
                    0
                }
                | if keyboard_protocol {
                    KEYBOARD_PROTOCOL_STATE_FEATURE
                } else {
                    0
                }
                | if national_replacement {
                    NATIONAL_REPLACEMENT_CHARSET_FEATURE
                } else {
                    0
                };
            assert_eq!(u16_at(&snapshot, 10), MINOR_V2_8, "dimensions {dimensions}");
            assert_eq!(u16_at(&snapshot, 12), MINOR_V2_8, "dimensions {dimensions}");
            assert_eq!(u64_at(&snapshot, 64), expected_features as u64);
            assert_eq!(u64_at(&snapshot, 72), 0);
            assert_eq!(
                u32_at(&snapshot, 40),
                if keyboard_protocol {
                    SECTION_COUNT_V2_6
                } else {
                    SECTION_COUNT
                } as u32
            );
            let version = SnapshotVersion::V2_8 {
                history,
                repeat,
                cursor,
                extended_sgr,
                win32_input,
                keyboard_protocol,
                national_replacement,
            };
            for (index, tag) in version.tags().enumerate() {
                let base = HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE;
                assert_eq!(u32_at(&snapshot, base), tag);
                assert_eq!(u16_at(&snapshot, base + 4), CRITICAL);
                assert_eq!(u16_at(&snapshot, base + 6), version.section_version(tag));
            }
            let restored = import(&snapshot).expect("V2.8 restore");
            assert_eq!(
                restored.output_profile(),
                OutputProfile::Utf8WithStandaloneC1
            );
            assert_eq!(restored, terminal, "dimensions {dimensions}");
            assert_eq!(export(&restored).expect("canonical re-export"), snapshot);
        }
    }

    #[test]
    fn standalone_c1_snapshot_restores_profile_for_suffix_and_ris_keeps_it() {
        let mut direct = Terminal::new_with_output_profile(
            8,
            2,
            0,
            crate::TerminalOptions::default(),
            OutputProfile::Utf8WithStandaloneC1,
        )
        .expect("C1 terminal");
        direct
            .charset
            .designate(GraphicRegister::G0, CharsetSize::Characters94, b'A')
            .expect("British designation");
        direct.modes.national_replacement_character_set_mode = true;
        let checkpoint = export(&direct).expect("C1 + NRC checkpoint");
        assert_eq!(u16_at(&checkpoint, 10), MINOR_V2_8);
        assert_ne!(
            u64_at(&checkpoint, 64) & NATIONAL_REPLACEMENT_CHARSET_FEATURE as u64,
            0
        );
        let mut restored = import(&checkpoint).expect("restore C1 checkpoint");
        let suffix = crate::Event::Output {
            source_sequence: 0,
            bytes: &[0x9b, b'2', b';', b'3', b'H', b'X'],
        };
        direct.apply_event(suffix).expect("direct raw C1 suffix");
        restored
            .apply_event(suffix)
            .expect("restored raw C1 suffix");
        assert_eq!(restored, direct);
        assert_eq!(
            export(&restored).expect("restored suffix snapshot"),
            export(&direct).expect("direct suffix snapshot")
        );

        restored
            .apply_event(crate::Event::Output {
                source_sequence: 1,
                bytes: b"\x1bc",
            })
            .expect("RIS");
        assert_eq!(
            restored.output_profile(),
            OutputProfile::Utf8WithStandaloneC1
        );
        let reset_snapshot = export(&restored).expect("post-RIS C1 snapshot");
        assert_eq!(u16_at(&reset_snapshot, 10), MINOR_V2_8);
        assert_eq!(
            u64_at(&reset_snapshot, 64) & NATIONAL_REPLACEMENT_CHARSET_FEATURE as u64,
            0
        );
        assert_eq!(
            export(&import(&reset_snapshot).expect("post-RIS restore")).expect("post-RIS closure"),
            reset_snapshot
        );
    }

    #[test]
    fn standalone_c1_snapshot_rejects_feature_section_and_legacy_mixes() {
        let c1_terminal = Terminal::new_with_output_profile(
            8,
            2,
            0,
            crate::TerminalOptions::default(),
            OutputProfile::Utf8WithStandaloneC1,
        )
        .expect("C1 terminal");
        let snapshot = export(&c1_terminal).expect("V2.8 snapshot");

        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[64..72].copy_from_slice(&(REQUIRED_FEATURES_V2_0 as u64).to_le_bytes());
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[64..72]
                .copy_from_slice(&((REQUIRED_FEATURES_V2_8 | (1_u128 << 63)) as u64).to_le_bytes());
        });
        assert_mutation_error(&snapshot, TerminalError::IncompatibleSnapshot, |bytes| {
            bytes[64..72].copy_from_slice(
                &((REQUIRED_FEATURES_V2_8 | KEYBOARD_PROTOCOL_STATE_FEATURE) as u64).to_le_bytes(),
            );
        });

        let mut kitty = c1_terminal.clone();
        kitty.main_keyboard_protocol.push(1);
        let kitty_snapshot = export(&kitty).expect("V2.8 Kitty snapshot");
        assert_mutation_error(
            &kitty_snapshot,
            TerminalError::IncompatibleSnapshot,
            |bytes| {
                bytes[64..72].copy_from_slice(&(REQUIRED_FEATURES_V2_8 as u64).to_le_bytes());
            },
        );

        assert_mutation_error(&snapshot, TerminalError::MalformedSnapshot, |bytes| {
            bytes[64..72].copy_from_slice(
                &((REQUIRED_FEATURES_V2_8 | NATIONAL_REPLACEMENT_CHARSET_FEATURE) as u64)
                    .to_le_bytes(),
            );
            let mode = directory_index_for_tag(bytes, MODE_BANK);
            let charset = directory_index_for_tag(bytes, CHARSET_STATE);
            set_directory_u16(bytes, mode, 6, 3);
            set_directory_u16(bytes, charset, 6, 2);
        });

        let mut c1_nrc = c1_terminal;
        c1_nrc.modes.national_replacement_character_set_mode = true;
        let c1_nrc_snapshot = export(&c1_nrc).expect("V2.8 NRC snapshot");
        assert_mutation_error(
            &c1_nrc_snapshot,
            TerminalError::MalformedSnapshot,
            |bytes| {
                bytes[64..72].copy_from_slice(&(REQUIRED_FEATURES_V2_8 as u64).to_le_bytes());
                let mode = directory_index_for_tag(bytes, MODE_BANK);
                let charset = directory_index_for_tag(bytes, CHARSET_STATE);
                set_directory_u16(bytes, mode, 6, 1);
                set_directory_u16(bytes, charset, 6, 1);
            },
        );

        let mut profiles = Vec::new();
        profiles.push(Terminal::new(8, 2).expect("V2.0"));
        let mut v2_1 = Terminal::new_with_options(
            8,
            2,
            0,
            crate::TerminalOptions {
                history_capacity_rows: 1,
            },
        )
        .expect("V2.1");
        profiles.push(v2_1.clone());
        v2_1.repeat_source = Some(RepeatSource::new('R'));
        profiles.push(v2_1.clone());
        v2_1.cursor_shape = CursorShape::Underline;
        profiles.push(v2_1.clone());
        v2_1.main.attributes.faint = true;
        profiles.push(v2_1.clone());
        v2_1.modes.win32_input_mode = true;
        profiles.push(v2_1.clone());
        v2_1.main_keyboard_protocol.push(1);
        profiles.push(v2_1.clone());
        v2_1.modes.national_replacement_character_set_mode = true;
        profiles.push(v2_1);
        for (minor, terminal) in profiles.iter().enumerate() {
            let legacy = export(terminal).expect("legacy profile");
            assert_eq!(u16_at(&legacy, 10), minor as u16);
            assert_mutation_error(&legacy, TerminalError::IncompatibleSnapshot, |bytes| {
                let features = u64_at(bytes, 64) | STANDALONE_C1_OUTPUT_PROFILE_FEATURE as u64;
                bytes[64..72].copy_from_slice(&features.to_le_bytes());
            });
        }
    }

    #[test]
    fn maximum_capacity_formula_and_preflight_scratch_are_bounded() {
        let payloads = [
            32,
            200,
            24,
            72,
            24,
            52,
            8 + SGR_STACK_CAPACITY * 8,
            crate::macros::MAX_SNAPSHOT_BYTES,
            MAX_PAGES_SECTION_BYTES,
            crate::HOST_GEOMETRY_V1_BYTES,
            crate::host_theme::THEME_STATE_BYTES,
            crate::MAX_SHELL_STATE_BYTES,
            crate::HOST_SIGNALS_V1_BYTES,
            MAX_OSC_SNAPSHOT_V2_BYTES,
            MAX_GRAPHEME_SECTION_BYTES,
            MAX_RENDITION_SECTION_BYTES,
            64,
            MAX_PRIMARY_ROWS_SECTION_BYTES,
            64,
            MAX_LEGACY_ROWS_SECTION_BYTES,
        ];
        let payload_total = payloads
            .into_iter()
            .try_fold(0_usize, |total, length| total.checked_add(length));
        let checked_bound = payload_total
            .and_then(|total| total.checked_add(ENVELOPE_BYTES))
            .and_then(|total| {
                (MAX_LEGACY_KNOWN_SECTIONS + 1)
                    .checked_mul(7)
                    .and_then(|padding| total.checked_add(padding))
            })
            .expect("capacity formula does not overflow usize");
        assert_eq!(checked_bound, MAX_CANONICAL_SNAPSHOT_BYTES);
        assert_eq!(MAX_CANONICAL_SNAPSHOT_BYTES, 66_070_235);
        assert_eq!(MAX_SNAPSHOT_BYTES - MAX_CANONICAL_SNAPSHOT_BYTES, 1_038_629);
        let graphics_bound = checked_bound
            .checked_add(39)
            .and_then(|total| total.checked_add(crate::graphics_state::MAX_GRAPHICS_STATE_BYTES))
            .and_then(|total| total.checked_add(20 * (MAX_GRAPHEME_TABLE_ENTRIES - 1)))
            .and_then(|total| total.checked_add(36))
            .expect("graphics capacity formula does not overflow usize");
        assert_eq!(graphics_bound, MAX_CANONICAL_GRAPHICS_SNAPSHOT_BYTES);
        assert_eq!(graphics_bound, 111_470_186);
        assert!(graphics_bound <= crate::MAX_GRAPHICS_SNAPSHOT_BYTES);

        assert_eq!(
            std::mem::size_of::<GraphemeWidths>(),
            GRAPHEME_WIDTH_WORDS * std::mem::size_of::<u64>()
        );
        assert_eq!(std::mem::size_of::<GraphemeWidths>(), 69_640);
        assert_eq!(
            std::mem::size_of::<GraphemeUses>(),
            GRAPHEME_WIDTH_WORDS * std::mem::size_of::<u64>()
        );
        assert_eq!(std::mem::size_of::<GraphemeUses>(), 69_640);
        assert_eq!(
            std::mem::size_of::<RenditionUses>(),
            RENDITION_USE_WORDS * std::mem::size_of::<u64>()
        );
        assert_eq!(std::mem::size_of::<RenditionUses>(), 69_640);
        let table_scratch = std::mem::size_of::<GraphemeWidths>()
            + std::mem::size_of::<GraphemeUses>()
            + std::mem::size_of::<RenditionUses>();
        assert_eq!(table_scratch, 208_920);
        assert!(table_scratch + std::mem::size_of::<Preflight>() < 210 * 1024);
    }

    fn drcs_snapshot_fixture() -> (Terminal, GlyphReference, GlyphReference) {
        let mut terminal = wrapped_history_terminal();
        let mut graphics = crate::graphics_state::GraphicsState::default();
        let install =
            |graphics: &mut crate::graphics_state::GraphicsState, payload, retained: &[u64]| {
                let download = crate::drcs::decode(&[1, 1, 0, 5, 1, 2, 8, 0], payload).unwrap();
                graphics.install_font(download, retained).unwrap();
                match graphics.fonts().lookup(b" @", false, 80, 24, 1) {
                    crate::drcs_bank::GlyphLookup::Glyph(glyph) => glyph,
                    other => panic!("missing fixture glyph: {other:?}"),
                }
            };
        let retired = install(&mut graphics, b" @~", &[]);
        let current = install(&mut graphics, b" @@", &[retired.resource]);
        assert_ne!(retired.resource, current.resource);
        assert!(
            !graphics
                .fonts()
                .referenced_resources()
                .any(|id| id == retired.resource)
        );
        terminal.graphics = Some(graphics);
        for (index, glyph) in [(0, retired), (4, retired), (5, current)] {
            terminal.main.cells[index].character = '?';
            terminal.main.cells[index].glyph = Some(glyph);
        }
        terminal.main.cells[6].character = '?';
        terminal.alternate.cells[0].character = '?';
        terminal.alternate.cells[0].glyph = Some(retired);
        terminal.alternate.meaningful_end[0] = 1;
        let mut page = Screen::new(terminal.columns, terminal.rows);
        page.cells[0].character = '?';
        page.cells[0].glyph = Some(current);
        page.meaningful_end[0] = 1;
        terminal.pages.extra[0] = Some(page);
        let mut repeat = RepeatSource::new('?');
        repeat.glyph = Some(current);
        terminal.repeat_source = Some(repeat);
        (terminal, retired, current)
    }

    #[test]
    fn drcs_snapshot_preserves_history_pages_repeat_and_retired_atlas() {
        let (terminal, retired, current) = drcs_snapshot_fixture();
        let bytes = export(&terminal).unwrap();
        let p = preflight_a(&bytes).unwrap();
        assert_eq!(p.version.minor(), 22);
        let index = directory_index_for_tag(&bytes, GRAPHEME_TABLE);
        assert_eq!(
            u16_at(&bytes, HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + 6),
            2
        );
        let graphemes =
            decode_graphemes(section(&bytes, &p, GRAPHEME_TABLE).unwrap(), p.version).unwrap();
        let variants: Vec<_> = graphemes
            .iter()
            .filter(|g| g.character == '?')
            .map(|g| g.glyph)
            .collect();
        assert_eq!(variants, [None, Some(retired), Some(current)]);
        let restored = import(&bytes).unwrap();
        assert_eq!(restored, terminal);
        assert_eq!(export(&restored).unwrap(), bytes);
        assert_eq!(
            restored.repeat_source.as_ref().unwrap().glyph,
            Some(current)
        );
        assert!(
            restored
                .graphics
                .as_ref()
                .unwrap()
                .resources()
                .get(retired.resource)
                .is_some()
        );
        let mut repeat_only = terminal;
        for cell in repeat_only
            .main
            .cells
            .iter_mut()
            .chain(&mut repeat_only.alternate.cells)
            .chain(
                repeat_only
                    .pages
                    .screens_mut()
                    .flat_map(|page| &mut page.cells),
            )
        {
            cell.glyph = None;
        }
        repeat_only.repeat_source.as_mut().unwrap().glyph = Some(retired);
        let bytes = export(&repeat_only).unwrap();
        let restored = import(&bytes).unwrap();
        assert_eq!(restored.repeat_source, repeat_only.repeat_source);
        assert_eq!(export(&restored).unwrap(), bytes);
    }

    #[test]
    fn drcs_snapshot_rejects_bad_glyph_references_during_preflight_without_pixels() {
        let (mut terminal, retired, current) = drcs_snapshot_fixture();
        let command = crate::kitty_graphics::TransferDecoder::default()
            .push(b"Ga=t,s=1,v=1,i=9;AQIDBA==")
            .unwrap()
            .unwrap();
        let graphics = terminal.graphics.as_mut().unwrap();
        graphics
            .apply_kitty(
                command,
                crate::graphics_state::Anchor {
                    page: 1,
                    logical_line: 0,
                    cell_offset: 0,
                },
                crate::HostGeometryV1::default(),
                &[retired.resource, current.resource],
            )
            .unwrap();
        let rgba = graphics
            .resources()
            .entries()
            .iter()
            .find(|resource| {
                resource.image().format() == crate::graphics_resources::ImageFormat::Rgba8888
            })
            .unwrap()
            .id();
        let bytes = export(&terminal).unwrap();
        let p = preflight_a(&bytes).unwrap();
        let table = section(&bytes, &p, GRAPHEME_TABLE).unwrap();
        let index = directory_index_for_tag(&bytes, GRAPHEME_TABLE);
        let table_offset = directory_value(&bytes, index, 8) as usize;
        let mut reader = Reader::new(table);
        reader.u64().unwrap();
        let glyph_offset = loop {
            let offset = reader.offset;
            let value = read_grapheme(&mut reader, p.version).unwrap();
            if value.glyph.is_some() {
                break table_offset + offset;
            }
        };
        for (relative, value) in [
            (4, 0u32),
            (4, u32::MAX),
            (12, u32::MAX),
            (16, 0),
            (16, 4097),
            (20, 0),
            (20, 4097),
        ] {
            let mut bad = bytes.clone();
            bad[glyph_offset + relative..glyph_offset + relative + 4]
                .copy_from_slice(&value.to_le_bytes());
            refresh_crc(&mut bad);
            let before = crate::graphics_resources::image_allocation_attempts();
            let preflight = preflight_a(&bad).unwrap();
            assert_eq!(
                preflight_b(&bad, &preflight),
                Err(TerminalError::MalformedSnapshot)
            );
            assert_eq!(import(&bad), Err(TerminalError::MalformedSnapshot));
            assert_eq!(
                crate::graphics_resources::image_allocation_attempts(),
                before
            );
        }
        for (relative, value) in [(2, 2), (3, 2)] {
            assert_mutation_error(&bytes, TerminalError::MalformedSnapshot, |bad| {
                bad[glyph_offset + relative] = value
            });
        }
        let mut bad_format = bytes.clone();
        bad_format[glyph_offset + 4..glyph_offset + 12].copy_from_slice(&rgba.to_le_bytes());
        bad_format[glyph_offset + 12..glyph_offset + 16].copy_from_slice(&0u32.to_le_bytes());
        bad_format[glyph_offset + 16..glyph_offset + 20].copy_from_slice(&1u32.to_le_bytes());
        bad_format[glyph_offset + 20..glyph_offset + 24].copy_from_slice(&1u32.to_le_bytes());
        refresh_crc(&mut bad_format);
        let before = crate::graphics_resources::image_allocation_attempts();
        let preflight = preflight_a(&bad_format).unwrap();
        assert_eq!(
            preflight_b(&bad_format, &preflight),
            Err(TerminalError::MalformedSnapshot)
        );
        assert_eq!(
            crate::graphics_resources::image_allocation_attempts(),
            before
        );
        let mut legacy = terminal;
        legacy.graphics = None;
        assert_eq!(
            encode_terminal(&legacy),
            Err(TerminalError::IncompatibleSnapshot)
        );
        assert_mutation_error(&bytes, TerminalError::IncompatibleSnapshot, |bad| {
            set_directory_u16(bad, index, 6, 1);
        });
    }

    #[test]
    fn graphics_snapshot_checks_screen_anchors_before_allocating_pixels() {
        let (mut terminal, retired, current) = drcs_snapshot_fixture();
        let command = crate::kitty_graphics::TransferDecoder::default()
            .push(b"Ga=T,s=1,v=1,i=9,C=1;AQIDBA==")
            .unwrap()
            .unwrap();
        terminal
            .graphics
            .as_mut()
            .unwrap()
            .apply_kitty(
                command,
                crate::graphics_state::Anchor {
                    page: 1,
                    logical_line: terminal.main.row_metadata[0].logical_line_id,
                    cell_offset: 0,
                },
                crate::HostGeometryV1 {
                    cell_width: 10,
                    cell_height: 20,
                    ..Default::default()
                },
                &[retired.resource, current.resource],
            )
            .unwrap();
        let bytes = export(&terminal).unwrap();
        assert_eq!(import(&bytes).unwrap(), terminal);
        let index = directory_index_for_tag(&bytes, GRAPHICS_STATE);
        let graphics = directory_value(&bytes, index, 8) as usize;
        let length = directory_value(&bytes, index, 16) as usize;
        let placement = graphics
            + 64
            + u32_at(&bytes, graphics + 36) as usize
            + crate::drcs_bank::DRCS_BANK_BYTES
            + 768
            + u32_at(&bytes, graphics + 28) as usize * 24;
        assert_eq!(u32_at(&bytes, graphics + 32), 1);
        for mutation in 0..3 {
            let mut bad = bytes.clone();
            match mutation {
                0 => bad[placement + 40] = 3, // Page 2 exists in the fixture; page 3 does not.
                1 => bad[placement + 16..placement + 24]
                    .copy_from_slice(&(u64::MAX - 1).to_le_bytes()),
                _ => bad[placement + 24..placement + 28].copy_from_slice(&u32::MAX.to_le_bytes()),
            }
            let crc = crc32c::checksum_parts(&[
                &bad[graphics..graphics + 40],
                &bad[graphics + 44..graphics + length],
            ]);
            bad[graphics + 40..graphics + 44].copy_from_slice(&crc.to_le_bytes());
            refresh_crc(&mut bad);
            let before = crate::graphics_resources::image_allocation_attempts();
            let p = preflight_a(&bad).unwrap();
            preflight_b(&bad, &p).expect("resource and row metadata remain valid");
            assert_eq!(import(&bad), Err(TerminalError::MalformedSnapshot));
            assert_eq!(
                crate::graphics_resources::image_allocation_attempts(),
                before
            );
        }
    }

    #[test]
    fn graphics_snapshot_rejects_unreferenced_graphemes_without_other_table_features() {
        let mut terminal = Terminal::new(1, 1).unwrap();
        terminal.graphics = Some(crate::graphics_state::GraphicsState::default());
        terminal.main.cells[0].character = 'A';
        terminal.main.meaningful_end[0] = 1;
        let version = SnapshotVersion::for_terminal(&terminal);
        assert!(!version.has_retained_primary_scrollback());
        assert!(!version.has_repeat_source());
        assert!(!version.has_cursor_style());
        let bytes = export(&terminal).unwrap();
        assert_eq!(import(&bytes).unwrap(), terminal);
        let index = directory_index_for_tag(&bytes, PRIMARY_ROWS);
        let row = row_record_offsets(&bytes, index)[0];
        assert_eq!(u32_at(&bytes, row + 20), 1);
        assert_eq!(u32_at(&bytes, row + 24), 1);
        let mut bad = bytes.clone();
        bad[row + 24..row + 28].copy_from_slice(&0u32.to_le_bytes());
        refresh_crc(&mut bad);
        let p = preflight_a(&bad).unwrap();
        assert_eq!(preflight_b(&bad, &p), Err(TerminalError::MalformedSnapshot));
        assert_eq!(import(&bad), Err(TerminalError::MalformedSnapshot));
    }

    #[test]
    fn drcs_snapshot_maximum_grapheme_table_is_bounded() {
        use crate::graphics_resources::{GraphicsResources, ImageData, ImageFormat};
        let mut resources = GraphicsResources::default();
        let mut graphemes = Vec::with_capacity(MAX_GRAPHEME_TABLE_ENTRIES);
        graphemes.push(GraphemeRef {
            character: ' ',
            combining: "",
            width: 1,
            glyph: None,
        });
        for _ in 0..(MAX_GRAPHEME_TABLE_ENTRIES - 1).div_ceil(4096) {
            let resource = resources
                .insert(ImageData::allocate(4096, 1, ImageFormat::Alpha8).unwrap())
                .unwrap();
            for x in 0..4096 {
                if graphemes.len() == MAX_GRAPHEME_TABLE_ENTRIES {
                    break;
                }
                graphemes.push(GraphemeRef {
                    character: '?',
                    combining: "",
                    width: 1,
                    glyph: Some(GlyphReference {
                        resource,
                        x,
                        width: 1,
                        height: 1,
                    }),
                });
            }
        }
        let version = SnapshotVersion::V2_22 {
            features: REQUIRED_FEATURES_V2_1 | GRAPHICS_FEATURE,
        };
        let mut writer = Writer::new();
        writer.limit = crate::MAX_GRAPHICS_SNAPSHOT_BYTES;
        encode_graphemes(&mut writer, &graphemes, version);
        let bytes = writer.finish().unwrap();
        assert_eq!(bytes.len(), 16 + (MAX_GRAPHEME_TABLE_ENTRIES - 1) * 28);
        assert!(bytes.len() <= MAX_GRAPHEME_SECTION_BYTES + 20 * (MAX_GRAPHEME_TABLE_ENTRIES - 1));
        let encoded_resources = resources.encode().unwrap();
        let frame = GraphicsResources::inspect(&encoded_resources).unwrap();
        let mut widths = GraphemeWidths::new();
        let before = crate::graphics_resources::image_allocation_attempts();
        assert_eq!(
            scan_graphemes(&bytes, &mut widths, version, Some(&frame)).unwrap(),
            MAX_GRAPHEME_TABLE_ENTRIES
        );
        assert_eq!(
            crate::graphics_resources::image_allocation_attempts(),
            before
        );
    }

    #[test]
    fn drcs_snapshot_preserves_raw_charset_names_and_checks_padding() {
        let mut terminal = Terminal::new(4, 2).unwrap();
        terminal.graphics = Some(crate::graphics_state::GraphicsState::default());
        for (state, prefix, final_byte) in [
            (&mut terminal.charset, &b""[..], b'1'),
            (&mut terminal.main_saved_charset, &b" "[..], b'@'),
            (&mut terminal.alternate_saved_charset, &b"&%"[..], b'@'),
        ] {
            state.set_designation(
                GraphicRegister::G0,
                CharsetDesignation::decode_with_soft(
                    CharsetSize::Characters94,
                    prefix,
                    final_byte,
                    true,
                )
                .unwrap(),
            );
        }
        terminal.user_preferred_charset =
            CharsetDesignation::decode_with_soft(CharsetSize::Characters94, b"", b'C', true)
                .unwrap();
        let bytes = export(&terminal).unwrap();
        let index = directory_index_for_tag(&bytes, CHARSET_STATE);
        let offset = directory_value(&bytes, index, 8) as usize;
        assert_eq!(
            u16_at(&bytes, HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE + 6),
            4
        );
        assert_eq!(directory_value(&bytes, index, 16), 88);
        assert_eq!(&bytes[offset + 8..offset + 12], &[94, b'1', 0, 0]);
        assert_eq!(import(&bytes).unwrap(), terminal);
        assert_eq!(export(&import(&bytes).unwrap()).unwrap(), bytes);
        for (relative, value) in [(8, 0), (9, 0), (10, b'@'), (11, b'@')] {
            assert_mutation_error(&bytes, TerminalError::MalformedSnapshot, |bad| {
                bad[offset + relative] = value
            });
        }
        let mut legacy = terminal;
        legacy.graphics = None;
        assert_eq!(
            encode_terminal(&legacy),
            Err(TerminalError::IncompatibleSnapshot)
        );
    }

    #[test]
    fn two_screens_over_one_cell_cap_roundtrip_tables_larger_than_one_screen() {
        let mut terminal = Terminal::new(512, 257).expect("terminal");
        assert!(terminal.main.cells.len() + terminal.alternate.cells.len() > MAX_CELLS);

        let mut next_scalar = u32::from('!');
        let mut next_rendition = 0;
        let main_count = terminal.main.cells.len();
        fill_unique_cells(
            &mut terminal.main,
            main_count,
            &mut next_scalar,
            &mut next_rendition,
        );
        fill_unique_cells(
            &mut terminal.alternate,
            MAX_CELLS - main_count,
            &mut next_scalar,
            &mut next_rendition,
        );

        let snapshot = export(&terminal).expect("snapshot above one-screen table cap");
        assert!(snapshot.len() < MAX_SNAPSHOT_BYTES);
        assert_eq!(directory_value(&snapshot, 6, 24), (MAX_CELLS + 1) as u64);
        assert_eq!(directory_value(&snapshot, 7, 24), (MAX_CELLS + 1) as u64);

        let restored = import(&snapshot).expect("import above one-screen table cap");
        assert_eq!(restored, terminal);
        assert_eq!(
            export(&restored).expect("canonical re-export above one-screen table cap"),
            snapshot
        );
    }
}
