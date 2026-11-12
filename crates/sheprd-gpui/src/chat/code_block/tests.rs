use super::*;
use core::prelude::v1::test;

#[test]
fn line_numbers_share_the_widest_numbers_width() {
    assert_eq!(digits(0), 1);
    assert_eq!(digits(9), 1);
    assert_eq!(digits(10), 2);
    assert_eq!(digits(1_000), 4);
    assert_eq!(line_number(7, 3), "  7");
    assert_eq!(line_number(120, 3), "120");
}

#[test]
fn stripes_alternate_only_rows_on_the_blocks_own_fill() {
    let theme = Theme::default();
    let stripe = stripe(&theme);
    assert_ne!(stripe, theme.surface);
    assert_eq!(row_fill(&theme, 0, theme.surface, true), theme.surface);
    assert_eq!(row_fill(&theme, 1, theme.surface, true), stripe);
    assert_eq!(row_fill(&theme, 2, theme.surface, true), theme.surface);
    assert_eq!(row_fill(&theme, 1, theme.surface, false), theme.surface);
    let tint = 0x123456;
    assert_eq!(row_fill(&theme, 1, tint, true), tint, "a diff tint stays");
}

#[test]
fn a_stripe_moves_toward_the_ink_in_light_and_dark_themes() {
    let luma = crate::contrast::luminance;
    for (surface, foreground) in [(0xffffff, 0x000000), (0x1e1e1e, 0xd4d4d4)] {
        let theme = Theme {
            surface,
            foreground,
            ..Theme::default()
        };
        let standard = stripe(&theme);
        assert!(
            (luma(standard) - luma(surface)).signum()
                == (luma(foreground) - luma(surface)).signum(),
            "{surface:06x}"
        );
        let high = Theme {
            contrast: crate::contrast::Contrast::High,
            ..theme
        };
        assert!(
            (luma(stripe(&high)) - luma(surface)).abs() > (luma(standard) - luma(surface)).abs(),
            "high contrast stripes stand further off"
        );
    }
}
