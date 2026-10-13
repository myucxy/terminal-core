use crate::{
    Terminal, TerminalError,
    charset::{CharsetSize, MappedGraphic},
    drcs_bank::{GlyphLookup, GlyphReference},
    graphics_resources::ImageFormat,
};

impl Terminal {
    pub(crate) fn resolve_graphic(&self, mapped: MappedGraphic) -> (char, Option<GlyphReference>) {
        let Some(position) = mapped.position else {
            return (mapped.character, None);
        };
        let Some(graphics) = &self.graphics else {
            return (mapped.character, None);
        };
        match graphics.fonts().lookup(
            mapped.designation.designator(),
            mapped.designation.size == CharsetSize::Characters96,
            self.columns,
            self.rows,
            position,
        ) {
            GlyphLookup::HardCharset => (mapped.character, None),
            GlyphLookup::Undefined => (' ', None),
            GlyphLookup::Glyph(glyph) => {
                // A mask may replace a hard repertoire too; its pixels do not establish Unicode text.
                ('\u{fffd}', Some(glyph))
            }
        }
    }

    pub(crate) fn process_mapped_graphic(
        &mut self,
        mapped: MappedGraphic,
    ) -> Result<(), TerminalError> {
        let (character, glyph) = self.resolve_graphic(mapped);
        if let Some(glyph) = glyph {
            self.print_terminal_graphic(character, Some(glyph), false)?;
            self.repeat_source = Some(crate::RepeatSource {
                character,
                combining: String::new(),
                width: 1,
                glyph: Some(glyph),
            });
            Ok(())
        } else {
            self.process_character(character)
        }
    }

    fn graphic_cells(&self) -> impl Iterator<Item = &crate::Cell> {
        self.main
            .cells
            .iter()
            .chain(self.alternate.cells.iter())
            .chain(
                self.pages
                    .extra
                    .iter()
                    .flatten()
                    .flat_map(|screen| screen.cells.iter()),
            )
    }

    pub(crate) fn cell_graphic_resources(&self) -> Vec<u64> {
        let mut ids: Vec<_> = self
            .graphic_cells()
            .filter_map(|c| c.glyph.map(|g| g.resource))
            .chain(
                self.repeat_source
                    .as_ref()
                    .and_then(|r| r.glyph)
                    .map(|g| g.resource),
            )
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    pub(crate) fn validate_graphic_cells(&self) -> Result<(), TerminalError> {
        for (glyph, width, continuation, combining) in self
            .graphic_cells()
            .filter_map(|c| {
                c.glyph
                    .map(|g| (g, c.width, c.continuation, c.combining.as_str()))
            })
            .chain(
                self.repeat_source
                    .as_ref()
                    .and_then(|r| r.glyph.map(|g| (g, r.width, false, r.combining.as_str()))),
            )
        {
            let image = self
                .graphics
                .as_ref()
                .and_then(|state| state.resources().get(glyph.resource))
                .ok_or(TerminalError::MalformedSnapshot)?
                .image();
            if width != 1
                || continuation
                || !combining.is_empty()
                || image.format() != ImageFormat::Alpha8
                || glyph.width == 0
                || glyph.height == 0
                || glyph.x >= image.width()
                || glyph.width > image.width() - glyph.x
                || glyph.height > image.height()
            {
                return Err(TerminalError::MalformedSnapshot);
            }
        }
        Ok(())
    }
}
