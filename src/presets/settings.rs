//! 应用设置 —— TOML 持久化配置
//!
//! 新配置使用 TOML 保存。读取时保留一次性 JSON 迁移，以兼容已有安装。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
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
        Self::load_from(&app::settings_path())
    }

    /// 从指定 TOML 路径加载设置，并自动迁移同目录下的旧 JSON 文件。
    pub fn load_from(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path).context("无法读取 TOML 设置文件")?;
            return toml::from_str(&content).context("TOML 设置文件格式错误");
        }

        let legacy_path = path.with_file_name("settings.json");
        if legacy_path.exists() {
            let content =
                std::fs::read_to_string(&legacy_path).context("无法读取旧 JSON 设置文件")?;
            let settings: Self =
                serde_json::from_str(&content).context("旧 JSON 设置文件格式错误")?;
            settings
                .save_to(path)
                .context("无法将旧 JSON 设置迁移为 TOML")?;
            return Ok(settings);
        }

        let settings = Self::default();
        settings.save_to(path)?;
        Ok(settings)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&app::settings_path())
    }

    /// 将设置通过临时文件原子替换到指定路径。
    pub fn save_to(&self, path: &Path) -> Result<()> {
        let dir = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir).context("无法创建设置目录")?;

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("设置文件名无效"))?;
        let temporary_path = dir.join(format!(".{file_name}.{}.tmp", std::process::id()));
        let content = toml::to_string_pretty(self).context("无法序列化 TOML 设置")?;

        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary_path)
            .context("无法创建临时设置文件")?;
        file.write_all(content.as_bytes())
            .context("无法写入临时设置文件")?;
        file.flush().context("无法刷新临时设置文件")?;
        file.sync_all().context("无法同步临时设置文件")?;
        drop(file);

        std::fs::rename(&temporary_path, path)
            .with_context(|| format!("无法用临时设置文件替换 {}", path.to_string_lossy()))?;
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
