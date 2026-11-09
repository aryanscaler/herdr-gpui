//! Links in a herdr-core pane: read from its own frame, opened only by a
//! click with the link modifier held, and handed to the window to open.

use super::{
    accessibility::look,
    worker::{Scripted, size, until},
};
use crate::{
    core_terminals::{
        CoreTerminalView, LinkClick,
        view::ToWindow,
        worker::{self, Command},
    },
    terminal::{RowTarget, frame_link_at},
    terminal_painter::TerminalPainter,
};
use gpui::{
    Entity, ExternalPaths, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px,
};
use sheprd_backend::{ClientPaneInputEvent, FrameData, Role, SurfaceUpdate, TerminalId};
use sheprd_client::protocol::CellData;
use std::{cell::RefCell, path::PathBuf, rc::Rc};

const URL: &str = "https://example.com/a";

/// A frame showing `rows`, each padded with blanks to `width`.
pub(super) fn frame(width: u16, rows: &[&str]) -> FrameData {
    let cells = rows
        .iter()
        .flat_map(|row| {
            let mut symbols: Vec<String> = row.chars().map(String::from).collect();
            symbols.resize(usize::from(width), " ".into());
            symbols
        })
        .map(|symbol| CellData {
            symbol,
            fg: 0,
            bg: 0,
            modifier: 0,
            skip: false,
            hyperlink: None,
        })
        .collect();
    FrameData {
        cells,
        width,
        height: u16::try_from(rows.len()).unwrap(),
        cursor: None,
        hyperlinks: Vec::new(),
        graphics: Vec::new(),
    }
}

#[test]
fn a_whole_frame_reads_addresses_and_paths() {
    let shown = frame(40, &[&format!("see {URL} now"), "edit src/main.rs:12 then"]);
    let link = frame_link_at(&shown, 6, 0, &[]).unwrap();
    assert_eq!(link.target, RowTarget::Web(URL.into()));
    assert_eq!((link.row, link.columns), (0, 4..4 + URL.len() as u16));
    let path = frame_link_at(&shown, 7, 1, &[]).unwrap();
    assert_eq!(
        path.target,
        RowTarget::Path {
            path: "src/main.rs".into(),
            line: Some(12),
            explicit: false,
        }
    );
    // A blank is no link; any word may name a file, which a check decides.
    assert_eq!(frame_link_at(&shown, 3, 0, &[]), None);
    // Outside the frame is nothing.
    assert_eq!(frame_link_at(&shown, 0, 5, &[]), None);
}

pub(super) type Asked = Rc<RefCell<Vec<ToWindow>>>;

/// A view showing `rows`, drawn, and what it asks of its window.
pub(super) fn shown<'a>(
    cx: &'a mut TestAppContext,
    rows: &[&str],
    observe: bool,
) -> (
    Entity<CoreTerminalView>,
    &'a mut VisualTestContext,
    Scripted,
    Asked,
) {
    let backend = Scripted::default();
    backend.push(vec![SurfaceUpdate::Frame(frame(40, rows))]);
    let link = worker::spawn(
        backend.clone(),
        TerminalId::new("t1").unwrap(),
        size(40, 4),
        Role::Active,
    )
    .unwrap();
    let painter = Rc::new(RefCell::new(TerminalPainter::default()));
    let asked: Asked = Rc::default();
    let record = asked.clone();
    let (view, cx) = cx.add_window_view(|_, cx| {
        CoreTerminalView::new(
            TerminalId::new("t1").unwrap(),
            link,
            look(),
            painter,
            observe,
            cx,
        )
        .with_window(Rc::new(move |ask, _, _| record.borrow_mut().push(ask)))
    });
    until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)))
            .then_some(())
    });
    cx.run_until_parked();
    (view, cx, backend, asked)
}

/// The middle of the cell at `column`, `row`.
pub(super) fn at(
    view: &Entity<CoreTerminalView>,
    cx: &mut VisualTestContext,
    column: u16,
    row: u16,
) -> gpui::Point<gpui::Pixels> {
    let bounds = cx.update(|_, cx| view.read(cx).bounds().unwrap());
    let look = look();
    bounds.origin
        + point(
            px((f32::from(column) + 0.5) * look.cell_width),
            px((f32::from(row) + 0.5) * look.cell_height),
        )
}

pub(super) fn link_modifier() -> Modifiers {
    crate::keymap::secondary_key()
}

fn opened(asked: &Asked) -> Vec<LinkClick> {
    asked
        .borrow()
        .iter()
        .filter_map(|ask| match ask {
            ToWindow::OpenLink(click) => Some(click.clone()),
            ToWindow::Copied | ToWindow::Refused(_) => None,
        })
        .collect()
}

#[gpui::test]
fn a_link_modifier_click_opens_the_address(cx: &mut TestAppContext) {
    let (view, cx, _, asked) = shown(cx, &[&format!("see {URL} now")], false);
    let link = at(&view, cx, 8, 0);
    cx.simulate_mouse_move(link, None, link_modifier());
    cx.update(|_, cx| assert_eq!(view.read(cx).link_rows(), [(0, 4..4 + URL.len() as u16)]));
    cx.simulate_mouse_down(link, MouseButton::Left, link_modifier());
    cx.simulate_mouse_up(link, MouseButton::Left, link_modifier());
    assert_eq!(
        opened(&asked),
        [LinkClick {
            target: RowTarget::Web(URL.into()),
            terminal: TerminalId::new("t1").unwrap(),
            cwd: None,
            alt: false,
        }]
    );
    // Letting go of the modifier drops the underline.
    cx.simulate_modifiers_change(Modifiers::none());
    cx.update(|_, cx| assert!(view.read(cx).link_rows().is_empty()));
}

#[gpui::test]
fn a_plain_click_or_a_release_elsewhere_opens_nothing(cx: &mut TestAppContext) {
    let (view, cx, _, asked) = shown(cx, &[&format!("see {URL} now")], false);
    let link = at(&view, cx, 8, 0);
    cx.simulate_mouse_move(link, None, Modifiers::none());
    cx.update(|_, cx| assert!(view.read(cx).link_rows().is_empty()));
    cx.simulate_click(link, Modifiers::none());
    let off = at(&view, cx, 1, 0);
    cx.simulate_mouse_down(link, MouseButton::Left, link_modifier());
    cx.simulate_mouse_up(off, MouseButton::Left, link_modifier());
    assert!(opened(&asked).is_empty());
}

#[gpui::test]
fn dropped_files_paste_their_escaped_paths(cx: &mut TestAppContext) {
    let (view, cx, backend, _) = shown(cx, &["$"], false);
    let paths =
        ExternalPaths(vec![PathBuf::from("/tmp/Shot 1.png"), PathBuf::from("/tmp/a")].into());
    cx.update(|window, cx| view.update(cx, |view, cx| view.drop_files(&paths, window, cx)));
    let pasted = until(|| {
        backend
            .received
            .lock()
            .unwrap()
            .iter()
            .find_map(|command| match command {
                Command::Input(ClientPaneInputEvent::Paste(text)) => Some(text.clone()),
                _ => None,
            })
    });
    assert_eq!(pasted, "/tmp/Shot\\ 1.png /tmp/a");
}

#[gpui::test]
fn a_refused_drop_is_said_and_pastes_nothing(cx: &mut TestAppContext) {
    let (view, cx, backend, asked) = shown(cx, &["$"], false);
    let paths = ExternalPaths(vec![PathBuf::from("/tmp/a\nb")].into());
    cx.update(|window, cx| view.update(cx, |view, cx| view.drop_files(&paths, window, cx)));
    assert!(matches!(&asked.borrow()[..], [ToWindow::Refused(_)]));
    assert!(
        !backend
            .received
            .lock()
            .unwrap()
            .iter()
            .any(|command| matches!(command, Command::Input(_)))
    );
}

#[gpui::test]
fn another_hosts_pane_refuses_this_computers_files(cx: &mut TestAppContext) {
    let (view, cx, backend, asked) = shown(cx, &["$"], false);
    cx.update(|_, cx| view.update(cx, |view, cx| view.set_remote(true, cx)));
    let paths = ExternalPaths(vec![PathBuf::from("/tmp/a")].into());
    cx.update(|window, cx| view.update(cx, |view, cx| view.drop_files(&paths, window, cx)));
    assert!(matches!(
        &asked.borrow()[..],
        [ToWindow::Refused(crate::Error::FileDrop(
            crate::window::file_drop::Error::OtherHost
        ))]
    ));
    assert!(
        !backend
            .received
            .lock()
            .unwrap()
            .iter()
            .any(|command| matches!(command, Command::Input(_)))
    );
}

#[gpui::test]
fn a_mirror_takes_no_drop(cx: &mut TestAppContext) {
    let (view, cx, backend, asked) = shown(cx, &["$"], true);
    let paths = ExternalPaths(vec![PathBuf::from("/tmp/a")].into());
    cx.update(|window, cx| view.update(cx, |view, cx| view.drop_files(&paths, window, cx)));
    assert!(asked.borrow().is_empty());
    assert!(
        !backend
            .received
            .lock()
            .unwrap()
            .iter()
            .any(|command| matches!(command, Command::Input(_)))
    );
}

#[gpui::test]
fn a_release_off_the_pane_opens_nothing(cx: &mut TestAppContext) {
    let (view, cx, _, asked) = shown(cx, &[&format!("see {URL} now")], false);
    let link = at(&view, cx, 8, 0);
    cx.simulate_mouse_move(link, None, link_modifier());
    cx.simulate_mouse_down(link, MouseButton::Left, link_modifier());
    // Above the first row, which the cell lookup would hold on that row.
    let above = link - point(px(0.), px(40.));
    cx.simulate_mouse_up(above, MouseButton::Left, link_modifier());
    assert!(opened(&asked).is_empty());
}

#[gpui::test]
fn a_relative_path_is_read_again_once_the_folder_is_known(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("notes.txt"), "hi").unwrap();
    let (view, cx, _, _) = shown(cx, &["see notes.txt"], false);
    let path = at(&view, cx, 6, 0);
    cx.simulate_mouse_move(path, None, link_modifier());
    cx.run_until_parked();
    cx.update(|_, cx| assert!(view.read(cx).link_rows().is_empty()));
    // The folder arrives later than the screen, with the pointer still there.
    cx.update(|_, cx| view.update(cx, |view, cx| view.set_cwd(Some(folder.path()), cx)));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).link_rows(), [(0, 4..13)]));
}

#[gpui::test]
fn another_hosts_paths_are_no_links_but_its_addresses_are(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("notes.txt"), "hi").unwrap();
    let (view, cx, _, asked) = shown(cx, &[&format!("notes.txt {URL}")], false);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_cwd(Some(folder.path()), cx);
            view.set_remote(true, cx);
        });
    });
    // The same name on this disk is not the file the other host printed.
    let path = at(&view, cx, 2, 0);
    cx.simulate_mouse_move(path, None, link_modifier());
    cx.run_until_parked();
    cx.update(|_, cx| assert!(view.read(cx).link_rows().is_empty()));
    cx.simulate_mouse_down(path, MouseButton::Left, link_modifier());
    cx.simulate_mouse_up(path, MouseButton::Left, link_modifier());
    assert!(opened(&asked).is_empty());
    let address = at(&view, cx, 14, 0);
    cx.simulate_mouse_move(address, None, link_modifier());
    cx.simulate_mouse_down(address, MouseButton::Left, link_modifier());
    cx.simulate_mouse_up(address, MouseButton::Left, link_modifier());
    assert_eq!(opened(&asked).len(), 1);
    // Back on this computer's core, the file is a link again.
    cx.update(|_, cx| view.update(cx, |view, cx| view.set_remote(false, cx)));
    cx.simulate_mouse_move(path, None, link_modifier());
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).link_rows(), [(0, 0..9)]));
}

#[gpui::test]
fn a_file_made_after_its_path_was_printed_becomes_a_link(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let (view, cx, backend, _) = shown(cx, &["see notes.txt"], false);
    cx.update(|_, cx| view.update(cx, |view, cx| view.set_cwd(Some(folder.path()), cx)));
    let path = at(&view, cx, 6, 0);
    cx.simulate_mouse_move(path, None, link_modifier());
    cx.run_until_parked();
    cx.update(|_, cx| assert!(view.read(cx).link_rows().is_empty()));
    // A build writes the file and prints its path again, under the pointer.
    std::fs::write(folder.path().join("notes.txt"), "hi").unwrap();
    backend.push(vec![SurfaceUpdate::Frame(frame(
        40,
        &["see notes.txt", "$"],
    ))]);
    until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)))
            .then_some(())
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).link_rows(), [(0, 4..13)]));
}

#[test]
fn a_terminal_found_by_name_has_its_details() {
    let mut infos = crate::core_terminals::infos::Infos::default();
    let mut shell = sheprd_backend::TerminalInfo {
        agent: sheprd_client::protocol::AgentStatus::Unknown,
        cwd: Some("/w".into()),
        foreground: None,
        id: TerminalId::new("t4").unwrap(),
        labels: Default::default(),
        name: Some("herdr-ui".into()),
        reporter: None,
        state: sheprd_backend::RunState::Running,
        title: None,
    };
    infos.insert(shell.clone());
    let by_name = TerminalId::new("herdr-ui").unwrap();
    assert_eq!(infos.named(&by_name), Some(&shell));
    assert_eq!(infos.named(&shell.id), Some(&shell));
    shell.name = None;
    infos.insert(shell);
    assert_eq!(infos.named(&by_name), None);
}

#[test]
fn a_terminal_only_a_mirror_shows_is_still_this_computers() {
    use crate::{
        browser::TabId,
        core_terminals::{CoreTerminals, Host, TabPanes, panes::Panes},
    };
    let id = |name| TerminalId::new(name).unwrap();
    let mut terminals = CoreTerminals::default();
    terminals.tabs.insert(
        TabId::test(1),
        TabPanes::new(Host::Local, Panes::single(id("t1")), true),
    );
    terminals.tabs.insert(
        TabId::test(2),
        TabPanes::new(
            Host::Remote(crate::browser::Scope::endpoint("ssh:box")),
            Panes::single(id("t2")),
            false,
        ),
    );
    // No tab driving it takes a split, but the editor still opens on
    // this computer's core rather than beside a Herdr 1 pane.
    assert_eq!(terminals.tab_of(&id("t1")), None);
    assert!(terminals.mirrors(&id("t1")));
    // Another host's `t2` is no terminal of this computer's.
    assert!(!terminals.mirrors(&id("t2")));
}

/// `URL` after "see ", wrapped at 16 columns.
fn wrapped_url() -> FrameData {
    let text = format!("see {URL} now");
    frame(16, &[&text[..16], &text[16..]])
}

#[test]
fn an_address_the_terminal_wrapped_is_read_across_its_rows() {
    let shown = wrapped_url();
    let link = frame_link_at(&shown, 3, 1, &[true, false]).unwrap();
    assert_eq!(link.target, RowTarget::Web(URL.into()));
    assert_eq!(link.rows().collect::<Vec<_>>(), [(0, 4..16), (1, 0..9)]);
    // The same from its first row.
    assert_eq!(frame_link_at(&shown, 6, 0, &[true, false]), Some(link));
    // A line that merely filled its row is not read on into the next: the
    // row is the whole address the program printed.
    let ended = frame_link_at(&shown, 6, 0, &[false, false]).unwrap();
    assert_eq!(ended.rows().collect::<Vec<_>>(), [(0, 4..16)]);
    // Before the terminal says which rows wrapped, nothing is guessed.
    assert_eq!(frame_link_at(&shown, 6, 0, &[]), None);
}

#[gpui::test]
fn a_wrapped_address_opens_once_the_terminal_says_it_wrapped(cx: &mut TestAppContext) {
    let text = format!("see {URL} now");
    let (view, cx, backend, asked) = shown(cx, &[], false);
    backend.push(vec![
        SurfaceUpdate::Frame(frame(16, &[&text[..16], &text[16..]])),
        SurfaceUpdate::Wraps(vec![true, false]),
    ]);
    until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)))
            .then_some(())
    });
    cx.run_until_parked();
    let tail = at(&view, cx, 3, 1);
    cx.simulate_mouse_move(tail, None, link_modifier());
    cx.update(|_, cx| assert_eq!(view.read(cx).link_rows(), [(0, 4..16), (1, 0..9)]));
    cx.simulate_mouse_down(tail, MouseButton::Left, link_modifier());
    cx.simulate_mouse_up(tail, MouseButton::Left, link_modifier());
    assert_eq!(
        opened(&asked)
            .into_iter()
            .map(|click| click.target)
            .collect::<Vec<_>>(),
        [RowTarget::Web(URL.into())]
    );
}

#[test]
fn a_blank_the_terminal_wrapped_still_ends_the_address() {
    // "https://example.com/" fills 20 columns; the space wraps to the next
    // row, where it reads as indent before "trailing".
    let shown = frame(20, &["https://example.com/", " trailing"]);
    let link = frame_link_at(&shown, 4, 0, &[true, false]).unwrap();
    assert_eq!(link.target, RowTarget::Web("https://example.com/".into()));
    assert_eq!(link.rows().collect::<Vec<_>>(), [(0, 0..20)]);
}

#[test]
fn an_address_that_fills_its_row_ends_at_a_newline() {
    // The program ended the line at the edge; the next line is its own.
    let shown = frame(20, &["https://example.com/", "next prompt"]);
    let link = frame_link_at(&shown, 4, 0, &[false, false]).unwrap();
    assert_eq!(link.target, RowTarget::Web("https://example.com/".into()));
    assert_eq!(link.rows().collect::<Vec<_>>(), [(0, 0..20)]);
    // Wrapped, it goes on.
    let wrapped = frame_link_at(&shown, 4, 0, &[true, false]).unwrap();
    assert_eq!(
        wrapped.target,
        RowTarget::Web("https://example.com/next".into())
    );
}

#[gpui::test]
fn the_modifier_held_over_another_pane_hovers_nothing_here(cx: &mut TestAppContext) {
    let (view, cx, _, _) = shown(cx, &[&format!("{URL} at the edge")], false);
    let link = at(&view, cx, 2, 0);
    cx.simulate_mouse_move(link, None, link_modifier());
    cx.update(|_, cx| assert!(!view.read(cx).link_rows().is_empty()));
    // Leaving the pane drops the underline: moves off it reach it no more.
    let away = link - point(px(200.), px(0.));
    cx.simulate_mouse_move(away, None, link_modifier());
    cx.update(|_, cx| assert!(view.read(cx).link_rows().is_empty()));
    // The modifier pressed with the pointer over a pane beside this one,
    // left of its first column, where the cell lookup would hold the
    // pointer on the address there.
    let beside = cx.update(|_, cx| view.read(cx).bounds().unwrap().origin) - point(px(5.), px(0.));
    cx.update(|_, cx| view.update(cx, |view, cx| view.hover_at(beside, link_modifier(), cx)));
    cx.update(|_, cx| assert!(view.read(cx).link_rows().is_empty()));
}
