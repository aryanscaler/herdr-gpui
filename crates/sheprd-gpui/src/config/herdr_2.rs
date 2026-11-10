//! `herdr_2_scrollback`: how many lines of output each Herdr 2 terminal
//! keeps in the app to scroll back through. The app emulates herdr-core's
//! terminals itself, so this is the app's to bound, as Herdr 1 bounds its own.
use std::ops::RangeInclusive;

/// Lines kept when the config names none.
pub(crate) const HERDR_2_SCROLLBACK: usize = 10_000;
/// The fewest and most lines a config may ask for; one outside is clamped.
pub(crate) const HERDR_2_SCROLLBACK_RANGE: RangeInclusive<usize> = 1_000..=100_000;

/// The lines `configured` asks for, within bounds.
pub(crate) fn scrollback(configured: Option<usize>) -> usize {
    configured.map_or(HERDR_2_SCROLLBACK, |lines| {
        lines.clamp(
            *HERDR_2_SCROLLBACK_RANGE.start(),
            *HERDR_2_SCROLLBACK_RANGE.end(),
        )
    })
}
