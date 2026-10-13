use crate::{Attributes, CsiSequence, Terminal, TerminalError, UnderlineStyle};

pub(crate) const CAPACITY: usize = 10;
pub(crate) const SELECTORS: u32 = 1
    | (1 << 1)
    | (1 << 2)
    | (1 << 3)
    | (1 << 4)
    | (1 << 5)
    | (1 << 7)
    | (1 << 8)
    | (1 << 9)
    | (1 << 21)
    | (1 << 30)
    | (1 << 31);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SavedRendition {
    pub attributes: Attributes,
    pub mask: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SgrStack {
    entries: [Option<SavedRendition>; CAPACITY],
    len: usize,
}

impl SgrStack {
    pub fn entries(&self) -> impl Iterator<Item = &SavedRendition> {
        self.entries[..self.len].iter().flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn push(&mut self, entry: SavedRendition) {
        if self.len == CAPACITY {
            self.entries.rotate_left(1);
            self.len -= 1;
        }
        self.entries[self.len] = Some(entry);
        self.len += 1;
    }

    fn pop(&mut self) -> Option<SavedRendition> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        self.entries[self.len].take()
    }
}

impl Terminal {
    pub(super) fn push_rendition(&mut self, csi: &CsiSequence) {
        let mut mask = u32::from(csi.parameters().is_empty());
        for &selector in csi.parameters() {
            if (1..=31).contains(&selector) {
                mask |= (1_u32 << selector) & SELECTORS;
            }
        }
        self.sgr_stack.push(SavedRendition {
            attributes: self.active_screen().attributes,
            mask,
        });
    }

    pub(super) fn pop_rendition(&mut self, csi: &CsiSequence) -> Result<(), TerminalError> {
        if !csi.parameters().is_empty() {
            return Err(TerminalError::Unsupported("XTPOPSGR parameters"));
        }
        let Some(entry) = self.sgr_stack.pop() else {
            return Ok(());
        };
        let target = &mut self.active_screen_mut().attributes;
        if entry.mask & 1 != 0 {
            *target = entry.attributes;
            return Ok(());
        }
        let saved = entry.attributes;
        let has = |bit: u32| entry.mask & (1_u32 << bit) != 0;
        macro_rules! restore {
            ($bit:expr, $field:ident) => {
                if has($bit) {
                    target.$field = saved.$field;
                }
            };
        }
        restore!(1, bold);
        restore!(2, faint);
        restore!(3, italic);
        restore!(5, blink);
        restore!(7, inverse);
        restore!(8, conceal);
        restore!(9, strikethrough);
        restore!(30, foreground);
        restore!(31, background);
        match (has(4), has(21)) {
            (true, true) => target.underline_style = saved.underline_style,
            (true, false) => match saved.underline_style {
                UnderlineStyle::None | UnderlineStyle::Double => {
                    if target.underline_style != UnderlineStyle::Double {
                        target.underline_style = UnderlineStyle::None;
                    }
                }
                style => target.underline_style = style,
            },
            (false, true) => {
                if saved.underline_style == UnderlineStyle::Double {
                    target.underline_style = UnderlineStyle::Double;
                } else if target.underline_style == UnderlineStyle::Double {
                    target.underline_style = UnderlineStyle::None;
                }
            }
            (false, false) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Event};

    fn feed(t: &mut Terminal, bytes: &[u8]) {
        t.apply_event(Event::Output {
            source_sequence: t.next_source_sequence(),
            bytes,
        })
        .unwrap();
    }

    #[test]
    fn complete_rendition_and_aliases_restore_at_every_split() {
        for (push, pop) in [("#{", "#}"), ("#p", "#q")] {
            let input = format!(
                "\x1b[1;2;3;4:3;5;7;8;9;53;38;2;1;2;3;48;5;99;58;2;4;5;6m\x1b[{push}\x1b[0m\x1b[{pop}"
            );
            for split in 0..=input.len() {
                let mut t = Terminal::new(8, 2).unwrap();
                feed(&mut t, &input.as_bytes()[..split]);
                feed(&mut t, &input.as_bytes()[split..]);
                let attr = t.active_screen().attributes;
                assert!(
                    attr.bold
                        && attr.faint
                        && attr.italic
                        && attr.blink
                        && attr.inverse
                        && attr.conceal
                        && attr.strikethrough
                        && attr.overline
                );
                assert_eq!(attr.underline_style, UnderlineStyle::Curly);
                assert_eq!(attr.foreground, Color::Rgb(1, 2, 3));
                assert_eq!(attr.background, Color::Indexed(99));
                assert_eq!(attr.underline_color, Color::Rgb(4, 5, 6));
                assert!(t.sgr_stack.is_empty());
            }
        }
    }

    #[test]
    fn selected_attributes_leave_other_attributes_and_protection_untouched() {
        let mut t = Terminal::new(8, 2).unwrap();
        feed(
            &mut t,
            b"\x1b[1;31;44m\x1b[1;30#{\x1b[0;3;32;45m\x1b[1\"q\x1b[#}",
        );
        let attr = t.active_screen().attributes;
        assert!(attr.bold && attr.italic);
        assert_eq!(attr.foreground, Color::Indexed(1));
        assert_eq!(attr.background, Color::Indexed(5));
        feed(&mut t, b"X");
        assert!(t.active_screen().cells[0].protected);
        feed(&mut t, b"\x1b[999#{\x1b[33m\x1b[#}");
        assert_eq!(t.active_screen().attributes.foreground, Color::Indexed(3));
    }

    #[test]
    fn selected_underline_styles_follow_independent_single_double_flags() {
        for (before, selector, current, expected) in [
            ("4:3", "4", "21", UnderlineStyle::Curly),
            ("21", "4", "21", UnderlineStyle::Double),
            ("21", "4", "4", UnderlineStyle::None),
            ("24", "21", "4:5", UnderlineStyle::Dashed),
            ("24", "21", "21", UnderlineStyle::None),
            ("21", "21", "4:3", UnderlineStyle::Double),
            ("24", "4;21", "21", UnderlineStyle::None),
        ] {
            let mut t = Terminal::new(8, 2).unwrap();
            feed(
                &mut t,
                format!("\x1b[{before}m\x1b[{selector}#{{\x1b[{current}m\x1b[#}}").as_bytes(),
            );
            assert_eq!(t.active_screen().attributes.underline_style, expected);
        }
    }

    #[test]
    fn overflow_discards_oldest_and_empty_pop_does_nothing() {
        let mut t = Terminal::new(8, 2).unwrap();
        for index in 0..12 {
            feed(&mut t, format!("\x1b[38;5;{index}m\x1b[#{{").as_bytes());
        }
        assert_eq!(t.sgr_stack.entries().count(), CAPACITY);
        for index in (2..12).rev() {
            feed(&mut t, b"\x1b[#}");
            assert_eq!(
                t.active_screen().attributes.foreground,
                Color::Indexed(index)
            );
        }
        feed(&mut t, b"\x1b[#}");
        assert_eq!(t.active_screen().attributes.foreground, Color::Indexed(2));
    }

    #[test]
    fn snapshot_suffix_alt_screen_and_reset_preserve_defined_stack_lifetime() {
        let mut t = Terminal::new(8, 2).unwrap();
        feed(&mut t, b"\x1b[2;4:3;31m\x1b[#{\x1b[0m\x1b[?1049h\x1b[!p");
        let snapshot = t.export_snapshot_v2().unwrap();
        assert_eq!(u16::from_le_bytes([snapshot[10], snapshot[11]]), 11);
        let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
        assert_eq!(restored.export_snapshot_v2().unwrap(), snapshot);
        for input in [
            b"\x1b[#}A".as_slice(),
            b"\x1b[?1049l",
            b"\x1b[#{\x1bc\x1b[#}",
        ] {
            feed(&mut t, input);
            feed(&mut restored, input);
            assert_eq!(t, restored);
        }
        assert!(t.sgr_stack.is_empty());
        assert_eq!(t.active_screen().attributes, Attributes::default());
    }
}
