pub const RELEASE_TAG: &str = "zellij-plugin-latest";
pub const RELEASE_ASSET_NAME: &str = "yeet-and-yoink-zellij-break.wasm";
pub const RELEASE_REPO: &str = "https://github.com/smallstepman/yeet-and-yoink";

pub fn release_wasm_url() -> &'static str {
    "https://github.com/smallstepman/yeet-and-yoink/releases/download/zellij-plugin-latest/yeet-and-yoink-zellij-break.wasm"
}

pub fn release_page_url() -> &'static str {
    "https://github.com/smallstepman/yeet-and-yoink/releases/tag/zellij-plugin-latest"
}

pub fn instructions() -> String {
    format!(
        "Zellij can load the yeet-and-yoink break plugin straight from GitHub Releases.\n\n\
Add this to your `~/.config/zellij/config.kdl`:\n\n\
load_plugins {{\n    {release_url}\n}}\n\n\
If you already have a `load_plugins` block, add that URL inside it.\n\
Then restart zellij or start a new session.\n\
When zellij prompts for plugin permissions, accept them.\n\n\
`yny` will use the same release URL automatically unless you override \
`[runtime.zellij].break_plugin` with a local `.wasm` path.\n\n\
Release page:\n  {release_page}",
        release_url = release_wasm_url(),
        release_page = release_page_url(),
    )
}

#[cfg(test)]
mod tests {
    use super::{instructions, release_page_url, release_wasm_url, RELEASE_ASSET_NAME, RELEASE_REPO, RELEASE_TAG};

    #[test]
    fn instructions_include_load_plugins_snippet_and_release_links() {
        let text = instructions();

        assert!(text.contains("load_plugins {"));
        assert!(text.contains(release_wasm_url()));
        assert!(text.contains(release_page_url()));
        assert!(text.contains("[runtime.zellij].break_plugin"));
        assert!(release_wasm_url().contains(RELEASE_TAG));
        assert!(release_wasm_url().contains(RELEASE_ASSET_NAME));
        assert!(release_wasm_url().starts_with(RELEASE_REPO));
    }
}
