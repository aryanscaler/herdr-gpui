//! A host with herdr-core (Herdr 2) and no compatible Herdr 1 is a setup of
//! its own, not a host missing Herdr.

use super::*;

const COMPATIBLE: &str = r#"{"endpoint_protocol_generation":1,"endpoint_capabilities":["surface_interest","presentation_effects_fence","health_check"],"remote_bridge_idle_timeout":true}"#;

fn probe(lines: &[&str]) -> Option<HostProbe> {
    let mut output = lines.join("\n");
    output.push('\n');
    classify_probe(output.as_bytes())
}

#[test]
fn herdr_core_alone_is_its_own_setup() {
    assert_eq!(
        probe(&[PROBE_CORE, PROBE_DONE]),
        Some(HostProbe::Core),
        "no Herdr 1 at all"
    );
    assert_eq!(
        probe(&[
            PROBE_CANDIDATE,
            r#"{"endpoint_protocol_generation":0}"#,
            "",
            PROBE_CORE,
            PROBE_DONE
        ]),
        Some(HostProbe::Core),
        "an outdated Herdr 1 beside it"
    );
}

#[test]
fn a_compatible_herdr_1_still_wins_beside_herdr_core() {
    assert_eq!(
        probe(&[
            PROBE_CANDIDATE,
            COMPATIBLE,
            r#"{"running":true}"#,
            PROBE_CORE,
            PROBE_DONE
        ]),
        Some(HostProbe::Running)
    );
    assert_eq!(probe(&[PROBE_DONE]), Some(HostProbe::Missing));
    // A probe cut short says nothing, core or not.
    assert_eq!(probe(&[PROBE_CORE]), None);
}
