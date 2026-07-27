use std::path::Path;

use super::UiServerState;
use serde_json;

struct UiAssets {
    scripts: Vec<String>,
    style: String,
}

impl UiAssets {
    fn bundled() -> Self {
        Self {
            scripts: vec![
                include_str!("../ui/utils.js").to_string(),
                include_str!("../ui/nav.js").to_string(),
                include_str!("../ui/controls.js").to_string(),
                include_str!("../ui/sections.js").to_string(),
                include_str!("../ui/handlers.js").to_string(),
            ],
            style: include_str!("../ui/style.css").to_string(),
        }
    }

    fn from_dir(dir: &Path) -> Option<Self> {
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
        Some(Self {
            scripts: vec![
                read("utils.js")?,
                read("nav.js")?,
                read("controls.js")?,
                read("sections.js")?,
                read("handlers.js")?,
            ],
            style: read("style.css")?,
        })
    }

    fn load() -> Self {
        if let Ok(dir) = std::env::var("COPPER_UI_DIR") {
            if let Some(assets) = Self::from_dir(Path::new(&dir)) {
                return assets;
            }
        }
        if let Some(assets) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("ui")))
            .and_then(|dir| Self::from_dir(&dir))
        {
            return assets;
        }
        Self::bundled()
    }
}

pub(super) fn render_html(state: &UiServerState) -> String {
    let model = serde_json::json!({
        "selectedExtensionId": state.selected_extension_id,
        "descriptors": state.descriptors,
        "discoverableDescriptors": state.discoverable_descriptors,
        "coreExtensionIds": state.core_extension_ids,
        "allowClose": state.allow_close,
        "authToken": if state.transport == super::UiTransport::Http {
            state.auth_token.as_str()
        } else {
            ""
        },
        "transport": if state.transport == super::UiTransport::Bones {
            "bones"
        } else {
            "http"
        },
        "settingsProtocol": super::COPPER_SETTINGS_PROTOCOL_V1,
        "coreUiTheme": initial_ui_theme(state),
    });
    let model_inline = serde_json::to_string(&model).unwrap_or_else(|_| "{}".to_string());

    let assets = UiAssets::load();
    let style = &assets.style;
    let script = assets.scripts.join("\n");

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Copper Settings</title>
  <style>
{style}
  </style>
</head>
<body>
  <div class="layout">
    <aside class="sidebar">
      <div class="title">Settings</div>
      <p class="subtitle">Manifest-driven pages with optional tabs.</p>
      <div id="nav"></div>
    </aside>
    <main class="main">
      <div class="page-eyebrow" id="pageEyebrow">Extension</div>
      <h1 class="page-title" id="pageTitle">Copper</h1>
      <p class="page-sub" id="pageSub">Core Copper configuration</p>
      <div class="tab-row" id="tabs"></div>
      <section id="contentView"></section>
      <div class="btn-row">
        <button class="primary" id="saveBtn">Save settings</button>
        <button id="reloadExtensionsBtn">Reload extensions</button>
        <button id="closeBtn">Close</button>
      </div>
      <div class="status-msg" id="statusMsg"></div>
    </main>
  </div>
  <script>
    const model = {model_inline};
{script}
    renderSection().catch(err => setStatus('Load failed: ' + err));
  </script>
</body>
</html>
"#
    )
}

fn initial_ui_theme(state: &UiServerState) -> String {
    state
        .state_store
        .inspect_core_config()
        .ok()
        .and_then(|loaded| {
            loaded
                .value
                .get("uiTheme")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "light".to_string())
}
