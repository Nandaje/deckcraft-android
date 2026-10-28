//! A touch-friendly in-app file browser for Android.
//!
//! Android has no synchronous file dialog that a native thread can block on, so the Android shell
//! uses the same asynchronous path as the browser build: `Services::open_async` queues a
//! [`Request::Open`] and the chosen file reaches the app through `Services::inbox`;
//! `Services::download` (Save a Copy, Export, Print) queues a [`Request::Save`] that writes the
//! bytes where the user points. The browser reads and writes shared storage directly, which is
//! what the "All files access" permission (`MANAGE_EXTERNAL_STORAGE`, requested by the Java
//! activity) is for.
//!
//! Nothing here is Android-specific: it is plain `std::fs` plus egui, so it is unit-tested on the
//! host.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use deckcraft_ui_egui::{Inbox, InboxEvent, InboxFile};

pub const PRESENTATION_EXTS: &[&str] = &["deckcraft", "pptx", "potx", "ppsx", "txt", "md"];
pub const PICTURE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];
pub const AUDIO_EXTS: &[&str] = &["wav", "mp3", "m4a", "m4b", "aac", "flac", "ogg", "oga", "opus", "aif", "aiff", "aifc", "caf", "mka"];
pub const VIDEO_EXTS: &[&str] = &["mp4", "m4v", "mov", "webm", "mkv"];
pub const THEME_EXTS: &[&str] = &["thmx", "pptx", "potx", "ppsx"];

/// Touch target height for list rows and buttons, in points.
const ROW_HEIGHT: f32 = 44.0;

/// The file types offered for a picker purpose (`open`, `picture`, `audio`, `video`, `theme`).
pub fn extensions(purpose: &str) -> &'static [&'static str] {
    match purpose {
        "picture" => PICTURE_EXTS,
        "audio" => AUDIO_EXTS,
        "video" => VIDEO_EXTS,
        "theme" => THEME_EXTS,
        _ => PRESENTATION_EXTS,
    }
}

fn title(purpose: &str) -> &'static str {
    match purpose {
        "picture" => "Insert a picture",
        "audio" => "Insert audio",
        "video" => "Insert a video",
        "theme" => "Browse for themes",
        "drop" => "Open a file",
        _ => "Open a presentation",
    }
}

/// What the app asked the browser to do.
#[derive(Debug)]
pub enum Request {
    /// Pick a file for `purpose`; deliver it to the inbox under the request `id`.
    Open { purpose: String, id: u64 },
    /// Choose where to write `bytes`, suggesting `name`.
    Save { name: String, bytes: Vec<u8> },
}

/// Requests queued by the `Services` closures (which can't borrow the shell) for the next frame.
#[derive(Clone, Default)]
pub struct Requests(Arc<Mutex<VecDeque<Request>>>);

impl Requests {
    pub fn push(&self, r: Request) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push_back(r);
    }

    fn pop(&self) -> Option<Request> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).pop_front()
    }
}

/// How a request ended, for the status bar.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Opened(String),
    Saved(PathBuf),
    Failed(String),
    Cancelled,
}

/// One row of a folder listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

/// Whether `name` ends in one of `exts` (case-insensitive, without the dot).
pub fn has_extension(name: &str, exts: &[&str]) -> bool {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => exts.iter().any(|e| e.eq_ignore_ascii_case(ext)),
        _ => false,
    }
}

/// The folders and (when `exts` is given) matching files in `dir`: folders first, then files,
/// each sorted by name without regard to case. Hidden entries (a leading dot) are left out.
pub fn list_dir(dir: &Path, exts: Option<&[&str]>) -> std::io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        // `metadata` follows symlinks, so a linked folder lists as a folder.
        let Ok(meta) = std::fs::metadata(e.path()) else { continue };
        let is_dir = meta.is_dir();
        if !is_dir && exts.is_some_and(|x| !has_extension(&name, x)) {
            continue;
        }
        out.push(Entry { name, path: e.path(), is_dir, size: if is_dir { 0 } else { meta.len() } });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

/// `name` made safe as a single file name on Android's shared storage (FAT-style rules apply on
/// SD cards): path separators, reserved punctuation and control characters become `_`.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    if cleaned.is_empty() { "Presentation".to_string() } else { cleaned }
}

/// Quick-access folders under the shared storage `root`, plus removable volumes under `/storage`.
pub fn places(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = vec![("Internal storage".to_string(), root.to_path_buf())];
    for d in ["Documents", "Download", "Pictures", "DCIM", "Movies", "Music"] {
        let p = root.join(d);
        if p.is_dir() {
            out.push((d.to_string(), p));
        }
    }
    // SD cards and USB drives mount as /storage/XXXX-XXXX next to `emulated` and `self`.
    if let Ok(volumes) = std::fs::read_dir("/storage") {
        for v in volumes.flatten() {
            let name = v.file_name().to_string_lossy().into_owned();
            if name != "emulated" && name != "self" && v.path().is_dir() {
                out.push((format!("SD card {name}"), v.path()));
            }
        }
    }
    out
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    format!("{v:.1} {}", UNITS[unit])
}

struct Active {
    request: Request,
    dir: PathBuf,
    entries: Result<Vec<Entry>, String>,
    /// File name being typed (save).
    name: String,
    /// Offer every file, not only the purpose's types (open).
    all_files: bool,
    /// An existing file the user asked to replace; saving again confirms.
    overwrite: Option<PathBuf>,
}

impl Active {
    fn exts(&self) -> Option<&'static [&'static str]> {
        match &self.request {
            Request::Open { purpose, .. } if !self.all_files => Some(extensions(purpose)),
            Request::Open { .. } => None,
            // Saving shows the folder's files so name clashes are visible.
            Request::Save { .. } => None,
        }
    }

    fn refresh(&mut self) {
        self.entries = list_dir(&self.dir, self.exts()).map_err(|e| e.to_string());
    }

    fn go(&mut self, dir: PathBuf) {
        self.dir = dir;
        self.overwrite = None;
        self.refresh();
    }
}

/// The browser: takes one [`Request`] at a time from the queue and draws it as a modal.
pub struct Browser {
    root: PathBuf,
    requests: Requests,
    inbox: Inbox,
    active: Option<Active>,
    last_open_dir: Option<PathBuf>,
    last_save_dir: Option<PathBuf>,
}

/// What a frame of the modal asked for (applied after drawing, so the borrow of `Active` ends).
enum Action {
    None,
    Go(PathBuf),
    Pick(PathBuf),
    Save(PathBuf),
    ToggleAll,
    Cancel,
    Grant,
}

impl Browser {
    /// `root` is the shared storage root (`/storage/emulated/0`).
    pub fn new(root: PathBuf, requests: Requests, inbox: Inbox) -> Self {
        Self { root, requests, inbox, active: None, last_open_dir: None, last_save_dir: None }
    }

    pub fn is_open(&self) -> bool {
        self.active.is_some()
    }

    /// Take the next queued request when idle.
    fn start_next(&mut self) {
        if self.active.is_some() {
            return;
        }
        let Some(request) = self.requests.pop() else { return };
        let (dir, name) = match &request {
            Request::Open { .. } => (self.last_open_dir.clone().unwrap_or_else(|| self.root.clone()), String::new()),
            Request::Save { name, .. } => {
                let fallback = self.root.join("Documents");
                let dir = self.last_save_dir.clone().unwrap_or(if fallback.is_dir() { fallback } else { self.root.clone() });
                let base = Path::new(name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                (dir, sanitize_file_name(&base))
            }
        };
        let mut a = Active { request, dir, entries: Ok(Vec::new()), name, all_files: false, overwrite: None };
        a.refresh();
        self.active = Some(a);
    }

    /// Draw the browser when a request is pending. `grant` asks Android for file access again.
    /// Returns how the request ended, once it has.
    pub fn show(&mut self, ctx: &egui::Context, grant: &mut dyn FnMut()) -> Option<Outcome> {
        self.start_next();
        let root = self.root.clone();
        let a = self.active.as_mut()?;
        let mut action = Action::None;
        let screen = ctx.content_rect();
        let width = (screen.width() * 0.94).min(720.0);
        let list_height = (screen.height() * 0.5).max(160.0);
        let modal = egui::Modal::new(egui::Id::unique("deckcraft-android-file-browser")).show(ctx, |ui| {
            ui.set_width(width);
            draw(ui, a, &root, list_height, &mut action);
        });
        if modal.should_close() && matches!(action, Action::None) {
            action = Action::Cancel;
        }
        self.apply(action, grant)
    }

    fn apply(&mut self, action: Action, grant: &mut dyn FnMut()) -> Option<Outcome> {
        let a = self.active.as_mut()?;
        match action {
            Action::None => None,
            Action::Go(dir) => {
                a.go(dir);
                None
            }
            Action::ToggleAll => {
                a.all_files = !a.all_files;
                a.refresh();
                None
            }
            Action::Grant => {
                grant();
                None
            }
            Action::Cancel => {
                let a = self.active.take()?;
                if let Request::Open { id, .. } = a.request {
                    self.push(InboxEvent::Cancelled { id });
                }
                Some(Outcome::Cancelled)
            }
            Action::Pick(path) => {
                let a = self.active.take()?;
                let Request::Open { id, .. } = a.request else { return None };
                self.last_open_dir = Some(a.dir);
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                match std::fs::read(&path) {
                    Ok(bytes) => {
                        self.push(InboxEvent::File(InboxFile { id, name: name.clone(), bytes }));
                        Some(Outcome::Opened(name))
                    }
                    Err(e) => {
                        self.push(InboxEvent::Cancelled { id });
                        Some(Outcome::Failed(format!("Couldn't read {name}: {e}")))
                    }
                }
            }
            Action::Save(path) => {
                if path.exists() && a.overwrite.as_deref() != Some(path.as_path()) {
                    a.overwrite = Some(path);
                    return None;
                }
                let a = self.active.take()?;
                let Request::Save { bytes, .. } = a.request else { return None };
                self.last_save_dir = Some(a.dir);
                match std::fs::write(&path, &bytes) {
                    Ok(()) => Some(Outcome::Saved(path)),
                    Err(e) => Some(Outcome::Failed(format!("Couldn't save {}: {e}", path.display()))),
                }
            }
        }
    }

    fn push(&self, event: InboxEvent) {
        self.inbox.lock().unwrap_or_else(PoisonError::into_inner).push(event);
    }
}

/// The folder shown relative to the storage root, for the header.
fn display_dir(dir: &Path, root: &Path) -> String {
    match dir.strip_prefix(root) {
        Ok(rel) if rel.as_os_str().is_empty() => "Internal storage".to_string(),
        Ok(rel) => format!("Internal storage / {}", rel.to_string_lossy().replace('/', " / ")),
        Err(_) => dir.display().to_string(),
    }
}

fn draw(ui: &mut egui::Ui, a: &mut Active, root: &Path, list_height: f32, action: &mut Action) {
    let saving = matches!(a.request, Request::Save { .. });
    let heading = match &a.request {
        Request::Open { purpose, .. } => title(purpose),
        Request::Save { .. } => "Save a copy",
    };
    ui.heading(heading);
    ui.add_space(4.0);

    // Places: quick jumps, wrapped for narrow phones.
    ui.horizontal_wrapped(|ui| {
        for (label, path) in places(root) {
            if ui.add(egui::Button::new(label).selected(a.dir == path).min_size(egui::vec2(0.0, ROW_HEIGHT * 0.8))).clicked() {
                *action = Action::Go(path);
            }
        }
    });
    ui.separator();

    ui.horizontal(|ui| {
        let up = a.dir.parent().map(Path::to_path_buf);
        if ui.add_enabled(up.is_some(), egui::Button::new("⬆ Up").min_size(egui::vec2(64.0, ROW_HEIGHT * 0.8))).clicked()
            && let Some(up) = up
        {
            *action = Action::Go(up);
        }
        ui.add(egui::Label::new(egui::RichText::new(display_dir(&a.dir, root)).strong()).truncate());
    });

    egui::ScrollArea::vertical().max_height(list_height).auto_shrink([false, false]).show(ui, |ui| match &a.entries {
        Err(e) => {
            ui.add_space(8.0);
            ui.colored_label(ui.visuals().error_fg_color, format!("This folder can't be read: {e}"));
            ui.label(
                "DeckCraft needs file access to open and save presentations. Allow it in \
                 Settings › Apps › DeckCraft › Permissions › Files (\"Allow management of all files\").",
            );
            if ui.add(egui::Button::new("Grant file access").min_size(egui::vec2(0.0, ROW_HEIGHT))).clicked() {
                *action = Action::Grant;
            }
        }
        Ok(entries) if entries.is_empty() => {
            ui.add_space(8.0);
            ui.weak(if saving { "This folder is empty." } else { "No matching files in this folder." });
        }
        Ok(entries) => {
            let w = ui.available_width();
            for e in entries {
                let label = if e.is_dir { format!("📁  {}", e.name) } else { format!("📄  {}    ({})", e.name, human_size(e.size)) };
                let button = egui::Button::new(label).frame(false).min_size(egui::vec2(w, ROW_HEIGHT));
                if ui.add(button).clicked() {
                    *action = match (e.is_dir, saving) {
                        (true, _) => Action::Go(e.path.clone()),
                        (false, false) => Action::Pick(e.path.clone()),
                        // Tapping an existing file while saving takes its name.
                        (false, true) => {
                            a.name = e.name.clone();
                            a.overwrite = None;
                            Action::None
                        }
                    };
                }
            }
        }
    });
    ui.separator();

    if saving {
        ui.horizontal(|ui| {
            ui.label("File name");
            let edit = egui::TextEdit::singleline(&mut a.name).desired_width(f32::INFINITY).min_size(egui::vec2(0.0, ROW_HEIGHT * 0.8));
            if ui.add(edit).changed() {
                a.overwrite = None;
            }
        });
        if let Some(p) = &a.overwrite {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            ui.colored_label(ui.visuals().warn_fg_color, format!("“{name}” already exists. Tap Replace to overwrite it."));
        }
    } else if let Request::Open { purpose, .. } = &a.request {
        let mut all = a.all_files;
        let kinds = extensions(purpose).join(", ");
        if ui.checkbox(&mut all, format!("Show all files (not only {kinds})")).changed() {
            *action = Action::ToggleAll;
        }
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(96.0, ROW_HEIGHT))).clicked() {
            *action = Action::Cancel;
        }
        if saving {
            let label = if a.overwrite.is_some() { "Replace" } else { "Save here" };
            let name = sanitize_file_name(&a.name);
            if ui.add(egui::Button::new(label).min_size(egui::vec2(120.0, ROW_HEIGHT))).clicked() {
                *action = Action::Save(a.dir.join(name));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deckcraft-android-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn extensions_match_without_regard_to_case() {
        assert!(has_extension("Talk.PPTX", PRESENTATION_EXTS));
        assert!(has_extension("a.deckcraft", PRESENTATION_EXTS));
        assert!(!has_extension("photo.png", PRESENTATION_EXTS));
        assert!(!has_extension(".pptx", PRESENTATION_EXTS), "a dotfile has no extension");
        assert!(!has_extension("pptx", PRESENTATION_EXTS));
        assert_eq!(extensions("picture"), PICTURE_EXTS);
        assert_eq!(extensions("anything else"), PRESENTATION_EXTS);
    }

    #[test]
    fn listing_puts_folders_first_filters_files_and_hides_dotfiles() {
        let d = temp_dir("list");
        std::fs::create_dir(d.join("zeta")).unwrap();
        std::fs::create_dir(d.join("Alpha")).unwrap();
        std::fs::write(d.join("b.pptx"), b"x").unwrap();
        std::fs::write(d.join("A.deckcraft"), b"xy").unwrap();
        std::fs::write(d.join("photo.png"), b"p").unwrap();
        std::fs::write(d.join(".hidden.pptx"), b"h").unwrap();
        let names: Vec<String> = list_dir(&d, Some(PRESENTATION_EXTS)).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["Alpha", "zeta", "A.deckcraft", "b.pptx"]);
        let all = list_dir(&d, None).unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(all.iter().find(|e| e.name == "A.deckcraft").map(|e| e.size), Some(2));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(sanitize_file_name("Q3: plan/review?.pptx"), "Q3_ plan_review_.pptx");
        assert_eq!(sanitize_file_name("  ...  "), "Presentation");
        assert_eq!(sanitize_file_name("Deck.deckcraft"), "Deck.deckcraft");
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(human_size(12), "12 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn picking_a_file_delivers_it_to_the_inbox_under_the_request_id() {
        let d = temp_dir("pick");
        std::fs::write(d.join("talk.pptx"), b"PK").unwrap();
        let requests = Requests::default();
        let inbox = Inbox::default();
        let mut b = Browser::new(d.clone(), requests.clone(), inbox.clone());
        requests.push(Request::Open { purpose: "open".into(), id: 7 });
        b.start_next();
        assert!(b.is_open());
        let out = b.apply(Action::Pick(d.join("talk.pptx")), &mut || {});
        assert_eq!(out, Some(Outcome::Opened("talk.pptx".into())));
        assert!(!b.is_open());
        let events = inbox.lock().unwrap();
        assert!(matches!(&events[..], [InboxEvent::File(f)] if f.id == 7 && f.name == "talk.pptx" && f.bytes == b"PK"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cancelling_an_open_tells_the_app() {
        let d = temp_dir("cancel");
        let requests = Requests::default();
        let inbox = Inbox::default();
        let mut b = Browser::new(d.clone(), requests.clone(), inbox.clone());
        requests.push(Request::Open { purpose: "picture".into(), id: 3 });
        b.start_next();
        assert_eq!(b.apply(Action::Cancel, &mut || {}), Some(Outcome::Cancelled));
        assert!(matches!(&inbox.lock().unwrap()[..], [InboxEvent::Cancelled { id: 3 }]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn saving_over_an_existing_file_asks_first() {
        let d = temp_dir("save");
        let target = d.join("Deck.pptx");
        std::fs::write(&target, b"old").unwrap();
        let requests = Requests::default();
        let mut b = Browser::new(d.clone(), requests.clone(), Inbox::default());
        requests.push(Request::Save { name: "Deck.pptx".into(), bytes: b"new".to_vec() });
        b.start_next();
        // The root has no Documents folder, so the root itself is offered.
        assert_eq!(b.active.as_ref().map(|a| a.dir.clone()), Some(d.clone()));
        assert_eq!(b.active.as_ref().map(|a| a.name.clone()).as_deref(), Some("Deck.pptx"));
        assert_eq!(b.apply(Action::Save(target.clone()), &mut || {}), None, "the first tap only warns");
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(b.apply(Action::Save(target.clone()), &mut || {}), Some(Outcome::Saved(target.clone())));
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&d);
    }
}
