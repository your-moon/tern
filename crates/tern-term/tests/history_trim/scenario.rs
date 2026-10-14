//! Deterministic terminal sessions and a byte-exact dump of the grid they leave.
//!
//! Uses only the public `alacritty_terminal` API, so the same file runs against upstream 0.26.0
//! and against the vendored crate that trims history rows. `golden.txt` holds the dump hashes
//! upstream produced; the vendored crate must reproduce every one.

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;

pub const HISTORY: usize = 300;

#[derive(Clone, Copy)]
pub struct Size {
    pub cols: usize,
    pub lines: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

pub struct Session {
    pub term: Term<VoidListener>,
    parser: Processor,
    rng: u64,
}

impl Session {
    pub fn new(seed: u64, cols: usize, lines: usize, history: usize) -> Self {
        let config = Config {
            scrolling_history: history,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &Size { cols, lines }, VoidListener),
            parser: Processor::new(),
            rng: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, cols: usize, lines: usize) {
        self.term.resize(Size { cols, lines });
    }

    fn next(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn text(&mut self, out: &mut String, max: u64) {
        const POOL: &[&str] = &[
            "a",
            "b",
            "c",
            "x",
            "Z",
            "7",
            " ",
            " ",
            "-",
            "e\u{301}",
            "\u{5b57}",
            "\u{1f987}",
            "\u{ff21}",
            "hello ",
            "wrap ",
            "\t",
        ];
        // Skewed short, so most lines leave a long blank tail and a few wrap.
        let len = self.below(max);
        for _ in 0..self.below(len + 1) {
            let i = self.below(POOL.len() as u64) as usize;
            out.push_str(POOL[i]);
        }
    }

    /// `ops` well-formed operations: text, colours, tabs, cursor moves, erases, scroll regions.
    pub fn random_ops(&mut self, ops: usize) -> Vec<u8> {
        let (cols, lines) = (self.term.columns() as u64, self.term.screen_lines() as u64);
        let mut s = String::new();
        for _ in 0..ops {
            match self.below(100) {
                0..=39 => {
                    self.text(&mut s, cols * 2);
                    s.push_str("\r\n");
                }
                40..=47 => self.text(&mut s, 12),
                48..=55 => match self.below(7) {
                    0 | 5 => s.push_str("\x1b[0m"),
                    6 => s.push_str("\x1b[49m"),
                    1 => s.push_str(&format!("\x1b[{}m", 31 + self.below(7))),
                    2 => s.push_str(&format!("\x1b[48;5;{}m", self.below(256))),
                    3 => s.push_str(&format!(
                        "\x1b[38;2;{};{};{}m",
                        self.below(256),
                        self.below(256),
                        self.below(256)
                    )),
                    _ => s.push_str(&format!(
                        "\x1b[{}m",
                        [1, 3, 4, 7, 9][self.below(5) as usize]
                    )),
                },
                56..=63 => {
                    let n = 1 + self.below(cols);
                    let c = ['A', 'B', 'C', 'D'][self.below(4) as usize];
                    s.push_str(&format!("\x1b[{n}{c}"));
                }
                64..=67 => s.push_str(&format!(
                    "\x1b[{};{}H",
                    1 + self.below(lines),
                    1 + self.below(cols)
                )),
                68..=79 => {
                    // Erase with whatever background is current: blank cells with a colour.
                    let seq = ["\x1b[K", "\x1b[1K", "\x1b[2K", "\x1b[J", "\x1b[1J"];
                    s.push_str(seq[self.below(5) as usize]);
                }
                80..=84 => {
                    let n = 1 + self.below(cols / 2);
                    let c = ['X', 'P', '@'][self.below(3) as usize];
                    s.push_str(&format!("\x1b[{n}{c}"));
                }
                85..=88 => {
                    let top = 1 + self.below(lines - 1);
                    let bottom = top + 1 + self.below(lines - top);
                    s.push_str(&format!("\x1b[{top};{bottom}r"));
                }
                89..=90 => s.push_str("\x1b[r"),
                91..=93 => {
                    let n = 1 + self.below(4);
                    let c = ['S', 'T', 'L', 'M'][self.below(4) as usize];
                    s.push_str(&format!("\x1b[{n}{c}"));
                }
                94 => {
                    s.push_str("\x1b[?1049h\x1b[2J");
                    self.text(&mut s, 30);
                    s.push_str("\x1b[?1049l");
                }
                95 => s.push_str("\x1b[0m"),
                96 => s.push_str("\x1b[49m"),
                97 => s.push_str("\x1b[2J"),
                _ => s.push_str("\r\n\r\n"),
            }
        }
        s.into_bytes()
    }

    /// Plain wrapped lines of varied length, for reflow: every other line overflows the width.
    pub fn wrapped_lines(&mut self, count: usize) -> Vec<u8> {
        let mut s = String::new();
        for i in 0..count {
            let len = if i % 2 == 0 {
                self.below(120)
            } else {
                self.below(30)
            };
            for _ in 0..len {
                s.push(char::from(b'a' + self.below(26) as u8));
            }
            if self.below(5) == 0 {
                s.push_str("\u{5b57}\u{5b57}");
            }
            s.push_str("\r\n");
        }
        s.into_bytes()
    }
}

/// Every line from the top of history to the bottom of the screen, each cell's character,
/// zero-width characters, colours and flags, plus cursor and display offset.
pub fn dump(term: &Term<VoidListener>) -> String {
    let grid = term.grid();
    let mut out = String::new();
    out.push_str(&format!(
        "cols={} lines={} history={} offset={} cursor={:?} wrap={}\n",
        grid.columns(),
        grid.screen_lines(),
        grid.history_size(),
        grid.display_offset(),
        grid.cursor.point,
        grid.cursor.input_needs_wrap,
    ));
    for line in grid.topmost_line().0..=grid.bottommost_line().0 {
        let row = &grid[Line(line)];
        out.push_str(&format!("{line} len={}:", row.len()));
        for col in 0..grid.columns() {
            let cell = &row[Column(col)];
            out.push_str(&format!(
                "[{:?} {:?} {:?} {:#x} {:?}]",
                cell.c,
                cell.fg,
                cell.bg,
                cell.flags.bits(),
                cell.zerowidth()
            ));
        }
        out.push('\n');
    }
    out
}

/// `dump` after every history row has been given its full width back through the mutable
/// accessors. A row stored trimmed must read exactly like one that never was.
pub fn dump_expanded(term: &Term<VoidListener>) -> String {
    let mut full = Term::new(
        Config {
            scrolling_history: HISTORY,
            ..Config::default()
        },
        &Size {
            cols: term.columns(),
            lines: term.screen_lines(),
        },
        VoidListener,
    );
    *full.grid_mut() = term.grid().clone();
    let grid = full.grid_mut();
    for line in grid.topmost_line().0..=grid.bottommost_line().0 {
        let _ = &mut grid[Line(line)][..];
    }
    dump(&full)
}

pub fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Records a labelled hash of the dump, after checking the expanded copy dumps identically.
fn checkpoint(out: &mut Vec<(String, u64)>, label: String, term: &Term<VoidListener>) {
    let text = dump(term);
    assert!(
        text == dump_expanded(term),
        "trimmed rows read differently from full ones at {label}"
    );
    out.push((label, fnv(&text)));
}

/// Random output with resizes, taller and shorter screens and scrolling in between.
pub fn mixed(seed: u64) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut s = Session::new(seed, 40, 10, HISTORY);
    let l = |n: &str| format!("mixed{seed}/{n}");
    let ops = s.random_ops(1500);
    s.feed(&ops);
    checkpoint(&mut out, l("fed"), &s.term);
    s.resize(23, 10);
    checkpoint(&mut out, l("narrow"), &s.term);
    let ops = s.random_ops(500);
    s.feed(&ops);
    checkpoint(&mut out, l("fed-narrow"), &s.term);
    s.resize(67, 10);
    checkpoint(&mut out, l("wide"), &s.term);
    s.resize(67, 16);
    checkpoint(&mut out, l("taller"), &s.term);
    let ops = s.random_ops(400);
    s.feed(&ops);
    s.term.scroll_display(Scroll::Delta(7));
    let ops = s.random_ops(100);
    s.feed(&ops);
    checkpoint(&mut out, l("scrolled-back"), &s.term);
    s.resize(67, 6);
    checkpoint(&mut out, l("shorter"), &s.term);
    s.resize(31, 9);
    checkpoint(&mut out, l("narrow-again"), &s.term);
    s.resize(90, 9);
    checkpoint(&mut out, l("wide-again"), &s.term);
    let ops = s.random_ops(600);
    s.feed(&ops);
    checkpoint(&mut out, l("final"), &s.term);
    out
}

/// A long history of wrapped lines through narrower/wider and wider/narrower resizes.
pub fn reflow(seed: u64) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let l = |n: &str| format!("reflow{seed}/{n}");

    let mut s = Session::new(seed, 80, 12, HISTORY);
    let text = s.wrapped_lines(500);
    s.feed(&text);
    checkpoint(&mut out, l("fed"), &s.term);
    for (i, cols) in [30, 120, 45, 80, 20, 200, 80].into_iter().enumerate() {
        s.resize(cols, 12);
        checkpoint(&mut out, l(&format!("narrow-wide-{i}-{cols}")), &s.term);
    }

    let mut s = Session::new(seed, 30, 12, HISTORY);
    let text = s.wrapped_lines(500);
    s.feed(&text);
    for (i, cols) in [90, 15, 60, 30].into_iter().enumerate() {
        s.resize(cols, 12);
        checkpoint(&mut out, l(&format!("wide-narrow-{i}-{cols}")), &s.term);
    }
    out
}

pub fn all() -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for seed in 1..=6 {
        out.extend(mixed(seed));
    }
    for seed in 1..=3 {
        out.extend(reflow(seed));
    }
    out
}
