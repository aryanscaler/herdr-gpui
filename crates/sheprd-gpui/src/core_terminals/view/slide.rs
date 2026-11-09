//! Smooth scrolling for a herdr-core view. The view's emulator scrolls its
//! history in whole rows; with `smooth_scroll` on, each scroll slides into
//! place instead of jumping, the frame before it drawn behind to fill the
//! rows the slide uncovers, as a Herdr 1 pane's wheel scroll is drawn.
//! New output is never slid: only a move through unchanged text is.

use super::CoreTerminalView;
use crate::core_terminals::screen::Screen;
use sheprd_backend::FrameData;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// How long a scroll takes to slide into place, as a Herdr 1 pane's.
const SPREAD: Duration = Duration::from_millis(48);

pub(super) struct Slide {
    /// The frame shown before the scroll.
    behind: Arc<FrameData>,
    /// Rows the content moved down from `behind`: positive scrolls into
    /// the history, negative back toward the live screen.
    moved: f32,
    /// Rows the frame is drawn above its place when the slide starts.
    from: f32,
    since: Instant,
}

/// Where to draw the view's frame, and the frame behind it, while sliding.
pub(crate) struct Sliding {
    /// Rows the frame is drawn above its place; negative draws it below.
    pub(crate) lift: f32,
    pub(crate) behind: Arc<FrameData>,
    /// Rows the frame behind is drawn below the frame.
    pub(crate) behind_at: f32,
}

impl CoreTerminalView {
    /// Starts a slide from `before` to the screen now shown, when only its
    /// place in the history changed.
    pub(super) fn slide_from(&mut self, before: &Screen) {
        if !self.look.smooth_scroll {
            self.slide = None;
            return;
        }
        let (Some(behind), Some(frame)) = (&before.frame, &self.screen.frame) else {
            return;
        };
        let moved = i64::try_from(self.screen.history.offset).unwrap_or(i64::MAX)
            - i64::try_from(before.history.offset).unwrap_or(i64::MAX);
        let same_text = before.revision == self.screen.revision
            && (behind.width, behind.height) == (frame.width, frame.height);
        if moved == 0 || !same_text || moved.unsigned_abs() >= u64::from(frame.height) {
            // Output or a jump too far to slide: draw it where it is.
            if !same_text || moved != 0 {
                self.slide = None;
            }
            return;
        }
        let now = crate::motion::now();
        // A slide still under way goes on from where it is drawn, as far as
        // the frame behind, `moved` rows off, can fill what it uncovers:
        // past that, rows it never showed would be left blank.
        let left = self.sliding(now).map_or(0., |sliding| sliding.lift);
        let moved = moved as f32;
        self.slide = Some(Slide {
            behind: behind.clone(),
            moved,
            from: (moved + left).clamp(moved.min(0.), moved.max(0.)),
            since: now,
        });
    }

    /// Rows the frame is drawn above its place at `now`, so the pointer
    /// lands on the row painted under it while a slide is under way.
    pub(super) fn lift(&self, now: Instant) -> f32 {
        self.slide
            .as_ref()
            .and_then(|slide| {
                let progress =
                    crate::motion::progress(slide.since, now, crate::motion::duration(SPREAD))?;
                Some(slide.from * (1. - progress))
            })
            .unwrap_or(0.)
    }

    /// The slide at `now`, or `None` once it has landed.
    pub(crate) fn sliding(&mut self, now: Instant) -> Option<Sliding> {
        let slide = self.slide.as_ref()?;
        let Some(progress) =
            crate::motion::progress(slide.since, now, crate::motion::duration(SPREAD))
        else {
            self.slide = None;
            return None;
        };
        let lift = slide.from * (1. - progress);
        Some(Sliding {
            lift,
            behind: slide.behind.clone(),
            behind_at: slide.moved,
        })
    }
}
