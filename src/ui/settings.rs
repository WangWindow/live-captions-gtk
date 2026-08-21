use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use adw::gtk as gtk4;
use adw::gtk::{glib, pango};
use adw::prelude::*;
use adw::{ActionRow, ExpanderRow, PreferencesGroup, PreferencesPage};

use crate::downloader;
use crate::presets::{self, APP_NAME, DownloadMsg, Settings, SettingsHandle};

pub type OnChanged = Rc<dyn Fn()>;

pub struct SettingsWindow;

impl SettingsWindow {
    pub fn show(
        existing: &mut Option<adw::Window>,
        parent: &gtk4::Window,
        settings: SettingsHandle,
        on_changed: OnChanged,
    ) {
        if let Some(win) = existing
            && win.is_visible()
        {
            win.present();
            return;
        }

        let window = adw::Window::builder()
            .title("Live Captions 设置")
            .default_width(550)
            .modal(false)
            .build();

        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        let headerbar = adw::HeaderBar::new();
        let stack = gtk4::Stack::new();
        let switcher = gtk4::StackSwitcher::new();
        switcher.set_stack(Some(&stack));
        headerbar.set_title_widget(Some(&switcher));
        vbox.append(&headerbar);

        let page = PreferencesPage::new();
        stack.add_titled(&page, Some("general"), "通用");

        let audio = PreferencesGroup::builder().title("音频").build();
        audio.add(&build_source_row(&settings, &on_changed));
        audio.add(&build_language_row(&settings, &on_changed));
        page.add(&audio);

        let display = PreferencesGroup::builder().title("显示").build();
        display.add(&build_font_row(&settings, &on_changed));
        display.add(&build_line_width_row(&settings, &on_changed));
        display.add(&build_punctuation_row(&settings, &on_changed));
        page.add(&display);

        let about = PreferencesGroup::builder().title("关于").build();
        let about_row = ActionRow::builder()
            .title("关于 Live Captions")
            .activatable(true)
            .build();
        about_row.add_suffix(&gtk4::Image::from_icon_name("go-next-symbolic"));
        let ap = parent.clone();
        about_row.connect_activated(move |_| show_about_dialog(&ap));
        about.add(&about_row);
        page.add(&about);

        let model_page = PreferencesPage::new();
        stack.add_titled(&model_page, Some("models"), "模型");

        let models = PreferencesGroup::builder().title("本地模型").build();
        let list_scrolled = gtk4::ScrolledWindow::new();
        list_scrolled.set_min_content_height(120);
        list_scrolled.set_vexpand(true);
        let list_box = gtk4::ListBox::new();
        list_box.set_selection_mode(gtk4::SelectionMode::None);
        list_scrolled.set_child(Some(&list_box));
        models.add(&list_scrolled);
        populate_model_list(&list_box, settings.clone(), &window);

        let suffix = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let add_btn = gtk4::Button::builder()
            .icon_name("folder-open-symbolic")
            .tooltip_text("导入模型目录")
            .has_frame(false)
            .build();
        {
            let s = settings.clone();
            let lb = list_box.clone();
            let w = window.clone();
            add_btn.connect_clicked(move |_| open_import_dialog(s.clone(), lb.clone(), &w));
        }
        suffix.append(&add_btn);
        models.set_header_suffix(Some(&suffix));
        model_page.add(&models);

        let dl = PreferencesGroup::builder().build();
        let asr_expander = ExpanderRow::builder().title("识别模型").build();

        // ASR 模型
        for model_info in presets::ASR_MODELS {
            asr_expander.add_row(&build_download_row(
                model_info,
                settings.clone(),
                list_box.clone(),
                window.clone(),
            ));
        }
        dl.add(&asr_expander);

        // 标点模型
        let punct_expander = ExpanderRow::builder().title("标点模型").build();
        for model_info in presets::PUNCT_MODELS {
            punct_expander.add_row(&build_download_row(
                model_info,
                settings.clone(),
                list_box.clone(),
                window.clone(),
            ));
        }
        dl.add(&punct_expander);
        model_page.add(&dl);

        vbox.append(&stack);
        window.set_content(Some(&vbox));
        *existing = Some(window.clone());
        window.present();
    }
}

fn build_download_row(
    model_info: &'static presets::ModelInfo,
    settings: SettingsHandle,
    list_box: gtk4::ListBox,
    window: adw::Window,
) -> ActionRow {
    let row = ActionRow::builder()
        .title(model_info.name)
        .subtitle(model_info.description)
        .build();
    let button = gtk4::Button::from_icon_name("folder-download-symbolic");
    button.set_valign(gtk4::Align::Center);
    button.set_has_frame(false);
    button.set_tooltip_text(Some("下载模型"));
    let download_row = row.clone();
    let download_button = button.clone();
    button.connect_clicked(move |_| {
        start_download(
            model_info,
            &download_row,
            &download_button,
            settings.clone(),
            list_box.clone(),
            &window,
        );
    });
    row.add_suffix(&button);
    row
}

fn start_download(
    model_info: &'static presets::ModelInfo,
    row: &ActionRow,
    button: &gtk4::Button,
    settings: SettingsHandle,
    list_box: gtk4::ListBox,
    window: &adw::Window,
) {
    row.set_subtitle(&format!("正在下载 {}…", model_info.name));
    button.set_icon_name("process-stop-symbolic");
    button.set_tooltip_text(Some("正在下载"));
    button.set_sensitive(false);

    let dir = match Settings::ensure_models_dir() {
        Ok(d) => d,
        Err(e) => {
            row.set_subtitle(&format!("错误: {e}"));
            button.set_icon_name("view-refresh-symbolic");
            button.set_tooltip_text(Some("重试下载"));
            button.set_sensitive(true);
            return;
        }
    };
    let model_dir = dir.join(model_info.dir_name);

    // 检查是否已完整下载
    let all_exist = model_info.is_complete(&model_dir);
    if all_exist {
        row.set_subtitle("模型已存在 ✓");
        button.set_icon_name("checkbox-checked-symbolic");
        button.set_tooltip_text(Some("模型已安装"));
        button.set_sensitive(true);
        download_done(
            model_dir.to_string_lossy().into_owned(),
            &settings,
            &list_box,
            window,
            model_info.category,
        );
        return;
    }

    let (tx, rx) = mpsc::channel::<DownloadMsg>();
    let d = dir.clone();
    std::thread::spawn(move || downloader::download_model(model_info, &d, &tx));

    let row = row.clone();
    let button = button.clone();
    let s = settings.clone();
    let lb = list_box.clone();
    let w = window.clone();
    let name = model_info.name.to_string();
    let cat = model_info.category;
    glib::timeout_add_local(Duration::from_millis(200), move || match rx.try_recv() {
        Ok(DownloadMsg::Progress { downloaded, total }) => {
            let text = if total > 0 {
                format!(
                    "{} ({:.0}%)",
                    name,
                    downloaded as f64 / total as f64 * 100.0
                )
            } else {
                format!("{} ({} MB)", name, downloaded / 1_048_576)
            };
            row.set_subtitle(&text);
            glib::ControlFlow::Continue
        }
        Ok(DownloadMsg::Done(path)) => {
            row.set_subtitle("下载完成 ✓");
            button.set_icon_name("checkbox-checked-symbolic");
            button.set_tooltip_text(Some("模型已安装"));
            button.set_sensitive(true);
            download_done(path, &s, &lb, &w, cat);
            glib::ControlFlow::Break
        }
        Ok(DownloadMsg::Error(e)) => {
            row.set_subtitle(&format!("失败: {e}"));
            button.set_icon_name("view-refresh-symbolic");
            button.set_tooltip_text(Some("重试下载"));
            button.set_sensitive(true);
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => {
            row.set_subtitle("下载中断");
            button.set_icon_name("view-refresh-symbolic");
            button.set_tooltip_text(Some("重试下载"));
            button.set_sensitive(true);
            glib::ControlFlow::Break
        }
    });
}

fn download_done(
    path: String,
    settings: &SettingsHandle,
    list_box: &gtk4::ListBox,
    window: &adw::Window,
    category: presets::ModelCategory,
) {
    match category {
        presets::ModelCategory::Punctuation => {
            let mut guard = settings.write().unwrap();
            guard.punct_model_path = path.clone();
            if !guard.installed_models.contains(&path) {
                guard.installed_models.push(path.clone());
            }
            let _ = guard.save();
        }
        _ => {
            let mut guard = settings.write().unwrap();
            if !guard.installed_models.contains(&path) {
                guard.installed_models.push(path.clone());
            }
            guard.model_path = path;
            let _ = guard.save();
        }
    }
    populate_model_list(list_box, settings.clone(), window);
}

fn build_source_row(settings: &SettingsHandle, on_changed: &OnChanged) -> ActionRow {
    let row = ActionRow::builder().title("音频源").build();

    let current = settings.read().unwrap();
    let model = gtk4::StringList::new(&["系统音频", "麦克风"]);
    let idx = if current.use_microphone { 1 } else { 0 };
    let dd = gtk4::DropDown::new(Some(model), None::<gtk4::Expression>);
    dd.set_selected(idx);
    dd.set_valign(gtk4::Align::Center);
    drop(current);

    let row_suffix = row.clone();
    let s = settings.clone();
    let cb = on_changed.clone();
    dd.connect_selected_notify(move |dd| {
        let mut s = s.write().unwrap();
        s.use_microphone = dd.selected() == 1;
        let _ = s.save();
        drop(s);
        cb();
    });

    row_suffix.add_suffix(&dd);
    row.set_activatable_widget(Some(&dd));
    row
}

fn build_language_row(settings: &SettingsHandle, on_changed: &OnChanged) -> ActionRow {
    let row = ActionRow::builder().title("识别语言").build();

    let current = settings.read().unwrap();
    let model = gtk4::StringList::new(&["自动检测", "简体中文", "English"]);
    let values = ["auto", "zh", "en"];
    let idx = values
        .iter()
        .position(|&v| v == current.language)
        .unwrap_or(0);
    let dd = gtk4::DropDown::new(Some(model), None::<gtk4::Expression>);
    dd.set_selected(idx as u32);
    dd.set_valign(gtk4::Align::Center);
    drop(current);

    let s = settings.clone();
    let cb = on_changed.clone();
    dd.connect_selected_notify(move |dd| {
        let mut s = s.write().unwrap();
        s.language = values[dd.selected() as usize].into();
        let _ = s.save();
        drop(s);
        cb();
    });

    row.add_suffix(&dd);
    row.set_activatable_widget(Some(&dd));
    row
}

fn build_font_row(settings: &SettingsHandle, on_changed: &OnChanged) -> ActionRow {
    let row = ActionRow::builder().title("字体").build();

    let current = settings.read().unwrap();
    let btn = gtk4::FontButton::builder()
        .valign(gtk4::Align::Center)
        .use_font(true)
        .build();
    btn.set_font_desc(&pango::FontDescription::from_string(&current.font_name));
    drop(current);

    let s = settings.clone();
    let cb = on_changed.clone();
    btn.connect_font_set(move |btn| {
        let mut s = s.write().unwrap();
        s.font_name = btn.font_desc().map(|d| d.to_string()).unwrap_or_default();
        let _ = s.save();
        drop(s);
        cb();
    });

    row.add_suffix(&btn);
    row.set_activatable_widget(Some(&btn));
    row
}

fn build_line_width_row(settings: &SettingsHandle, on_changed: &OnChanged) -> ActionRow {
    let row = ActionRow::builder().title("窗口宽度").build();

    let current = settings.read().unwrap();
    let adj = gtk4::Adjustment::new(current.line_width as f64, 20.0, 140.0, 5.0, 10.0, 0.0);
    drop(current);

    let scale = gtk4::Scale::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .adjustment(&adj)
        .draw_value(false)
        .width_request(200)
        .valign(gtk4::Align::Center)
        .build();

    let s = settings.clone();
    let cb = on_changed.clone();
    scale.connect_value_changed(move |sc| {
        let mut s = s.write().unwrap();
        s.line_width = sc.value() as i32;
        let _ = s.save();
        drop(s);
        cb();
    });

    row.add_suffix(&scale);
    row
}

fn build_punctuation_row(settings: &SettingsHandle, on_changed: &OnChanged) -> ActionRow {
    let row = ActionRow::builder().title("自动标点").build();

    let current = settings.read().unwrap();
    let sw = gtk4::Switch::builder()
        .valign(gtk4::Align::Center)
        .active(current.auto_punctuation)
        .build();
    drop(current);

    let s = settings.clone();
    let cb = on_changed.clone();
    sw.connect_active_notify(move |sw| {
        let mut s = s.write().unwrap();
        s.auto_punctuation = sw.is_active();
        let _ = s.save();
        drop(s);
        cb();
    });

    row.add_suffix(&sw);
    row.set_activatable_widget(Some(&sw));
    row
}

fn show_about_dialog(parent: &gtk4::Window) {
    adw::AboutDialog::builder()
        .application_name(APP_NAME)
        .version(env!("CARGO_PKG_VERSION"))
        .developer_name("Live Captions GTK contributors")
        .license_type(gtk4::License::Gpl30)
        .website("https://github.com/WangWindow/live-captions-gtk")
        .build()
        .present(Some(parent));
}

fn populate_model_list(list_box: &gtk4::ListBox, settings: SettingsHandle, win: &adw::Window) {
    while let Some(row) = list_box.last_child() {
        list_box.remove(&row);
    }

    let s = settings.read().unwrap();
    let current_asr = s.model_path.clone();
    let current_punct = s.punct_model_path.clone();
    let installed = model_paths(&s);
    drop(s);

    for path in &installed {
        let p = std::path::Path::new(path);
        let path_str = path.clone();
        let model = presets::find_model_by_dir(p);
        let name = model
            .map(|model| model.name.to_string())
            .or_else(|| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| path_str.clone());

        let row_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row_box.set_margin_start(12);
        row_box.set_margin_end(12);
        row_box.set_margin_top(6);
        row_box.set_margin_bottom(6);

        if let Some(info) = model {
            let (icon_name, icon_tip) = match info.category {
                presets::ModelCategory::Asr => ("audio-input-microphone-symbolic", "语音识别模型"),
                presets::ModelCategory::Punctuation => ("document-edit-symbolic", "标点恢复模型"),
            };
            let marker = gtk4::Image::from_icon_name(icon_name);
            marker.set_pixel_size(20);
            marker.set_tooltip_text(Some(icon_tip));
            row_box.append(&marker);
        } else {
            let marker = gtk4::Image::from_icon_name("dialog-warning-symbolic");
            marker.set_pixel_size(20);
            marker.set_tooltip_text(Some("未知或不完整的模型"));
            row_box.append(&marker);
        }

        let info = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        info.set_valign(gtk4::Align::Center);
        info.set_hexpand(true);
        let nl = gtk4::Label::new(Some(&name));
        nl.set_halign(gtk4::Align::Start);
        nl.set_xalign(0.0);
        nl.set_tooltip_text(Some(&path_str));

        let subtitle = if let Some(model) = model {
            match model.category {
                presets::ModelCategory::Asr if current_asr == path_str => {
                    format!("{} · 当前使用", model.description)
                }
                presets::ModelCategory::Punctuation if current_punct == path_str => {
                    format!("{} · 当前使用", model.description)
                }
                _ => model.description.to_string(),
            }
        } else {
            "目录不完整或不是内置模型，无法用于当前引擎".to_string()
        };
        let sl = gtk4::Label::new(Some(&subtitle));
        sl.set_halign(gtk4::Align::Start);
        sl.set_xalign(0.0);
        sl.add_css_class("dim-label");
        info.append(&nl);
        info.append(&sl);
        row_box.append(&info);

        if let Some(model) = model
            && model.category == presets::ModelCategory::Asr
        {
            let cb = gtk4::CheckButton::new();
            cb.set_active(path_str == current_asr);
            cb.set_valign(gtk4::Align::Center);
            let s = settings.clone();
            let lb = list_box.clone();
            let ps = path_str.clone();
            let w = win.clone();
            cb.connect_toggled(move |this| {
                if !this.is_active() {
                    return;
                }
                let mut guard = s.write().unwrap();
                guard.model_path = ps.clone();
                let _ = guard.save();
                drop(guard);
                populate_model_list(&lb, s.clone(), &w);
            });
            row_box.prepend(&cb);
        }

        let del = gtk4::Button::from_icon_name("user-trash-symbolic");
        del.set_has_frame(false);
        del.set_valign(gtk4::Align::Center);
        del.set_tooltip_text(Some("删除此模型"));
        let is_active = model.is_some_and(|model| match model.category {
            presets::ModelCategory::Asr => current_asr == path_str,
            presets::ModelCategory::Punctuation => current_punct == path_str,
        });
        if is_active {
            del.set_sensitive(false);
            del.set_tooltip_text(Some("当前使用中的模型，请先切换后再删除"));
        }
        row_box.append(&del);

        {
            let s = settings.clone();
            let lb = list_box.clone();
            let ps = path_str.clone();
            let n = name.clone();
            let w = win.clone();
            del.connect_clicked(move |_| show_delete_confirm(&n, &ps, &s, &lb, &w));
        }

        list_box.append(&row_box);
    }
}

fn model_paths(settings: &Settings) -> Vec<String> {
    let mut paths = settings.installed_models.clone();
    if !settings.punct_model_path.is_empty() && !paths.contains(&settings.punct_model_path) {
        paths.push(settings.punct_model_path.clone());
    }

    let models_dir = Settings::models_dir();
    for model in presets::all_models() {
        let path = models_dir.join(model.dir_name);
        if model.is_complete(&path) {
            let path = path.to_string_lossy().into_owned();
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

fn show_delete_confirm(
    name: &str,
    path: &str,
    settings: &SettingsHandle,
    list_box: &gtk4::ListBox,
    parent: &adw::Window,
) {
    let dialog = adw::AlertDialog::builder()
        .heading("删除模型")
        .body(format!("确定要删除 \"{name}\" 吗？\n\n{path}"))
        .close_response("cancel")
        .build();
    dialog.add_response("cancel", "取消");
    dialog.add_response("delete", "删除");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let s = settings.clone();
    let lb = list_box.clone();
    let ps = path.to_string();
    let pw = parent.clone();
    dialog.connect_response(None, move |_, resp| {
        if resp == "delete" {
            let mut guard = s.write().unwrap();
            guard.installed_models.retain(|m| m != &ps);
            if guard.model_path == ps {
                guard.model_path.clear();
            }
            if guard.punct_model_path == ps {
                guard.punct_model_path.clear();
            }
            let _ = guard.save();
            drop(guard);
            let _ = std::fs::remove_dir_all(&ps);
            populate_model_list(&lb, s.clone(), &pw);
        }
    });

    dialog.present(Some(parent));
}

fn open_import_dialog(settings: SettingsHandle, list_box: gtk4::ListBox, parent: &adw::Window) {
    let dialog = gtk4::FileChooserNative::builder()
        .title("选择模型目录")
        .action(gtk4::FileChooserAction::SelectFolder)
        .build();

    let s = settings;
    let lb = list_box;
    let pw = parent.clone();
    dialog.connect_response(move |d, resp| {
        if resp == gtk4::ResponseType::Accept
            && let Some(path) = d.file().and_then(|f| f.path())
        {
                let ps = path.to_string_lossy().into_owned();
                let Some(model) = presets::find_model_by_dir(&path) else {
                    let err_dialog = adw::AlertDialog::builder()
                        .heading("无效的模型目录")
                        .body("请选择包含完整内置模型文件的目录。当前仅支持模型页列出的 sherpa-onnx 模型。")
                        .build();
                    err_dialog.add_response("ok", "确定");
                    err_dialog.present(Some(&pw));
                    return;
                };
                let mut guard = s.write().unwrap();
                if !guard.installed_models.contains(&ps) {
                    guard.installed_models.push(ps.clone());
                }
                match model.category {
                    presets::ModelCategory::Asr => guard.model_path = ps,
                    presets::ModelCategory::Punctuation => guard.punct_model_path = ps,
                }
                let _ = guard.save();
                drop(guard);
                populate_model_list(&lb, s.clone(), &pw);
        }
    });
    dialog.show();
}
