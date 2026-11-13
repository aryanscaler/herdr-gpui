use super::*;

fn ssh(target: &str) -> ConnectTarget {
    ConnectTarget::Ssh {
        target: target.into(),
        session: "default".into(),
    }
}

#[test]
fn a_device_edited_to_name_another_machine_gets_the_tools_again() {
    let mut tried = Tried::default();
    let scope = crate::browser::Scope::endpoint("ssh:box");
    assert!(tried.first(scope.clone(), &ssh("old"), Choice::Installed));
    assert!(!tried.first(scope.clone(), &ssh("old"), Choice::Installed));
    assert!(tried.first(scope.clone(), &ssh("new"), Choice::Installed));
    assert!(tried.first(scope, &ssh("new"), Choice::Declined));
}
