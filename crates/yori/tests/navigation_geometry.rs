use yori::geometry::EditorGeometry;

#[test]
fn jumps_leave_context_and_clamp_at_document_edges() {
    // Toolbar + file header occupy 80 pixels, leaving twenty visible text rows.
    let geometry = EditorGeometry::new(15.0, 25.0, 1000.0, 480.0, 80.0, 82.0, 20.0);

    assert!((geometry.change_scroll_top(10, 100) - 140.0).abs() < f32::EPSILON);
    assert!(geometry.change_scroll_top(0, 100).abs() < f32::EPSILON);
    assert!(
        (geometry.change_scroll_top(99, 100) - geometry.vertical_scroll_limit(100)).abs()
            < f32::EPSILON
    );

    let hit = geometry.hit(15.0 + 82.0, 25.0 + 80.0, 0.0, 0.0);
    assert_eq!(hit.row, 0);
    assert!(!hit.in_gutter);
}

#[test]
fn tiny_viewports_prioritize_the_target_row_over_context() {
    let geometry = EditorGeometry::new(0.0, 0.0, 400.0, 100.0, 80.0, 82.0, 20.0);
    assert!((geometry.change_scroll_top(10, 100) - 200.0).abs() < f32::EPSILON);
    assert!(geometry.change_scroll_top(0, 1).abs() < f32::EPSILON);
}
