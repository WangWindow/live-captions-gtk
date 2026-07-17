//! Live Captions GTK —— 实时语音转字幕桌面应用
//!
//! 基于 Whisper.cpp 语音识别引擎，支持麦克风和系统音频捕获，
//! 以浮动字幕条的形式在桌面上实时显示识别文本。

mod asr;
mod audio;
mod downloader;
mod pipeline;
mod presets;
mod ui;

use adw::Application;
use adw::prelude::*;
use gtk4::glib;
use std::sync::{Arc, RwLock};

use presets::{APP_ID, Settings};

fn main() -> glib::ExitCode {
    let settings = Settings::load().unwrap_or_else(|e| {
        eprintln!("加载设置失败: {e}");
        Settings::default()
    });
    let settings = Arc::new(RwLock::new(settings));

    let app = Application::builder().application_id(APP_ID).build();
    let s = settings.clone();
    app.connect_activate(move |app| {
        let _ = ui::build_ui(app.upcast_ref(), s.clone());
    });
    app.run()
}
