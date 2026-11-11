//! The main view, for another GPUI app's window to host. [`prepare`] reads
//! what [`crate::app`] reads before its event loop, and [`Host`] installs it
//! and makes the view, on the Local target, in the regions a layout places.
//!
//! The host owns the window, so the view registers itself with it (see
//! `window::main_window`), and what looks for main windows (the control
//! socket, system notifications, links) finds it there. Startup is the app's
//! own (`app::install_services`), the menu bar is the app's, kept by the host
//! (see [`keep_menus`]), and the log is the app's, as a layer of the host's
//! (see [`log_layer`]).

pub use crate::{
    sidebar::hosted::{Action, Drag, HostedState, Menu, Palette, Stamp},
    window::host_regions::{Region, Sizes},
};

use crate::{
    SheprdWindow,
    app::{InitialAppearance, StartupAppearance, Stores, light_appearance, shared_sound},
    config::Config,
};
use gpui::{
    AnyView, App, AppContext as _, Context, Empty, Entity, IntoElement, Render, Subscription,
    WeakEntity, Window,
};
#[cfg(feature = "integration-test")]
use gpui::{
    Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    ScrollDelta, ScrollEvent, point, px,
};
use sheprd_client::ConnectTarget;
#[cfg(feature = "integration-test")]
use std::time::Duration;

/// What is read before the event loop; see [`prepare`].
pub struct Startup {
    appearance: StartupAppearance,
    stores: Stores,
}

/// Runs the app's private entry points when this process was started as one:
/// the app starts its own executable to read the clipboard, apply an update,
/// or relay a command, and waits for the answer. Call it first in `main`,
/// before the host reads the command line, and exit with what it returns.
pub fn run_helpers() -> Option<std::process::ExitCode> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    crate::app::run_helpers(&mut args)
}

/// Reads settings and stores. Call it before the host's event loop, so the
/// first frame does no disk work on the UI thread.
pub fn prepare() -> Startup {
    // A panic leaves a report where the app's own do, beside the host's.
    crate::crash::install(crate::APP_VERSION);
    sheprd_client::set_command_environment(crate::login_env::apply);
    sheprd_remote::config::set_loader(Config::load_remote);
    // Before anything resolves a config path or the daemon's socket.
    crate::login_env::adopt_config_home();
    Startup {
        appearance: StartupAppearance::load(Config::load_startup),
        stores: Stores::load(true),
    }
}

fn install(startup: Startup, cx: &mut App) {
    startup.stores.install(cx);
    cx.set_global(startup.appearance.select(light_appearance(cx)));
    // Everything the app's own startup sets up: the control socket, system
    // notifications, links, the Dock badge and icon, menus and keys.
    crate::app::install_services(true, cx);
}

/// Shows `png` (1024px) and `icns` as the app's icon, in the Dock and the
/// About box, in place of Sheprd's. Call it before the first [`Host`].
pub fn icon(png: &'static [u8], icns: &'static [u8]) {
    crate::app_icon::set_host(png, icns);
}

/// The app's log, as a layer for the host's subscriber: the file the log
/// window reads and crash reports quote, written as the app writes it. Make it
/// once, before the host starts its workers.
pub fn log_layer()
-> Box<dyn tracing_subscriber::Layer<tracing_subscriber::Registry> + Send + Sync + 'static> {
    sheprd_diagnostics::layer(crate::log_targets::LOG_TARGETS)
}

/// The window's menu bar for the latest config, as the app installs it, for
/// a host that keeps the bar itself; see [`keep_menus`].
pub fn menus(cx: &App) -> Vec<gpui::Menu> {
    crate::menus::bar(cx)
}

/// Has the host keep the menu bar: whenever the window would set its bar
/// again (a layout picked, the language changed), `refresh` runs instead, and
/// the host builds its bar with [`menus`].
pub fn keep_menus(refresh: impl Fn(&mut App) + 'static, cx: &mut App) {
    cx.set_global(crate::menus::HostMenus(std::rc::Rc::new(refresh)));
}

/// The menu bar's actions the window handles. In a host's window the focus
/// may sit in the host's own layout, outside the window's view, so an action
/// chosen there reaches no handler: each is sent on from the view's focus,
/// as if it had been chosen with the view focused.
fn forward_menu_actions(
    view: WeakEntity<SheprdWindow>,
    handle: gpui::AnyWindowHandle,
    cx: &mut App,
) {
    fn forward<A: gpui::Action>(
        view: WeakEntity<SheprdWindow>,
        handle: gpui::AnyWindowHandle,
        cx: &mut App,
    ) {
        thread_local! {
            /// Set while an action is sent on, so one the view does not
            /// handle either comes back here once and stops.
            static SENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        }
        cx.on_action(move |action: &A, cx| {
            if SENDING.get() {
                return;
            }
            let Some(view) = view.upgrade() else {
                return;
            };
            let focus = view.read(cx).focus.clone();
            let action = action.boxed_clone();
            SENDING.set(true);
            let _ = handle.update(cx, |_, window, cx| {
                focus.dispatch_action(action.as_ref(), window, cx);
            });
            SENDING.set(false);
        });
    }
    use crate::actions::{
        RingBellPreview, SetChatLayout, SetLayout, SetTabLayout, ShowSystemNotificationPreview,
        ShowToastPreview, ShowUpdateDownloadPreview, ShowUpdateHomebrewPreview,
    };
    use crate::{
        CheckForUpdates, Minimize, PlaySound, RunCommand, ShowHerdrNotDetected, ShowUpdatePreview,
        ZoomWindow,
    };
    // keep-sorted start
    forward::<CheckForUpdates>(view.clone(), handle, cx);
    forward::<Minimize>(view.clone(), handle, cx);
    forward::<PlaySound>(view.clone(), handle, cx);
    forward::<RingBellPreview>(view.clone(), handle, cx);
    forward::<RunCommand>(view.clone(), handle, cx);
    forward::<SetChatLayout>(view.clone(), handle, cx);
    forward::<SetLayout>(view.clone(), handle, cx);
    forward::<SetTabLayout>(view.clone(), handle, cx);
    forward::<ShowHerdrNotDetected>(view.clone(), handle, cx);
    forward::<ShowSystemNotificationPreview>(view.clone(), handle, cx);
    forward::<ShowToastPreview>(view.clone(), handle, cx);
    forward::<ShowUpdateDownloadPreview>(view.clone(), handle, cx);
    forward::<ShowUpdateHomebrewPreview>(view.clone(), handle, cx);
    forward::<ShowUpdatePreview>(view.clone(), handle, cx);
    forward::<ZoomWindow>(view, handle, cx);
    // keep-sorted end
}

/// The app's icons and other assets, for the host's application.
pub fn assets() -> impl gpui::AssetSource {
    crate::icons::Icons
}

/// The main window as views for a host's layout; see [`Region`].
#[derive(Clone)]
pub struct Host {
    window: Entity<SheprdWindow>,
}

impl Host {
    /// Makes the view on the Local target. `startup` is what [`prepare`] read,
    /// `None` when an earlier host already installed it.
    pub fn new(startup: Option<Startup>, window: &mut Window, cx: &mut App) -> Self {
        let window_handle = window.window_handle();
        // The first host installs the app, and its view checks for updates
        // as the app's first window does.
        let updater = startup.map(|startup| {
            install(startup, cx);
            crate::updater::Updater::start(cx.global::<InitialAppearance>().config.updates.channel)
        });
        let sound = shared_sound(cx);
        let window = cx.new(|cx| {
            let mut view = SheprdWindow::new(
                ConnectTarget::Local,
                window,
                cx,
                #[cfg(feature = "integration-test")]
                false,
            );
            view.hosted = true;
            view.use_sound(sound);
            if let Some(updater) = updater {
                view.updater = updater;
            }
            view
        });
        forward_menu_actions(window.downgrade(), window_handle, cx);
        Self { window }
    }

    /// The terminals, tabs and pages: everything the regions leave.
    pub fn main(&self) -> AnyView {
        self.window.clone().into()
    }

    pub fn region(&self, region: Region, cx: &mut App) -> AnyView {
        let window = self.window.clone();
        cx.new(|cx: &mut Context<RegionView>| {
            // Regions show the window's state, so they redraw with it.
            let observing = cx.observe(&window, |_, _, cx| cx.notify());
            RegionView {
                _observing: observing,
                region,
                window: window.downgrade(),
            }
        })
        .into()
    }

    /// The window's state for a layout of mods; see `sidebar::hosted`.
    pub fn state(&self, cx: &App) -> HostedState {
        self.window.read(cx).hosted_state(cx)
    }

    /// Equal stamps say [`Self::state`] has nothing new from the window.
    pub fn stamp(&self, cx: &App) -> Stamp {
        self.window.read(cx).hosted_stamp()
    }

    /// Does what a layout of mods asked.
    pub fn act(&self, action: Action, window: &mut Window, cx: &mut App) {
        self.window
            .update(cx, |view, cx| view.hosted_action(action, window, cx));
    }

    pub fn sizes(&self, window: &Window, cx: &App) -> Sizes {
        self.window.read(cx).region_sizes(window)
    }

    /// Calls `changed` whenever the main window changes, with its sizes.
    pub fn observe_sizes<T: 'static>(
        &self,
        window: &mut Window,
        cx: &mut Context<T>,
        mut changed: impl FnMut(&mut T, Sizes, &mut Context<T>) + 'static,
    ) -> Subscription {
        cx.observe_in(&self.window, window, move |this, view, window, cx| {
            let sizes = view.read(cx).region_sizes(window);
            changed(this, sizes, cx);
        })
    }
}

/// A [`Region`] drawn through the main window, as `SidebarView` is.
struct RegionView {
    _observing: Subscription,
    region: Region,
    window: WeakEntity<SheprdWindow>,
}

impl Render for RegionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let region = self.region;
        self.window
            .update(cx, |view, cx| view.render_region(region, window, cx))
            .unwrap_or_else(|_| Empty.into_any_element())
    }
}

/// What [`drive`] does to the hosted view, in window coordinates.
#[cfg(feature = "integration-test")]
pub enum Drive {
    /// Presses at `from`, drags to `to`, and releases.
    Drag { from: (f32, f32), to: (f32, f32) },
    /// Moves the pointer to `at`.
    Hover { at: (f32, f32) },
    /// Presses and releases the right button at `at`, as a context click.
    RightClick { at: (f32, f32) },
    /// Scrolls by `y` pixels at `at`; negative goes down the content.
    Scroll { at: (f32, f32), y: f32 },
    /// Types each keystroke in turn, a space between them.
    Keys(String),
    /// Types a keystroke such as `cmd-shift-p`.
    Key(String),
}

/// Sends `input` through the window as the platform would, after `delay`, for
/// checking that a pane offset and a host's focus leave input working.
#[cfg(feature = "integration-test")]
pub fn drive(view: AnyView, input: Drive, delay: Duration, window: &Window, cx: &mut App) {
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(delay).await;
            let _ = cx.update(|window, cx| match input {
                Drive::Drag { from, to } => {
                    let from = point(px(from.0), px(from.1));
                    let to = point(px(to.0), px(to.1));
                    let events = [
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: from,
                            pressed_button: None,
                            modifiers: Modifiers::default(),
                        }),
                        PlatformInput::MouseDown(MouseDownEvent {
                            button: MouseButton::Left,
                            position: from,
                            modifiers: Modifiers::default(),
                            click_count: 1,
                            first_mouse: false,
                        }),
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: to,
                            pressed_button: Some(MouseButton::Left),
                            modifiers: Modifiers::default(),
                        }),
                        PlatformInput::MouseUp(MouseUpEvent {
                            button: MouseButton::Left,
                            position: to,
                            modifiers: Modifiers::default(),
                            click_count: 1,
                        }),
                    ];
                    for (step, event) in events.into_iter().enumerate() {
                        let result = window.dispatch_event(event, cx);
                        if let Ok(view) = view.clone().downcast::<SheprdWindow>() {
                            let view = view.read(cx);
                            tracing::info!(
                                step,
                                prevented = result.default_prevented,
                                selection = view.selection.is_some(),
                                menu = view.menu.page.is_some(),
                                "drive: after event"
                            );
                        }
                    }
                    if let Ok(view) = view.downcast::<SheprdWindow>() {
                        let view = view.read(cx);
                        tracing::info!(
                            bounds = ?view.bounds,
                            grid = ?view.grid_position(from),
                            ready = view.live.surface_ready(),
                            selection = view.selection.is_some(),
                            menu = view.menu.page.is_some(),
                            "drive: terminal geometry"
                        );
                    }
                }
                Drive::RightClick { at } => {
                    let position = point(px(at.0), px(at.1));
                    window.dispatch_event(
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position,
                            pressed_button: None,
                            modifiers: Modifiers::default(),
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        PlatformInput::MouseDown(MouseDownEvent {
                            button: MouseButton::Right,
                            position,
                            modifiers: Modifiers::default(),
                            click_count: 1,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        PlatformInput::MouseUp(MouseUpEvent {
                            button: MouseButton::Right,
                            position,
                            modifiers: Modifiers::default(),
                            click_count: 1,
                        }),
                        cx,
                    );
                }
                Drive::Hover { at } => {
                    window.dispatch_event(
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: point(px(at.0), px(at.1)),
                            pressed_button: None,
                            modifiers: Modifiers::default(),
                        }),
                        cx,
                    );
                }
                Drive::Scroll { at, y } => {
                    window.dispatch_event(
                        PlatformInput::Scroll(ScrollEvent {
                            position: point(px(at.0), px(at.1)),
                            delta: ScrollDelta::Pixels(point(px(0.), px(y))),
                            ..Default::default()
                        }),
                        cx,
                    );
                }
                Drive::Keys(text) => {
                    if let Ok(view) = view.clone().downcast::<SheprdWindow>() {
                        let view = view.read(cx);
                        tracing::info!(
                            focused = view.focus.is_focused(window),
                            contains = view.focus.contains_focused(window, cx),
                            ready = view.live.surface_ready(),
                            "drive: before typing"
                        );
                    }
                    for key in text.split_whitespace() {
                        if let Ok(keystroke) = Keystroke::parse_with_os(key, crate::keymap::HOST_OS)
                        {
                            let handled = window.dispatch_keystroke(keystroke, cx);
                            tracing::info!(key, handled, "drive: keystroke");
                        }
                    }
                }
                Drive::Key(text) => match Keystroke::parse_with_os(&text, crate::keymap::HOST_OS) {
                    Ok(keystroke) => {
                        let handled = window.dispatch_keystroke(keystroke, cx);
                        tracing::info!(text, handled, "drive: keystroke");
                    }
                    Err(error) => tracing::warn!(%error, "drive: bad keystroke"),
                },
            });
        })
        .detach();
}
