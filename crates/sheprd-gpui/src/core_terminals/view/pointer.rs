//! The pointer over a herdr-core pane. A program that reads the mouse gets
//! its presses, drags, and wheel; otherwise a press selects text, a double
//! click a word and a triple click a line, and the wheel scrolls history.
//! Shift selects even over a program that reads the mouse, as terminals do.

use super::CoreTerminalView;
use crate::{
    core_terminals::selection::{Cell, Selection},
    terminal::wheel_rows,
    terminal_painter::{Highlight, Tint},
};
use gpui::{prelude::*, *};
use sheprd_backend::{
    ClientMouseButton, ClientMouseKind, ClientMousePosition, ClientPaneInputEvent,
};
use std::time::{Duration, Instant};

/// How often a drag held past the top or bottom edge scrolls a row.
const AUTOSCROLL: Duration = Duration::from_millis(50);

/// Where a press is going: to the program, or into a selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Press {
    /// A press the program reads; `bare` when it was a passthrough
    /// right-click, whose whole gesture reaches the program without the
    /// modifier, as Herdr strips it.
    Reported {
        button: ClientMouseButton,
        bare: bool,
    },
    Selecting,
}

fn button(button: MouseButton) -> Option<ClientMouseButton> {
    match button {
        MouseButton::Left => Some(ClientMouseButton::Left),
        MouseButton::Right => Some(ClientMouseButton::Right),
        MouseButton::Middle => Some(ClientMouseButton::Middle),
        _ => None,
    }
}

/// The modifiers a gesture reports: none for a passthrough right-click,
/// which reaches the program without Herdr's
/// `right_click_passthrough_modifier`. A herdr-core pane has no pane menu
/// yet, so its other right-clicks reach the program as they are.
fn reported(held: Modifiers, bare: bool) -> Modifiers {
    if bare { Modifiers::default() } else { held }
}

fn modifier_bits(modifiers: Modifiers) -> u8 {
    u8::from(modifiers.shift)
        | (u8::from(modifiers.control) << 1)
        | (u8::from(modifiers.alt) << 2)
        | (u8::from(modifiers.platform) << 3)
}

impl CoreTerminalView {
    /// The history row at the top of the screen.
    pub(crate) fn top(&self) -> u32 {
        let history = self.screen.history;
        u32::try_from(history.history.saturating_sub(history.offset)).unwrap_or(u32::MAX)
    }

    /// The cell under `position`, held inside the screen.
    pub(super) fn cell_at(&self, position: Point<Pixels>) -> Option<Cell> {
        self.cell_at_when(position, crate::motion::now())
    }

    /// The row under `position` at `now`, as a press would find it.
    #[cfg(test)]
    pub(crate) fn row_at(&self, position: Point<Pixels>, now: Instant) -> Option<u16> {
        self.cell_at_when(position, now).map(|cell| cell.row)
    }

    /// The cell painted under `position` at `now`, a slide's lift included.
    fn cell_at_when(&self, position: Point<Pixels>, now: Instant) -> Option<Cell> {
        let bounds = self.bounds?;
        let frame = self.screen.frame.as_ref()?;
        if frame.width == 0 || frame.height == 0 {
            return None;
        }
        let x = f32::from(position.x - bounds.origin.x) / self.look.cell_width.max(1.);
        let y = f32::from(position.y - bounds.origin.y) / self.look.cell_height.max(1.)
            + self.lift(now);
        // Clamped to the screen first, so the casts cannot truncate.
        let column = x.clamp(0., f32::from(frame.width - 1)) as u16;
        let row = y.clamp(0., f32::from(frame.height - 1)) as u16;
        Some(Cell { row, column })
    }

    fn report(&self, kind: ClientMouseKind, cell: Cell, modifiers: Modifiers, lines: u16) {
        self.send(ClientPaneInputEvent::Mouse {
            kind,
            position: ClientMousePosition::Cell {
                column: cell.column,
                row: cell.row,
            },
            geometry: None,
            modifiers: modifier_bits(modifiers),
            lines,
        });
    }

    pub(super) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if self.press_link(event) {
            return;
        }
        let Some(cell) = self.cell_at(event.position) else {
            return;
        };
        if self.screen.mouse_reporting && !event.modifiers.shift {
            if let Some(pressed) = button(event.button) {
                let bare = event.button == MouseButton::Right
                    && self.look.right_click_passthrough.matches(event.modifiers);
                self.press = Some(Press::Reported {
                    button: pressed,
                    bare,
                });
                let modifiers = reported(event.modifiers, bare);
                self.report(ClientMouseKind::Down(pressed), cell, modifiers, 1);
                // The program took the click, so the pane's menu stays shut.
                cx.stop_propagation();
            }
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        let Some(frame) = self.screen.frame.as_deref() else {
            return;
        };
        let top = self.top();
        self.selection = Some(match event.click_count {
            2 => Selection::word(frame, top, cell),
            count if count >= 3 => Selection::line(frame, top, cell),
            _ => Selection::at(cell.point(top)),
        });
        self.press = Some(Press::Selecting);
        self.dragged_to = Some(event.position);
        cx.notify();
    }

    pub(super) fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hover_link(event.position, event.modifiers, cx);
        let Some(cell) = self.cell_at(event.position) else {
            return;
        };
        match self.press {
            Some(Press::Reported { button, bare }) if event.pressed_button.is_some() => {
                let modifiers = reported(event.modifiers, bare);
                self.report(ClientMouseKind::Drag(button), cell, modifiers, 1);
            }
            Some(Press::Selecting) if event.pressed_button == Some(MouseButton::Left) => {
                self.dragged_to = Some(event.position);
                let top = self.top();
                if let Some(selection) = &mut self.selection {
                    selection.extend(cell.point(top));
                    cx.notify();
                }
            }
            // Movement with no button held, for a program that asks for
            // every motion (mode 1003); the encoder drops it otherwise.
            None if event.pressed_button.is_none()
                && self.screen.mouse_reporting
                && !event.modifiers.shift
                && self.hovered != Some(cell) =>
            {
                self.hovered = Some(cell);
                self.report(ClientMouseKind::Moved, cell, event.modifiers, 1);
            }
            _ => {}
        }
    }

    pub(super) fn mouse_up(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.release_link(event, window, cx) {
            return;
        }
        match self.press.take() {
            Some(Press::Reported { button, bare }) => {
                if let Some(cell) = self.cell_at(event.position) {
                    let modifiers = reported(event.modifiers, bare);
                    self.report(ClientMouseKind::Up(button), cell, modifiers, 1);
                }
            }
            // Releasing a selection copies it while `copy_on_select` is on;
            // otherwise it stays highlighted for Cmd-C or Ctrl-C.
            Some(Press::Selecting)
                if self.look.copy_on_select
                    && event.button == MouseButton::Left
                    && self.copy_selection() =>
            {
                self.announce_copy();
            }
            Some(Press::Selecting) | None => {}
        }
        self.dragged_to = None;
    }

    /// A move outside the view while a selection is dragged, which the
    /// view's own listener, firing only while hovered, does not see.
    pub(super) fn drag_outside(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let outside = self
            .bounds
            .is_some_and(|bounds| !bounds.contains(&event.position));
        if !outside
            || self.press != Some(Press::Selecting)
            || event.pressed_button != Some(MouseButton::Left)
        {
            return;
        }
        self.dragged_to = Some(event.position);
        let top = self.top();
        if let (Some(cell), Some(selection)) =
            (self.cell_at(event.position), self.selection.as_mut())
        {
            selection.extend(cell.point(top));
            cx.notify();
        }
    }

    /// Runs every tick while a selection is dragged: a pointer held above
    /// or below the screen scrolls the history a row at a time, and the
    /// selection follows to the row that comes into view.
    pub(super) fn follow_drag(&mut self, cx: &mut Context<Self>) {
        let (Some(Press::Selecting), Some(pointer), Some(bounds)) =
            (self.press, self.dragged_to, self.bounds)
        else {
            return;
        };
        let up = pointer.y < bounds.top();
        if !up && pointer.y <= bounds.bottom() {
            return;
        }
        let top = self.top();
        if let (Some(cell), Some(selection)) = (self.cell_at(pointer), self.selection.as_mut()) {
            selection.extend(cell.point(top));
            cx.notify();
        }
        let now = Instant::now();
        if self
            .scrolled_at
            .is_some_and(|at| now.saturating_duration_since(at) < AUTOSCROLL)
        {
            return;
        }
        let history = self.screen.history;
        let offset = if up {
            history.offset.saturating_add(1).min(history.history)
        } else {
            history.offset.saturating_sub(1)
        };
        if offset != history.offset {
            self.scroll_history(offset);
            self.scrolled_at = Some(now);
        }
    }

    /// Whole rows of wheel motion, the remainder kept for the next event,
    /// as one event carrying the count: the terminal scrolls history by it,
    /// or repeats it as reports for a program reading the wheel.
    pub(super) fn scroll_wheel(
        &mut self,
        event: &ScrollEvent,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        let Some(cell) = self.cell_at(event.position) else {
            return;
        };
        self.wheel_rest += wheel_rows(event, self.look.cell_height, self.look.scroll_lines);
        let steps = self.wheel_rest.trunc();
        self.wheel_rest -= steps;
        if steps == 0. {
            return;
        }
        let kind = if steps > 0. {
            ClientMouseKind::ScrollUp
        } else {
            ClientMouseKind::ScrollDown
        };
        // Clamped to a u16 first, so the cast cannot truncate.
        let lines = steps.abs().min(f32::from(u16::MAX)) as u16;
        self.report(kind, cell, event.modifiers, lines);
    }

    /// Ends the copy of `copied`: the copied flash, and its highlight
    /// dropped unless `keep_selection_after_copy` keeps it. A selection
    /// begun since the copy was asked for is not the copy's to drop.
    pub(super) fn finish_copy(
        &mut self,
        copied: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.look.keep_selection && self.selection == Some(copied) {
            self.selection = None;
        }
        cx.notify();
        if let Some(to_window) = &self.to_window {
            to_window(super::ToWindow::Copied, window, cx);
        }
    }

    /// Copies the selection; false when nothing is selected. The text is
    /// read from the history, shown or not, so wrapped rows are joined the
    /// same way however far the view has scrolled, and copied when it
    /// arrives.
    pub(crate) fn copy_selection(&mut self) -> bool {
        let Some(range) = self.selection.and_then(|selection| selection.range()) else {
            return false;
        };
        self.copy_range(range);
        true
    }

    /// The selection as the painter tints it.
    pub(crate) fn highlights(&self) -> Vec<Highlight> {
        let (width, height) = self
            .screen
            .frame
            .as_ref()
            .map_or((0, 0), |frame| (frame.width, frame.height));
        let top = self.top();
        self.selection
            .map(|selection| {
                selection
                    .visible_rows(width, top, height)
                    .into_iter()
                    .map(|(row, columns)| Highlight {
                        row,
                        columns,
                        tint: Tint::Selection,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
