//! How a paragraph's words and inline formulas sit in their lines.

use gpui::{AnyElement, IntoElement as _, ParentElement as _, Styled as _, div};

/// What an inline item is, for where a line may break around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Piece {
    /// A word, and whether it ends in whitespace.
    Word {
        spaced: bool,
    },
    Math,
    Break,
}

/// Whether `next` must stay on the line of `previous`. A browser breaks a
/// line only after whitespace, so no break falls between a formula and the
/// text it touches: `$x$, then` or `($x$)`. Words keep their own breaks.
pub(super) fn joined(previous: Piece, next: Piece) -> bool {
    match (previous, next) {
        (Piece::Math, Piece::Word { .. } | Piece::Math) => true,
        (Piece::Word { spaced }, Piece::Math) => !spaced,
        _ => false,
    }
}

/// A paragraph's items in runs no line breaks inside. A run of several
/// items is one flat non-wrapping row, however long it grows.
#[derive(Default)]
pub(super) struct Runs {
    done: Vec<AnyElement>,
    run: Vec<AnyElement>,
    last: Option<Piece>,
}

impl Runs {
    pub(super) fn push(&mut self, piece: Piece, item: AnyElement) {
        if !self.last.is_some_and(|last| joined(last, piece)) {
            self.flush();
        }
        self.run.push(item);
        self.last = Some(piece);
    }

    pub(super) fn finish(mut self) -> Vec<AnyElement> {
        self.flush();
        self.done
    }

    fn flush(&mut self) {
        if self.run.len() > 1 {
            let run = div()
                .flex()
                .flex_none()
                .items_baseline()
                .children(self.run.drain(..));
            self.done.push(run.into_any_element());
        } else {
            self.done.append(&mut self.run);
        }
    }
}

/// Distance from the top of a line of prose to its baseline. GPUI centers
/// the font's ascent and descent in the line height. Fonts report descent
/// with either sign (font-kit's is negative), which
/// `TextSystem::baseline_offset` adds as is; lines use its magnitude.
pub(super) fn text_baseline(line: f32, ascent: f32, descent: f32) -> f32 {
    (line - ascent - descent.abs()) / 2. + ascent
}

/// The vertical layout of an inline formula's box, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rise {
    /// From the top of the box's content to the top of the picture.
    pub(super) top: f32,
    /// The box's height, border included.
    pub(super) height: f32,
    /// Room kept below the box for a formula deeper than a line of prose.
    pub(super) margin_bottom: f32,
}

/// Lays out a formula whose picture is `height` tall with its TeX baseline
/// `baseline` below its top, in a line `line` tall whose prose baseline is
/// `text_baseline` below its top. GPUI reports no text baselines to layout,
/// so a row's items line up by their bottom edges: the box ends as far below
/// the TeX baseline as a word's line ends below the prose's. A deeper
/// formula keeps the rest as a margin, which alignment ignores. `border` is
/// the selection border inside the box, above the picture.
pub(super) fn rise(line: f32, text_baseline: f32, baseline: f32, height: f32, border: f32) -> Rise {
    let below = line - text_baseline;
    let top = (text_baseline - baseline).max(0.);
    Rise {
        top,
        height: border + top + baseline + below,
        margin_bottom: (height - baseline - below).max(0.),
    }
}
