// Generated from the `Handler` trait of vte 0.15 by forwarding every method to the emulator.
// Only `input`, `set_hyperlink` and `terminal_attribute` are written by hand, in `guard.rs`.

use alacritty_terminal::vte::ansi::{
    CharsetIndex, ClearMode, CursorShape, CursorStyle, Handler, KeyboardModes, KeyboardModesApplyBehavior, LineClearMode, Mode, ModifyOtherKeys,
    PrivateMode, Rgb, ScpCharPath, ScpUpdateMode, StandardCharset, TabulationClearMode,
};
use cursor_icon::CursorIcon;

use super::Guard;

impl Handler for Guard<'_> {
    fn input(&mut self, c: char) {
        self.input_char(c);
    }

    fn set_hyperlink(&mut self, link: Option<alacritty_terminal::vte::ansi::Hyperlink>) {
        self.hyperlink(link);
    }

    fn terminal_attribute(&mut self, attr: alacritty_terminal::vte::ansi::Attr) {
        self.attribute(attr);
    }

    fn set_title(&mut self, a0: Option<String>) {
        Handler::set_title(self.term, a0);
    }

    fn set_cursor_style(&mut self, a0: Option<CursorStyle>) {
        Handler::set_cursor_style(self.term, a0);
    }

    fn set_cursor_shape(&mut self, a0: CursorShape) {
        Handler::set_cursor_shape(self.term, a0);
    }

    fn goto(&mut self, a0: i32, a1: usize) {
        Handler::goto(self.term, a0, a1);
    }

    fn goto_line(&mut self, a0: i32) {
        Handler::goto_line(self.term, a0);
    }

    fn goto_col(&mut self, a0: usize) {
        Handler::goto_col(self.term, a0);
    }

    fn insert_blank(&mut self, a0: usize) {
        Handler::insert_blank(self.term, a0);
    }

    fn move_up(&mut self, a0: usize) {
        Handler::move_up(self.term, a0);
    }

    fn move_down(&mut self, a0: usize) {
        Handler::move_down(self.term, a0);
    }

    fn identify_terminal(&mut self, a0: Option<char>) {
        Handler::identify_terminal(self.term, a0);
    }

    fn device_status(&mut self, a0: usize) {
        Handler::device_status(self.term, a0);
    }

    fn move_forward(&mut self, a0: usize) {
        Handler::move_forward(self.term, a0);
    }

    fn move_backward(&mut self, a0: usize) {
        Handler::move_backward(self.term, a0);
    }

    fn move_down_and_cr(&mut self, a0: usize) {
        Handler::move_down_and_cr(self.term, a0);
    }

    fn move_up_and_cr(&mut self, a0: usize) {
        Handler::move_up_and_cr(self.term, a0);
    }

    fn put_tab(&mut self, a0: u16) {
        Handler::put_tab(self.term, a0);
    }

    fn backspace(&mut self) {
        Handler::backspace(self.term);
    }

    fn carriage_return(&mut self) {
        Handler::carriage_return(self.term);
    }

    fn linefeed(&mut self) {
        Handler::linefeed(self.term);
    }

    fn bell(&mut self) {
        Handler::bell(self.term);
    }

    fn substitute(&mut self) {
        Handler::substitute(self.term);
    }

    fn newline(&mut self) {
        Handler::newline(self.term);
    }

    fn set_horizontal_tabstop(&mut self) {
        Handler::set_horizontal_tabstop(self.term);
    }

    fn scroll_up(&mut self, a0: usize) {
        Handler::scroll_up(self.term, a0);
    }

    fn scroll_down(&mut self, a0: usize) {
        Handler::scroll_down(self.term, a0);
    }

    fn insert_blank_lines(&mut self, a0: usize) {
        Handler::insert_blank_lines(self.term, a0);
    }

    fn delete_lines(&mut self, a0: usize) {
        Handler::delete_lines(self.term, a0);
    }

    fn erase_chars(&mut self, a0: usize) {
        Handler::erase_chars(self.term, a0);
    }

    fn delete_chars(&mut self, a0: usize) {
        Handler::delete_chars(self.term, a0);
    }

    fn move_backward_tabs(&mut self, a0: u16) {
        Handler::move_backward_tabs(self.term, a0);
    }

    fn move_forward_tabs(&mut self, a0: u16) {
        Handler::move_forward_tabs(self.term, a0);
    }

    fn save_cursor_position(&mut self) {
        Handler::save_cursor_position(self.term);
    }

    fn restore_cursor_position(&mut self) {
        Handler::restore_cursor_position(self.term);
    }

    fn clear_line(&mut self, a0: LineClearMode) {
        Handler::clear_line(self.term, a0);
    }

    fn clear_screen(&mut self, a0: ClearMode) {
        Handler::clear_screen(self.term, a0);
    }

    fn clear_tabs(&mut self, a0: TabulationClearMode) {
        Handler::clear_tabs(self.term, a0);
    }

    fn set_tabs(&mut self, a0: u16) {
        Handler::set_tabs(self.term, a0);
    }

    fn reset_state(&mut self) {
        Handler::reset_state(self.term);
    }

    fn reverse_index(&mut self) {
        Handler::reverse_index(self.term);
    }

    fn set_mode(&mut self, a0: Mode) {
        Handler::set_mode(self.term, a0);
    }

    fn unset_mode(&mut self, a0: Mode) {
        Handler::unset_mode(self.term, a0);
    }

    fn report_mode(&mut self, a0: Mode) {
        Handler::report_mode(self.term, a0);
    }

    fn set_private_mode(&mut self, a0: PrivateMode) {
        Handler::set_private_mode(self.term, a0);
    }

    fn unset_private_mode(&mut self, a0: PrivateMode) {
        Handler::unset_private_mode(self.term, a0);
    }

    fn report_private_mode(&mut self, a0: PrivateMode) {
        Handler::report_private_mode(self.term, a0);
    }

    fn set_scrolling_region(&mut self, a0: usize, a1: Option<usize>) {
        Handler::set_scrolling_region(self.term, a0, a1);
    }

    fn set_keypad_application_mode(&mut self) {
        Handler::set_keypad_application_mode(self.term);
    }

    fn unset_keypad_application_mode(&mut self) {
        Handler::unset_keypad_application_mode(self.term);
    }

    fn set_active_charset(&mut self, a0: CharsetIndex) {
        Handler::set_active_charset(self.term, a0);
    }

    fn configure_charset(&mut self, a0: CharsetIndex, a1: StandardCharset) {
        Handler::configure_charset(self.term, a0, a1);
    }

    fn set_color(&mut self, a0: usize, a1: Rgb) {
        Handler::set_color(self.term, a0, a1);
    }

    fn dynamic_color_sequence(&mut self, a0: String, a1: usize, a2: &str) {
        Handler::dynamic_color_sequence(self.term, a0, a1, a2);
    }

    fn reset_color(&mut self, a0: usize) {
        Handler::reset_color(self.term, a0);
    }

    fn clipboard_store(&mut self, a0: u8, a1: &[u8]) {
        Handler::clipboard_store(self.term, a0, a1);
    }

    fn clipboard_load(&mut self, a0: u8, a1: &str) {
        Handler::clipboard_load(self.term, a0, a1);
    }

    fn decaln(&mut self) {
        Handler::decaln(self.term);
    }

    fn push_title(&mut self) {
        Handler::push_title(self.term);
    }

    fn pop_title(&mut self) {
        Handler::pop_title(self.term);
    }

    fn text_area_size_pixels(&mut self) {
        Handler::text_area_size_pixels(self.term);
    }

    fn text_area_size_chars(&mut self) {
        Handler::text_area_size_chars(self.term);
    }

    fn set_mouse_cursor_icon(&mut self, a0: CursorIcon) {
        Handler::set_mouse_cursor_icon(self.term, a0);
    }

    fn report_keyboard_mode(&mut self) {
        Handler::report_keyboard_mode(self.term);
    }

    fn push_keyboard_mode(&mut self, a0: KeyboardModes) {
        Handler::push_keyboard_mode(self.term, a0);
    }

    fn pop_keyboard_modes(&mut self, a0: u16) {
        Handler::pop_keyboard_modes(self.term, a0);
    }

    fn set_keyboard_mode(&mut self, a0: KeyboardModes, a1: KeyboardModesApplyBehavior) {
        Handler::set_keyboard_mode(self.term, a0, a1);
    }

    fn set_modify_other_keys(&mut self, a0: ModifyOtherKeys) {
        Handler::set_modify_other_keys(self.term, a0);
    }

    fn report_modify_other_keys(&mut self) {
        Handler::report_modify_other_keys(self.term);
    }

    fn set_scp(&mut self, a0: ScpCharPath, a1: ScpUpdateMode) {
        Handler::set_scp(self.term, a0, a1);
    }
}
