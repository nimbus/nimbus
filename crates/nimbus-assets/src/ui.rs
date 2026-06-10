use std::borrow::Cow;

use rust_embed::Embed;

const SPA_INDEX: &str = "index.html";

const AUTH_PAGE_TEMPLATE: &str = include_str!("../embedded/ui-auth/auth.html");
const AUTH_PAGE_SCRIPT: &str = include_str!("../embedded/ui-auth/auth.js");

#[derive(Debug)]
pub struct UiAsset {
    pub data: Cow<'static, [u8]>,
}

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../packages/nimbus-ui/dist/"]
struct UiAssets;

pub fn asset(path: &str) -> Option<UiAsset> {
    UiAssets::get(path).map(|asset| UiAsset { data: asset.data })
}

pub fn iter() -> impl Iterator<Item = Cow<'static, str>> {
    UiAssets::iter()
}

pub fn index_html() -> Option<UiAsset> {
    asset(SPA_INDEX)
}

pub fn auth_page_template() -> &'static str {
    AUTH_PAGE_TEMPLATE
}

pub fn auth_page_script() -> &'static str {
    AUTH_PAGE_SCRIPT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_index_is_embedded() {
        let index = index_html().expect("index.html is embedded");
        assert!(
            !index.data.is_empty(),
            "embedded UI index.html must not be empty"
        );
    }

    #[test]
    fn auth_static_assets_are_embedded() {
        assert!(
            auth_page_template().contains(r#"<script src="/ui/auth.js" defer></script>"#),
            "auth template must load the external same-origin auth script"
        );
        assert!(
            auth_page_script().contains("button[data-copy]")
                && auth_page_script().contains("navigator.clipboard.writeText"),
            "auth script should contain the browser-side copy binding"
        );
    }
}
