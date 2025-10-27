//! A basic interface for fixtures to render "previews" to some kind of visualizer.
//! For starters, we'll just allow things to write to the terminal. Mostly for
//! debugging as well as offline practice learning the controller.

use std::{
    cell::{Cell, RefCell},
    fmt::Display,
    io::Write,
};

use number::UnipolarFloat;
use owo_colors::OwoColorize;

use crate::{color::ColorRgb, fixture::FixtureGroup, util::unipolar_to_range};

/// Write previews into the terminal using text and command codes.
///
/// Assumes that whatever we're writing into is infallible - ignores all errors.
struct TerminalFixturePreview<'a> {
    /// True once we've written something.
    written: Cell<bool>,
    /// Something to write to the terminal on the first write.
    leader: &'a dyn Display,
    /// Writer to write into.
    w: RefCell<&'a mut dyn Write>,
}

impl<'a> TerminalFixturePreview<'a> {
    pub fn new(w: &'a mut dyn Write, leader: &'a dyn Display) -> Self {
        Self {
            written: Default::default(),
            leader,
            w: RefCell::new(w),
        }
    }

    /// Write something.
    fn write(&self, d: impl Display) {
        let mut w = self.w.borrow_mut();
        if !self.written.replace(true) {
            let _ = write!(w, "{}", self.leader);
        }
        let _ = write!(w, "{}", d);
    }

    /// Return the number of lines written.
    pub fn line_count(&self) -> usize {
        self.written.get() as usize
    }
}

impl<'a> FixturePreview for TerminalFixturePreview<'a> {
    fn color(&self, [r, g, b]: ColorRgb) {
        self.write("▮".truecolor(r, g, b).on_truecolor(r, g, b));
    }

    fn intensity_u8(&self, i: u8) {
        self.write("▮".truecolor(i, i, i).on_truecolor(i, i, i));
    }

    fn finish(self) {
        if self.written.get() {
            let _ = writeln!(self.w.borrow_mut());
        }
    }
}

pub trait FixturePreview {
    /// Indicate in a preview that a fixture is a particular color.
    /// Fixtures may call this multiple times.
    fn color(&self, c: ColorRgb);

    /// Indicate a fixture intensity in a preview.
    fn intensity(&self, i: UnipolarFloat) {
        self.intensity_u8(unipolar_to_range(0, 255, i));
    }

    /// Indicate a fixture intensity in a preview.
    fn intensity_u8(&self, i: u8);

    /// Complete preview for this fixture and perform any finalization required.
    fn finish(self);
}

pub trait Previewer {
    fn for_group(&self, g: &FixtureGroup) -> &dyn FixturePreview;
}

const BRICK: &str = "▮";
