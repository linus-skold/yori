use super::*;

#[test]
fn conflict_outline_stays_clear_of_dividers_at_fractional_widths() {
    for width in 900..1300 {
        let pane_width = display_units(width) / 3.0;
        for column in 0..3 {
            let left = display_units(column) * pane_width;
            let outline = outline_bounds(left, pane_width);

            assert!(
                outline.end <= left + pane_width - 2.0,
                "outline shares divider at width {width}, pane {column}"
            );
            assert!(outline.start < outline.end);
        }
    }
}
