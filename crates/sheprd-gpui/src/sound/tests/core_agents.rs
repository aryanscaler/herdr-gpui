//! Sounds for herdr-core agents, which the app asks for itself.

use super::*;

#[test]
fn an_agent_sound_plays_once_across_windows() {
    let (service, played) = Service::recording();
    assert!(service.play_agent(Sound::Request, Some("claude"), "t1"));
    // A second window hearing the same change at once.
    assert!(service.play_agent(Sound::Request, Some("claude"), "t1"));
    assert_eq!(
        played.recv_timeout(Duration::from_secs(3)).unwrap(),
        Sound::Request
    );
    assert!(played.recv_timeout(Duration::from_millis(300)).is_err());
    // Another terminal is its own news.
    assert!(service.play_agent(Sound::Done, Some("codex"), "t2"));
    assert_eq!(
        played.recv_timeout(Duration::from_secs(3)).unwrap(),
        Sound::Done
    );
}

#[test]
fn an_agent_the_settings_turn_off_stays_quiet() {
    let (service, played) = Service::recording();
    // Droid's sounds are off unless the settings turn them on.
    assert!(service.play_agent(Sound::Done, Some("droid"), "t1"));
    assert!(played.recv_timeout(Duration::from_millis(300)).is_err());
}
