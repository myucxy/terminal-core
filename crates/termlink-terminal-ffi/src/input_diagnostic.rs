use termlink_terminal_core::{
    InputEncodeError, InputEvent, Key, KittyKey, KittyKeyEventValidationError, Modifiers,
};

use crate::{
    STATUS_FAILED_CLOSED, STATUS_INVALID_ARGUMENT, STATUS_LIMIT_EXCEEDED, STATUS_UNSUPPORTED,
};

pub(super) const MAX_INPUT_DIAGNOSTIC_BYTES: usize = 2048;

/// A bounded, ASCII-only account of one semantic input encoding failure.
/// No paste or associated-text payload is copied into the diagnostic.
pub(super) fn format_input_diagnostic(error: &InputEncodeError, event: InputEvent<'_>) -> Vec<u8> {
    let status = match super::status_for_input_error(error) {
        STATUS_FAILED_CLOSED => "failed_closed",
        STATUS_INVALID_ARGUMENT => "invalid_argument",
        STATUS_LIMIT_EXCEEDED => "limit_exceeded",
        STATUS_UNSUPPORTED => "unsupported",
        _ => unreachable!("all input encoding statuses are classified"),
    };
    let reason = match error {
        InputEncodeError::UnsupportedVt52Key(_) => "unsupported_vt52_key",
        InputEncodeError::TerminalFailedClosed => "terminal_failed_closed",
        InputEncodeError::Win32InputModeDisabled => "win32_input_mode_disabled",
        InputEncodeError::KittyKeyEventRequired => "kitty_key_event_required",
        InputEncodeError::InvalidKittyFlags { .. } => "invalid_kitty_flags",
        InputEncodeError::KittyAssociatedTextTooLarge { .. } => "kitty_associated_text_too_large",
        InputEncodeError::KittyTextContainsControl(_) => "kitty_text_contains_control",
        InputEncodeError::InvalidKittyKeyEvent(value) => match value {
            KittyKeyEventValidationError::ShiftedKeyWithoutShift => {
                "kitty_shifted_key_without_shift"
            }
            KittyKeyEventValidationError::AlternateKeyForNonCharacter => {
                "kitty_alternate_key_for_non_character"
            }
            KittyKeyEventValidationError::AssociatedTextOnRelease => {
                "kitty_associated_text_on_release"
            }
            KittyKeyEventValidationError::UnidentifiedTextMustBePress => {
                "kitty_unidentified_text_must_be_press"
            }
            KittyKeyEventValidationError::UnidentifiedTextRequiresAssociatedText => {
                "kitty_unidentified_text_requires_associated_text"
            }
        },
        InputEncodeError::UnsupportedKittyLegacyEncoding { .. } => {
            "unsupported_kitty_legacy_encoding"
        }
        InputEncodeError::EmptyPaste => "empty_paste",
        InputEncodeError::PasteTooLarge { .. } => "paste_too_large",
        InputEncodeError::UnsupportedModifiers { .. } => "unsupported_modifiers",
        InputEncodeError::UnsupportedControlCharacter(_) => "unsupported_control_character",
        InputEncodeError::InvalidMouseButtonState { .. } => "invalid_mouse_button_state",
        InputEncodeError::MissingMousePixelPosition => "missing_mouse_pixel_position",
        InputEncodeError::MouseCellOutOfBounds { .. } => "mouse_cell_out_of_bounds",
        InputEncodeError::MouseCoordinateOutOfRange { .. } => "mouse_coordinate_out_of_range",
        InputEncodeError::AllocationFailed => "allocation_failed",
    };
    let (kind, mut key, mut modifiers) = match event {
        InputEvent::Key(key_event) => {
            let (name, _) = legacy_key(key_event.key);
            ("key", name, Some(legacy_modifiers(key_event.modifiers)))
        }
        InputEvent::KittyKey(key_event) => {
            let (name, _) = kitty_key(key_event.key);
            (
                "kitty_key",
                name,
                Some(kitty_modifiers(key_event.modifiers)),
            )
        }
        InputEvent::Win32Key(key_event) => (
            "win32_key",
            format!("VK_{:04X}", key_event.virtual_key),
            None,
        ),
        InputEvent::Paste(_) => ("paste", "none".into(), None),
        InputEvent::Mouse(mouse_event) => (
            "mouse",
            "none".into(),
            Some(legacy_modifiers(mouse_event.modifiers)),
        ),
        InputEvent::FocusGained | InputEvent::FocusLost => ("focus", "none".into(), None),
    };
    let mut codepoint = None;
    match error {
        InputEncodeError::UnsupportedVt52Key(value) => {
            (key, codepoint) = legacy_key(*value);
        }
        InputEncodeError::UnsupportedModifiers {
            key: value,
            modifiers: value_modifiers,
        } => {
            (key, codepoint) = legacy_key(*value);
            modifiers = Some(legacy_modifiers(*value_modifiers));
        }
        InputEncodeError::UnsupportedKittyLegacyEncoding {
            key: value,
            modifiers: value_modifiers,
        } => {
            (key, codepoint) = kitty_key(*value);
            modifiers = Some(kitty_modifiers(*value_modifiers));
        }
        InputEncodeError::UnsupportedControlCharacter(value)
        | InputEncodeError::KittyTextContainsControl(value) => codepoint = Some(*value),
        _ => {}
    }
    let codepoint = codepoint.map_or_else(
        || "none".to_owned(),
        |value| format!("U+{:04X}", value as u32),
    );
    let modifiers = modifiers.map_or_else(|| "none".to_owned(), |value| format!("0x{value:02X}"));
    let diagnostic = format!(
        "TL-INPUT-DIAG/1\ndirection=input_encoding\nstatus={status}\nkind={kind}\nreason={reason}\nkey={key}\ncodepoint={codepoint}\nmodifiers={modifiers}\n"
    );
    debug_assert!(diagnostic.is_ascii() && diagnostic.len() <= MAX_INPUT_DIAGNOSTIC_BYTES);
    diagnostic.into_bytes()
}

fn legacy_key(key: Key) -> (String, Option<char>) {
    match key {
        Key::Character(value) => ("Character".into(), Some(value)),
        Key::Keypad(value) => (format!("Keypad_{value:?}"), None),
        value => (format!("{value:?}"), None),
    }
}

fn kitty_key(key: KittyKey) -> (String, Option<char>) {
    match key {
        KittyKey::Character(value) => ("Character".into(), Some(value)),
        KittyKey::Functional(value) => (format!("{value:?}"), None),
        KittyKey::UnidentifiedText => ("UnidentifiedText".into(), None),
    }
}

fn legacy_modifiers(value: Modifiers) -> u8 {
    value.shift as u8
        | (value.alt as u8) << 1
        | (value.control as u8) << 2
        | (value.meta as u8) << 3
}

fn kitty_modifiers(value: termlink_terminal_core::KittyModifiers) -> u8 {
    value.shift as u8
        | (value.alt as u8) << 1
        | (value.control as u8) << 2
        | (value.super_key as u8) << 3
        | (value.hyper as u8) << 4
        | (value.meta as u8) << 5
        | (value.caps_lock as u8) << 6
        | (value.num_lock as u8) << 7
}

#[cfg(test)]
mod tests {
    use super::*;
    use termlink_terminal_core::{KittyKeyEvent, KittyKeyEventType, KittyModifiers, PasteEvent};

    #[test]
    fn diagnostic_never_copies_paste_or_kitty_text_payload() {
        let paste = InputEvent::Paste(PasteEvent {
            bytes: b"private-paste-text",
        });
        let paste_diagnostic = format_input_diagnostic(
            &InputEncodeError::PasteTooLarge {
                actual: 42,
                maximum: 10,
            },
            paste,
        );
        assert!(paste_diagnostic.is_ascii());
        assert!(paste_diagnostic.len() <= MAX_INPUT_DIAGNOSTIC_BYTES);
        assert!(
            !paste_diagnostic
                .windows(b"private".len())
                .any(|v| v == b"private")
        );

        let kitty = InputEvent::KittyKey(KittyKeyEvent {
            key: KittyKey::Character('a'),
            modifiers: KittyModifiers::NONE,
            event_type: KittyKeyEventType::Press,
            shifted_key: None,
            base_layout_key: None,
            associated_text: "private-kitty-text",
        });
        let kitty_diagnostic = format_input_diagnostic(
            &InputEncodeError::KittyAssociatedTextTooLarge {
                actual_bytes: 18,
                maximum_bytes: 10,
                actual_scalars: 18,
                maximum_scalars: 10,
            },
            kitty,
        );
        assert!(kitty_diagnostic.is_ascii());
        assert!(kitty_diagnostic.len() <= MAX_INPUT_DIAGNOSTIC_BYTES);
        assert!(
            !kitty_diagnostic
                .windows(b"private".len())
                .any(|v| v == b"private")
        );
        assert!(
            kitty_diagnostic
                .windows(b"codepoint=none\n".len())
                .any(|v| v == b"codepoint=none\n")
        );
    }
}
