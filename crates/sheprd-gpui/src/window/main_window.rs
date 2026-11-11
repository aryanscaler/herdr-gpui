//! The app's main windows, wherever they are shown. A main view is usually
//! its window's root, but a host (Moonframe) may show it inside a window of
//! its own, under another root. Such a view registers itself with the window
//! it was made in, and what looks for main windows (the control socket,
//! notifications, links, settings) asks here rather than only for windows
//! whose root is a main view.

use crate::window::SheprdWindow;
use anyhow::{Context as _, Result};
#[cfg(feature = "cloud")]
use gpui::Entity;
use gpui::{
    AnyWindowHandle, App, AppContext, Context, Global, WeakEntity, Window, WindowHandle, WindowId,
};

/// A main view and the window that shows it, used as a `WindowHandle` to it
/// would be.
#[derive(Clone, Debug)]
pub(crate) enum MainWindow {
    /// The window's root is the main view: the app's own windows, handled
    /// exactly as a `WindowHandle` handles them.
    Root(WindowHandle<SheprdWindow>),
    /// The main view inside a host's window, under the host's root.
    Hosted {
        handle: AnyWindowHandle,
        view: WeakEntity<SheprdWindow>,
    },
}

impl PartialEq for MainWindow {
    fn eq(&self, other: &Self) -> bool {
        self.window_id() == other.window_id()
    }
}

impl Eq for MainWindow {}

/// The main views made inside a host's window; closed ones are dropped as
/// views register.
#[derive(Default)]
struct Registry(Vec<(AnyWindowHandle, WeakEntity<SheprdWindow>)>);

impl Global for Registry {}

impl MainWindow {
    /// Records the view `cx` makes as shown in `window`, so it is found when
    /// `window`'s root is another view (a host's).
    pub(crate) fn register(window: &Window, cx: &mut Context<SheprdWindow>) {
        let entry = (window.window_handle(), cx.weak_entity());
        let registry = cx.default_global::<Registry>();
        registry.0.retain(|(_, view)| view.upgrade().is_some());
        registry.0.push(entry);
    }

    /// Every open main window, front to back.
    pub(crate) fn all(cx: &App) -> Vec<Self> {
        cx.windows()
            .into_iter()
            .filter_map(|handle| Self::from_any(handle, cx))
            .collect()
    }

    /// The main window `window` shows, if it shows one.
    pub(crate) fn of(window: &Window, cx: &App) -> Option<Self> {
        Self::from_any(window.window_handle(), cx)
    }

    fn from_any(handle: AnyWindowHandle, cx: &App) -> Option<Self> {
        if let Some(root) = handle.downcast::<SheprdWindow>() {
            return Some(Self::Root(root));
        }
        let registry = cx.try_global::<Registry>()?;
        registry
            .0
            .iter()
            .find(|(window, view)| {
                window.window_id() == handle.window_id() && view.upgrade().is_some()
            })
            .map(|(handle, view)| Self::Hosted {
                handle: *handle,
                view: view.clone(),
            })
    }

    pub(crate) fn window_id(&self) -> WindowId {
        match self {
            Self::Root(handle) => handle.window_id(),
            Self::Hosted { handle, .. } => handle.window_id(),
        }
    }

    /// The main view, as `WindowHandle::entity` gives it.
    #[cfg(feature = "cloud")]
    pub(crate) fn entity<C: AppContext>(&self, cx: &C) -> Result<Entity<SheprdWindow>> {
        match self {
            Self::Root(handle) => handle.entity(cx),
            Self::Hosted { view, .. } => view.upgrade().context("the main view has closed"),
        }
    }

    /// Updates the main view with its window, as `WindowHandle::update` does.
    pub(crate) fn update<C, R>(
        &self,
        cx: &mut C,
        update: impl FnOnce(&mut SheprdWindow, &mut Window, &mut Context<SheprdWindow>) -> R,
    ) -> Result<R>
    where
        C: AppContext,
    {
        match self {
            Self::Root(handle) => handle.update(cx, update),
            Self::Hosted { handle, view } => {
                let view = view.clone();
                cx.update_window(*handle, move |_, window, cx| {
                    let view = view.upgrade().context("the main view has closed")?;
                    Ok(view.update(cx, |view, cx| update(view, window, cx)))
                })?
            }
        }
    }

    /// Reads the main view, as `WindowHandle::read` does.
    pub(crate) fn read<'a>(&self, cx: &'a App) -> Result<&'a SheprdWindow> {
        match self {
            Self::Root(handle) => handle.read(cx),
            Self::Hosted { view, .. } => {
                let view = view.upgrade().context("the main view has closed")?;
                Ok(view.read(cx))
            }
        }
    }

    /// Reads the main view, as `WindowHandle::read_with` does.
    pub(crate) fn read_with<C, R>(
        &self,
        cx: &C,
        read: impl FnOnce(&SheprdWindow, &App) -> R,
    ) -> Result<R>
    where
        C: AppContext,
    {
        match self {
            Self::Root(handle) => handle.read_with(cx, read),
            Self::Hosted { view, .. } => {
                let view = view.upgrade().context("the main view has closed")?;
                Ok(cx.read_entity(&view, read))
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
