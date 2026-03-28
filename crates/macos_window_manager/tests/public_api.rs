use macos_window_manager::{
    ActiveSpaceFocusTargetHint, NativeBounds, NativeDirection,
};

#[test]
fn public_api_smoke_test() {
    let hint = ActiveSpaceFocusTargetHint {
        space_id: 7,
        bounds: NativeBounds {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        },
    };

    assert_eq!(hint.space_id, 7);
    assert!(matches!(NativeDirection::West, NativeDirection::West));
}
