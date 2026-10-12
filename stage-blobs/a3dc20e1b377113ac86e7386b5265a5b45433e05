//! Geometry shared by the browser's custom wheel, scrollbar and reveal path.
//!
//! Keep the same nearest-item policy as MUI's ListState: an oversized row
//! reveals its header rather than aligning its bottom beyond the viewport.

pub(super) fn reveal_offset(offset: f64, viewport: f64, top: f64, bottom: f64) -> f64 {
    if top < offset {
        top
    } else if bottom > offset + viewport {
        if bottom - top > viewport {
            top
        } else {
            bottom - viewport
        }
    } else {
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::reveal_offset;

    #[test]
    fn oversized_row_reveals_its_header_in_a_short_viewport() {
        // Search-result rows can be taller than the resized browser pane.
        // Bottom alignment previously chose 136 and clipped the header at 100.
        assert_eq!(reveal_offset(0., 24., 100., 160.), 100.);
        assert_eq!(reveal_offset(100., 24., 100., 160.), 100.);
        assert_eq!(reveal_offset(120., 24., 100., 160.), 100.);
    }

    #[test]
    fn ordinary_rows_move_only_far_enough_to_be_visible() {
        assert_eq!(reveal_offset(0., 100., 120., 150.), 50.);
        assert_eq!(reveal_offset(100., 100., 60., 90.), 60.);
        assert_eq!(reveal_offset(50., 100., 80., 110.), 50.);
    }

    #[test]
    fn exact_viewport_row_and_boundary_rows_do_not_overscroll() {
        assert_eq!(reveal_offset(0., 40., 80., 120.), 80.);
        assert_eq!(reveal_offset(80., 40., 80., 120.), 80.);
        assert_eq!(reveal_offset(80., 40., 90., 120.), 80.);
    }
}
