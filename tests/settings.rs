use std::fs;
use std::path::PathBuf;

use live_captions_gtk::presets::Settings;

fn test_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "live-captions-settings-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after the Unix epoch")
            .as_nanos()
    ))
}

#[test]
fn settings_round_trip_uses_toml_and_atomic_target() {
    let root = test_root("round-trip");
    fs::create_dir_all(&root).expect("the temporary settings directory should be created");
    let path = root.join("settings.toml");

    let settings = Settings {
        language: "zh".into(),
        line_width: 72,
        installed_models: vec!["/models/zipformer".into()],
        ..Settings::default()
    };
    settings.save_to(&path).expect("settings should be saved");

    let content = fs::read_to_string(&path).expect("the TOML file should be readable");
    assert!(content.contains("language = \"zh\""));
    assert!(!path.with_extension("json").exists());

    let loaded = Settings::load_from(&path).expect("TOML settings should be loaded");
    assert_eq!(loaded.language, "zh");
    assert_eq!(loaded.line_width, 72);
    assert_eq!(loaded.installed_models, vec!["/models/zipformer"]);

    fs::remove_dir_all(&root).expect("the temporary settings directory should be removed");
}

#[test]
fn legacy_json_is_migrated_and_retained_as_backup() {
    let root = test_root("migration");
    fs::create_dir_all(&root).expect("the temporary settings directory should be created");
    let path = root.join("settings.toml");
    let legacy_path = root.join("settings.json");
    fs::write(
        &legacy_path,
        r#"{
            "language": "en",
            "line_width": 64,
            "auto_punctuation": false
        }"#,
    )
    .expect("the legacy JSON file should be written");

    let loaded = Settings::load_from(&path).expect("legacy settings should be migrated");

    assert_eq!(loaded.language, "en");
    assert_eq!(loaded.line_width, 64);
    assert!(!loaded.auto_punctuation);
    assert!(path.exists());
    assert!(legacy_path.exists());
    assert!(
        fs::read_to_string(&path)
            .expect("the migrated TOML file should be readable")
            .contains("language = \"en\"")
    );

    fs::remove_dir_all(&root).expect("the temporary settings directory should be removed");
}
