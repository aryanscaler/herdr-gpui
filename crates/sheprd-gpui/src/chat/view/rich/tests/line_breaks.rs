use super::*;
use core::prelude::v1::test;
use inline::joined;

const SPACED: Piece = Piece::Word { spaced: true };
const TOUCHING: Piece = Piece::Word { spaced: false };

#[test]
fn a_formula_keeps_the_text_after_it_on_its_line() {
    // `$i$-th`, `$x$, then`, and `$x$ then`: a browser breaks only after
    // the whitespace, which ends the word, not before the word.
    assert!(joined(Piece::Math, TOUCHING));
    assert!(joined(Piece::Math, SPACED));
    assert!(joined(Piece::Math, Piece::Math));
}

#[test]
fn a_formula_stays_with_touching_text_before_it_but_not_after_a_space() {
    // `($x$)` keeps the parenthesis; `the $x$` may break after `the `.
    assert!(joined(TOUCHING, Piece::Math));
    assert!(!joined(SPACED, Piece::Math));
}

#[test]
fn words_and_line_breaks_keep_their_own_breaks() {
    for (previous, next) in [
        (TOUCHING, TOUCHING),
        (TOUCHING, SPACED),
        (SPACED, TOUCHING),
        (Piece::Break, Piece::Math),
        (Piece::Math, Piece::Break),
        (TOUCHING, Piece::Break),
        (Piece::Break, TOUCHING),
    ] {
        assert!(!joined(previous, next), "{previous:?} then {next:?}");
    }
}

#[test]
fn touching_items_form_one_flat_run_split_by_spaces_and_breaks() {
    let item = || div().into_any_element();
    let mut runs = Runs::default();
    // `the ` | `$x$` `,` `$y$` `)` ... | break | `next`
    runs.push(SPACED, item());
    runs.push(Piece::Math, item());
    for _ in 0..1_000 {
        runs.push(TOUCHING, item());
        runs.push(Piece::Math, item());
    }
    runs.push(Piece::Break, item());
    runs.push(TOUCHING, item());
    assert_eq!(runs.finish().len(), 4);
}
