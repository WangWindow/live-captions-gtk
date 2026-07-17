//! 应用信息 & 路径常量

use std::path::PathBuf;

pub const APP_ID: &str = "live.captions.gtk";
pub const APP_NAME: &str = "Live Captions";

const SETTINGS_FILENAME: &str = "settings.json";
const APP_CONFIG_DIR: &str = "live.captions.gtk";
const MODELS_SUBDIR: &str = "models";

pub fn config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn models_dir() -> PathBuf {
    config_dir().join(APP_CONFIG_DIR).join(MODELS_SUBDIR)
}

pub fn settings_path() -> PathBuf {
    config_dir().join(APP_CONFIG_DIR).join(SETTINGS_FILENAME)
}

pub fn default_model_path() -> String {
    models_dir()
        .join("sherpa-onnx-streaming-zipformer-zh-int8-2025-06-30")
        .to_string_lossy()
        .into_owned()
}

pub fn default_language() -> String {
    "auto".into()
}
pub fn default_font() -> String {
    "Sans Regular 24".into()
}
pub fn default_use_microphone() -> bool {
    false
}
pub fn default_punct_model_path() -> String {
    String::new()
}
pub fn default_auto_punctuation() -> bool {
    true
}
pub fn default_line_width() -> i32 {
    50
}
