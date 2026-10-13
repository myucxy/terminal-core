//! Bounded DEC page memory. Alternate-screen applications retain one page.
use crate::{CsiSequence, Screen, Terminal, TerminalError};

pub(crate) const MAX_PAGES: u8 = 6;
pub(crate) const MAX_EXTRA_CELLS: usize = 32 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PageStore {
    pub(crate) active: u8,
    pub(crate) visible: u8,
    pub(crate) saved: u8,
    pub(crate) coupled: bool,
    pub(crate) extra: [Option<Screen>; MAX_PAGES as usize - 1],
}

impl Default for PageStore {
    fn default() -> Self {
        Self {
            active: 1,
            visible: 1,
            saved: 1,
            coupled: true,
            extra: std::array::from_fn(|_| None),
        }
    }
}

impl PageStore {
    pub(crate) fn screens(&self) -> impl Iterator<Item = &Screen> {
        self.extra.iter().flatten()
    }
    pub(crate) fn screens_mut(&mut self) -> impl Iterator<Item = &mut Screen> {
        self.extra.iter_mut().flatten()
    }
    pub(crate) fn has_state(&self) -> bool {
        !self.coupled
            || self.active != 1
            || self.visible != 1
            || self.saved != 1
            || self.screens().next().is_some()
    }
    pub(crate) fn screen<'a>(&'a self, main: &'a Screen, page: u8) -> &'a Screen {
        if page == 1 {
            main
        } else {
            self.extra[usize::from(page - 2)]
                .as_ref()
                .expect("allocated page")
        }
    }
    pub(crate) fn screen_mut<'a>(&'a mut self, main: &'a mut Screen, page: u8) -> &'a mut Screen {
        if page == 1 {
            main
        } else {
            self.extra[usize::from(page - 2)]
                .as_mut()
                .expect("allocated page")
        }
    }
    pub(crate) fn validate(
        &self,
        columns: u32,
        rows: u32,
        profile: crate::WidthProfile,
    ) -> Result<(), TerminalError> {
        for id in [self.active, self.visible, self.saved] {
            if !(1..=MAX_PAGES).contains(&id)
                || (id != 1 && self.extra[usize::from(id - 2)].is_none())
            {
                return Err(TerminalError::MalformedSnapshot);
            }
        }
        if self.coupled && self.active != self.visible {
            return Err(TerminalError::MalformedSnapshot);
        }
        if self.screens().map(|s| s.cells.len()).sum::<usize>() > MAX_EXTRA_CELLS {
            return Err(TerminalError::MalformedSnapshot);
        }
        for screen in self.screens() {
            if screen.columns != columns
                || screen.rows != rows
                || screen.width_profile != profile
                || screen.requested_history_rows() != 0
            {
                return Err(TerminalError::MalformedSnapshot);
            }
            screen.validate()?;
        }
        Ok(())
    }
}

impl Terminal {
    pub(super) fn active_page(&self) -> u8 {
        if self.active_alternate {
            1
        } else {
            self.pages.active
        }
    }
    pub(super) fn visible_page(&self) -> u8 {
        if self.active_alternate {
            1
        } else {
            self.pages.visible
        }
    }
    pub(super) fn visible_screen(&self) -> &Screen {
        if self.active_alternate {
            &self.alternate
        } else {
            self.pages.screen(&self.main, self.pages.visible)
        }
    }
    pub(super) fn page_screen(&self, page: u8) -> &Screen {
        if self.active_alternate {
            &self.alternate
        } else {
            self.pages.screen(&self.main, page)
        }
    }
    pub(super) fn ensure_page(&mut self, page: u8) -> Result<(), TerminalError> {
        if self.active_alternate || page == 1 || self.pages.extra[usize::from(page - 2)].is_some() {
            return Ok(());
        }
        let cells = self.columns as usize * self.rows as usize;
        if cells > MAX_EXTRA_CELLS - self.pages.screens().map(|s| s.cells.len()).sum::<usize>() {
            return Err(TerminalError::ProtocolResourceLimit(
                "DEC page memory exceeds 32768 additional cells",
            ));
        }
        let mut screen = Screen::new(self.columns, self.rows);
        screen.width_profile = self.width_profile();
        self.pages.extra[usize::from(page - 2)] = Some(screen);
        Ok(())
    }
    pub(super) fn move_page(&mut self, page: u32) -> Result<(), TerminalError> {
        if self.active_alternate {
            return Ok(());
        }
        let page = page.clamp(1, u32::from(MAX_PAGES)) as u8;
        self.ensure_page(page)?;
        let old = self.active_screen();
        let (row, column, wrap, attributes, protected) = (
            old.cursor_row,
            old.cursor_column,
            old.wrap_pending,
            old.attributes,
            old.protected,
        );
        self.pages.active = page;
        if self.pages.coupled {
            self.pages.visible = page;
        }
        let target = self.active_screen_mut();
        target.position_cursor(row, column);
        target.wrap_pending = wrap;
        target.attributes = attributes;
        target.set_protected(protected);
        self.revalidate_active_wrap_pending();
        Ok(())
    }
    pub(super) fn copy_page_rectangle(&mut self, csi: &CsiSequence) -> Result<(), TerminalError> {
        crate::require_max_parameters(csi, 8, "DECCRA parameters")?;
        let page = |index| csi.parameter(index, 1).clamp(1, u16::from(MAX_PAGES)) as u8;
        let source = page(4);
        let destination = page(7);
        let Some(rectangle) = self.rectangle_coordinates(csi, 0) else {
            return Ok(());
        };
        let origin = self.rectangle_origin_coordinates(csi, 5);
        self.ensure_page(source)?;
        self.ensure_page(destination)?;
        let target_page = if self.active_alternate {
            7
        } else {
            destination
        };
        if self.graphics.as_ref().is_some_and(|graphics| {
            graphics
                .placements()
                .iter()
                .any(|p| p.anchor.page == target_page)
        }) {
            let from = if self.active_alternate || source == destination {
                None
            } else if source == 1 {
                Some(self.main.clone())
            } else {
                self.pages.extra[usize::from(source - 2)].clone()
            };
            return self.with_page_graphics_motion(
                target_page,
                crate::GraphicMotion::Preserve,
                |screen| {
                    screen.copy_rectangle_from(from.as_ref(), rectangle, origin);
                    if screen.has_line_renditions() {
                        screen.finish_line_edit()?;
                    }
                    Ok(())
                },
            );
        }
        if self.active_alternate || source == destination {
            let screen = if self.active_alternate {
                &mut self.alternate
            } else {
                self.pages.screen_mut(&mut self.main, source)
            };
            screen.copy_rectangle_from(None, rectangle, origin);
        } else if destination == 1 {
            let from = self.pages.extra[usize::from(source - 2)]
                .as_ref()
                .expect("allocated page");
            self.main.copy_rectangle_from(Some(from), rectangle, origin);
        } else if source == 1 {
            self.pages.extra[usize::from(destination - 2)]
                .as_mut()
                .expect("allocated page")
                .copy_rectangle_from(Some(&self.main), rectangle, origin);
        } else {
            let (left, right) = self
                .pages
                .extra
                .split_at_mut(usize::from(source.max(destination) - 2));
            let (from, to) = if source < destination {
                (
                    left[usize::from(source - 2)].as_ref().unwrap(),
                    right[0].as_mut().unwrap(),
                )
            } else {
                (
                    right[0].as_ref().unwrap(),
                    left[usize::from(destination - 2)].as_mut().unwrap(),
                )
            };
            to.copy_rectangle_from(Some(from), rectangle, origin);
        }
        let target = if self.active_alternate {
            &mut self.alternate
        } else {
            self.pages.screen_mut(&mut self.main, destination)
        };
        if target.has_line_renditions() {
            target.finish_line_edit()?;
        }
        Ok(())
    }
}
