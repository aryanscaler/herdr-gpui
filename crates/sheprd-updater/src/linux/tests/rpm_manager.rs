use super::*;

#[test]
fn dnf_keeps_priority_and_zypper_covers_only_its_absence() {
    for (dnf, zypper, expected) in [
        (true, true, RpmManager::Dnf),
        (true, false, RpmManager::Dnf),
        (false, true, RpmManager::Zypper),
        // With neither installed the spawn fails naming dnf, the program an RPM
        // distribution is expected to have, rather than an absent one.
        (false, false, RpmManager::Dnf),
    ] {
        assert_eq!(RpmManager::resolve(dnf, zypper), expected);
    }
}

#[test]
fn each_manager_is_a_fixed_program_that_answers_its_own_prompts() {
    for manager in [RpmManager::Dnf, RpmManager::Zypper] {
        let program = Path::new(manager.program());
        // Root transactions run with a cleared environment, so the program is
        // always an absolute path and is never resolved through PATH.
        assert!(program.is_absolute(), "{manager:?}");
        assert_eq!(program.parent(), Some(Path::new("/usr/bin")), "{manager:?}");
        let arguments = manager.arguments();
        assert_eq!(arguments.first().copied(), Some("install"), "{manager:?}");
        // Nothing can answer a prompt: each manager must accept the transaction
        // and, for zypper, the unsigned release RPM as well.
        match manager {
            RpmManager::Dnf => assert!(arguments.contains(&"--assumeyes")),
            RpmManager::Zypper => {
                assert!(arguments.contains(&"-y"));
                assert!(arguments.contains(&"--allow-unsigned-rpm"));
            }
        }
    }
}

#[test]
fn the_rpm_command_reaches_this_hosts_manager_with_a_literal_path() {
    let path =
        Path::new("/var/cache/.herdr-package-a1/Sheprd-20261108.1-x86_64-unknown-linux-gnu.rpm");
    let manager = RpmManager::detect();
    let command = install_command(Format::Rpm, path);
    assert_eq!(command.get_program(), manager.program());
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(&args[..args.len() - 1], manager.arguments());
    assert_eq!(args.last().copied(), Some(path.as_os_str()));
    // An openSUSE host has no dnf, so its RPMs must never be handed to one.
    if manager == RpmManager::Zypper {
        assert!(!Path::new("/usr/bin/dnf").is_file());
    }
}
