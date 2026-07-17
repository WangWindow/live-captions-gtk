//! 应用设置 —— JSON 持久化配置
//!
//! Settings 的加载、保存逻辑。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use super::app;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "app::default_model_path")]
    pub model_path: String,

    #[serde(default = "app::default_language")]
    pub language: String,

    #[serde(default)]
    pub installed_models: Vec<String>,

    #[serde(default = "app::default_font")]
    pub font_name: String,

    #[serde(default = "app::default_line_width")]
    pub line_width: i32,

    #[serde(default = "app::default_use_microphone")]
    pub use_microphone: bool,

    #[serde(default = "app::default_punct_model_path")]
    pub punct_model_path: String,

    #[serde(default = "app::default_auto_punctuation")]
    pub auto_punctuation: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model_path: app::default_model_path(),
            language: app::default_language(),
            installed_models: Vec::new(),
            font_name: app::default_font(),
            line_width: app::default_line_width(),
            use_microphone: app::default_use_microphone(),
            punct_model_path: app::default_punct_model_path(),
            auto_punctuation: app::default_auto_punctuation(),
        }
    }
}

impl Settings {
    pub fn load() -> Result<Self> {
        let path = app::settings_path();
        if !path.exists() {
            let s = Settings::default();
            s.save()?;
            return Ok(s);
        }
        let content = std::fs::read_to_string(&path).context("无法读取设置文件")?;
        Ok(serde_json::from_str(&content).context("设置文件格式错误")?)
    }

    pub fn save(&self) -> Result<()> {
        let dir = app::config_dir().join("live.captions.gtk");
        std::fs::create_dir_all(&dir).context("无法创建设置目录")?;
        let content = serde_json::to_string_pretty(self).context("无法序列化设置")?;
        std::fs::write(&app::settings_path(), content).context("无法写入设置文件")?;
        Ok(())
    }

    pub fn models_dir() -> PathBuf {
        app::models_dir()
    }

    pub fn ensure_models_dir() -> Result<PathBuf> {
        let dir = Self::models_dir();
        std::fs::create_dir_all(&dir).context("无法创建模型目录")?;
        Ok(dir)
    }
}

pub type SettingsHandle = Arc<RwLock<Settings>>;
