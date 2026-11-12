use super::*;
use crate::browser::TabId;

/// The fixture window with workspace `w0`'s saved layout showing a web
/// page, as on the first show of the workspace after a restart. The tab is
/// in the store, but no page was ever made for it.
fn restored(
    cx: &mut gpui::TestAppContext,
) -> (Entity<SheprdWindow>, &mut VisualTestContext, TabId) {
    let (view, cx) = window(cx);
    let tab_scope = view.read_with(cx, |view, _| scope(&view.endpoints[0]));
    let id = cx.update(|_, cx| {
        Store::update(cx, |store| {
            store.open(tab_scope, "w0", Some(url("https://a.test/")), None)
        })
        .unwrap()
    });
    let pick = format!(
        r#"{{"kind":"page","id":{}}}"#,
        serde_json::to_string(&id).unwrap()
    );
    let saved =
        format!(r#"{{"groups":[{{"pick":{pick},"share":1.0,"tabs":[{pick}]}}],"active":0}}"#);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            let key = view.browser_key().unwrap();
            view.browser.layouts.remove(&key);
            view.browser
                .saved
                .insert(key, serde_json::from_str(&saved).unwrap());
        })
    });
    draw(cx);
    (view, cx, id)
}

fn tick(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| view.update(cx, |view, cx| view.poll_browser(window, cx)));
    cx.run_until_parked();
}

/// Whether the tab's page was made: a headless window may record why it
/// could not be instead.
fn made(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext, id: TabId) -> bool {
    view.read_with(cx, |view, _| {
        view.browser.pages.contains(id)
            || view.browser.pages.is_building(id)
            || view.browser.failed.contains_key(&id)
    })
}

#[gpui::test]
fn a_page_a_restored_layout_shows_gets_its_page_on_the_next_tick(cx: &mut gpui::TestAppContext) {
    let (view, cx, id) = restored(cx);
    let shown = cx.update(|_, cx| {
        let view = view.read(cx);
        view.group_shown(view.group_slots()[0].id, cx)
    });
    assert_eq!(shown, crate::browser::groups::Shown::Page(id));
    assert!(!made(&view, cx, id), "drawing makes no page");
    tick(&view, cx);
    assert!(made(&view, cx, id));
}

#[gpui::test]
fn a_page_that_could_not_be_made_is_not_retried_every_tick(cx: &mut gpui::TestAppContext) {
    let (view, cx, id) = restored(cx);
    view.update(cx, |view, _| {
        view.browser.failed.insert(id, "no web view".into());
    });
    tick(&view, cx);
    view.read_with(cx, |view, _| {
        assert!(!view.browser.pages.contains(id));
        assert_eq!(
            view.browser.failed.get(&id).map(|message| message.as_ref()),
            Some("no web view")
        );
    });
}
