use super::*;
use std::{cell::Cell, rc::Rc};

#[gpui::test]
fn a_host_keeping_the_bar_builds_it_instead(cx: &mut gpui::TestAppContext) {
    let built = Rc::new(Cell::new(0));
    cx.update(|cx| {
        let count = built.clone();
        cx.set_global(HostMenus(Rc::new(move |_| count.set(count.get() + 1))));
        install(cx);
        install(cx);
    });
    assert_eq!(built.get(), 2);
    // The bar the host builds from is the window's own.
    let bar = cx.update(|cx| bar(cx));
    assert!(
        bar.iter()
            .any(|menu| menu.name.as_ref() == fl!("menus-herdr"))
    );
}
