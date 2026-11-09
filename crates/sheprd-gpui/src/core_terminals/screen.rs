//! What one attached terminal shows, built from its backend's updates.

use sheprd_backend::{Exit, FrameData, KeyReporting, ScrollMetrics, SurfaceUpdate};
use sheprd_client::{SurfaceImages, protocol::SurfaceGraphicsPlacement};
use std::sync::Arc;

/// The pixels of the images a terminal places, shared with the painter and
/// told apart by identity: the worker publishes a new set when it changes.
#[derive(Clone, Debug, Default)]
pub(crate) struct Images(pub(crate) Arc<SurfaceImages>);

impl PartialEq for Images {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Images {}

/// The latest screen of one terminal. The worker applies updates to it and
/// the window takes a copy when it changed; `Arc` keeps that copy cheap and
/// lets the worker patch rows without disturbing a frame being painted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Screen {
    /// Bells rung since the window last took the screen.
    pub(crate) bells: u32,
    /// The latest OSC 52 clipboard write since the window last took the
    /// screen: bounded UTF-8 text the emulator decoded.
    pub(crate) clipboard: Option<String>,
    pub(crate) ended: Option<Exit>,
    /// Why the terminal could not be shown, for its tab to say.
    pub(crate) failure: Option<String>,
    pub(crate) frame: Option<Arc<FrameData>>,
    /// Where the terminal's images sit on its screen (kitty graphics).
    pub(crate) graphics: Arc<[SurfaceGraphicsPlacement]>,
    /// Where the view is in its history.
    pub(crate) history: ScrollMetrics,
    /// The pixels those placements name, kept by the worker.
    pub(crate) images: Images,
    /// The key events the program asked for beyond typed text.
    pub(crate) keys: KeyReporting,
    /// Whether the program reads the mouse, so presses go to it.
    pub(crate) mouse_reporting: bool,
    /// How often the text changed; history points hold for one revision.
    pub(crate) revision: u64,
    pub(crate) title: Option<String>,
    /// Whether each row's line goes on to the next, as the terminal
    /// wrapped it: what lets an address be read across rows.
    pub(crate) wraps: Arc<[bool]>,
}

impl Screen {
    /// The frame's columns and rows, once one is shown.
    pub(crate) fn grid(&self) -> Option<(u16, u16)> {
        self.frame.as_ref().map(|frame| (frame.width, frame.height))
    }

    /// A copy for the window, moving out what happens once (bells and the
    /// clipboard write) so the next take does not repeat them.
    pub(crate) fn take(&mut self) -> Self {
        let bells = std::mem::take(&mut self.bells);
        let clipboard = self.clipboard.take();
        Self {
            bells,
            clipboard,
            ..self.clone()
        }
    }

    pub(crate) fn apply(&mut self, update: SurfaceUpdate) {
        match update {
            SurfaceUpdate::Bell => self.bells = self.bells.saturating_add(1),
            SurfaceUpdate::Clipboard(text) => self.clipboard = Some(text),
            SurfaceUpdate::Ended(exit) => self.ended = Some(exit),
            SurfaceUpdate::Frame(frame) => self.frame = Some(Arc::new(frame)),
            // The worker has moved the scene's bytes into its image store.
            SurfaceUpdate::Graphics(scene) => self.graphics = scene.placements.into(),
            SurfaceUpdate::KeyReporting(keys) => self.keys = keys,
            SurfaceUpdate::MouseReporting(reporting) => self.mouse_reporting = reporting,
            SurfaceUpdate::Rows { cursor, rows } => {
                // Rows only patch a frame already shown.
                let Some(frame) = self.frame.as_mut() else {
                    return;
                };
                let frame = Arc::make_mut(frame);
                let width = usize::from(frame.width);
                for row in rows {
                    if row.y >= frame.height {
                        continue;
                    }
                    let start = usize::from(row.y) * width + usize::from(row.x);
                    let room = width.saturating_sub(usize::from(row.x));
                    for (slot, cell) in frame.cells[start..]
                        .iter_mut()
                        .zip(row.cells.into_iter().take(room))
                    {
                        *slot = cell;
                    }
                }
                frame.cursor = cursor;
            }
            SurfaceUpdate::Scroll { metrics, revision } => {
                self.history = metrics;
                self.revision = revision;
            }
            SurfaceUpdate::Title(title) => {
                self.title = Some(title).filter(|title| !title.is_empty())
            }
            SurfaceUpdate::Wraps(wraps) => self.wraps = wraps.into(),
        }
    }
}
