use super::*;

#[test]
fn tabs_keep_their_herdr_unless_herdr_2_is_chosen_and_there() {
    use Generation::{Herdr1, Herdr2};
    // Herdr 1 → Herdr 1, unchanged, and Herdr 2 → Herdr 2.
    assert_eq!(destination_generation(Herdr1, true, false), Herdr1);
    assert_eq!(destination_generation(Herdr2, true, false), Herdr2);
    // Herdr 1 → Herdr 2, with Herdr 2 chosen for new tabs.
    assert_eq!(destination_generation(Herdr1, true, true), Herdr2);
    // Herdr 2 → Herdr 1, where the destination has no herdr-core.
    assert_eq!(destination_generation(Herdr2, false, false), Herdr1);
    assert_eq!(destination_generation(Herdr1, false, true), Herdr1);
}

#[test]
fn a_tab_too_big_for_herdr_2_stays_in_herdr_1() {
    let ids: Vec<String> = (0..=MAX_PANES).map(|n| format!("p{n}")).collect();
    let moves = (Generation::Herdr1, Generation::Herdr2);
    let plan = |ids: &[String]| {
        plan_tree(
            moves,
            false,
            None,
            Node::row(ids).unwrap(),
            &[],
            &[],
            &mut 0,
        )
    };
    assert_eq!(plan(&ids).unwrap().into, Generation::Herdr1);
    let small = plan(&ids[..2]).unwrap();
    assert_eq!(small.into, Generation::Herdr2);
    assert_eq!(small.generation, Generation::Herdr1);
}

#[test]
fn a_tab_too_big_for_herdr_2_is_refused_where_the_app_keeps_the_workspace() {
    let ids: Vec<String> = (0..=MAX_PANES).map(|n| format!("p{n}")).collect();
    let moves = (Generation::Herdr1, Generation::Herdr2);
    let plan =
        |ids: &[String]| plan_tree(moves, true, None, Node::row(ids).unwrap(), &[], &[], &mut 0);
    assert!(matches!(
        plan(&ids),
        Err(Error::TabTooLarge { panes, most: MAX_PANES }) if panes == MAX_PANES + 1
    ));
    assert_eq!(plan(&ids[..MAX_PANES]).unwrap().into, Generation::Herdr2);
}
