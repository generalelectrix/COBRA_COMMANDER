//! A basic interface for fixtures to render "previews" to some kind of visualizer.
//! For starters, we'll just allow things to write to the terminal. Mostly for
//! debugging as well as offline practice learning the controller.

use std::{
    cell::{Cell, RefCell},
    fmt::Display,
    io::{Stdout, StdoutLock, Write, stdout},
};

use owo_colors::OwoColorize;

use crate::color::ColorRgb;

/// Manage state for preview via terminal/ANSI escape codes.
pub struct TerminalPreview {
    lines_written: Cell<usize>,
    stdout: Stdout,
}

impl Default for TerminalPreview {
    fn default() -> Self {
        let mut s = stdout();
        let _ = write!(s, "{}", termion::cursor::Hide);
        Self {
            lines_written: Default::default(),
            stdout: s,
        }
    }
}

impl Drop for TerminalPreview {
    fn drop(&mut self) {
        let _ = write!(self.stdout, "{}", termion::cursor::Show);
        let _ = self.stdout.flush();
    }
}

impl TerminalPreview {
    fn fixture<'a>(&'a self, leader: &'a dyn Display) -> TerminalFixturePreview<'a> {
        TerminalFixturePreview {
            preview: self,
            written: Default::default(),
            leader,
            w: RefCell::new(self.stdout.lock()),
        }
    }

    fn add_line(&self) {
        self.lines_written.set(self.lines_written.get() + 1);
    }

    fn start_frame(&self) {
        let mut w = self.stdout.lock();
        let n = self.lines_written.take();
        for _ in 0..n {
            let _ = write!(w, "{}", termion::cursor::Up(1));
        }
        let _ = w.flush();
    }
}

/// Write previews into the terminal using text and command codes.
///
/// Assumes that whatever we're writing into is infallible - ignores all errors.
pub struct TerminalFixturePreview<'a> {
    /// Reference back to the preview that created this.
    preview: &'a TerminalPreview,
    /// True once we've written something.
    written: Cell<bool>,
    /// Something to write to the terminal on the first write.
    leader: &'a dyn Display,
    /// Writer to write into.
    w: RefCell<StdoutLock<'static>>,
}

impl<'a> TerminalFixturePreview<'a> {
    /// Write something.
    fn write(&self, d: impl Display) {
        let mut w = self.w.borrow_mut();
        if !self.written.replace(true) {
            let _ = write!(w, "{}: ", self.leader);
        }
        let _ = write!(w, "{}", d);
    }

    fn color(&self, [r, g, b]: ColorRgb) {
        self.write("▮".truecolor(r, g, b).on_truecolor(r, g, b));
    }

    fn intensity_u8(&self, i: u8) {
        self.write("▮".truecolor(i, i, i).on_truecolor(i, i, i));
    }
}

impl<'a> Drop for TerminalFixturePreview<'a> {
    fn drop(&mut self) {
        if self.written.get() {
            self.preview.add_line();
            let _ = writeln!(self.w.borrow_mut());
        }
    }
}

/// Previewer implementations.
#[derive(Default)]
pub enum Previewer {
    #[default]
    Off,
    Terminal(TerminalPreview),
}

impl Previewer {
    pub fn terminal() -> Self {
        Self::Terminal(TerminalPreview::default())
    }

    /// Initialize the previewer at the start of a frame.
    pub fn start_frame(&self) {
        match self {
            Self::Off => (),
            Self::Terminal(t) => t.start_frame(),
        }
    }

    pub fn for_group<'a>(&'a self, leader: &'a dyn Display) -> FixturePreviewer<'a> {
        match self {
            Self::Off => FixturePreviewer::Off,
            Self::Terminal(t) => FixturePreviewer::Terminal(t.fixture(leader)),
        }
    }
}

#[derive(Default)]
pub enum FixturePreviewer<'a> {
    #[default]
    Off,
    Terminal(TerminalFixturePreview<'a>),
}

impl<'a> FixturePreviewer<'a> {
    #[expect(unused)]
    /// Preview a color.
    pub fn color(&self, c: ColorRgb) {
        match self {
            Self::Off => (),
            Self::Terminal(t) => t.color(c),
        }
    }

    /// Preview a color, computed using a closure.
    ///
    /// Only call the closure if the color will actually be used.
    pub fn color_lazy(&self, f: impl FnOnce() -> ColorRgb) {
        match self {
            Self::Off => (),
            Self::Terminal(t) => t.color(f()),
        }
    }

    pub fn intensity_u8(&self, i: u8) {
        match self {
            Self::Off => (),
            Self::Terminal(t) => t.intensity_u8(i),
        }
    }
}
