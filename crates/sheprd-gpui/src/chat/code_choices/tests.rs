use super::*;
use sheprd_chat_protocol::EntryId;

fn key(part: usize) -> PageKey {
    PageKey {
        entry: EntryId(1),
        part,
    }
}

#[test]
fn a_block_follows_the_default_until_its_own_button_chooses() {
    let mut choices = CodeChoices::default();
    let id = VisualId::of("code");
    assert!(!choices.wraps(id, false));
    assert!(choices.wraps(id, true));
    choices.set_wrap(id, true);
    assert!(choices.wraps(id, false), "a choice outlives the default");
    choices.set_wrap(id, false);
    assert!(!choices.wraps(id, true));
    assert!(choices.wraps(VisualId::of("other"), true));
}

#[test]
fn each_default_draws_html_its_own_way() {
    let choices = CodeChoices::default();
    assert_eq!(
        choices.html_view(key(0), HtmlRender::Default, true),
        HtmlView::Card
    );
    assert_eq!(
        choices.html_view(key(0), HtmlRender::Never, true),
        HtmlView::Code
    );
    assert_eq!(
        choices.html_view(key(0), HtmlRender::Always, true),
        HtmlView::Load
    );
    assert_eq!(
        choices.html_view(key(0), HtmlRender::Always, false),
        HtmlView::Card,
        "without pages in the window, always cannot open one per fence"
    );
}

#[test]
fn the_switch_overrides_every_default() {
    let mut choices = CodeChoices::default();
    choices.set_shows_code(key(0), true);
    choices.set_shows_code(key(1), false);
    for default in HtmlRender::ALL.iter().copied() {
        assert_eq!(choices.html_view(key(0), default, true), HtmlView::Code);
        assert_eq!(choices.html_view(key(1), default, true), HtmlView::Card);
    }
}

#[test]
fn only_an_explicit_switch_to_code_stops_a_loading_page() {
    let mut choices = CodeChoices::default();
    assert!(!choices.shows_code(key(0)));
    choices.set_shows_code(key(0), true);
    assert!(choices.shows_code(key(0)));
    choices.set_shows_code(key(0), false);
    assert!(!choices.shows_code(key(0)));
}

#[test]
fn always_loads_a_fence_once() {
    let mut choices = CodeChoices::default();
    let page: Arc<str> = "<p>hi</p>".into();
    choices.queue_load(key(0), &page);
    choices.queue_load(key(0), &page);
    choices.queue_load(key(1), &page);
    let queued: Vec<_> = choices.take_queued().into_iter().map(|(k, _)| k).collect();
    assert_eq!(queued, [key(0), key(1)]);
    assert!(choices.take_queued().is_empty());
    assert_eq!(
        choices.html_view(key(0), HtmlRender::Always, true),
        HtmlView::Card,
        "a hidden or closed page stays a card"
    );
}
