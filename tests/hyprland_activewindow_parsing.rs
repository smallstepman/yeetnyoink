use serde_json;

#[test]
fn parses_realistic_activewindow_payload() {
    let sample = r#"{
        "address": "0x2a",
        "class": "foot",
        "title": "shell",
        "pid": 123,
        "mapped": true,
        "geometry": {"x": 100, "y": 200, "w": 800, "h": 600},
        "monitor": {"name": "DP-1", "scale": 1},
        "properties": {"urgent": false, "sticky": false},
        "workspaces": ["1", "2"],
        "layer": "top",
        "decorations": {"borders": true, "rounded": false}
    }"#;

    // Import the adapter type from the crate under test
    use yeetnyoink::adapters::window_managers::hyprland::HyprlandClient;

    let window: HyprlandClient = serde_json::from_str(sample).expect("should parse realistic hyprctl payload");
    assert_eq!(window.address, "0x2a");
    assert_eq!(window.class.as_deref(), Some("foot"));
    assert_eq!(window.mapped.unwrap_or(false), true);
}
