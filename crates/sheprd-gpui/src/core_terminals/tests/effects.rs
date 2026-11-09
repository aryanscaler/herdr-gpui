//! Bells and OSC 52 clipboard writes from a herdr-core terminal reach the
//! window once each, however often its screen is taken.

use super::worker::{Scripted, size, until};
use crate::core_terminals::{effects::Effects, screen::Screen, worker};
use sheprd_backend::{Role, SurfaceUpdate, TerminalId};

#[test]
fn a_taken_screen_hands_over_bells_and_the_clipboard_once() {
    let mut screen = Screen::default();
    screen.apply(SurfaceUpdate::Title("vim".into()));
    screen.apply(SurfaceUpdate::Bell);
    screen.apply(SurfaceUpdate::Clipboard("one".into()));
    screen.apply(SurfaceUpdate::Clipboard("two".into()));
    screen.apply(SurfaceUpdate::Bell);
    let taken = screen.take();
    assert_eq!(
        (
            taken.bells,
            taken.clipboard.as_deref(),
            taken.title.as_deref()
        ),
        (2, Some("two"), Some("vim"))
    );
    let again = screen.take();
    assert_eq!(
        (again.bells, again.clipboard, again.title.as_deref()),
        (0, None, Some("vim"))
    );
}

#[test]
fn newer_effects_add_bells_and_replace_the_clipboard() {
    let mut effects = Effects {
        bells: u32::MAX - 1,
        clipboard: Some("old".into()),
    };
    effects.merge(Effects {
        bells: 3,
        clipboard: None,
    });
    assert_eq!(
        effects,
        Effects {
            bells: u32::MAX,
            clipboard: Some("old".into()),
        }
    );
    effects.merge(Effects {
        bells: 0,
        clipboard: Some(String::new()),
    });
    assert_eq!(
        effects.clipboard.as_deref(),
        Some(""),
        "an empty write clears"
    );
}

#[test]
fn the_mailbox_delivers_bells_and_the_clipboard_once() {
    let backend = Scripted::default();
    backend.push(vec![
        SurfaceUpdate::Bell,
        SurfaceUpdate::Clipboard("copied".into()),
        SurfaceUpdate::Title("sh".into()),
    ]);
    let link = worker::spawn(
        backend.clone(),
        TerminalId::new("t1").unwrap(),
        size(80, 24),
        Role::Active,
    )
    .unwrap();
    let screen = until(|| link.mailbox.take());
    assert_eq!(
        (screen.bells, screen.clipboard.as_deref()),
        (1, Some("copied"))
    );
    backend.push(vec![SurfaceUpdate::Title("vim".into())]);
    let next = until(|| link.mailbox.take());
    assert_eq!(
        (next.bells, next.clipboard, next.title.as_deref()),
        (0, None, Some("vim"))
    );
}

/// What a view takes from a bell and a clipboard write, twice.
fn effects_of_a_view(cx: &mut gpui::TestAppContext, observe: bool) -> (Effects, Effects) {
    let backend = Scripted::default();
    backend.push(vec![
        SurfaceUpdate::Bell,
        SurfaceUpdate::Clipboard("copied".into()),
    ]);
    let (view, cx, _) = super::accessibility::open_on(cx, backend, observe);
    until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)))
            .then_some(())
    });
    cx.update(|_, cx| view.update(cx, |view, _| (view.take_effects(), view.take_effects())))
}

#[gpui::test]
fn a_driving_view_hands_its_effects_over_once(cx: &mut gpui::TestAppContext) {
    let (first, second) = effects_of_a_view(cx, false);
    assert_eq!(
        first,
        Effects {
            bells: 1,
            clipboard: Some("copied".into()),
        }
    );
    assert_eq!(second, Effects::default());
}

#[gpui::test]
fn a_mirror_neither_rings_nor_copies(cx: &mut gpui::TestAppContext) {
    let (first, second) = effects_of_a_view(cx, true);
    assert_eq!((first, second), (Effects::default(), Effects::default()));
}

#[test]
fn a_cores_bells_follow_the_mute_of_the_device_it_runs_on() {
    use crate::{browser::Scope, core_terminals::Host};
    assert_eq!(Host::Local.endpoint(), crate::endpoint::LOCAL);
    // A saved host's scope is its endpoint ID, which its mute names.
    let host = Host::of(&Scope::endpoint("ssh:box"));
    assert_eq!(host.endpoint(), "ssh:box");
}
