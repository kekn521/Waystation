use waystation::ui::layout::{LayoutMode, mode};

#[test]
fn mode_boundaries() {
    // Wide: w >= 120 && h >= 38
    assert_eq!(mode(120, 38), LayoutMode::Wide);
    assert_eq!(mode(200, 60), LayoutMode::Wide);
    // Just under Wide thresholds -> Compact
    assert_eq!(mode(120, 37), LayoutMode::Compact);
    assert_eq!(mode(119, 38), LayoutMode::Compact);
    // Single lower bound: w >= 90 && h >= 30 keeps Compact
    assert_eq!(mode(90, 30), LayoutMode::Compact);
    // Just under Single thresholds -> Single
    assert_eq!(mode(89, 30), LayoutMode::Single);
    assert_eq!(mode(90, 29), LayoutMode::Single);
    // TooSmall lower bounds
    assert_eq!(mode(59, 18), LayoutMode::TooSmall);
    assert_eq!(mode(60, 17), LayoutMode::TooSmall);
    assert_eq!(mode(59, 17), LayoutMode::TooSmall);
    // A mid-range terminal is Compact
    assert_eq!(mode(100, 34), LayoutMode::Compact);
}
