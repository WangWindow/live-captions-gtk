//! 主窗口 —— 浮动字幕条

use adw::prelude::*;
use gtk4::{glib, pango};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::pipeline::{PipelineController, PipelineMsg};
use crate::presets::{APP_NAME, SettingsHandle};

const PLACEHOLDER_TEXT: &str = "等待音频输入…";
const LOADING_TEXT: &str = "加载模型中…";
const LISTENING_TEXT: &str = "正在监听…";
const MEASURE_CHAR: char = 'M';
const FALLBACK_WIDTH: i32 = 600;
const FALLBACK_HEIGHT: i32 = 120;

pub struct CaptionWindow {
    pub window: gtk4::ApplicationWindow,
    _caption_a: gtk4::Label,
    _caption_b: gtk4::Label,
    _text_stack: gtk4::Stack,
    _caption_revealer: gtk4::Revealer,
    _listening_label: gtk4::Label,
    _play_btn: gtk4::Button,
    _settings: SettingsHandle,
    _pipeline: Rc<RefCell<Option<PipelineController>>>,
    _settings_win: Rc<RefCell<Option<adw::Window>>>,
}

impl CaptionWindow {
    pub fn build(app: &gtk4::Application, settings: SettingsHandle) -> Self {
        let line_width = {
            let s = settings.read().unwrap_or_else(|e| e.into_inner());
            s.line_width.max(10).min(200)
        };

        // ---- 窗口 ----
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title(APP_NAME)
            .decorated(false)
            .resizable(false)
            .default_width(FALLBACK_WIDTH)
            .default_height(FALLBACK_HEIGHT)
            .build();

        // ---- 左侧工具栏 ----
        let side_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        side_box.set_valign(gtk4::Align::Center);
        side_box.set_margin_start(6);
        side_box.set_margin_end(2);

        let play_btn = gtk4::Button::new();
        play_btn.set_icon_name("media-playback-start-symbolic");
        play_btn.set_has_frame(false);
        play_btn.set_tooltip_text(Some("开始监听"));

        side_box.append(&play_btn);

        let settings_button = gtk4::Button::from_icon_name("emblem-system-symbolic");
        settings_button.set_has_frame(false);
        settings_button.set_tooltip_text(Some("设置"));

        side_box.append(&settings_button);

        // ---- 字幕区域 ----
        // 双标签轮流显示，利用 Stack crossfade 实现文字变化时平滑过渡
        let caption_a = gtk4::Label::new(Some(PLACEHOLDER_TEXT));
        let caption_b = gtk4::Label::new(Some(PLACEHOLDER_TEXT));
        for lbl in [&caption_a, &caption_b] {
            lbl.set_halign(gtk4::Align::Fill);
            lbl.set_valign(gtk4::Align::Center);
            lbl.set_wrap(true);
            lbl.set_wrap_mode(pango::WrapMode::WordChar);
            lbl.set_lines(2);
            lbl.set_xalign(0.0);
            lbl.set_margin_start(16);
            lbl.set_margin_end(16);
            lbl.set_margin_top(10);
            lbl.set_margin_bottom(10);
        }

        let text_stack = gtk4::Stack::new();
        text_stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
        text_stack.set_transition_duration(180);
        text_stack.add_titled(&caption_a, Some("a"), "a");
        text_stack.add_titled(&caption_b, Some("b"), "b");
        text_stack.set_visible_child_name("a");

        // 暗色背景容器
        let caption_bg = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        caption_bg.set_halign(gtk4::Align::Center);
        caption_bg.set_valign(gtk4::Align::Center);
        caption_bg.append(&text_stack);

        // Revealer 用于字幕出现/隐藏
        let revealer = gtk4::Revealer::new();
        revealer.set_child(Some(&caption_bg));
        revealer.set_transition_type(gtk4::RevealerTransitionType::Crossfade);
        revealer.set_transition_duration(200);

        // 监听中提示
        let listening_label = gtk4::Label::new(Some(LISTENING_TEXT));
        listening_label.set_halign(gtk4::Align::Fill);
        listening_label.set_valign(gtk4::Align::Center);
        listening_label.set_xalign(0.0);

        // 堆叠：监听提示 ↔ 字幕
        let stack = gtk4::Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
        stack.set_transition_duration(250);
        stack.add_titled(&listening_label, Some("listening"), "listening");
        stack.add_titled(&revealer, Some("caption"), "caption");
        stack.set_visible_child_name("listening");

        let overlay = gtk4::Overlay::new();
        overlay.set_hexpand(true);
        overlay.set_vexpand(true);
        overlay.set_child(Some(&stack));

        // ---- 关闭按钮 ----
        let close_button = gtk4::Button::from_icon_name("window-close-symbolic");
        close_button.set_has_frame(false);
        close_button.set_valign(gtk4::Align::Start);
        close_button.set_margin_top(6);
        close_button.set_margin_end(6);
        close_button.set_size_request(24, 24);

        // ---- 主布局 ----
        let main_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        main_box.append(&side_box);
        main_box.append(&overlay);
        main_box.append(&close_button);

        let window_handle = gtk4::WindowHandle::new();
        window_handle.set_child(Some(&main_box));
        window.set_child(Some(&window_handle));

        apply_pango_measurement(&caption_a, &window, line_width);

        // ---- 信号连接 ----
        let pipeline: Rc<RefCell<Option<PipelineController>>> = Rc::new(RefCell::new(None));
        let is_listening = Rc::new(RefCell::new(false));
        let active_tab = Rc::new(RefCell::new("a"));

        let pipeline_ref = pipeline.clone();
        let listening_ref = is_listening.clone();
        let active_tab_ref = active_tab.clone();
        let settings_ref = settings.clone();
        let caption_a_ref = caption_a.clone();
        let caption_b_ref = caption_b.clone();
        let text_stack_ref = text_stack.clone();
        let stack_ref = stack.clone();
        let revealer_ref = revealer.clone();
        play_btn.connect_clicked(move |btn| {
            let mut listening = listening_ref.borrow_mut();
            if *listening {
                // ---- 停止 ----
                *listening = false;
                btn.set_icon_name("media-playback-start-symbolic");
                btn.set_tooltip_text(Some("开始监听"));
                if let Some(ctrl) = pipeline_ref.borrow_mut().take() {
                    drop(ctrl);
                }
                caption_a_ref.set_text(PLACEHOLDER_TEXT);
                caption_b_ref.set_text(PLACEHOLDER_TEXT);
                text_stack_ref.set_visible_child_name("a");
                *active_tab_ref.borrow_mut() = "a";
                stack_ref.set_visible_child_name("caption");
                revealer_ref.set_reveal_child(false);
            } else {
                // ---- 启动 ----
                let mic = {
                    let s = settings_ref.read().unwrap_or_else(|e| e.into_inner());
                    s.use_microphone
                };
                btn.set_icon_name("media-playback-stop-symbolic");
                btn.set_tooltip_text(Some(if mic { "停止" } else { "停止" }));
                stack_ref.set_visible_child_name("listening");
                *listening = true;

                let (receiver, ctrl) = PipelineController::start(settings_ref.clone(), mic);
                pipeline_ref.borrow_mut().replace(ctrl);

                let caption_a = caption_a_ref.clone();
                let caption_b = caption_b_ref.clone();
                let text_stack = text_stack_ref.clone();
                let active = active_tab_ref.clone();
                let stack = stack_ref.clone();
                let revealer = revealer_ref.clone();
                let pipeline = pipeline_ref.clone();
                let listening = listening_ref.clone();
                let receiver = RefCell::new(receiver);

                glib::timeout_add_local(Duration::from_millis(80), move || {
                    while let Ok(msg) = receiver.borrow_mut().try_recv() {
                        match msg {
                            PipelineMsg::Loading => {
                                caption_a.set_text(LOADING_TEXT);
                                text_stack.set_visible_child_name("a");
                                *active.borrow_mut() = "a";
                                stack.set_visible_child_name("caption");
                                revealer.set_reveal_child(true);
                            }
                            PipelineMsg::Ready => {
                                stack.set_visible_child_name("listening");
                            }
                            PipelineMsg::Text(text) => {
                                // 当前显示 a → 更新 b 并切到 b（反之亦然）
                                let next = if *active.borrow() == "a" { "b" } else { "a" };
                                let target = if next == "a" { &caption_a } else { &caption_b };
                                target.set_text(&text);
                                text_stack.set_visible_child_name(next);
                                *active.borrow_mut() = next;
                                stack.set_visible_child_name("caption");
                                revealer.set_reveal_child(true);
                            }
                            PipelineMsg::Error(e) => {
                                caption_a.set_text(&e);
                                text_stack.set_visible_child_name("a");
                                *active.borrow_mut() = "a";
                                stack.set_visible_child_name("caption");
                                revealer.set_reveal_child(true);
                                pipeline.borrow_mut().take();
                                *listening.borrow_mut() = false;
                                return glib::ControlFlow::Break;
                            }
                        }
                    }
                    if pipeline.borrow().is_none() {
                        return glib::ControlFlow::Break;
                    }
                    glib::ControlFlow::Continue
                });
            }
        });

        let pipeline_for_settings = pipeline.clone();
        let listening_for_settings = is_listening.clone();
        let play_btn_for_settings = play_btn.clone();
        let caption_a_for_settings = caption_a.clone();
        let caption_b_for_settings = caption_b.clone();
        let text_stack_for_settings = text_stack.clone();
        let listening_label_for_settings = listening_label.clone();
        let stack_for_settings = stack.clone();
        let revealer_for_settings = revealer.clone();
        let sw_settings = settings.clone();
        let sw_window = window.clone();
        let sw_label = caption_a.clone();
        let settings_win = Rc::new(RefCell::new(None::<adw::Window>));
        let sw_settings_win = settings_win.clone();
        settings_button.connect_clicked(move |_| {
            let label = sw_label.clone();
            let win = sw_window.clone();
            let s = sw_settings.clone();

            let pl = pipeline_for_settings.clone();
            let ls = listening_for_settings.clone();
            let pb = play_btn_for_settings.clone();
            let ca = caption_a_for_settings.clone();
            let cb = caption_b_for_settings.clone();
            let ts = text_stack_for_settings.clone();
            let ll = listening_label_for_settings.clone();
            let stk = stack_for_settings.clone();
            let rvl = revealer_for_settings.clone();
            let on_changed: Rc<dyn Fn()> = Rc::new(move || {
                let st = s.read().unwrap_or_else(|e| e.into_inner());
                // 更新字体/尺寸（字幕 + 监听提示都要更新）
                apply_font_to_label(&label, &st.font_name);
                apply_font_to_label(&cb, &st.font_name);
                apply_font_to_label(&ll, &st.font_name);
                apply_pango_measurement(&label, &win, st.line_width.max(10).min(200));

                // 如果正在监听，切设置后停止流水线（下次启动用新配置）
                if *ls.borrow() {
                    if let Some(ctrl) = pl.borrow_mut().take() {
                        drop(ctrl);
                    }
                    *ls.borrow_mut() = false;
                    pb.set_icon_name("media-playback-start-symbolic");
                    pb.set_tooltip_text(Some("开始监听"));
                    ca.set_text(PLACEHOLDER_TEXT);
                    cb.set_text(PLACEHOLDER_TEXT);
                    ts.set_visible_child_name("a");
                    stk.set_visible_child_name("caption");
                    rvl.set_reveal_child(false);
                }
            });

            crate::ui::settings::SettingsWindow::show(
                &mut *sw_settings_win.borrow_mut(),
                sw_window.upcast_ref(),
                sw_settings.clone(),
                on_changed,
            );
        });

        close_button.connect_clicked({
            let w = window.clone();
            move |_| w.close()
        });

        // ---- 初始化 ----
        let slf = Self {
            _caption_a: caption_a,
            _caption_b: caption_b,
            _text_stack: text_stack,
            _caption_revealer: revealer,
            _listening_label: listening_label,
            _play_btn: play_btn,
            window,
            _settings: settings,
            _pipeline: pipeline,
            _settings_win: settings_win,
        };

        {
            let s = slf._settings.read().unwrap_or_else(|e| e.into_inner());
            apply_font_to_label(&slf._caption_a, &s.font_name);
            apply_font_to_label(&slf._caption_b, &s.font_name);
            apply_font_to_label(&slf._listening_label, &s.font_name);
            apply_pango_measurement(&slf._caption_a, &slf.window, s.line_width.max(10).min(200));
        }

        slf
    }
}

fn apply_font_to_label(label: &gtk4::Label, font_name: &str) {
    if font_name.is_empty() {
        label.set_attributes(None::<&pango::AttrList>);
        return;
    }
    let desc = pango::FontDescription::from_string(font_name);
    let attr_list = pango::AttrList::new();
    if let Some(family) = desc.family() {
        let mut attr = pango::AttrString::new_family(&family);
        attr.set_start_index(0);
        attr.set_end_index(u32::MAX);
        attr_list.insert(attr);
    }
    let size = desc.size();
    if size > 0 {
        let mut attr = pango::AttrSize::new_size_absolute(size);
        attr.set_start_index(0);
        attr.set_end_index(u32::MAX);
        attr_list.insert(attr);
    }
    label.set_attributes(Some(&attr_list));
}

fn apply_pango_measurement(
    caption_label: &gtk4::Label,
    window: &gtk4::ApplicationWindow,
    line_width: i32,
) {
    let measure_text: String = std::iter::repeat(MEASURE_CHAR)
        .take(line_width as usize)
        .collect();
    let layout = caption_label.create_pango_layout(Some(&measure_text));
    let (caption_w, line_h) = layout.pixel_size();
    let caption_height = line_h * 2 + 4;
    caption_label.set_size_request(caption_w, caption_height);
    let new_w = caption_w + 80;
    let new_h = caption_height + 20;

    window.set_default_size(new_w, new_h);
}

pub fn build_ui(app: &gtk4::Application, settings: SettingsHandle) -> CaptionWindow {
    let cw = CaptionWindow::build(app, settings);
    cw.window.present();
    cw
}
