//! Text field adapted from gpui's `examples/input.rs`; supports an optional multi-line mode
//! (Alt+Enter inserts a newline, Enter submits).

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, rgba, size, App, Bounds, ClipboardItem,
    Context, CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine,
    SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window,
};
use unicode_segmentation::UnicodeSegmentation;

actions!(
    todobar_input,
    [
        Backspace, Delete, Left, Right, SelectLeft, SelectRight, SelectAll, Home, End, Paste, Cut,
        Copy, Submit, Newline, Cancel, Up, Down,
    ]
);

pub fn bind_keys(cx: &mut App) {
    let ctx = Some("TodoInput");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("ctrl-a", SelectAll, ctx),
        KeyBinding::new("ctrl-v", Paste, ctx),
        KeyBinding::new("ctrl-c", Copy, ctx),
        KeyBinding::new("ctrl-x", Cut, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("enter", Submit, ctx),
        KeyBinding::new("alt-enter", Newline, ctx),
        KeyBinding::new("escape", Cancel, ctx),
        KeyBinding::new("up", Up, ctx),
        KeyBinding::new("down", Down, ctx),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Submit,
    Cancel,
    Up,
    Down,
}

impl EventEmitter<InputEvent> for TextInput {}

pub struct TextInput {
    focus_handle: FocusHandle,
    pub content: SharedString,
    pub placeholder: SharedString,
    pub multiline: bool,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Vec<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

impl TextInput {
    pub fn new(placeholder: &str, multiline: bool, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.to_string().into(),
            multiline,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: vec![],
            last_bounds: None,
            is_selecting: false,
        }
    }

    pub fn text(&self) -> String {
        self.content.to_string()
    }

    pub fn set_text(&mut self, text: &str, window: &Window, cx: &mut Context<Self>) {
        self.content = text.to_string().into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    fn line_count(&self) -> usize {
        if self.multiline {
            self.content.matches('\n').count() + 1
        } else {
            1
        }
    }

    /// (line index, byte offset of line start) containing `offset`.
    fn line_of(&self, offset: usize) -> (usize, usize) {
        let mut start = 0;
        for (i, line) in self.content.split('\n').enumerate() {
            let end = start + line.len();
            if offset <= end {
                return (i, start);
            }
            start = end + 1;
        }
        (0, 0)
    }

    fn line_starts(&self) -> Vec<usize> {
        let mut starts = vec![0];
        for (i, b) in self.content.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        starts
    }

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), window, cx);
        } else {
            self.move_to(self.selected_range.start, window, cx)
        }
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), window, cx);
        } else {
            self.move_to(self.selected_range.end, window, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
        self.select_to(self.content.len(), window, cx)
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        let (_, start) = self.line_of(self.cursor_offset());
        self.move_to(start, window, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        let (i, start) = self.line_of(self.cursor_offset());
        let len = self.content.split('\n').nth(i).map_or(0, str::len);
        self.move_to(start + len, window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == prev {
                return;
            }
            self.select_to(prev, window, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                return;
            }
            self.select_to(next, window, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Submit);
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace_text_in_range(None, "\n", window, cx);
        }
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Cancel);
    }

    fn vertical(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let (line, start) = self.line_of(self.cursor_offset());
        let target = line as isize + delta;
        if !self.multiline || target < 0 || target as usize >= self.line_count() {
            cx.emit(if delta < 0 { InputEvent::Up } else { InputEvent::Down });
            return;
        }
        let col = self.content[start..self.cursor_offset()].graphemes(true).count();
        let starts = self.line_starts();
        let t = target as usize;
        let text = self.content.split('\n').nth(t).unwrap_or("");
        let offset: usize = text.graphemes(true).take(col).map(str::len).sum();
        self.move_to(starts[t] + offset, window, cx);
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1, window, cx);
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1, window, cx);
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        let idx = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(idx, window, cx);
        } else {
            self.move_to(idx, window, cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), window, cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = if self.multiline { text } else { text.replace('\n', " ") };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let Some(bounds) = self.last_bounds.as_ref() else { return 0 };
        let line_h = bounds.size.height / self.line_count() as f32;
        let row = ((position.y - bounds.top()) / line_h).floor().max(0.0) as usize;
        let row = row.min(self.last_layout.len().saturating_sub(1));
        let Some(line) = self.last_layout.get(row) else { return 0 };
        let start = self.line_starts().get(row).copied().unwrap_or(0);
        start + line.closest_index_for_x(position.x - bounds.left())
    }

    fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn set_selected_text_range(&mut self, range_utf16: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = self.range_from_utf16(&range_utf16);
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn text_length_utf16(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<usize> {
        Some(self.content.encode_utf16().count())
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.selection_reversed = false;
        self.marked_range.take();
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        self.marked_range = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        let insertion_start_utf16 = self.offset_to_utf16(range.start);
        let inserted_length_utf16 = new_text.encode_utf16().count();
        self.selected_range = new_selected_range_utf16
            .map(|selected| {
                self.range_from_utf16(
                    &(insertion_start_utf16 + selected.start.min(inserted_length_utf16)
                        ..insertion_start_utf16 + selected.end.min(inserted_length_utf16)),
                )
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let (row, start) = self.line_of(range.start);
        let line = self.last_layout.get(row)?;
        let line_h = bounds.size.height / self.line_count() as f32;
        let top = bounds.top() + line_h * row as f32;
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start.saturating_sub(start)), top),
            point(bounds.left() + line.x_for_index(range.end.saturating_sub(start)), top + line_h),
        ))
    }

    fn selection_bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.bounds_for_range(range_utf16, bounds, window, cx).into_iter().collect()
    }

    fn character_index_for_point(
        &mut self,
        pt: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let idx = self.index_for_mouse_position(pt);
        Some(self.offset_to_utf16(idx))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    lines: Vec<ShapedLine>,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let lines = self.input.read(cx).line_count();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (window.line_height() * lines as f32).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_h = window.line_height();
        let empty = content.is_empty();
        let mut color = style.color;
        if empty {
            color.a *= 0.45;
        }
        let text: SharedString = if empty { input.placeholder.clone() } else { content.clone() };
        let starts = input.line_starts();
        let marked = input.marked_range.clone();

        let mut lines = vec![];
        let pieces: Vec<&str> = if empty { vec![text.as_ref()] } else { text.split('\n').collect() };
        for (i, piece) in pieces.iter().enumerate() {
            let start = if empty { 0 } else { starts[i] };
            let run = TextRun {
                len: piece.len(),
                font: style.font(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs = match marked.as_ref().filter(|_| !empty) {
                Some(m) if m.start < start + piece.len() && m.end > start => {
                    let ms = m.start.max(start) - start;
                    let me = m.end.min(start + piece.len()) - start;
                    [
                        TextRun { len: ms, ..run.clone() },
                        TextRun {
                            len: me - ms,
                            underline: Some(UnderlineStyle { color: Some(color), thickness: px(1.0), wavy: false }),
                            ..run.clone()
                        },
                        TextRun { len: piece.len() - me, ..run },
                    ]
                    .into_iter()
                    .filter(|r| r.len > 0)
                    .collect()
                }
                _ => vec![run],
            };
            lines.push(window.text_system().shape_line(
                SharedString::from(piece.to_string()),
                font_size,
                &runs,
                None,
            ));
        }

        let mut selections = vec![];
        let mut cursor_quad = None;
        if selected_range.is_empty() {
            let (row, start) = input.line_of(cursor);
            if let Some(line) = lines.get(row) {
                let x = if empty { px(0.) } else { line.x_for_index(cursor - start) };
                cursor_quad = Some(fill(
                    Bounds::new(
                        point(bounds.left() + x, bounds.top() + line_h * row as f32),
                        size(px(1.5), line_h),
                    ),
                    style.color,
                ));
            }
        } else {
            for (row, line) in lines.iter().enumerate() {
                let start = starts[row];
                let end = start + line.len;
                let s = selected_range.start.max(start);
                let e = selected_range.end.min(end);
                if s <= e && selected_range.start <= end && selected_range.end >= start {
                    let top = bounds.top() + line_h * row as f32;
                    selections.push(fill(
                        Bounds::from_corners(
                            point(bounds.left() + line.x_for_index(s - start), top),
                            point(bounds.left() + line.x_for_index(e - start) + px(2.), top + line_h),
                        ),
                        rgba(0x0a84ff55),
                    ));
                }
            }
        }
        PrepaintState { lines, cursor: cursor_quad, selections }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.input.clone()), cx);
        for s in prepaint.selections.drain(..) {
            window.paint_quad(s);
        }
        let line_h = window.line_height();
        for (row, line) in prepaint.lines.iter().enumerate() {
            let origin = point(bounds.origin.x, bounds.origin.y + line_h * row as f32);
            line.paint(origin, line_h, gpui::TextAlign::Left, None, window, cx).ok();
        }
        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }
        let lines = std::mem::take(&mut prepaint.lines);
        self.input.update(cx, |input, _| {
            input.last_layout = lines;
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .w_full()
            .key_context("TodoInput")
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

