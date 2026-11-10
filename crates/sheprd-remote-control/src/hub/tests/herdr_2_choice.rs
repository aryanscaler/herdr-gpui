//! "New terminals use Herdr 2" lets a phone start a core where Herdr 1 is
//! installed: read from the user's overrides first, then Sheprd's file.

use crate::hub::wake::herdr_2_chosen;

#[test]
fn the_overrides_decide_then_the_settings_and_otherwise_herdr_1_stays() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("config-gpui.toml");
    let overrides = dir.path().join("config-gpui.local.toml");
    assert!(!herdr_2_chosen(&settings), "nothing says so");

    std::fs::write(&settings, "herdr_2_terminals = true\n").unwrap();
    assert!(herdr_2_chosen(&settings));
    std::fs::write(
        &overrides,
        "layout = \"compact\"\nherdr_2_terminals = false\n",
    )
    .unwrap();
    assert!(!herdr_2_chosen(&settings), "the user's overrides win");
    std::fs::write(&overrides, "herdr_2_terminals = true\n").unwrap();
    std::fs::write(&settings, "herdr_2_terminals = false\n").unwrap();
    assert!(herdr_2_chosen(&settings));

    std::fs::write(&overrides, "herdr_2_terminals = [\n").unwrap();
    assert!(
        !herdr_2_chosen(&settings),
        "unreadable overrides leave the file"
    );
    std::fs::write(&overrides, "[remote]\nherdr_2_terminals = true\n").unwrap();
    assert!(
        !herdr_2_chosen(&settings),
        "only the top-level setting counts"
    );
}
