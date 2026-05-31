use super::render_script_a::CONFIG_UI_SCRIPT_A;
use super::render_script_b::CONFIG_UI_SCRIPT_B;
use super::render_script_c::CONFIG_UI_SCRIPT_C;
use super::render_script_d::CONFIG_UI_SCRIPT_D;
use super::render_style::CONFIG_UI_STYLE;
use super::UiServerState;
use crate::control_plane::UI_AUTH_HEADER;

use serde_json;

pub(super) fn render_html(state: &UiServerState) -> String {
    let model = serde_json::json!({
        "selectedExtensionId": state.selected_extension_id,
        "descriptors": state.descriptors,
        "discoverableDescriptors": state.discoverable_descriptors,
        "coreExtensionIds": state.core_extension_ids,
        "allowClose": state.allow_close,
        "authToken": state.auth_token,
        "coreUiTheme": initial_ui_theme(state),
    });
    let model_inline = serde_json::to_string(&model).unwrap_or_else(|_| "{}".to_string());

    let style = unescape_template(CONFIG_UI_STYLE);
    let script = [
        CONFIG_UI_SCRIPT_A,
        CONFIG_UI_SCRIPT_B,
        CONFIG_UI_SCRIPT_C,
        CONFIG_UI_SCRIPT_D,
    ]
    .into_iter()
    .map(unescape_template)
    .collect::<String>()
    .replace("{UI_AUTH_HEADER}", UI_AUTH_HEADER);

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Copper Settings</title>
{style}
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
        <button id="closeBtn">Close UI Server</button>
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

fn unescape_template(value: &str) -> String {
    value.replace("{{", "{").replace("}}", "}")
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
        .unwrap_or_else(|| "obsidian-light".to_string())
}
