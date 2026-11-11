//! A host's rows drop one row on another, before or after it, and the move
//! is the one a drag of the window's own row to that gap sends.
use super::*;

#[test]
fn dropping_on_a_row_lands_before_or_after_its_unit() {
    let list = list();
    // `c` dropped before `b` lands before `b`.
    assert_eq!(landing(&list, "c", "b", false), request(&["c"], Some("b")));
    // After `b` is before `c`, its own place: nothing moves.
    assert_eq!(landing(&list, "c", "b", true), None);
    // On a group's worktree, a top-level row lands relative to the group.
    assert_eq!(landing(&list, "c", "a1", false), request(&["c"], Some("a")));
    // `b` dropped after `c` goes last.
    assert_eq!(landing(&list, "b", "c", true), request(&["b"], None));
}

#[test]
fn a_worktree_lands_only_among_its_siblings() {
    let list = list();
    assert_eq!(
        landing(&list, "a2", "a1", false),
        request(&["a2"], Some("a1"))
    );
    // A row outside its group is no place for it.
    assert_eq!(landing(&list, "a2", "c", false), None);
    // Nor is a row no snapshot holds.
    assert_eq!(landing(&list, "a2", "gone", false), None);
}
