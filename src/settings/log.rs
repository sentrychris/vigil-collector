//! The log as the Settings window shows it: the newest lines, those of
//! them that match what is typed into the filter, folded to the width of
//! the box.
//!
//! The box draws only the rows in view and asks for each by number, so
//! the folding is done once per width, filter and set of lines, and kept.

use std::collections::VecDeque;

use crate::log::Line;

/// How many lines are kept: as many as the program itself keeps.
const KEEP: usize = 2000;

/// How a row is coloured.
pub const ORDINARY: i32 = 0;
pub const ERROR: i32 = 1;
pub const WARNING: i32 = 2;
pub const DETAIL: i32 = 3;

struct Entry {
    text: String,
    level: i32,
}

/// One row of the box: a line, or the part of a long one that fits.
#[derive(Clone, Copy)]
struct Row {
    line: usize,
    start: usize,
    end: usize,
}

/// The rows for one width and filter.
struct Folded {
    columns: usize,
    filter: String,
    rows: Vec<Row>,
}

#[derive(Default)]
pub struct Log {
    lines: VecDeque<Entry>,
    /// The number of the newest line taken (see ``log::lines_after``).
    newest: u64,
    folded: Option<Folded>,
}

impl Log {
    /// The number to ask for lines after.
    pub fn newest(&self) -> u64 {
        self.newest
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Takes lines newer than those it has. Says whether there were any.
    pub fn take(&mut self, lines: Vec<Line>, newest: u64) -> bool {
        self.newest = newest;
        if lines.is_empty() {
            return false;
        }
        for line in lines {
            if self.lines.len() >= KEEP {
                self.lines.pop_front();
            }
            self.lines.push_back(Entry {
                level: level_of(&line.text),
                text: line.text,
            });
        }
        self.folded = None;
        true
    }

    /// How many rows the lines matching ``filter`` make at ``columns``
    /// characters to a row.
    pub fn rows(&mut self, columns: usize, filter: &str) -> usize {
        self.folded(columns, filter).rows.len()
    }

    /// One of those rows: its text and how it is coloured.
    pub fn row(&mut self, index: usize, columns: usize, filter: &str) -> Option<(&str, i32)> {
        let row = *self.folded(columns, filter).rows.get(index)?;
        let entry = self.lines.get(row.line)?;
        Some((entry.text.get(row.start..row.end)?, entry.level))
    }

    fn folded(&mut self, columns: usize, filter: &str) -> &Folded {
        let columns = columns.max(1);
        let current = self
            .folded
            .as_ref()
            .is_some_and(|folded| folded.columns == columns && folded.filter == filter);
        if !current {
            let needle = filter.trim().to_lowercase();
            let mut rows = Vec::new();
            for (line, entry) in self.lines.iter().enumerate() {
                if !needle.is_empty() && !entry.text.to_lowercase().contains(&needle) {
                    continue;
                }
                fold(&entry.text, columns, |start, end| {
                    rows.push(Row { line, start, end });
                });
            }
            self.folded = Some(Folded {
                columns,
                filter: filter.into(),
                rows,
            });
        }
        self.folded.as_ref().expect("just folded")
    }
}

/// How a line is coloured: by the first level named near its start.
fn level_of(text: &str) -> i32 {
    let start: String = text.chars().take(48).collect();
    start
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .find_map(|word| match word {
            "ERROR" => Some(ERROR),
            "WARN" => Some(WARNING),
            "DEBUG" | "TRACE" => Some(DETAIL),
            "INFO" => Some(ORDINARY),
            _ => None,
        })
        .unwrap_or(ORDINARY)
}

/// Cuts ``text`` into rows of at most ``columns`` characters and calls
/// ``row`` with where each starts and ends. A row ends after a space if
/// there is one in its second half, otherwise wherever it is full.
fn fold(text: &str, columns: usize, mut row: impl FnMut(usize, usize)) {
    let mut start = 0;
    loop {
        let rest = &text[start..];
        // Where the row would end if it were simply filled.
        let Some((full, _)) = rest.char_indices().nth(columns) else {
            row(start, text.len());
            return;
        };
        let half = rest
            .char_indices()
            .nth(columns / 2)
            .map_or(full, |(at, _)| at);
        let end = match rest[half..full].rfind(' ') {
            Some(space) => half + space + 1,
            None => full,
        };
        row(start, start + end);
        start += end;
    }
}

#[cfg(test)]
mod tests {
    use super::{DETAIL, ERROR, Log, ORDINARY, WARNING, fold, level_of};
    use crate::log::Line;

    fn lines(texts: &[&str], from: u64) -> Vec<Line> {
        texts
            .iter()
            .enumerate()
            .map(|(i, text)| Line {
                seq: from + i as u64,
                text: (*text).into(),
            })
            .collect()
    }

    fn folded(text: &str, columns: usize) -> Vec<&str> {
        let mut rows = Vec::new();
        fold(text, columns, |start, end| rows.push(&text[start..end]));
        rows
    }

    #[test]
    fn a_line_that_fits_is_one_row() {
        assert_eq!(folded("", 10), [""]);
        assert_eq!(folded("short", 10), ["short"]);
        assert_eq!(folded("exactly10!", 10), ["exactly10!"]);
    }

    #[test]
    fn a_long_line_is_cut_after_a_space_when_one_is_near() {
        assert_eq!(
            folded("Listening on 0.0.0.0:4500 now", 16),
            ["Listening on ", "0.0.0.0:4500 now"]
        );
    }

    #[test]
    fn a_line_with_nowhere_to_break_is_cut_where_it_is_full() {
        assert_eq!(
            folded("abcdefghijklmnopqrstuvwxyz", 10),
            ["abcdefghij", "klmnopqrst", "uvwxyz"]
        );
        // Characters of more than one byte are never cut in half.
        assert_eq!(folded("ééééé", 2), ["éé", "éé", "é"]);
    }

    #[test]
    fn the_level_is_the_first_one_named_near_the_start() {
        assert_eq!(
            level_of("2026-11-14 10:00:00  INFO Vigil started"),
            ORDINARY
        );
        assert_eq!(level_of("2026-11-14 10:00:00  WARN No WebView2"), WARNING);
        assert_eq!(level_of("2026-11-14 10:00:00 ERROR port in use"), ERROR);
        assert_eq!(level_of("2026-11-14 10:00:00 DEBUG tick"), DETAIL);
        assert_eq!(level_of("2026-11-14 10:00:00 TRACE tick"), DETAIL);
        // Part of another word, or too far along, doesn't count.
        assert_eq!(level_of("2026-11-14 10:00:00 ERRORS galore"), ORDINARY);
        let late = format!("{} ERROR", "x".repeat(60));
        assert_eq!(level_of(&late), ORDINARY);
    }

    #[test]
    fn rows_are_asked_for_by_number() {
        let mut log = Log::default();
        assert!(log.is_empty());
        assert!(!log.take(Vec::new(), 0));
        assert!(log.take(
            lines(&["INFO one", "ERROR two is a longer line", "INFO three"], 1),
            3
        ));
        assert_eq!(log.newest(), 3);
        assert_eq!(log.rows(80, ""), 3);
        assert_eq!(
            log.row(1, 80, ""),
            Some(("ERROR two is a longer line", ERROR))
        );
        assert_eq!(log.row(3, 80, ""), None);

        // Narrower, the long line takes two rows, both coloured as it is.
        assert_eq!(log.rows(16, ""), 4);
        assert_eq!(log.row(1, 16, ""), Some(("ERROR two is a ", ERROR)));
        assert_eq!(log.row(2, 16, ""), Some(("longer line", ERROR)));
    }

    #[test]
    fn the_filter_ignores_case_and_the_space_around_it() {
        let mut log = Log::default();
        log.take(lines(&["INFO Started", "WARN slow", "INFO stopped"], 1), 3);
        assert_eq!(log.rows(80, " ST "), 2);
        assert_eq!(log.row(1, 80, " ST "), Some(("INFO stopped", ORDINARY)));
        assert_eq!(log.rows(80, "nothing like it"), 0);
        // New lines are folded in.
        log.take(lines(&["INFO restarted"], 4), 4);
        assert_eq!(log.rows(80, " ST "), 3);
    }

    #[test]
    fn only_the_newest_lines_are_kept() {
        let mut log = Log::default();
        let many: Vec<String> = (0..2100).map(|i| format!("line {i}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        log.take(lines(&many, 1), 2100);
        assert_eq!(log.rows(80, ""), 2000);
        assert_eq!(log.row(0, 80, ""), Some(("line 100", ORDINARY)));
    }
}
