mod tests {
    use super::{collect_specs, AdditionalTrayController};
    use crate::extension::Registry;
    use std::fs;
    use std::path::Path;
    use std::sync::{atomic::AtomicBool, Arc};
    use tempfile::tempdir;

    fn write_extension(root: &Path, manifest: &str) {
        fs::create_dir_all(root).expect("create extension dir");
        fs::write(root.join("manifest.json"), manifest).expect("write manifest");
        fs::write(root.join("main.ts"), "export default function(){}").expect("write main.ts");
    }

    #[test]
    fn initialize_without_enabled_extensions_creates_empty_controller() {
        let temp = tempdir().expect("tempdir");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let (settings_ui, _requests) = crate::config_ui::settings_ui_channel();
        let controller = AdditionalTrayController::initialize(
            Arc::new(AtomicBool::new(true)),
            settings_ui,
            &registry,
        )
        .expect("controller");
        assert!(controller.specs().is_empty());
    }

    #[test]
    fn collect_specs_discovers_tray_specs_from_registry_metadata() {
        let temp = tempdir().expect("tempdir");
        write_extension(
            &temp.path().join("windows-display-manager"),
            r#"{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "windows-display-manager",
                "name": "Windows Display Manager",
                "version": "1.0.0",
                "trigger": "windows-display",
                "permissions": ["ui", "store"],
                "actions": [{ "id": "status", "label": "Status", "script": "return;" }],
                "tray": {
                    "provider": "windows-display",
                    "title": "Windows Display Manager",
                    "tooltip": "Taskbar and display shortcuts"
                }
            }"#,
        );
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let specs = collect_specs(&registry);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].extension_id, "windows-display-manager");
        assert_eq!(specs[0].provider, "windows-display");
        assert_eq!(specs[0].title, "Windows Display Manager");
    }
}
