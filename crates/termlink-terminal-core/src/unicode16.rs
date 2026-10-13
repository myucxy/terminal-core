//! UAX #29 extended grapheme clusters, Unicode 16.0.0 (GB1–GB999).
//! Source property tables and conformance data retain Unicode License V3.
#[path = "unicode16_data.rs"]
mod data;

const CR: u8 = 1;
const LF: u8 = 2;
const CONTROL: u8 = 3;
const EXTEND: u8 = 4;
const ZWJ: u8 = 5;
const RI: u8 = 6;
const PREPEND: u8 = 7;
const SPACING_MARK: u8 = 8;
const L: u8 = 9;
const V: u8 = 10;
const T: u8 = 11;
const LV: u8 = 12;
const LVT: u8 = 13;

fn property(character: char, table: &[(u32, u32, u8)]) -> u8 {
    let value = u32::from(character);
    let position = table.partition_point(|(_, end, _)| *end < value);
    table
        .get(position)
        .filter(|(start, _, _)| *start <= value)
        .map_or(0, |(_, _, property)| *property)
}

fn grapheme_property(character: char) -> u8 {
    if character.is_ascii() {
        match character {
            '\r' => CR,
            '\n' => LF,
            '\0'..='\u{1f}' | '\u{7f}' => CONTROL,
            _ => 0,
        }
    } else {
        property(character, data::GRAPHEME_BREAK)
    }
}

/// Constant-memory streaming context. It never buffers an unbounded cluster.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct GraphemeSegmenter {
    previous: Option<u8>,
    odd_regional_indicators: bool,
    pictograph_extend: bool,
    zwj_after_pictograph: bool,
    indic: u8,
}

impl GraphemeSegmenter {
    pub(crate) fn break_before(&mut self, character: char) -> bool {
        let next = grapheme_property(character);
        let (pictograph, indic) = if character.is_ascii() {
            (false, 0)
        } else {
            (
                property(character, data::EXTENDED_PICTOGRAPHIC) != 0,
                property(character, data::INDIC_CONJUNCT_BREAK),
            )
        };
        let boundary = match self.previous {
            None => true,                                                    // GB1
            Some(CR) if next == LF => false,                                 // GB3
            Some(CR | LF | CONTROL) => true,                                 // GB4
            Some(_) if matches!(next, CR | LF | CONTROL) => true,            // GB5
            Some(L) if matches!(next, L | V | LV | LVT) => false,            // GB6
            Some(LV | V) if matches!(next, V | T) => false,                  // GB7
            Some(LVT | T) if next == T => false,                             // GB8
            Some(_) if matches!(next, EXTEND | ZWJ | SPACING_MARK) => false, // GB9/9a
            Some(PREPEND) => false,                                          // GB9b
            Some(_) if indic == 1 && self.indic == 2 => false,               // GB9c
            Some(ZWJ) if self.zwj_after_pictograph && pictograph => false,   // GB11
            Some(RI) if next == RI && self.odd_regional_indicators => false, // GB12/13
            Some(_) => true,                                                 // GB999
        };
        self.odd_regional_indicators = next == RI && !self.odd_regional_indicators;
        self.zwj_after_pictograph = next == ZWJ && self.pictograph_extend;
        self.pictograph_extend = pictograph || (next == EXTEND && self.pictograph_extend);
        self.indic = match indic {
            1 => 1,                    // Consonant
            2 => self.indic,           // Extend
            3 if self.indic != 0 => 2, // Linker after a consonant
            _ => 0,
        };
        self.previous = Some(next);
        boundary
    }
}

/// Profile 2 uses narrow ambiguous characters, Unicode EAW W/F, and emoji presentation.
/// Width zero is retained for an orphan mark; the terminal chooses its visible base cell.
pub(crate) fn cluster_width(cluster: &str) -> u8 {
    let presentation = cluster
        .chars()
        .rev()
        .find(|c| matches!(c, '\u{fe0e}' | '\u{fe0f}'));
    let text_presentation = presentation == Some('\u{fe0e}');
    let emoji_presentation = presentation == Some('\u{fe0f}');
    let keycap = cluster.ends_with('\u{20e3}')
        && cluster.starts_with(|c: char| c == '#' || c == '*' || c.is_ascii_digit());
    let mut width = u8::from(keycap) * 2;
    for scalar in cluster.chars() {
        let property_value = grapheme_property(scalar);
        if matches!(property_value, CR | LF | CONTROL | EXTEND | ZWJ)
            || property(scalar, data::DEFAULT_IGNORABLE) != 0
        {
            continue;
        }
        let wide = property(scalar, data::WIDE) != 0
            || (!text_presentation && property(scalar, data::EMOJI_PRESENTATION) != 0)
            || (emoji_presentation && property(scalar, data::EMOJI) != 0);
        width = width.max(if wide { 2 } else { 1 });
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_official_unicode_16_grapheme_break_test() {
        let fixture = include_str!("../../../third_party/unicode/16.0.0/GraphemeBreakTest.txt");
        let mut cases = 0;
        for (line_number, line) in fixture.lines().enumerate() {
            let content = line.split('#').next().unwrap().trim();
            if content.is_empty() {
                continue;
            }
            cases += 1;
            let mut segmenter = GraphemeSegmenter::default();
            let mut expected = false;
            for token in content.split_whitespace() {
                match token {
                    "÷" => expected = true,
                    "×" => expected = false,
                    _ => {
                        let scalar =
                            char::from_u32(u32::from_str_radix(token, 16).unwrap()).unwrap();
                        assert_eq!(
                            segmenter.break_before(scalar),
                            expected,
                            "official line {}, token {token}",
                            line_number + 1
                        );
                    }
                }
            }
            assert!(expected, "GB2 end of text");
        }
        assert_eq!(cases, 1093);
    }

    #[test]
    fn fixed_width_profile_covers_presentation_flags_indic_and_marks() {
        for (cluster, expected) in [
            ("a", 1),
            ("a\u{fe0f}", 1),
            ("©", 1),
            ("©\u{fe0f}", 2),
            ("©\u{fe0e}", 1),
            ("中", 2),
            ("·", 1),
            ("🇨🇳", 2),
            ("👍🏽", 2),
            ("👩\u{200d}💻", 2),
            ("\u{301}", 0),
            ("a\u{301}", 1),
            ("क्\u{200d}ष", 1),
            ("#\u{fe0f}\u{20e3}", 2),
            ("1\u{20e3}", 2),
            ("\u{1100}\u{1161}\u{11a8}", 2),
            ("\u{200b}", 0),
        ] {
            assert_eq!(cluster_width(cluster), expected, "{cluster:?}");
        }
    }

    #[test]
    fn streaming_context_does_not_leak_across_unrelated_clusters() {
        let cases = [
            ("🇨🇳🇺🇸🇯", vec![true, false, true, false, true]),
            (
                "👩\u{301}\u{200d}💻a\u{200d}💻",
                vec![true, false, false, false, true, false, true],
            ),
            ("क्\u{200d}षक", vec![true, false, false, false, true]),
        ];
        for (text, expected) in cases {
            let mut context = GraphemeSegmenter::default();
            assert_eq!(
                text.chars()
                    .map(|c| context.break_before(c))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}
