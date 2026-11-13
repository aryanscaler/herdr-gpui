use super::*;
use core::prelude::v1::test;
use inline::{Rise, rise, text_baseline};

// A 16pt line of prose whose font rises 10pt and falls 3pt: GPUI centers
// the 13pt in the line, so the baseline is 1.5pt + 10pt below its top.
const LINE: f32 = 16.;
const PROSE: f32 = 11.5;
const BORDER: f32 = 1.;

/// Where the picture's TeX baseline lands, from the top of its box.
fn picture_baseline(rise: Rise, baseline: f32) -> f32 {
    BORDER + rise.top + baseline
}

#[test]
fn the_prose_baseline_uses_the_descent_by_magnitude_whatever_its_sign() {
    assert_eq!(text_baseline(LINE, 10., 3.), PROSE);
    assert_eq!(text_baseline(LINE, 10., -3.), PROSE);
}

#[test]
fn a_formula_box_ends_as_far_below_its_baseline_as_a_word_does() {
    // A word's box is the line: it ends LINE - PROSE below the baseline.
    // Short (`x`), tall (`\frac{a}{b}`), and taller than the line.
    for (baseline, height) in [(6., 7.), (14., 20.), (30., 40.)] {
        let rise = rise(LINE, PROSE, baseline, height, BORDER);
        assert_eq!(
            rise.height - picture_baseline(rise, baseline),
            LINE - PROSE,
            "baseline {baseline}, height {height}"
        );
    }
}

#[test]
fn a_formula_shorter_than_the_prose_keeps_a_full_line() {
    let rise = rise(LINE, PROSE, 6., 7., BORDER);
    assert_eq!(rise.top, PROSE - 6.);
    assert_eq!(rise.height, BORDER + LINE);
    assert_eq!(rise.margin_bottom, 0.);
}

#[test]
fn a_formula_deeper_than_the_prose_keeps_its_depth_below_as_a_margin() {
    // A descender reaching 8pt below the TeX baseline, past the line's 4.5pt.
    let (baseline, height) = (10., 18.);
    let rise = rise(LINE, PROSE, baseline, height, BORDER);
    assert_eq!(rise.margin_bottom, 8. - (LINE - PROSE));
    // The picture's bottom is the bottom of the box's margin.
    assert_eq!(BORDER + rise.top + height, rise.height + rise.margin_bottom);
}
