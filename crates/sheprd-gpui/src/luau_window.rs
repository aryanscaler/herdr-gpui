//! All of Sheprd in a window Moonframe lays out from `moonframe/app/main.luau`,
//! so the layout around it can change without a rebuild: the `moonframe`
//! feature, off by default. A build with it opens this window on a plain
//! launch; see [`wanted`].

use {
    crate::embed::{Action, Drag, Host, HostedState, Menu, Region, Sizes, Stamp, Startup},
    async_channel::{Receiver, Sender, bounded},
    futures_lite::future::or,
    moonframe::{
        Data, Handle, HostView, Moonframe, Native, ViewInit,
        gpui_kit::{prelude::*, *},
    },
    serde_json::Value,
    std::{
        cell::RefCell,
        process::ExitCode,
        rc::Rc,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    },
};

mod theme;

/// How often state is read anyway: the machine's load, the activity counts
/// and Git's marks change without the window saying so.
const REFRESH: Duration = Duration::from_secs(2);
/// Actions waiting for the UI: clicks, so few. More are dropped, not queued.
const ACTION_QUEUE: usize = 64;
/// The build's own icon: Sheprd's paw in Moonframe's frame. Regenerate with
/// `scripts/generate-moonframe-icon.py` after changing `moonframe/assets/icon.svg`.
const ICON_PNG: &[u8] = include_bytes!("../moonframe/assets/icon.png");
const ICON_ICNS: &[u8] = include_bytes!("../moonframe/assets/Sheprd-Moonframe.icns");

/// One provider's data: `app.<name>`, published as it changes. Each part of
/// the state has its own, so a change runs only the views that read that part.
#[derive(Clone, Default)]
struct Feed {
    handle: Arc<Mutex<Option<Handle>>>,
    wake: Option<Sender<()>>,
}

impl Feed {
    fn new(wake: Sender<()>) -> Self {
        Self {
            handle: Arc::default(),
            wake: Some(wake),
        }
    }

    /// Whether the provider has started and took `state`.
    fn publish(&self, state: &Value) -> bool {
        let Ok(handle) = self.handle.lock() else {
            return false;
        };
        let Some(handle) = handle.as_ref() else {
            return false;
        };
        handle.publish_json(state);
        true
    }

    /// Publishes `state` unless it is what `last` holds. Whether mods have it:
    /// a provider not started yet gets it when it does.
    fn offer(&self, last: &mut Option<Value>, state: Value) -> bool {
        if last.as_ref() == Some(&state) {
            return true;
        }
        let taken = self.publish(&state);
        if taken {
            *last = Some(state);
        }
        taken
    }
}

impl Native for Feed {
    fn start(&mut self, host: &Handle) -> Result<(), String> {
        if let Ok(mut handle) = self.handle.lock() {
            *handle = Some(host.clone());
        }
        // What was read before this provider started never reached it.
        if let Some(wake) = &self.wake {
            let _ = wake.try_send(());
        }
        Ok(())
    }

    fn tick(&mut self, _: &Handle) {}

    fn receive(&mut self, _: &Handle, _: &str, _: Data) {}
}

/// The `sheprd` provider: `app.sheprd`, and what mods send it.
#[derive(Clone)]
struct Bridge {
    actions: Sender<Action>,
    feed: Feed,
}

impl Native for Bridge {
    fn start(&mut self, host: &Handle) -> Result<(), String> {
        self.feed.start(host)
    }

    fn tick(&mut self, _: &Handle) {}

    fn receive(&mut self, _: &Handle, _: &str, message: Data) {
        if let Some(action) = action(&message) {
            let _ = self.actions.try_send(action);
        }
    }
}

/// The providers state is published through.
#[derive(Clone)]
struct Feeds {
    load: Feed,
    main: Feed,
    theme: Feed,
}

/// What each of [`Feeds`] last took.
#[derive(Default)]
struct Published {
    load: Option<Value>,
    main: Option<Value>,
    theme: Option<Value>,
}

impl Feeds {
    /// Whether every provider has `state`.
    fn offer(&self, last: &mut Published, state: HostedState) -> bool {
        // `palette` is for the toolkit, not mods.
        let load = self.load.offer(&mut last.load, state.load);
        let main = self.main.offer(&mut last.main, state.main);
        let theme = self.theme.offer(&mut last.theme, state.theme);
        load && main && theme
    }
}

/// What a mod's message to `provider:sheprd` asks for, if anything. A row
/// on another device names it with `endpoint`; without one the selected
/// device's is meant.
fn action(message: &Data) -> Option<Action> {
    let text = |key: &str| message.get(key).and_then(Data::as_str).map(str::to_owned);
    let flag = |key: &str| message.get(key).and_then(Data::as_bool);
    let endpoint = text("endpoint");
    let menu = |kind: &str| match kind {
        "agent" => text("pane").map(|pane| Menu::Agent {
            endpoint: endpoint.clone(),
            pane,
        }),
        "device" => endpoint.clone().map(Menu::Device),
        "workspace" => text("workspace").map(|workspace| Menu::Workspace {
            endpoint: endpoint.clone(),
            workspace,
        }),
        _ => None,
    };
    text("command")
        .map(Action::Command)
        .or_else(|| {
            text("focus_workspace").map(|workspace| Action::FocusWorkspace {
                endpoint: endpoint.clone(),
                workspace,
            })
        })
        .or_else(|| {
            text("focus_pane").map(|pane| Action::FocusPane {
                endpoint: endpoint.clone(),
                pane,
            })
        })
        .or_else(|| text("panel").map(Action::Panel))
        .or_else(|| text("menu").and_then(|kind| menu(&kind)).map(Action::Menu))
        .or_else(|| {
            text("note").map(|workspace| Action::EditNote {
                endpoint: endpoint.clone(),
                workspace,
            })
        })
        .or_else(|| {
            text("toggle_repository").map(|key| Action::ToggleRepository {
                endpoint: endpoint.clone(),
                key,
            })
        })
        .or_else(|| {
            let target = text("target")?;
            text("move_workspace").map(|workspace| Action::MoveWorkspace {
                workspace,
                target,
                after: flag("after").unwrap_or(false),
            })
        })
        .or_else(|| sidebar_width(message))
        .or_else(|| text("toggle_device").map(Action::ToggleDevice))
        .or_else(|| text("select_device").map(Action::SelectDevice))
        .or_else(|| flag("fold_all").map(Action::FoldAll))
        .or_else(|| flag("toggle_agent_sort").map(|_| Action::ToggleAgentSort))
        .or_else(|| flag("open_build_pr").map(|_| Action::OpenBuildPullRequest))
}

/// A drag of the sidebar's edge: `pointer = { sidebar_width = true }` sends
/// its `phase`, its `dx` since the press, and the press's `clicks`.
fn sidebar_width(message: &Data) -> Option<Action> {
    message.get("sidebar_width").and_then(Data::as_bool)?;
    let dx = message.get("dx").and_then(Data::as_f64).unwrap_or(0.0) as f32;
    let phase = match message.get("phase").and_then(Data::as_str)? {
        "down" => Drag::Down {
            reset: message.get("clicks").and_then(Data::as_f64) == Some(2.0),
        },
        "move" => Drag::Move,
        "up" => Drag::Up,
        _ => return None,
    };
    Some(Action::SidebarWidth { phase, dx })
}

/// What is read before the event loop, and the host the first region makes.
struct Shared {
    actions: Receiver<Action>,
    feeds: Feeds,
    host: Option<Host>,
    startup: Option<Startup>,
    /// Told the window changed, or a provider started.
    wake: Sender<()>,
    woken: Receiver<()>,
}

/// Whether this launch opens the Luau window: a plain launch, or Moonframe's
/// own `--check` of the layout. Anything else (a `sheprd` command for the
/// running app, `--socket`, `--session`) is the app's, as without the feature.
pub(crate) fn wanted(args: &[std::ffi::OsString]) -> bool {
    match args {
        [] => true,
        [first, ..] => first == "--check",
    }
}

/// Runs the app in the Luau window until it quits. The app's private entry
/// points (`app::run_helpers`) have already had their turn.
pub(crate) fn run() -> ExitCode {
    // Before the app's startup sets the Dock icon and the About image.
    crate::embed::icon(ICON_PNG, ICON_ICNS);
    // Read before the event loop, as the app's own startup does.
    let (actions, received) = bounded(ACTION_QUEUE);
    // One is enough: a wake only says to look.
    let (wake, woken) = bounded(1);
    let feeds = Feeds {
        load: Feed::new(wake.clone()),
        main: Feed::new(wake.clone()),
        theme: Feed::new(wake.clone()),
    };
    let bridge = Bridge {
        actions,
        feed: feeds.main.clone(),
    };
    let (load, theme) = (feeds.load.clone(), feeds.theme.clone());
    let shared = Rc::new(RefCell::new(Shared {
        actions: received,
        feeds,
        host: None,
        startup: Some(crate::embed::prepare()),
        wake,
        woken,
    }));
    Moonframe::new("sheprd-moonframe")
        .title("Sheprd")
        .app_id("dev.sheprd.moonframe")
        .version(env!("CARGO_PKG_VERSION"))
        .app_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/moonframe/app"))
        .icon(ICON_PNG)
        .assets(crate::embed::assets())
        // Sheprd's own menu bar, as its window has it; Moonframe adds its
        // Developer menu.
        .menus(crate::embed::menus)
        // Sheprd's log file, which its Logs window reads, beside stderr.
        .log_layer(crate::embed::log_layer())
        // The sparkles: ask an agent to change the layout or add something.
        .keep(["feedback"])
        .native("sheprd", move || bridge.clone())
        .native("sheprd_load", move || load.clone())
        .native("sheprd_theme", move || theme.clone())
        .view("sheprd", move |init: &ViewInit, window, cx| {
            SheprdRegion::new(init, &shared, window, cx)
        })
        .run()
}

/// One region of the main window, where Luau placed it.
struct SheprdRegion {
    /// Over the whole window, for the main region; see `render`.
    overlay: Option<AnyView>,
    _sizes: Option<Subscription>,
    view: AnyView,
}

impl SheprdRegion {
    fn new(
        init: &ViewInit,
        shared: &Rc<RefCell<Shared>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let host = host(shared, window, cx);
        let name = init.props().get("region").and_then(Data::as_str);
        let region = match name {
            Some("activity") => Some(Region::ActivityBar),
            Some("header") => Some(Region::Header),
            Some("sidebar") => Some(Region::Sidebar),
            Some("status") => Some(Region::StatusBar),
            _ => None,
        };
        let view = match region {
            Some(region) => host.region(region, cx),
            None => host.main(),
        };
        #[cfg(feature = "integration-test")]
        if region.is_none() {
            drive(&view, window, cx);
        }
        // The main region tells the layout what size the others want.
        let sizes = region.is_none().then(|| {
            let messenger = init.messenger();
            let mut last = None;
            let mut report = move |sizes: Sizes| {
                if last.replace(sizes) != Some(sizes) {
                    messenger.send(Data::map([
                        ("activity_bar", f64::from(sizes.activity_bar).into()),
                        ("header", f64::from(sizes.header).into()),
                        ("left", f64::from(sizes.left).into()),
                        ("status_bar", f64::from(sizes.status_bar).into()),
                    ]));
                }
            };
            report(host.sizes(window, cx));
            let wake = shared.borrow().wake.clone();
            host.observe_sizes(window, cx, move |_, sizes, _| {
                let _ = wake.try_send(());
                report(sizes);
            })
        });
        let overlay = region.is_none().then(|| host.region(Region::Overlay, cx));
        if region.is_none() {
            let shared = shared.borrow();
            take_actions(host.clone(), shared.actions.clone(), window, cx);
            publish_state(
                host.clone(),
                shared.feeds.clone(),
                shared.woken.clone(),
                window,
                cx,
            );
            let _ = shared.wake.try_send(());
        }
        Self {
            overlay,
            _sizes: sizes,
            view,
        }
    }
}

/// Does what mods ask as it arrives: a click must not wait for a tick.
fn take_actions(
    host: Host,
    actions: Receiver<Action>,
    window: &mut Window,
    cx: &mut Context<SheprdRegion>,
) {
    cx.spawn_in(window, async move |_, cx| {
        while let Ok(action) = actions.recv().await {
            if cx
                .update(|window, cx| host.act(action, window, cx))
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

/// Offers mods the window's state as soon as it changes. The window notifies
/// for every terminal frame, so its [`Stamp`] says first whether anything mods
/// show changed, and only then is state built. [`REFRESH`] covers the rest,
/// and keeps its time however busy the window is.
fn publish_state(
    host: Host,
    feeds: Feeds,
    woken: Receiver<()>,
    window: &mut Window,
    cx: &mut Context<SheprdRegion>,
) {
    cx.spawn_in(window, async move |_, cx| {
        let mut follow = theme::Follow::default();
        let mut last = Published::default();
        let mut stamp: Option<Stamp> = None;
        let mut refresh = Instant::now() + REFRESH;
        loop {
            let wait = refresh.saturating_duration_since(Instant::now());
            let timer = cx.background_executor().timer(wait);
            let woke = or(async { Some(woken.recv().await) }, async {
                timer.await;
                None
            })
            .await;
            // The region went: nothing wakes this any more.
            if matches!(woke, Some(Err(_))) {
                break;
            }
            let due = Instant::now() >= refresh;
            if due {
                refresh = Instant::now() + REFRESH;
            }
            let alive = cx.update(|_, cx| {
                let now = host.stamp(cx);
                if due || stamp.as_ref() != Some(&now) {
                    let state = host.state(cx);
                    follow.apply(&state.palette, cx);
                    let delivered = feeds.offer(&mut last, state);
                    stamp = delivered.then_some(now);
                }
            });
            if alive.is_err() {
                break;
            }
        }
    })
    .detach();
}

fn host(shared: &RefCell<Shared>, window: &mut Window, cx: &mut App) -> Host {
    let shared = &mut *shared.borrow_mut();
    let startup = &mut shared.startup;
    shared
        .host
        .get_or_insert_with(|| {
            // Before the host installs: from then on Sheprd's own changes to
            // its menus (a layout picked, the language) rebuild Moonframe's bar.
            crate::embed::keep_menus(moonframe::refresh_menus, cx);
            Host::new(startup.take(), window, cx)
        })
        .clone()
}

impl HostView for SheprdRegion {}

impl Render for SheprdRegion {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Menus and toasts use window coordinates and cover every region. They
        // are painted here rather than placed by Luau: they are not part of the
        // layout, and as an element they would cover it for Inspect.
        let overlay = self.overlay.clone().map(|overlay| {
            let size = window.viewport_size();
            deferred(
                anchored()
                    .position_mode(AnchoredPositionMode::Window)
                    .position(point(px(0.), px(0.)))
                    .child(div().w(size.width).h(size.height).child(overlay)),
            )
        });
        // `Terminal` keeps Moonframe's Escape off: it belongs to the terminal.
        div()
            .key_context("Terminal")
            .size_full()
            .child(self.view.clone())
            .children(overlay)
    }
}

/// Sends `SPIKE_DRIVE` to the view, steps apart by `;` a second after each
/// other: `keys:a b enter`, `drag:x1,y1,x2,y2`, `hover:x,y`, `rclick:x,y`, or
/// `scroll:x,y,by`.
#[cfg(feature = "integration-test")]
fn drive(view: &AnyView, window: &Window, cx: &mut App) {
    use crate::embed::Drive;
    let Ok(steps) = std::env::var("SPIKE_DRIVE") else {
        return;
    };
    let start: u64 = std::env::var("SPIKE_DRIVE_DELAY")
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .unwrap_or(7);
    for (index, step) in steps.split(';').enumerate() {
        let numbers = |spec: &str| -> Vec<f32> {
            spec.split(',')
                .filter_map(|number| number.parse().ok())
                .collect()
        };
        let input = if let Some(keys) = step.strip_prefix("keys:") {
            Drive::Keys(keys.to_owned())
        } else if let Some(at) = step.strip_prefix("scroll:") {
            let [x, y, by] = numbers(at)[..] else {
                continue;
            };
            Drive::Scroll { at: (x, y), y: by }
        } else if let Some(at) = step.strip_prefix("hover:") {
            let [x, y] = numbers(at)[..] else { continue };
            Drive::Hover { at: (x, y) }
        } else if let Some(at) = step.strip_prefix("rclick:") {
            let [x, y] = numbers(at)[..] else { continue };
            Drive::RightClick { at: (x, y) }
        } else {
            let [x1, y1, x2, y2] = numbers(step.trim_start_matches("drag:"))[..] else {
                continue;
            };
            Drive::Drag {
                from: (x1, y1),
                to: (x2, y2),
            }
        };
        let delay = Duration::from_secs(start + index as u64);
        crate::embed::drive(view.clone(), input, delay, window, cx);
    }
}

#[cfg(test)]
mod tests;
