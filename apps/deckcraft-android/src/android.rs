//! The Android shell: logging, GPU setup, Android `Services`, and the eframe runner.

use std::path::{Path, PathBuf};

use deckcraft_engine::Session;
use deckcraft_ui_egui::{Inbox, Services, SlideApp};
use winit::platform::android::activity::AndroidApp;

use crate::browser::{Browser, Outcome, Request, Requests};
use crate::{audio, jni_bridge};

/// The primary shared storage volume (what Files calls "Internal storage").
const STORAGE_ROOT: &str = "/storage/emulated/0";

/// Start DeckCraft in the activity `app` and run until the activity is destroyed.
pub fn run(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("DeckCraft"));
    install_panic_logger();
    log::info!("DeckCraft {} (android {})", env!("CARGO_PKG_VERSION"), std::env::consts::ARCH);

    let data_dir = app.internal_data_path();
    let mut options = eframe::NativeOptions { android_app: Some(app.clone()), ..Default::default() };
    configure_gpu(&mut options);

    let result = eframe::run_native(
        "DeckCraft",
        options,
        Box::new(move |cc| {
            if let Some(rs) = &cc.wgpu_render_state {
                let info = rs.adapter.get_info();
                log::info!("GPU: {} ({:?}, driver {} {})", info.name, info.backend, info.driver, info.driver_info);
            }
            deckcraft_ui_egui::install_ime_pass_hooks(&cc.egui_ctx);
            let requests = Requests::default();
            let inbox = Inbox::default();
            let mut slides = SlideApp::new(Session::new(), services(requests.clone(), inbox.clone(), cc.egui_ctx.clone()));
            load_prefs(&mut slides, data_dir.as_deref());
            // AutoRecover: reopen what a previous run left unsaved (Android may kill a backgrounded
            // app at any time), then keep it current.
            slides.session.recovery_dir = data_dir.as_ref().map(|d| d.join("recovery"));
            if let Ok(v) = slides.session.execute("file.recovery.open", &serde_json::json!({}))
                && let Some(n) = v.get("opened").and_then(|o| o.as_array()).map(Vec::len).filter(|n| *n > 0)
            {
                slides.set_status(format!("Recovered {n} unsaved presentation{} from the last session", if n == 1 { "" } else { "s" }));
            }
            slides.integrated_titlebar = false;
            Ok(Box::new(Shell {
                slides,
                browser: Browser::new(PathBuf::from(STORAGE_ROOT), requests, inbox),
                activity: app,
                data_dir,
                focused: true,
            }))
        }),
    );
    if let Err(e) = result {
        log::error!("DeckCraft stopped: {e}");
    }
}

/// Render on the GPU through wgpu: Vulkan first, OpenGL ES as the fallback for devices whose
/// Vulkan driver is missing, and the high-performance adapter where there is a choice.
fn configure_gpu(options: &mut eframe::NativeOptions) {
    options.renderer = eframe::Renderer::Wgpu;
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        create.instance_descriptor.backends = eframe::wgpu::Backends::VULKAN | eframe::wgpu::Backends::GL;
        create.power_preference = eframe::wgpu::PowerPreference::HighPerformance;
    }
}

/// Panics go to logcat (Android drops standard error), then to the previous hook.
fn install_panic_logger() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}\n{}", std::backtrace::Backtrace::force_capture());
        previous(info);
    }));
}

fn load_prefs(app: &mut SlideApp, dir: Option<&Path>) {
    let Some(dir) = dir else { return };
    let (ui, prefs) = deckcraft_ui_egui::settings::load(dir);
    if let Some(ui) = ui {
        app.ui = ui;
    }
    if let Some(prefs) = prefs {
        app.session.prefs = prefs;
    }
}

fn save_prefs(app: &SlideApp, dir: Option<&Path>) {
    if let Some(dir) = dir {
        deckcraft_ui_egui::settings::save(dir, &app.ui, &app.session.prefs);
    }
}

/// Android services: no blocking dialogs, so `pick_open`/`pick_save` stay `None` and the UI uses
/// `open_async` and `download`, both answered by the in-app [`Browser`].
fn services(requests: Requests, inbox: Inbox, ctx: egui::Context) -> Services {
    let (open_q, open_ctx) = (requests.clone(), ctx.clone());
    let (save_q, save_ctx) = (requests.clone(), ctx.clone());
    let (write_q, write_ctx) = (requests, ctx);
    Services {
        open_async: Some(Box::new(move |purpose: &str, id: u64| {
            open_q.push(Request::Open { purpose: purpose.to_string(), id });
            open_ctx.request_repaint();
        })),
        download: Some(Box::new(move |name: &str, bytes: &[u8]| {
            save_q.push(Request::Save { name: name.to_string(), bytes: bytes.to_vec() });
            save_ctx.request_repaint();
        })),
        // A full path is written as is; a bare name (what the web-style save path hands over)
        // asks where to put it.
        write: Some(Box::new(move |path: &str, bytes: &[u8]| {
            if Path::new(path).is_absolute() {
                std::fs::write(path, bytes).map_err(|e| e.to_string())
            } else {
                write_q.push(Request::Save { name: path.to_string(), bytes: bytes.to_vec() });
                write_ctx.request_repaint();
                Ok(())
            }
        })),
        inbox: Some(inbox),
        audio_out: Some(Box::new(audio::CpalOut::default())),
        ..Default::default()
    }
}

struct Shell {
    slides: SlideApp,
    browser: Browser,
    activity: AndroidApp,
    data_dir: Option<PathBuf>,
    /// Whether the window had focus last frame; preferences are saved when it is lost, since
    /// Android may end a backgrounded app without `on_exit`.
    focused: bool,
}

impl eframe::App for Shell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.slides.logic(ctx);
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if self.focused && !focused {
            save_prefs(&self.slides, self.data_dir.as_deref());
        }
        self.focused = focused;
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.slides.raw_input_hook(ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.slides.ui(ui);
        let activity = self.activity.clone();
        let mut grant = move || jni_bridge::request_storage_access(&activity);
        if let Some(outcome) = self.browser.show(ui.ctx(), &mut grant) {
            match outcome {
                Outcome::Saved(p) => self.slides.set_status(format!("Saved {}", p.display())),
                Outcome::Failed(e) => {
                    log::warn!("{e}");
                    self.slides.set_status(e);
                }
                Outcome::Opened(_) | Outcome::Cancelled => {}
            }
            ui.ctx().request_repaint();
        }
    }

    fn on_exit(&mut self) {
        save_prefs(&self.slides, self.data_dir.as_deref());
    }
}
