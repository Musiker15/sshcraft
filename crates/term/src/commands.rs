//! Shell integration (OSC 133): which lines are prompts, and what each command did.
//!
//! A prompt line carries the mark bit (see `marks.rs`), so it moves with the line and goes when
//! the line is erased or leaves the history. The records of the commands are a bounded list, newest
//! last; prompt lines and records are paired from the newest end, so the rows still match their
//! commands after `clear` erased older prompts. The scans that look for prompt lines are bounded,
//! so a server that prints many marks or a very long history cannot make a frame expensive.

use std::collections::VecDeque;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use chrono::{DateTime, Utc};

use crate::filter::ShellMark;
use crate::marks::MARK;
use crate::{Direction, Notice, Terminal};

/// Commands remembered; older ones are dropped.
pub const MAX_COMMANDS: usize = 1000;
/// The most text [`Terminal::last_finished_output`] returns, in bytes.
pub const MAX_OUTPUT_BYTES: usize = 1 << 20;
/// How far up from the cursor a command start looks for its prompt line.
const PROMPT_SEARCH_LINES: i32 = 1000;
/// How many lines from the newest one the pairing of prompt lines and records looks at.
const PAIRING_LINES: i32 = 20_000;
/// Notices about commands kept until they are taken.
const MAX_NOTICES: usize = 256;
/// How far up a mark looks for the earlier mark of the same prompt (a prompt over several lines,
/// a prompt drawn again on the next line).
const PROMPT_HEIGHT: i32 = 50;

/// Where a command is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandState {
    /// The prompt is shown; nothing runs yet.
    Prompt,
    Running,
    Finished,
}

/// One command the shell reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRecord {
    pub state: CommandState,
    /// The exit code, when the shell gave one.
    pub exit: Option<i32>,
    /// When the interface first saw it running.
    pub started: Option<DateTime<Utc>>,
    /// When the interface first saw it finished.
    pub finished: Option<DateTime<Utc>>,
    /// Lines from the prompt line to the first line of output.
    output_offset: Option<usize>,
    /// Lines from the prompt line to the line after the output (where the cursor was at `D`).
    output_end: Option<usize>,
}

/// A visible prompt line and its command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRow {
    /// The row on the screen, 0 at the top.
    pub row: usize,
    pub record: CommandRecord,
}

#[derive(Debug, Default)]
pub(crate) struct Commands {
    records: VecDeque<CommandRecord>,
    notices: Vec<Notice>,
}

impl Commands {
    fn notice(&mut self, notice: Notice) {
        if self.notices.len() < MAX_NOTICES {
            self.notices.push(notice);
        }
    }
}

impl Terminal {
    fn line_marked(&self, line: i32) -> bool {
        let grid = self.term.grid();
        if line < grid.topmost_line().0 || line > grid.bottommost_line().0 {
            return false;
        }
        let row = &grid[Line(line)];
        (0..grid.columns()).any(|col| row[Column(col)].flags.contains(MARK))
    }

    /// Prompt lines from the newest line upwards, at most [`PAIRING_LINES`] lines, stopping at
    /// `top`.
    fn prompt_lines_from_bottom(&self, top: i32) -> Vec<i32> {
        let grid = self.term.grid();
        let bottom = grid.bottommost_line().0;
        let top = top.max(grid.topmost_line().0).max(bottom.saturating_sub(PAIRING_LINES));
        (top..=bottom).rev().filter(|line| self.line_marked(*line)).collect()
    }

    /// Marks `line` for its prompt: the whole line (`full`), or only its last cell, where the
    /// prompt text written after `A` does not reach.
    fn mark_line(&mut self, line: Line, full: bool) {
        let columns = self.term.grid().columns();
        let row = &mut self.term.grid_mut()[line];
        for col in 0..columns {
            row[Column(col)].flags.set(MARK, full || col + 1 == columns);
        }
    }

    fn unmark_line(&mut self, line: i32) {
        let columns = self.term.grid().columns();
        let row = &mut self.term.grid_mut()[Line(line)];
        for col in 0..columns {
            row[Column(col)].flags.remove(MARK);
        }
    }

    /// The nearest marked line above `line`, at most [`PROMPT_HEIGHT`] lines up.
    fn marked_above(&self, line: i32) -> Option<i32> {
        let low = line.saturating_sub(PROMPT_HEIGHT);
        (low..line).rev().find(|l| self.line_marked(*l))
    }

    /// Whether `line` carries only the mark `A` leaves (its last cell): the first line of a prompt
    /// whose `B` has not come yet. The line of an earlier prompt went through `B` and is marked
    /// whole.
    fn marked_by_prompt_start_only(&self, line: i32) -> bool {
        let grid = self.term.grid();
        if line < grid.topmost_line().0 || line > grid.bottommost_line().0 {
            return false;
        }
        let row = &grid[Line(line)];
        let last = grid.columns().saturating_sub(1);
        last > 0 && !row[Column(0)].flags.contains(MARK) && row[Column(last)].flags.contains(MARK)
    }

    pub(crate) fn apply_shell_mark(&mut self, mark: ShellMark) {
        // A full-screen program has its own screen: its marks would pair with nothing.
        if self.term.mode().contains(alacritty_terminal::term::TermMode::ALT_SCREEN) {
            return;
        }
        let cursor = self.term.grid().cursor.point.line;
        match mark {
            ShellMark::PromptStart => {
                if self.commands.records.back().is_some_and(|r| r.state == CommandState::Prompt) {
                    // The same prompt drawn again (an empty Enter, a redraw): its earlier mark
                    // goes, so one prompt keeps one marked line.
                    if !self.line_marked(cursor.0)
                        && let Some(old) = self.marked_above(cursor.0)
                    {
                        self.unmark_line(old);
                    }
                } else {
                    let records = &mut self.commands.records;
                    if records.len() >= MAX_COMMANDS {
                        records.pop_front();
                    }
                    records.push_back(CommandRecord {
                        state: CommandState::Prompt,
                        exit: None,
                        started: None,
                        finished: None,
                        output_offset: None,
                        output_end: None,
                    });
                }
                self.mark_line(cursor, false);
            }
            ShellMark::CommandStart => {
                // The prompt's own line is where the command is typed: it takes the mark from
                // the line `A` marked (the first line of a prompt over several lines). A prompt
                // drawn again after a new window size erased its own line first; the marked line
                // above is then an earlier prompt's, which keeps its mark.
                if !self.line_marked(cursor.0)
                    && self.commands.records.back().is_some_and(|r| r.state == CommandState::Prompt)
                    && let Some(old) = self.marked_above(cursor.0)
                    && self.marked_by_prompt_start_only(old)
                {
                    self.unmark_line(old);
                }
                self.mark_line(cursor, true);
            }
            ShellMark::OutputStart => {
                // Only a waiting prompt starts a command, so a flood of `C` costs nothing more.
                if self.commands.records.back().is_none_or(|r| r.state != CommandState::Prompt) {
                    return;
                }
                let low = cursor.0.saturating_sub(PROMPT_SEARCH_LINES);
                let prompt = (low..=cursor.0).rev().find(|line| self.line_marked(*line));
                if let Some(record) = self.commands.records.back_mut() {
                    record.state = CommandState::Running;
                    record.output_offset = prompt.and_then(|p| usize::try_from(cursor.0.saturating_sub(p)).ok());
                    self.commands.notice(Notice::CommandStarted);
                }
            }
            ShellMark::CommandEnd(exit) => {
                // A prompt that ran nothing (an empty Enter) stays a prompt.
                if self.commands.records.back().is_none_or(|r| r.state != CommandState::Running) {
                    return;
                }
                let low = cursor.0.saturating_sub(PROMPT_SEARCH_LINES);
                let prompt = (low..=cursor.0).rev().find(|line| self.line_marked(*line));
                if let Some(record) = self.commands.records.back_mut() {
                    record.state = CommandState::Finished;
                    record.exit = exit;
                    record.output_end = prompt.and_then(|p| usize::try_from(cursor.0.saturating_sub(p)).ok());
                    self.commands.notice(Notice::CommandFinished { exit });
                }
            }
        }
    }

    pub(crate) fn take_command_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.commands.notices)
    }

    /// The commands the shell reported, oldest first, at most [`MAX_COMMANDS`].
    pub fn command_history(&self) -> Vec<CommandRecord> {
        self.commands.records.iter().cloned().collect()
    }

    /// Gives commands that started or finished since the last call the time `now`.
    pub fn stamp_commands(&mut self, now: DateTime<Utc>) {
        self.changed();
        for record in self.commands.records.iter_mut().rev() {
            match record.state {
                CommandState::Prompt => {}
                CommandState::Running => {
                    if record.started.is_none() {
                        record.started = Some(now);
                    }
                }
                CommandState::Finished => {
                    if record.finished.is_some() {
                        // Everything older was stamped before.
                        break;
                    }
                    record.finished = Some(now);
                    record.started.get_or_insert(now);
                }
            }
        }
    }

    /// Scrolls the next prompt above (`Up`) or below (`Down`) the top of the view to the top.
    pub fn jump_to_prompt(&mut self, direction: Direction) -> bool {
        self.jump_to_mark(direction)
    }

    /// The visible prompt lines with their commands.
    pub(crate) fn command_rows(&self, display_offset: usize, rows: usize) -> Vec<CommandRow> {
        let offset = i32::try_from(display_offset).unwrap_or(i32::MAX);
        let top = offset.saturating_neg();
        let last_row = i32::try_from(rows).unwrap_or(i32::MAX).saturating_sub(1);
        let mut out: Vec<CommandRow> = self
            .prompt_lines_from_bottom(top)
            .into_iter()
            .zip(self.commands.records.iter().rev())
            .filter_map(|(line, record)| {
                let row = line.saturating_add(offset);
                (0..=last_row).contains(&row).then(|| CommandRow { row: usize::try_from(row).unwrap_or(0), record: record.clone() })
            })
            .collect();
        out.reverse();
        out
    }

    /// The output of the newest finished command, line by line, at most [`MAX_OUTPUT_BYTES`].
    pub fn last_finished_output(&self) -> Option<String> {
        let prompts = self.prompt_lines_from_bottom(i32::MIN);
        let (index, record) = self.commands.records.iter().rev().enumerate().find(|(_, r)| r.state == CommandState::Finished)?;
        let prompt = *prompts.get(index)?;
        let first = prompt.saturating_add(i32::try_from(record.output_offset?).ok()?);
        // The output ends where the cursor was when the command ended, else before the next
        // prompt line, else at the newest line.
        let next = index.checked_sub(1).and_then(|i| prompts.get(i).copied());
        let last = match record.output_end.and_then(|e| i32::try_from(e).ok()) {
            Some(end) => prompt.saturating_add(end).saturating_sub(1),
            None => next.map_or(self.term.grid().bottommost_line().0, |n| n.saturating_sub(1)),
        };
        Some(self.lines_text(first, last))
    }

    /// The lines of the output from the top row on screen to the bottom of the terminal, the one
    /// the top row belongs to included: a line that wraps over several rows counts once. A log
    /// viewer maps the screen to the output's line breaks with it.
    pub fn logical_lines_from_top(&self) -> usize {
        let grid = self.term.grid();
        let Some(last) = grid.columns().checked_sub(1) else { return 0 };
        let top = -i32::try_from(grid.display_offset()).unwrap_or(i32::MAX);
        let bottom = grid.bottommost_line().0;
        // Every row that is not continued in the next one ends a line.
        (top.max(grid.topmost_line().0)..=bottom).filter(|&line| !grid[Line(line)][Column(last)].flags.contains(Flags::WRAPLINE)).count()
    }

    /// The text of lines `first..=last`; a wrapped line continues without a line break.
    fn lines_text(&self, first: i32, last: i32) -> String {
        let grid = self.term.grid();
        let columns = grid.columns();
        let mut text = String::new();
        let first = first.max(grid.topmost_line().0);
        let last = last.min(grid.bottommost_line().0);
        for line in first..=last {
            let row = &grid[Line(line)];
            let mut part = String::new();
            for col in 0..columns {
                let cell = &row[Column(col)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                part.push(cell.c);
                if let Some(zerowidth) = cell.zerowidth() {
                    part.extend(zerowidth);
                }
            }
            let wrapped = columns.checked_sub(1).is_some_and(|c| row[Column(c)].flags.contains(Flags::WRAPLINE));
            if wrapped {
                text.push_str(&part);
            } else {
                text.push_str(part.trim_end());
                text.push('\n');
            }
            if text.len() > MAX_OUTPUT_BYTES {
                break;
            }
        }
        let mut end = text.trim_end().len().min(MAX_OUTPUT_BYTES);
        while !text.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        text.truncate(end);
        text
    }
}
