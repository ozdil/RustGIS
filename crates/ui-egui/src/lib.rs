//! L6: the RustGIS desktop front end, built around a ribbon (Home, Data, Style, Edit, Analyze,
//! View, Publish), the Layers and Browser panes, a Tools pane, the map view, the
//! attribute table and the status bar.
//!
//! The UI is thin: it reads [`Session`] state and runs commands by id ([`RustGisApp::run`]).
//! UI-only commands (`ui.*`: panes, dialogs, theme) are handled here.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[macro_use]
pub mod i18n;
mod analysispane;
mod assistantpane;
mod backstage;
mod chrome;
mod dialogs;
mod docpanes;
mod editpanes;
pub mod icons;
mod layoutview;
mod mapview;
mod panes;
mod pluginsui;
mod reportpane;
mod ribbon;
mod table;
pub mod theme;
mod update;
mod widgets;
mod workspacepanes;

use rustgis_engine::{Project, Session};
use serde_json::{Value, json};

pub use rustgis_assistant;
pub use rustgis_engine;

/// Files read asynchronously (browser uploads, drops) waiting to be opened: (file name, bytes).
pub type Inbox = std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

/// Starts the name of a file the web shell's `"text"` picker (Add Delimited Text) put in the
/// inbox, so the dialog's options go with that file and no other (a drop, another picker).
pub const DELIMITED_PICK: &str = "\u{1}delimited:";

/// A result an asynchronous service fills in when it's done (the app polls it each frame).
pub type Slot<T> = std::sync::Arc<std::sync::Mutex<Option<T>>>;

/// A shell's overrides of the Settings › Start-up preferences for one launch.
#[derive(Clone, Debug, Default)]
pub struct Start {
    /// Begin with an empty project (`Some(true)`) or the sample (`Some(false)`).
    pub blank: Option<bool>,
    /// A projection code: `EPSG:3010`, a preset id, `mercator` or `globe`.
    pub projection: Option<String>,
    /// The extent to open on, as `minLon,minLat,maxLon,maxLat`.
    pub extent: Option<String>,
}

/// Reads `minLon,minLat,maxLon,maxLat` (commas or spaces) into a valid longitude/latitude extent.
pub fn parse_extent(text: &str) -> Option<[f64; 4]> {
    let n: Vec<f64> = text.split([',', ' ']).filter(|x| !x.is_empty()).map(|x| x.trim().parse().ok()).collect::<Option<_>>()?;
    let e = rustgis_engine::cmd::navigate::lonlat_extent(Some(&serde_json::json!(n)))?;
    Some([e.min_x, e.min_y, e.max_x, e.max_y])
}

/// Platform hooks. The desktop app leaves them empty and uses native file dialogs; the web app
/// supplies an asynchronous file picker (results arrive in `inbox`) and browser downloads.
#[derive(Default)]
pub struct Services {
    /// Opens a file picker for `"data"` or `"project"`; the chosen file lands in `inbox`.
    pub open_async: Option<Box<dyn Fn(&str)>>,
    /// Hands bytes to the user as a download named `file_name`.
    pub download: Option<Box<dyn Fn(&str, Vec<u8>)>>,
    pub inbox: Option<Inbox>,
    /// Reads a saved preference (`language`, …).
    pub load_pref: Option<Box<dyn Fn(&str) -> Option<String>>>,
    /// Saves a preference.
    pub save_pref: Option<Box<dyn Fn(&str, &str)>>,
    /// Downloads a font by URL (web); the bytes land in `font_inbox`.
    pub fetch_font: Option<Box<dyn Fn(&str)>>,
    pub font_inbox: Option<Inbox>,
    /// The browser's GeoLibre runner: lists the tools' manifests (a JSON array) into the slot.
    pub geolibre_manifests: Option<Box<dyn Fn(Slot<Result<String, String>>)>>,
    /// …and runs one tool in its in-memory `/work` folder, putting the new files and printed
    /// lines in the slot. The desktop app runs tools through the `geolibre` program instead.
    pub geolibre_run: Option<Box<dyn Fn(rustgis_engine::cmd::gp::ExternalRun, Slot<rustgis_engine::cmd::gp::ExternalOutput>)>>,
    /// Downloads a URL into the slot (the browser's fetch; the desktop downloads on a thread).
    pub fetch: Option<Box<dyn Fn(&str, Slot<Result<Vec<u8>, String>>)>>,
    /// Makes an HTTP request (the AI assistant's calls to its model) and puts the response, of any
    /// status, in the slot (the browser's fetch; the desktop sends it on a thread).
    pub http: Option<Box<dyn Fn(rustgis_assistant::HttpRequest, Slot<Result<rustgis_assistant::HttpResponse, String>>)>>,
}

/// A new private folder for one print job: unique, created fresh (never an existing path) and,
/// on Unix, readable only by this user. Earlier jobs' folders from this run are removed first.
#[cfg(not(target_arch = "wasm32"))]
fn print_dir() -> Result<std::path::PathBuf, String> {
    let tmp = std::env::temp_dir();
    let prefix = format!("rustgis-print-{}-", std::process::id());
    if let Ok(rd) = std::fs::read_dir(&tmp) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) && e.file_type().is_ok_and(|t| t.is_dir()) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = tmp.join(format!("{prefix}{nanos}"));
    // Only Unix sets a mode on it.
    #[allow(unused_mut)]
    let mut b = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

#[cfg(target_arch = "wasm32")]
fn print_dir() -> Result<std::path::PathBuf, String> {
    Err("printing needs the desktop app".into())
}

/// Opens a file with the system's default application (native only).
#[cfg(not(target_arch = "wasm32"))]
fn open_with_system(path: &std::path::Path) -> Result<(), String> {
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(path).spawn().map(|_| ()).map_err(|e| format!("could not open {}: {e}", path.display()))
}

#[cfg(target_arch = "wasm32")]
fn open_with_system(_: &std::path::Path) -> Result<(), String> {
    Err("printing needs the desktop app".into())
}

/// A map or feature service Add From Service connects to: its capabilities arrive in the
/// background, then its layers are listed.
pub struct ServiceConn {
    pub url: String,
    /// `wms`, `wmts`, `wfs`, `ogcapi`, `arcgis`, or `auto`.
    pub kind: &'static str,
    pub slot: Slot<Result<Vec<u8>, String>>,
    /// The capabilities document, once it has arrived.
    pub text: Option<String>,
    /// What `service.layers` made of it.
    pub listing: Option<Value>,
    pub error: Option<String>,
}

/// Panes on the left of the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftPane {
    Contents,
    Catalog,
}

/// Panes on the right of the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightPane {
    Geoprocessing,
    Symbology,
    History,
    Details,
    Editing,
    Design,
    Workflows,
    Script,
    Tasks,
    Jobs,
    Profile,
    Diagnostics,
    Infographics,
    Report,
    Presentation,
    Notebook,
    Assistant,
    Shares,
}

pub struct UiState {
    pub tab: String,
    pub backstage: bool,
    pub backstage_page: &'static str,
    pub left: Option<LeftPane>,
    pub right: Option<RightPane>,
    pub dark: bool,
    /// The theme follows the system's light or dark setting until the user picks one.
    pub dark_follows_system: bool,
    /// Interface language preference: a language code or `auto`.
    pub language: String,
    /// Worldview preference for the built-in countries and world outline: `auto`, `default` or
    /// `china`.
    pub worldview: String,
    /// Start-up preference: begin with an empty project instead of the World Atlas sample.
    pub start_blank: bool,
    /// Start-up preference: the projection the first map starts in (a [`Projection`] code);
    /// empty for Web Mercator.
    ///
    /// [`Projection`]: rustgis_engine::map::Projection
    pub start_projection: String,
    /// The EPSG code typed in Settings › Start-up › Projection.
    pub start_projection_input: String,
    /// Start-up preference: the extent the map opens on, `[minLon, minLat, maxLon, maxLat]`;
    /// `None` for the full extent.
    pub start_extent: Option<[f64; 4]>,
    /// The start extent, fitted again once the map view has its real size.
    pub start_view: Option<[f64; 4]>,
    /// The ribbon shows only its tabs; clicking a tab opens it until the next click elsewhere.
    pub ribbon_minimized: bool,
    /// A minimized ribbon temporarily opened from its tab strip.
    pub ribbon_peek: bool,
    pub command_search: Option<String>,
    pub about: bool,
    pub shortcuts: bool,
    pub tab_strip_rect: Option<egui::Rect>,
    /// Tabs hidden from the ribbon (Help › Customize › Ribbon).
    pub hidden_tabs: Vec<String>,
    /// Extra Quick Access Toolbar commands.
    pub quick_access: Vec<String>,
    /// Saved pane arrangements: (name, left pane, right pane, tab).
    pub pane_sets: Vec<(String, Option<LeftPane>, Option<RightPane>, String)>,
    /// The command an item picked in the browser opens with (web).
    pub pending_open: Option<&'static str>,
    /// Add Delimited Text's options, for the file being picked (web).
    pub pending_csv: Option<Value>,
    /// Data downloading from web addresses (Add Data › From URL): the address, the parameters to
    /// add it with, and where the download lands.
    pub downloads: Vec<(u64, String, Value, Slot<Result<Vec<u8>, String>>)>,
    /// A link's data (`?data=`) has zoomed the map: the first of its files to land with something
    /// to show does, whichever it is.
    pub link_zoomed: bool,
    /// The service Add From Service is connected (or connecting) to.
    pub service: Option<ServiceConn>,
    /// A layout picture waiting for its image file (web).
    pub pending_layout_picture: bool,
    /// The corners of a picture graphic waiting for its image file.
    pub pending_picture: Option<Vec<rustgis_engine::geom::Coord>>,
    /// Text to put on the system clipboard at the end of the frame.
    pub copy_out: Option<String>,
    /// A web page to open at the end of the frame.
    pub open_url: Option<String>,
    /// An image to put on the system clipboard at the end of the frame.
    pub copy_image: Option<egui::ColorImage>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            tab: "Home".into(),
            backstage: false,
            backstage_page: "open",
            left: Some(LeftPane::Contents),
            right: None,
            dark: false,
            dark_follows_system: true,
            language: i18n::AUTO.into(),
            worldview: i18n::AUTO.into(),
            start_blank: false,
            start_projection: String::new(),
            start_projection_input: String::new(),
            start_extent: None,
            start_view: None,
            ribbon_minimized: false,
            ribbon_peek: false,
            command_search: None,
            about: false,
            shortcuts: false,
            tab_strip_rect: None,
            copy_out: None,
            open_url: None,
            copy_image: None,
            pending_picture: None,
            pending_layout_picture: false,
            pending_open: None,
            pending_csv: None,
            downloads: Vec::new(),
            link_zoomed: false,
            service: None,
            hidden_tabs: Vec::new(),
            quick_access: Vec::new(),
            pane_sets: Vec::new(),
        }
    }
}

pub struct RustGisApp {
    pub session: Session,
    pub ui: UiState,
    pub map: mapview::MapView,
    pub dialogs: dialogs::Dialogs,
    pub gp: panes::GpForm,
    pub table: table::TableState,
    pub layout: layoutview::LayoutView,
    pub forms: editpanes::Forms,
    pub workflow: analysispane::Workflow,
    pub ws: workspacepanes::WorkspaceState,
    /// The Plugin Manager and plugin downloads.
    pub plugins: pluginsui::PluginsState,
    /// Help › Check for Updates.
    pub update: update::UpdateState,
    pub reports: reportpane::ReportState,
    pub docs: docpanes::DocState,
    /// The AI Assistant's conversation and settings.
    pub assistant: assistantpane::AssistantState,
    fonts_frames: u32,
    applied_dark: Option<bool>,
    sent_title: String,
    pub quit_requested: bool,
    logo: Option<egui::TextureHandle>,
    pub services: Services,
    /// Extra font faces installed with the interface fonts: (name, face, region). The current
    /// language's region comes right after the interface font, so its glyph shapes win; the others
    /// are last-resort fallbacks. Faces are shared, so reinstalling copies nothing.
    extra_fonts: Vec<(String, std::sync::Arc<egui::FontData>, Option<rustgis_engine::render::fonts::CjkRegion>)>,
    /// The CJK region whose face was put first at the last font install.
    installed_region: Option<rustgis_engine::render::fonts::CjkRegion>,
    fonts_dirty: bool,
    fonts_requested: Vec<String>,
    /// Downloads that failed; cleared when a language is picked, which retries them.
    fonts_failed: Vec<String>,
    /// Fonts were installed this frame: `set_fonts` only takes effect on the next one.
    fonts_pending: bool,
    /// The language the interface is drawn in. It trails the chosen one until a font for its
    /// script is active, so switching to Chinese never draws a frame of missing-glyph boxes.
    shown_lang: i18n::Lang,
}

impl RustGisApp {
    pub fn new(session: Session) -> Self {
        let mut s = session;
        if s.active_layer.is_none() {
            s.active_layer = s.map().layers.first().map(|l| l.id);
        }
        RustGisApp {
            session: s,
            ui: UiState::default(),
            map: mapview::MapView::default(),
            dialogs: dialogs::Dialogs::default(),
            gp: panes::GpForm::default(),
            table: table::TableState::default(),
            layout: layoutview::LayoutView::default(),
            forms: editpanes::Forms::default(),
            workflow: analysispane::Workflow::default(),
            ws: workspacepanes::WorkspaceState::default(),
            plugins: pluginsui::PluginsState::default(),
            update: update::UpdateState::default(),
            reports: reportpane::ReportState::default(),
            docs: docpanes::DocState::default(),
            assistant: assistantpane::AssistantState::default(),
            fonts_frames: 0,
            applied_dark: None,
            sent_title: String::new(),
            quit_requested: false,
            logo: None,
            services: Services::default(),
            extra_fonts: Vec::new(),
            installed_region: None,
            fonts_dirty: false,
            fonts_requested: Vec::new(),
            fonts_failed: Vec::new(),
            fonts_pending: false,
            shown_lang: i18n::Lang::EN,
        }
    }

    pub fn with_services(mut self, services: Services) -> Self {
        self.services = services;
        if let Some(lang) = self.services.load_pref.as_ref().and_then(|load| load("language")) {
            self.ui.language = lang;
        }
        // A stored value that is neither `auto` nor a known worldview means `auto`.
        if let Some(w) = self.services.load_pref.as_ref().and_then(|load| load("worldview"))
            && rustgis_engine::sample::Worldview::from_code(&w).is_some()
        {
            self.ui.worldview = w.trim().to_ascii_lowercase();
        }
        self.apply_worldview();
        self.ui.start_blank = self.services.load_pref.as_ref().and_then(|load| load("start_project")).as_deref() == Some("blank");
        // A stored projection this version can't use means Web Mercator.
        if let Some(code) = self.services.load_pref.as_ref().and_then(|load| load("start_projection"))
            && let Ok(p) = rustgis_engine::map::Projection::from_code(&code)
        {
            self.ui.start_projection = p.code();
        }
        self.ui.start_extent = self.services.load_pref.as_ref().and_then(|load| load("start_extent")).and_then(|v| parse_extent(&v));
        if let Some(m) = self.services.load_pref.as_ref().and_then(|load| load("ribbon_minimized")) {
            self.ui.ribbon_minimized = m == "true";
        }
        let list = |s: Option<String>| -> Vec<String> {
            s.map(|v| v.split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect()).unwrap_or_default()
        };
        self.ui.hidden_tabs = ribbon::current_hidden_tabs(list(self.services.load_pref.as_ref().and_then(|load| load("hidden_tabs"))));
        self.ui.quick_access = list(self.services.load_pref.as_ref().and_then(|load| load("quick_access")));
        // The browser's GeoLibre tools, loaded in an earlier visit: load them again (the runner
        // comes from the browser's cache).
        if self.services.load_pref.as_ref().and_then(|load| load("geolibre_web")).as_deref() == Some("on") {
            panes::start_geolibre_load(&mut self);
        }
        // Plugins the web app installed in earlier visits (the desktop loads its plugins folder).
        pluginsui::restore(&mut self);
        // A GeoLibre runner located earlier (Locate… in the Tools pane).
        if let Some(path) = self.services.load_pref.as_ref().and_then(|load| load("geolibre_runner")).filter(|p| !p.is_empty())
            && let Err(e) = self.session.run("gp.setGeolibre", &json!({"path": path}))
        {
            log::warn!("{e}");
        }
        self
    }

    /// The language the interface is drawn in now (see [`Self::chosen_lang`]).
    pub fn lang(&self) -> i18n::Lang {
        self.shown_lang
    }

    /// The language picked in Settings › Language (or the system's, for `auto`).
    pub fn chosen_lang(&self) -> i18n::Lang {
        i18n::Lang::from_pref(&self.ui.language)
    }

    /// Whether the interface can be drawn in `lang` this frame: its script needs no extra font, or
    /// that font is installed and active (fonts installed this very frame aren't yet). A web app
    /// waits for the download; a failed one keeps the previous language. A desktop without the
    /// font has nothing to wait for.
    fn can_draw(&self, lang: i18n::Lang) -> bool {
        let Some(region) = Self::script_region(lang.script()) else { return true };
        let have = self.extra_fonts.iter().any(|(_, _, r)| *r == Some(region));
        (have || self.services.fetch_font.is_none()) && !self.fonts_pending && self.installed_region == Some(region)
    }

    /// On the desktop, an installed CJK face as a last-resort fallback, so CJK text (language names
    /// in the picker, place names in data) renders whatever the interface language. The bytes are
    /// borrowed from the font cache, not copied.
    fn add_cjk_fallback(&mut self) {
        use rustgis_engine::render::fonts::{CjkRegion, cjk_font};
        if self.extra_fonts.iter().any(|(n, _, _)| n == "cjk-fallback") {
            return;
        }
        let found = [CjkRegion::SimplifiedChinese, CjkRegion::Japanese, CjkRegion::Korean].into_iter().find_map(cjk_font);
        if let Some((bytes, index)) = found {
            let mut face = egui::FontData::from_static(bytes);
            face.index = *index;
            self.extra_fonts.push(("cjk-fallback".into(), std::sync::Arc::new(face), None));
        }
    }

    /// The CJK region of the current language, if any.
    fn current_region(&self) -> Option<rustgis_engine::render::fonts::CjkRegion> {
        Self::script_region(self.chosen_lang().script())
    }

    /// The CJK font region a script needs, if any.
    fn script_region(script: i18n::Script) -> Option<rustgis_engine::render::fonts::CjkRegion> {
        use rustgis_engine::render::fonts::CjkRegion;
        match script {
            i18n::Script::Cjk(i18n::CjkFont::SimplifiedChinese) => Some(CjkRegion::SimplifiedChinese),
            i18n::Script::Cjk(i18n::CjkFont::Japanese) => Some(CjkRegion::Japanese),
            i18n::Script::Cjk(i18n::CjkFont::Korean) => Some(CjkRegion::Korean),
            i18n::Script::Alphabetic => None,
        }
    }

    /// Makes sure the current language's script has a font: an installed CJK face on the desktop,
    /// a download on the web.
    fn ensure_script_font(&mut self) {
        use rustgis_engine::render::fonts::{cjk_font, cjk_font_url};
        // Switching between CJK languages reorders the faces already loaded.
        if self.current_region() != self.installed_region {
            self.fonts_dirty = true;
        }
        let Some(region) = self.current_region() else { return };
        let name = format!("cjk-{region:?}");
        if self.extra_fonts.iter().any(|(n, _, _)| *n == name) || self.fonts_requested.contains(&name) || self.fonts_failed.contains(&name)
        {
            return;
        }
        self.fonts_requested.push(name.clone());
        if let Some((bytes, index)) = cjk_font(region) {
            let mut face = egui::FontData::from_static(bytes);
            face.index = *index;
            self.extra_fonts.push((name, std::sync::Arc::new(face), Some(region)));
            self.fonts_dirty = true;
        } else if let Some(fetch) = &self.services.fetch_font {
            fetch(cjk_font_url(region));
        } else {
            log::warn!("no {region:?} font installed; text in that script may not display");
        }
    }

    /// Adds the waiting picture graphic with its image (`path` or base64 `image`).
    fn add_picture(&mut self, mut p: Value) -> Result<Value, String> {
        let pts = self.ui.pending_picture.take().ok_or("drag the picture's rectangle on the map first")?;
        p["shape"] = json!("picture");
        p["points"] = json!(pts.iter().map(|c| [c.x, c.y]).collect::<Vec<_>>());
        let r = self.run("graphics.add", p);
        if r.is_ok() && !self.session.keep_tool {
            self.session.tool = rustgis_engine::Tool::Explore;
        }
        r
    }

    /// A layout as PDF or PNG bytes, guarded like `Session::run`.
    fn layout_bytes(&mut self, li: usize, pdf: bool, dpi: f64) -> Result<Vec<u8>, String> {
        let session = &mut self.session;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rustgis_engine::cmd::layout::layout_bytes(session, li, pdf, dpi, true)
        }))
        .unwrap_or_else(|_| Err("the layout export failed unexpectedly".into()));
        if let Err(e) = &r {
            self.session.notify(e.clone(), true);
        }
        r
    }

    /// Opens a file by name and contents: a `.rgis` project, a raster, or vector data.
    /// Failures are reported to the user (command errors through `Session::run`, others here).
    pub fn open_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<Value, String> {
        self.open_bytes_with(name, bytes, None)
    }

    /// Opens a file from the inbox: one the `"text"` picker marked ([`DELIMITED_PICK`]) is read
    /// with Add Delimited Text's options.
    pub fn open_picked(&mut self, name: &str, bytes: &[u8]) -> Result<Value, String> {
        match name.strip_prefix(DELIMITED_PICK) {
            Some(name) => {
                let csv = self.ui.pending_csv.take();
                self.open_bytes_with(name, bytes, csv)
            }
            None => self.open_bytes(name, bytes),
        }
    }

    fn open_bytes_with(&mut self, name: &str, bytes: &[u8], csv: Option<Value>) -> Result<Value, String> {
        let lower = name.to_ascii_lowercase();
        // A plugin package: ask before installing it.
        if lower.ends_with(".rgplugin") {
            pluginsui::received(self, bytes.to_vec(), None);
            return Ok(Value::Null);
        }
        // Project items picked or dropped in the browser.
        let item = [
            (".rgismap", "map.import"),
            (".rgismpk", "map.import"),
            (".rgislyr", "layer.addLayerFile"),
            (".rgisstyle", "style.add"),
            (".rgistemplate", "project.fromTemplate"),
            (".rgistask", "task.import"),
            (".rgisreport", "report.open"),
            (".rgisrpttemplate", "report.open"),
            (".rgispres", "presentation.import"),
            (".rgisnb", "notebook.open"),
            (".ipynb", "notebook.open"),
            (".rgisdlpk", "dl.import"),
        ]
        .iter()
        .find(|(e, _)| lower.ends_with(e))
        .map(|(_, c)| *c);
        if let Some(cmd) = self.ui.pending_open.filter(|c| c.ends_with(".thumbnail")) {
            use base64::Engine;
            self.ui.pending_open = None;
            return self.run(cmd, json!({"image": base64::engine::general_purpose::STANDARD.encode(bytes)}));
        }
        if let Some(cmd) = self.ui.pending_open.take().or(item) {
            use base64::Engine;
            let r = self.run(cmd, json!({"data": base64::engine::general_purpose::STANDARD.encode(bytes)}));
            self.map.invalidate();
            return r;
        }
        if lower.ends_with(".rgispkg") {
            let text = rustgis_engine::cmd::items::package_text(bytes)?;
            let r = self.run("project.openText", json!({"text": text, "name": name}));
            self.map.invalidate();
            return r;
        }
        if lower.ends_with(".rgislayout") {
            let text = std::str::from_utf8(bytes).map_err(|_| "not a RustGIS layout".to_string())?;
            return self.run("layout.import", json!({"text": text}));
        }
        // A layout picture needs the layout still open.
        if self.session.active_layout.is_none() {
            self.ui.pending_layout_picture = false;
        }
        if self.ui.pending_layout_picture && [".png", ".jpg", ".jpeg"].iter().any(|e| lower.ends_with(e)) {
            use base64::Engine;
            self.ui.pending_layout_picture = false;
            let image = base64::engine::general_purpose::STANDARD.encode(bytes);
            return self.run("layout.addElement", json!({"kind": "picture", "image": image}));
        }
        if self.ui.pending_picture.is_some() && [".png", ".jpg", ".jpeg"].iter().any(|e| lower.ends_with(e)) {
            use base64::Engine;
            return self.add_picture(json!({"image": base64::engine::general_purpose::STANDARD.encode(bytes)}));
        }
        if rustgis_engine::data::raster::is_raster_name(name)
            || rustgis_engine::data::source::is_source_name(name)
            || rustgis_engine::data::io::is_multi_layer_name(name)
            || rustgis_engine::data::sheets::is_sheet_name(name)
        {
            use base64::Engine;
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            let mut p = json!({"fileName": name, "data": data});
            // A spreadsheet picked for Add Delimited Text takes its options.
            if let Some(Value::Object(opts)) = csv.filter(|_| rustgis_engine::data::sheets::is_sheet_name(name)) {
                for (k, v) in opts {
                    p[k] = v;
                }
            }
            return self.run("layer.addDataBytes", p);
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            let e = crate::i18n::fmt("{name} is not a text file: RustGIS opens GeoJSON, CSV, GeoTIFF and .rgis here", &[("name", name)]);
            self.session.notify(e.clone(), true);
            return Err(e);
        };
        if rustgis_engine::project::is_project_file(name) {
            let r = self.run("project.openText", json!({"text": text, "name": name}));
            self.map.invalidate();
            r
        } else {
            let mut p = json!({"fileName": name, "text": text});
            if let Some(Value::Object(opts)) = csv.filter(|_| [".csv", ".tsv", ".txt"].iter().any(|e| lower.ends_with(e))) {
                for (k, v) in opts {
                    p[k] = v;
                }
            }
            self.run("layer.addDataText", p)
        }
    }

    /// Offers bytes as a download (web).
    pub(crate) fn download_bytes(&mut self, name: &str, bytes: Vec<u8>) {
        self.download(name, bytes);
    }

    /// Offers bytes as a download (web) and reports whether a download hook exists.
    fn download(&mut self, name: &str, bytes: Vec<u8>) -> bool {
        match &self.services.download {
            Some(d) => {
                d(name, bytes);
                self.session.notify(crate::i18n::fmt("Downloaded {name}", &[("name", name)]), false);
                true
            }
            None => false,
        }
    }

    /// The RustGIS logo as a texture (rasterised once from `rustgis_render::logo`).
    pub fn logo(&mut self, ctx: &egui::Context) -> Option<egui::TextureId> {
        if self.logo.is_none()
            && let Some(pm) = rustgis_engine::render::logo::pixmap(256)
        {
            let img = egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data());
            self.logo = Some(ctx.load_texture("rustgis-logo", img, egui::TextureOptions::LINEAR));
        }
        self.logo.as_ref().map(|t| t.id())
    }

    /// Paints the logo into `rect`.
    pub fn paint_logo(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        if let Some(id) = self.logo(ui.ctx()) {
            ui.painter().image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
    }

    /// The sample "World Atlas" project.
    pub fn with_sample() -> Self {
        // Tests don't depend on the machine's time zone and locale.
        rustgis_engine::sample::apply_worldview(if cfg!(test) { "default" } else { i18n::AUTO });
        let mut s = Session::new(Project::default());
        let _ = s.run("project.sample", &Value::Null);
        s.dirty = false;
        RustGisApp::new(s)
    }

    /// Applies the worldview preference; an untouched sample project's countries follow it.
    fn apply_worldview(&mut self) {
        use rustgis_engine::sample;
        let before = sample::worldview();
        // Tests don't depend on the machine's time zone and locale.
        let pref = if cfg!(test) && self.ui.worldview == i18n::AUTO { "default" } else { self.ui.worldview.as_str() };
        let after = sample::apply_worldview(pref);
        if after != before {
            sample::swap_countries(&mut self.session, before, after);
            self.map.invalidate();
        }
    }

    /// Sets up the project the app opens with, from the Settings › Start-up preferences; a shell
    /// passes its own overrides (the web app's `?blank`, `?projection=` and `?extent=`). Call it
    /// once, after [`Self::with_services`] and before opening files. The start is not an edit to
    /// undo or save.
    pub fn start(&mut self, overrides: &Start) -> Result<(), String> {
        if overrides.blank.unwrap_or(self.ui.start_blank) {
            self.run("project.new", Value::Null)?;
        }
        let code = overrides.projection.clone().unwrap_or_else(|| self.ui.start_projection.clone());
        let mut result = Ok(());
        let mut projected = false;
        if !code.trim().is_empty() {
            match self.run("map.projection", serde_json::json!({"type": code})) {
                Ok(_) => projected = true,
                Err(e) => result = Err(e),
            }
        }
        let extent = match &overrides.extent {
            Some(text) => match parse_extent(text) {
                Some(e) => Some(e),
                None => {
                    result = result.and(Err(format!("extent '{text}' is minLon,minLat,maxLon,maxLat")));
                    self.ui.start_extent
                }
            },
            None => self.ui.start_extent,
        };
        if let Some(e) = extent {
            let _ = self.run("map.zoomToExtent", serde_json::json!({"extent": e}));
            self.ui.start_view = Some(e);
        } else if projected {
            let _ = self.run("map.zoomFull", Value::Null);
        }
        self.session.clear_history();
        self.map.invalidate();
        result
    }

    /// Runs a command by id: `ui.*` here, everything else in the engine.
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if !matches!(id, "ui.menu" | "ui.ribbon" | "ui.language" | "ui.worldview") {
            self.ui.ribbon_peek = false;
        }
        let r = if id.starts_with("ui.") { self.run_ui(id, &params) } else { self.session.run(id, &params) };
        if let Err(e) = &r {
            log::info!("{id}: {e}");
        }
        r
    }

    fn run_ui(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        let toggle_right = |ui: &mut UiState, pane: RightPane| ui.right = if ui.right == Some(pane) { None } else { Some(pane) };
        match id {
            "ui.pane.contents" => self.ui.left = if self.ui.left == Some(LeftPane::Contents) { None } else { Some(LeftPane::Contents) },
            "ui.pane.catalog" => self.ui.left = if self.ui.left == Some(LeftPane::Catalog) { None } else { Some(LeftPane::Catalog) },
            "ui.pane.geoprocessing" => toggle_right(&mut self.ui, RightPane::Geoprocessing),
            "ui.pane.symbology" => {
                if self.session.active_layer.is_none() {
                    return Err("select a layer in the Layers pane first".into());
                }
                toggle_right(&mut self.ui, RightPane::Symbology)
            }
            "ui.pane.history" => toggle_right(&mut self.ui, RightPane::History),
            "ui.pane.details" => toggle_right(&mut self.ui, RightPane::Details),
            "ui.pane.editing" => toggle_right(&mut self.ui, RightPane::Editing),
            "ui.pane.design" => toggle_right(&mut self.ui, RightPane::Design),
            "ui.pane.script" => toggle_right(&mut self.ui, RightPane::Script),
            // Opens the Workflows pane at a section (staying open when switching sections).
            "ui.pane.workflows" => {
                let section = p.get("section").and_then(Value::as_str);
                // A map pick started in another section (or before the pane closes) ends here.
                self.workflow.pick = None;
                match section.and_then(|s| analysispane::SECTIONS.iter().find(|x| x.0 == s)) {
                    Some((id, _)) => {
                        self.workflow.section = id;
                        self.workflow.result = None;
                        self.workflow.error = None;
                        self.ui.right = Some(RightPane::Workflows);
                    }
                    None => toggle_right(&mut self.ui, RightPane::Workflows),
                }
            }
            "ui.gp.open" => {
                let tool = p.get("tool").and_then(Value::as_str).and_then(rustgis_engine::analysis::tool);
                self.gp.open(tool.map(|t| t.id), &self.session);
                self.ui.right = Some(RightPane::Geoprocessing);
            }
            "ui.language" => {
                let code = p.get("code").and_then(Value::as_str).unwrap_or(i18n::AUTO);
                if code != i18n::AUTO && i18n::Lang::from_code(code).is_none() {
                    return Err(format!("unknown language '{code}'"));
                }
                self.ui.language = code.to_ascii_lowercase();
                self.fonts_failed.clear();
                if let Some(save) = &self.services.save_pref {
                    save("language", &self.ui.language);
                }
                self.map.invalidate();
            }
            "ui.worldview" => {
                let code = p.get("worldview").and_then(Value::as_str).unwrap_or(i18n::AUTO).to_ascii_lowercase();
                if code != i18n::AUTO && rustgis_engine::sample::Worldview::from_code(&code).is_none() {
                    return Err(format!("unknown worldview '{code}'"));
                }
                self.ui.worldview = code;
                if let Some(save) = &self.services.save_pref {
                    save("worldview", &self.ui.worldview);
                }
                self.apply_worldview();
            }
            "ui.startup" => {
                let blank = p.get("blank").and_then(Value::as_bool);
                // Validate before changing anything; `mercator` is stored as the default.
                let projection = match p.get("projection").and_then(Value::as_str) {
                    Some(code) => Some(rustgis_engine::map::Projection::from_code(code)?),
                    None => None,
                };
                // `"extent": "view"` takes the map's view, `null` the full extent.
                let extent = match p.get("extent") {
                    None => None,
                    Some(Value::Null) => Some(None),
                    Some(Value::String(v)) if v == "view" => {
                        let r = self.session.run("map.extent", &Value::Null)?;
                        Some(Some(rustgis_engine::cmd::navigate::lonlat_extent(r.get("extent")).ok_or("the view has no extent")?))
                    }
                    Some(v) => Some(Some(
                        rustgis_engine::cmd::navigate::lonlat_extent(Some(v))
                            .ok_or("extent is [minLon, minLat, maxLon, maxLat], \"view\" or null")?,
                    )),
                };
                if let Some(e) = extent {
                    self.ui.start_extent = e.map(|e| [e.min_x, e.min_y, e.max_x, e.max_y]);
                    if let Some(save) = &self.services.save_pref {
                        let text = self.ui.start_extent.map(|e| e.map(|x| format!("{x:.6}")).join(",")).unwrap_or_default();
                        save("start_extent", &text);
                    }
                }
                if let Some(b) = blank {
                    self.ui.start_blank = b;
                    if let Some(save) = &self.services.save_pref {
                        save("start_project", if b { "blank" } else { "sample" });
                    }
                }
                if let Some(proj) = projection {
                    self.ui.start_projection =
                        if proj == rustgis_engine::map::Projection::WebMercator { String::new() } else { proj.code() };
                    if let Some(save) = &self.services.save_pref {
                        save("start_projection", &self.ui.start_projection);
                    }
                }
                return Ok(serde_json::json!({
                    "blank": self.ui.start_blank,
                    "projection": if self.ui.start_projection.is_empty() { "mercator" } else { self.ui.start_projection.as_str() },
                    "extent": self.ui.start_extent,
                }));
            }
            "ui.ribbon" => {
                self.ui.ribbon_minimized = p.get("minimized").and_then(Value::as_bool).unwrap_or(!self.ui.ribbon_minimized);
                self.ui.ribbon_peek = false;
                if let Some(save) = &self.services.save_pref {
                    save("ribbon_minimized", if self.ui.ribbon_minimized { "true" } else { "false" });
                }
            }
            "ui.theme" => {
                self.ui.dark = p.get("dark").and_then(Value::as_bool).unwrap_or(!self.ui.dark);
                self.ui.dark_follows_system = false;
            }
            "ui.commandSearch" => self.ui.command_search = Some(String::new()),
            "ui.about" => self.ui.about = true,
            "ui.checkUpdate" => self.update.check(),
            "ui.plugins" => {
                self.plugins.open = p.get("open").and_then(Value::as_bool).unwrap_or(!self.plugins.open);
                if let Some(url) = p.get("registry").and_then(Value::as_str) {
                    self.plugins.url = url.to_string();
                    self.plugins.set_registry(None);
                }
                if let Some(tab) = p.get("tab").and_then(Value::as_str) {
                    self.plugins.tab = match tab {
                        "browse" => pluginsui::Tab::Browse,
                        "settings" => pluginsui::Tab::Settings,
                        _ => pluginsui::Tab::Installed,
                    };
                }
            }
            "ui.installPlugin" => pluginsui::install_from_file(self, p.get("path").and_then(Value::as_str)),
            // What a click with Explore does: identify there and show the popup.
            "ui.identify" => {
                // The engine checks the coordinates; the popup reuses them.
                let (lon, lat) = (p.get("lon").cloned().unwrap_or(Value::Null), p.get("lat").cloned().unwrap_or(Value::Null));
                let v = self.session.run("identify", &json!({"lon": lon, "lat": lat, "tolerancePx": 5}))?;
                if let (false, Some(lon), Some(lat)) = (v.is_null(), lon.as_f64(), lat.as_f64()) {
                    self.map.popup = Some(mapview::Popup { info: v, at: rustgis_engine::geom::Coord::new(lon, lat), index: 0 });
                } else {
                    self.map.popup = None;
                }
            }
            "ui.shortcuts" => self.ui.shortcuts = true,
            // Copy, cut and copy path also put their text on the system clipboard.
            "ui.copy" | "ui.cut" | "ui.copyPath" => {
                let cmd = match id {
                    "ui.copy" => "edit.copy",
                    "ui.cut" => "edit.cut",
                    _ => "layer.copyPath",
                };
                let v = self.run(cmd, p.clone())?;
                if let Some(t) = v.get("text").and_then(Value::as_str) {
                    self.ui.copy_out = Some(t.to_string());
                }
                if id == "ui.copyPath" {
                    self.session
                        .notify(i18n::fmt("Copied {path}", &[("path", v.get("text").and_then(Value::as_str).unwrap_or(""))]), false);
                }
                return Ok(v);
            }
            "ui.backstage" => self.ui.backstage = p.get("open").and_then(Value::as_bool).unwrap_or(!self.ui.backstage),
            "ui.tab" => {
                if let Some(t) = p.get("tab").and_then(Value::as_str) {
                    self.ui.tab = ribbon::current_tab(t).to_string();
                }
            }
            // Delimited text with its columns and coordinate system: the dialog, then (with the
            // dialog's options) the file.
            "ui.addDelimited" => {
                if !p.is_object() {
                    return self.run("ui.dialog", json!({"name": "addCsv"}));
                }
                if let Some(open) = &self.services.open_async {
                    self.ui.pending_layout_picture = false;
                    self.ui.pending_picture = None;
                    self.ui.pending_open = None;
                    self.ui.pending_csv = Some(p.clone());
                    open("text");
                } else if let Some(path) =
                    dialogs::pick_open(&["csv", "tsv", "txt", "xlsx", "xlsm", "xlsb", "xls", "ods"], tl!("Delimited text and spreadsheets"))
                {
                    let mut q = p.clone();
                    q["path"] = json!(path);
                    return self.run("layer.addData", q);
                }
            }
            // Data by its web address: the dialog, then (with an address) the download, off the
            // frame; it's added when it lands. A PMTiles archive is a tile layer, never downloaded.
            "ui.addUrl" => {
                let Some(url) = p.get("url").and_then(Value::as_str).map(str::trim).filter(|u| !u.is_empty()) else {
                    return self.run("ui.dialog", json!({"name": "addUrl"}));
                };
                if url.to_ascii_lowercase().split(['?', '#']).next().is_some_and(|u| u.ends_with(".pmtiles")) {
                    return self.run("layer.addUrl", p.clone());
                }
                let url = url.to_string();
                let slot: Slot<Result<Vec<u8>, String>> = Slot::default();
                let land = slot.clone();
                rustgis_engine::render::rustgis_tiles::cache::download(
                    &url,
                    Box::new(move |r| *land.lock().unwrap_or_else(|e| e.into_inner()) = Some(r)),
                );
                let mut q = json!({});
                for k in ["name", "layer"] {
                    if let Some(v) = p.get(k).filter(|v| v.as_str().is_some_and(|s| !s.trim().is_empty())) {
                        q[k] = v.clone();
                    }
                }
                if p.get("zoom").and_then(Value::as_bool) == Some(true) {
                    q["zoom"] = json!(true);
                }
                if p.get("service").and_then(Value::as_bool) == Some(true) {
                    q["service"] = json!(true);
                }
                self.session.notify(i18n::fmt("Downloading {url}…", &[("url", &url)]), false);
                let id = self.session.download_started();
                self.ui.downloads.push((id, url, q, slot));
            }
            // A map or feature service: the dialog, then (with an address) its capabilities,
            // fetched off the frame; the dialog lists its layers when they land.
            "ui.addService" => {
                let Some(url) = p.get("url").and_then(Value::as_str).map(str::trim).filter(|u| !u.is_empty()) else {
                    return self.run("ui.dialog", json!({"name": "addService"}));
                };
                let kind: &'static str = match p.get("type").and_then(Value::as_str).unwrap_or("auto") {
                    "wms" => "wms",
                    "wmts" => "wmts",
                    "wfs" => "wfs",
                    "ogcapi" => "ogcapi",
                    "arcgis" => "arcgis",
                    _ => "auto",
                };
                let caps = rustgis_engine::data::ogc::capabilities_url(url, (kind != "auto").then_some(kind));
                let slot: Slot<Result<Vec<u8>, String>> = Slot::default();
                let land = slot.clone();
                rustgis_engine::render::rustgis_tiles::cache::download(
                    &caps,
                    Box::new(move |r| *land.lock().unwrap_or_else(|e| e.into_inner()) = Some(r)),
                );
                // The dialog shows the address connected to, however the connection was asked for.
                self.dialogs.text = url.to_string();
                self.ui.service = Some(ServiceConn { url: url.to_string(), kind, slot, text: None, listing: None, error: None });
            }
            "ui.addData" => {
                if let Some(open) = &self.services.open_async {
                    // A picker cancelled earlier mustn't claim this file.
                    self.ui.pending_layout_picture = false;
                    self.ui.pending_picture = None;
                    self.ui.pending_open = None;
                    self.ui.pending_csv = None;
                    open("data");
                } else if let Some(path) = dialogs::pick_open(
                    &[
                        &[
                            "geojson",
                            "json",
                            "csv",
                            "tsv",
                            "xlsx",
                            "xlsm",
                            "xlsb",
                            "xls",
                            "ods",
                            "rss",
                            "georss",
                            "atom",
                            "xml",
                            "landxml",
                            "shp",
                            "zip",
                            "gpkg",
                            "parquet",
                            "geoparquet",
                            "fgb",
                            "duckdb",
                            "ddb",
                            "pmtiles",
                            "mbtiles",
                            "kml",
                            "kmz",
                            "gpx",
                            "gml",
                            "dxf",
                            "tab",
                            "mif",
                            "osm",
                            "pbf",
                            "tif",
                            "tiff",
                            "asc",
                        ][..],
                        // Rasters GDAL reads (every one it is asked to read).
                        #[cfg(not(target_arch = "wasm32"))]
                        rustgis_engine::data::raster::gdal::EXTENSIONS,
                    ]
                    .concat(),
                    tl!("Vector and raster data"),
                ) {
                    return self.run("layer.addData", json!({"path": path}));
                }
            }
            "ui.open" => {
                if let Some(open) = &self.services.open_async {
                    // A picker cancelled earlier mustn't claim this file.
                    self.ui.pending_layout_picture = false;
                    self.ui.pending_picture = None;
                    self.ui.pending_open = None;
                    open("project");
                    self.ui.backstage = false;
                } else if let Some(path) = dialogs::pick_open(&["rgis", "rgisx"], tl!("RustGIS project")) {
                    let r = self.run("project.open", json!({"path": path}));
                    self.ui.backstage = false;
                    self.map.invalidate();
                    return r;
                }
            }
            "ui.save" | "ui.saveAs" if self.services.download.is_some() => {
                let text = serde_json::to_string(&self.session.project).map_err(|e| e.to_string())?;
                let name = format!("{}.rgis", self.session.project.name);
                self.download(&name, text.into_bytes());
                self.session.dirty = false;
            }
            "ui.save" => {
                if self.session.path.is_some() {
                    return self.run("project.save", Value::Null);
                }
                return self.run("ui.saveAs", Value::Null);
            }
            "ui.saveAs" => {
                let name = format!("{}.rgis", self.session.project.name);
                if let Some(path) = dialogs::pick_save(&name, &["rgis"], tl!("RustGIS project")) {
                    return self.run("project.saveAs", json!({"path": path}));
                }
            }
            // Layouts: export (PNG/PDF), print, save and import layout files, draw frames.
            "ui.exportLayout" | "ui.printLayout" => {
                let li = self.session.active_layout.ok_or("open a layout first")?;
                let name = self.session.project.layouts.get(li).map(|l| l.name.clone()).unwrap_or_default();
                let print = id == "ui.printLayout";
                let pdf_only = print || p.get("format").and_then(Value::as_str) == Some("pdf");
                if self.services.download.is_some() {
                    let bytes = self.layout_bytes(li, true, if print { 300.0 } else { 150.0 })?;
                    self.download(&format!("{name}.pdf"), bytes);
                } else if print {
                    // Printing goes through the system's PDF viewer, which owns the printers.
                    let bytes = self.layout_bytes(li, true, 300.0)?;
                    let safe: String =
                        name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '_' }).collect();
                    let path = print_dir()?.join(format!("{safe}.pdf"));
                    rustgis_engine::data::io::write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
                    open_with_system(&path)?;
                    self.session.notify(i18n::fmt("Opened {name} for printing", &[("name", &path.display().to_string())]), false);
                } else {
                    let exts: &[&str] = if pdf_only { &["pdf"] } else { &["pdf", "png"] };
                    if let Some(path) = dialogs::pick_save(&format!("{name}.pdf"), exts, tl!("PDF or PNG")) {
                        return self.run("layout.export", json!({"layout": li, "path": path, "dpi": 200}));
                    }
                }
            }
            "ui.newReport" => {
                let r = self.run("report.new", Value::Null);
                if r.is_ok() {
                    self.ui.right = Some(RightPane::Report);
                }
                return r;
            }
            "ui.reportLayoutFile" => {
                if let Some(open) = &self.services.open_async {
                    self.ui.pending_open = Some("report.existingTemplate");
                    open("item");
                } else if let Some(path) = dialogs::pick_open(&["rgislayout"], tl!("RustGIS layout")) {
                    return self.run("report.existingTemplate", json!({"path": path}));
                }
            }
            // Reports: export as PDF (or PNG, the first page).
            "ui.exportReport" => {
                let i = rustgis_engine::cmd::report::index(&self.session, p)?;
                let name = self.session.project.reports.get(i).map(|r| r.name.clone()).unwrap_or_default();
                if self.services.download.is_some() {
                    let session = &mut self.session;
                    let bytes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        rustgis_engine::cmd::report::report_bytes(session, i, 150.0, true)
                    }))
                    .unwrap_or_else(|_| Err("the report export failed unexpectedly".into()))
                    .inspect_err(|e| self.session.notify(e.clone(), true))?
                    .0;
                    self.download(&format!("{name}.pdf"), bytes);
                } else if let Some(path) = dialogs::pick_save(&format!("{name}.pdf"), &["pdf", "png"], tl!("PDF or PNG")) {
                    return self.run("report.export", json!({"report": i, "path": path, "dpi": 200}));
                }
            }
            "ui.saveLayout" => {
                let li = self.session.active_layout.ok_or("open a layout first")?;
                let l = self.session.project.layouts.get(li).cloned().ok_or("open a layout first")?;
                if self.services.download.is_some() {
                    let text = serde_json::to_string_pretty(&l).map_err(|e| e.to_string())?;
                    self.download(&format!("{}.rgislayout", l.name), text.into_bytes());
                } else if let Some(path) = dialogs::pick_save(&format!("{}.rgislayout", l.name), &["rgislayout"], tl!("RustGIS layout")) {
                    return self.run("layout.save", json!({"layout": li, "path": path}));
                }
            }
            "ui.importLayout" => {
                if let Some(open) = &self.services.open_async {
                    // A picker cancelled earlier mustn't claim this file.
                    self.ui.pending_layout_picture = false;
                    self.ui.pending_picture = None;
                    self.ui.pending_open = None;
                    open("layout");
                } else if let Some(path) = dialogs::pick_open(&["rgislayout"], tl!("RustGIS layout")) {
                    return self.run("layout.import", json!({"path": path}));
                }
            }
            "ui.layoutPicture" => {
                let li = self.session.active_layout.ok_or("open a layout first")?;
                if let Some(open) = &self.services.open_async {
                    self.ui.pending_layout_picture = true;
                    self.ui.pending_picture = None;
                    self.ui.pending_open = None;
                    open("picture");
                } else if let Some(path) = dialogs::pick_open(&["png", "jpg", "jpeg"], tl!("Pictures")) {
                    let v = self.run("layout.addElement", json!({"layout": li, "kind": "picture", "path": path}))?;
                    self.layout.selected = v["element"].as_u64().map(|v| v as u32);
                }
            }
            // Project items: save (a file dialog, or a download on the web) and open.
            "ui.saveItem" => {
                let kind = p.get("kind").and_then(Value::as_str).ok_or("kind is required")?;
                let session = &mut self.session;
                let (name, bytes) =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rustgis_engine::cmd::items::item_bytes(session, kind, p)))
                        .unwrap_or_else(|_| Err("saving failed unexpectedly".into()))
                        .inspect_err(|e| self.session.notify(e.clone(), true))?;
                if self.services.download.is_some() {
                    self.download(&name, bytes);
                } else {
                    let (ext, label) =
                        rustgis_engine::cmd::items::ITEMS.iter().find(|i| i.0 == kind).map_or(("json", "File"), |i| (i.1, i.2));
                    if let Some(path) = dialogs::pick_save(&name, &[ext], tl!(label)) {
                        rustgis_engine::data::io::write_atomic(std::path::Path::new(&path), &bytes).map_err(|e| e.to_string())?;
                        self.session.notify(i18n::fmt("Saved {name}", &[("name", &path)]), false);
                    }
                }
            }
            "ui.openItem" => {
                let kind = p.get("kind").and_then(Value::as_str).ok_or("kind is required")?;
                let (cmd, exts, label): (&str, &[&str], &str) = match kind {
                    "map" => ("map.import", &["rgismap", "rgismpk", "rgis", "rgisx", "rgispkg"], "RustGIS maps and projects"),
                    "layer" => ("layer.addLayerFile", &["rgislyr"], "RustGIS layer file"),
                    "styles" => ("style.add", &["rgisstyle"], "RustGIS styles"),
                    "importStyles" => ("style.import", &["rgislyr", "rgismap", "rgis", "rgisx"], "RustGIS layers, maps and projects"),
                    "template" => ("project.fromTemplate", &["rgistemplate"], "RustGIS project template"),
                    "task" => ("task.import", &["rgistask"], "RustGIS task"),
                    "report" => ("report.open", &["rgisreport", "rgisrpttemplate"], "RustGIS report or report template"),
                    "presentation" => ("presentation.import", &["rgispres"], "RustGIS presentation"),
                    "notebook" => ("notebook.open", &["rgisnb", "ipynb"], "RustGIS or Jupyter notebook"),
                    "dlPackage" => ("dl.import", &["rgisdlpk"], "RustGIS deep learning package"),
                    k => return Err(format!("unknown item '{k}'")),
                };
                if let Some(open) = &self.services.open_async {
                    self.ui.pending_open = Some(cmd);
                    open("item");
                } else if let Some(path) = dialogs::pick_open(exts, tl!(label)) {
                    let r = self.run(cmd, json!({"path": path}));
                    self.map.invalidate();
                    return r;
                }
            }
            "ui.addFolder" => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    return self.run("project.addFolder", json!({"path": dir.display().to_string()}));
                }
                #[cfg(target_arch = "wasm32")]
                return Err("folder connections need the desktop app".into());
            }
            "ui.captureMap" => {
                let session = &mut self.session;
                let png = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    rustgis_engine::cmd::share::map_png(session, None, None, Some(192.0))
                }))
                .unwrap_or_else(|_| Err("the capture failed unexpectedly".into()))?;
                let pm = rustgis_engine::render::decode_png(&png).ok_or("could not draw the map")?;
                let size = [pm.width() as usize, pm.height() as usize];
                self.ui.copy_image = Some(egui::ColorImage::from_rgba_premultiplied(size, pm.data()));
                self.session.notify(tl!("Copied the map to the clipboard"), false);
            }
            "ui.printMap" => {
                let session = &mut self.session;
                let pdf = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rustgis_engine::cmd::share::map_pdf(session, 300.0)))
                    .unwrap_or_else(|_| Err("printing failed unexpectedly".into()))
                    .inspect_err(|e| self.session.notify(e.clone(), true))?;
                let name = format!("{}.pdf", self.session.map().name);
                if self.services.download.is_some() {
                    self.download(&name, pdf);
                } else {
                    let safe: String = self
                        .session
                        .map()
                        .name
                        .chars()
                        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '_' })
                        .collect();
                    let path = print_dir()?.join(format!("{safe}.pdf"));
                    rustgis_engine::data::io::write_atomic(&path, &pdf).map_err(|e| e.to_string())?;
                    open_with_system(&path)?;
                    self.session.notify(i18n::fmt("Opened {name} for printing", &[("name", &path.display().to_string())]), false);
                }
            }
            "ui.pane.tasks" => toggle_right(&mut self.ui, RightPane::Tasks),
            "ui.pane.jobs" => toggle_right(&mut self.ui, RightPane::Jobs),
            "ui.pane.profile" => toggle_right(&mut self.ui, RightPane::Profile),
            "ui.pane.diagnostics" => toggle_right(&mut self.ui, RightPane::Diagnostics),
            "ui.pane.infographics" => toggle_right(&mut self.ui, RightPane::Infographics),
            "ui.pane.report" => toggle_right(&mut self.ui, RightPane::Report),
            "ui.pane.presentation" => toggle_right(&mut self.ui, RightPane::Presentation),
            "ui.pane.notebook" => toggle_right(&mut self.ui, RightPane::Notebook),
            // `{"ask": text}` opens the pane and sends the text.
            "ui.pane.assistant" => match p.get("ask").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()) {
                Some(text) => {
                    self.ui.right = Some(RightPane::Assistant);
                    self.assistant.queued = Some(text.to_string());
                }
                None => toggle_right(&mut self.ui, RightPane::Assistant),
            },
            "ui.pane.shares" => toggle_right(&mut self.ui, RightPane::Shares),
            // Share As: to a file (recorded under Status › Jobs), or a download in the browser.
            "ui.share" => {
                let kind = p.get("kind").and_then(Value::as_str).ok_or("kind is required")?;
                let cmd = match kind {
                    "webMap" => "share.webMap",
                    "webLayer" => "share.webLayer",
                    "webStyle" => "share.webStyle",
                    "locator" => "share.locator",
                    "geodataService" => "share.geodataService",
                    "webTool" => "share.webTool",
                    "webScene" => "share.webScene",
                    "linkChart" => "share.linkChart",
                    k => return Err(format!("unknown share '{k}'")),
                };
                if self.services.download.is_some() {
                    return self.run("ui.saveItem", json!({"kind": kind}));
                }
                let (ext, label) = rustgis_engine::cmd::sharing::KINDS.iter().find(|k| k.0 == kind).map_or(("bin", "File"), |k| (k.1, k.2));
                let name = match kind {
                    "webLayer" | "webStyle" | "locator" => {
                        self.session.active_layer.and_then(|id| self.session.map().layer(id)).map(|l| l.name.clone())
                    }
                    _ => Some(self.session.map().name.clone()),
                }
                .unwrap_or_else(|| "shared".into());
                if let Some(path) = dialogs::pick_save(&format!("{name}.{ext}"), &[ext], tl!(label)) {
                    return self.run(cmd, json!({"path": path}));
                }
            }
            "ui.newPresentation" | "ui.newNotebook" => {
                let (cmd, pane) = if id == "ui.newPresentation" {
                    ("presentation.new", RightPane::Presentation)
                } else {
                    ("notebook.new", RightPane::Notebook)
                };
                let r = self.run(cmd, Value::Null);
                if r.is_ok() {
                    self.ui.right = Some(pane);
                }
                return r;
            }
            "ui.presentationPicture" => {
                if let Some(open) = &self.services.open_async {
                    self.ui.pending_layout_picture = false;
                    self.ui.pending_picture = None;
                    self.ui.pending_open = Some("presentation.imagePage");
                    open("picture");
                } else if let Some(path) = dialogs::pick_open(&["png", "jpg", "jpeg"], tl!("Pictures")) {
                    return self.run("presentation.imagePage", json!({"path": path}));
                }
            }
            "ui.presentationVideo" => {
                // In the browser a video is a web address, typed in the pane.
                if self.services.open_async.is_some() {
                    self.ui.right = Some(RightPane::Presentation);
                } else if let Some(path) = dialogs::pick_open(&["mp4", "webm", "ogv", "mov"], tl!("Videos")) {
                    return self.run("presentation.videoPage", json!({"source": path}));
                }
            }
            "ui.exportPresentation" => {
                let i = rustgis_engine::cmd::presentation::index(&self.session, p)?;
                let name = self.session.project.presentations.get(i).map(|r| r.name.clone()).unwrap_or_default();
                if self.services.download.is_some() {
                    let session = &mut self.session;
                    let bytes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        rustgis_engine::cmd::presentation::export_bytes(session, i, "html", 110.0)
                    }))
                    .unwrap_or_else(|_| Err("the presentation export failed unexpectedly".into()))
                    .inspect_err(|e| self.session.notify(e.clone(), true))?
                    .0;
                    self.download(&format!("{name}.html"), bytes);
                } else if let Some(path) =
                    dialogs::pick_save(&format!("{name}.html"), &["html", "pdf", "png"], tl!("Slideshow, PDF or PNG"))
                {
                    return self.run("presentation.export", json!({"presentation": i, "path": path}));
                }
            }
            "ui.catalogView" => self.ws.catalog_view = p.get("on").and_then(Value::as_bool).unwrap_or(!self.ws.catalog_view),
            "ui.itemPopups" => self.ws.item_popups = p.get("on").and_then(Value::as_bool).unwrap_or(!self.ws.item_popups),
            "ui.linkCursors" => self.ws.link_cursors = p.get("on").and_then(Value::as_bool).unwrap_or(!self.ws.link_cursors),
            // A second map beside the active one (for linked views and cursors).
            "ui.splitView" => {
                let others: Vec<usize> = (0..self.session.project.maps.len()).filter(|i| *i != self.session.project.active_map).collect();
                self.ws.split = match (self.ws.split, p.get("map").and_then(Value::as_str)) {
                    (_, Some(n)) => Some(
                        self.session
                            .project
                            .maps
                            .iter()
                            .position(|m| m.name.eq_ignore_ascii_case(n))
                            .ok_or_else(|| format!("no map '{n}'"))?,
                    ),
                    (Some(_), None) => None,
                    (None, None) => Some(*others.first().ok_or("add a second map first (Data › New Map)")?),
                };
            }
            "ui.paneSet" => {
                if let Some(n) = p.get("save").and_then(Value::as_str) {
                    self.ui.pane_sets.retain(|x| x.0 != n);
                    self.ui.pane_sets.push((n.to_string(), self.ui.left, self.ui.right, self.ui.tab.clone()));
                } else if let Some(n) = p.get("load").and_then(Value::as_str) {
                    let set = self.ui.pane_sets.iter().find(|x| x.0 == n).cloned().ok_or_else(|| format!("no pane set '{n}'"))?;
                    (self.ui.left, self.ui.right, self.ui.tab) = (set.1, set.2, ribbon::current_tab(&set.3).to_string());
                } else {
                    return Err("save or load a pane set by name".into());
                }
            }
            "ui.learn" => {
                // Only web pages: the URL can come from a script or command search.
                let url = p.get("url").and_then(Value::as_str).unwrap_or("https://rustgis.app/user-guide/").to_string();
                if !(url.starts_with("https://") || url.starts_with("http://")) {
                    return Err("ui.learn opens web pages (http or https) only".into());
                }
                self.ui.open_url = Some(url);
            }
            "ui.customizeRibbon" => {
                let tab = p.get("tab").and_then(Value::as_str).ok_or("tab is required")?.to_string();
                if tab == "Help" {
                    return Err("the Help tab always shows".into());
                }
                if self.ui.hidden_tabs.contains(&tab) {
                    self.ui.hidden_tabs.retain(|t| *t != tab);
                } else {
                    self.ui.hidden_tabs.push(tab);
                }
                if let Some(save) = &self.services.save_pref {
                    save("hidden_tabs", &self.ui.hidden_tabs.join(","));
                }
            }
            "ui.quickAccess" => {
                let id = p.get("command").and_then(Value::as_str).ok_or("command is required")?.to_string();
                if self.ui.quick_access.contains(&id) {
                    self.ui.quick_access.retain(|c| *c != id);
                } else if self.session.registry.get(&id).is_some() || id.starts_with("ui.") {
                    self.ui.quick_access.push(id);
                } else {
                    return Err(format!("unknown command '{id}'"));
                }
                if let Some(save) = &self.services.save_pref {
                    save("quick_access", &self.ui.quick_access.join(","));
                }
            }
            "ui.importThumbnail" => {
                let cmd = if p.get("map").and_then(Value::as_bool) == Some(true) { "map.thumbnail" } else { "project.thumbnail" };
                if let Some(open) = &self.services.open_async {
                    self.ui.pending_open = Some(cmd);
                    open("picture");
                } else if let Some(path) = dialogs::pick_open(&["png", "jpg", "jpeg"], tl!("Pictures")) {
                    return self.run(cmd, json!({"path": path}));
                }
            }
            "ui.animationPlay" => {
                if self.session.map().animation.len() < 2 {
                    return Err("add at least two keyframes".into());
                }
                self.ws.playing = if self.ws.playing.is_some() { None } else { Some((0.0, 0)) };
            }
            "ui.exportAnimation" => {
                if self.services.download.is_some() {
                    return Err("exporting animations needs the desktop app".into());
                }
                let name = format!("{} animation.zip", self.session.map().name);
                if let Some(path) = dialogs::pick_save(&name, &["zip"], tl!("PNG frames (zip)")) {
                    return self.run("animation.export", json!({"path": path}));
                }
            }
            "ui.magnifier" => self.map.magnifier = p.get("on").and_then(Value::as_bool).unwrap_or(!self.map.magnifier),
            "ui.layoutSelect" => self.layout.selected = p.get("element").and_then(Value::as_u64).map(|v| v as u32),
            "ui.layoutTool" => {
                self.layout.tool = match p.get("shape").and_then(Value::as_str) {
                    Some("polygon") => Some("polygon"),
                    Some("lasso") => Some("lasso"),
                    _ => None,
                };
            }
            "ui.exportMap" if self.services.download.is_some() => {
                // The same last-resort panic guard `Session::run` gives commands.
                let session = &mut self.session;
                let png = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    rustgis_engine::cmd::share::map_png(session, None, None, Some(192.0))
                }))
                .unwrap_or_else(|_| Err("Export Map failed unexpectedly".into()));
                let png = match png {
                    Ok(p) => p,
                    Err(e) => {
                        self.session.notify(e.clone(), true);
                        return Err(e);
                    }
                };
                let name = format!("{}.png", self.session.map().name);
                self.download(&name, png);
            }
            // Without a format, the Export Features dialog asks for one.
            "ui.exportLayer" if p.get("format").and_then(Value::as_str).is_none() => {
                self.dialogs.open("exportLayer", &self.session)?;
            }
            "ui.exportLayer" if self.services.download.is_some() => {
                let l = self
                    .session
                    .active_layer
                    .and_then(|id| self.session.map().layer(id))
                    .ok_or("select a layer in the Layers pane first")?;
                // Any format but GeoPackage, which needs SQLite.
                let ext = p.get("format").and_then(Value::as_str).unwrap_or("geojson").to_ascii_lowercase();
                // A streamed layer exports its whole source, not the features read for the view.
                let whole = if l.is_streamed() { Some(l.load_all()?) } else { None };
                let written = rustgis_engine::data::io::write_bytes(whole.as_ref().unwrap_or(&l.dataset), &ext).map_err(|e| e.to_string());
                let (bytes, left_out) = match written {
                    Ok(w) => w,
                    Err(e) => {
                        self.session.notify(e.clone(), true);
                        return Err(e);
                    }
                };
                let ext = if ext == "shp" { "zip".to_string() } else { ext };
                let name = format!("{}.{ext}", l.name);
                if left_out > 0 {
                    self.session.notify(format!("{name}: {left_out} features without drawable geometry were left out"), false);
                }
                self.download(&name, bytes);
            }
            "ui.exportRaster" if self.services.download.is_some() => {
                // The web app: encode once and download.
                match rustgis_engine::cmd::raster::geotiff_bytes(&self.session, &Value::Null) {
                    Ok((name, bytes)) => {
                        self.download(&format!("{name}.tif"), bytes);
                    }
                    Err(e) => {
                        self.session.notify(e.clone(), true);
                        return Err(e);
                    }
                }
            }
            "ui.exportRaster" => {
                let name = self
                    .session
                    .active_layer
                    .and_then(|id| self.session.map().layer(id))
                    .map(|l| format!("{}.tif", l.name))
                    .unwrap_or_default();
                if let Some(path) = dialogs::pick_save(&name, &["tif", "tiff"], tl!("GeoTIFF")) {
                    return self.run("share.exportRaster", json!({"path": path}));
                }
            }
            "ui.exportMap" => {
                let name = format!("{}.png", self.session.map().name);
                if let Some(path) = dialogs::pick_save(&name, &["png"], tl!("PNG image")) {
                    return self.run("share.exportMap", json!({"path": path, "dpi": 192}));
                }
            }
            "ui.exportLayer" => {
                let ext = p.get("format").and_then(Value::as_str).unwrap_or("geojson").to_ascii_lowercase();
                let label = dialogs::EXPORT_FORMATS
                    .iter()
                    .find(|(_, e)| *e == ext)
                    .map(|(l, _)| *l)
                    .ok_or_else(|| format!("unknown format '{ext}'"))?;
                let name = self
                    .session
                    .active_layer
                    .and_then(|id| self.session.map().layer(id))
                    .map(|l| format!("{}.{ext}", l.name))
                    .unwrap_or_default();
                if let Some(path) = dialogs::pick_save(&name, &[ext.as_str()], label) {
                    // The format chosen decides: its extension is added unless it was typed (a dot
                    // in the name, as in "U.S. Cities", isn't taken for one).
                    let typed = std::path::Path::new(&path).extension().is_some_and(|e| e.eq_ignore_ascii_case(&ext));
                    let path = if typed { path } else { format!("{path}.{ext}") };
                    return self.run("share.exportLayer", json!({"path": path}));
                }
            }
            "ui.dialog" => {
                let which = p.get("name").and_then(Value::as_str).unwrap_or("");
                self.dialogs.open(which, &self.session)?;
                // The text graphic dialog remembers where the map was clicked.
                if which == "graphicText" {
                    self.dialogs.a = p.get("lon").and_then(Value::as_f64).map(|v| v.to_string()).unwrap_or_default();
                    self.dialogs.b = p.get("lat").and_then(Value::as_f64).map(|v| v.to_string()).unwrap_or_default();
                }
            }
            // Picks the image for a picture graphic whose corners were just dragged.
            "ui.addPicture" => {
                if self.ui.pending_picture.is_none() {
                    return Err("drag the picture's rectangle on the map first".into());
                }
                if let Some(open) = &self.services.open_async {
                    self.ui.pending_layout_picture = false;
                    self.ui.pending_open = None;
                    open("picture");
                } else if let Some(path) = dialogs::pick_open(&["png", "jpg", "jpeg"], tl!("Pictures")) {
                    return self.add_picture(json!({"path": path}));
                } else {
                    self.ui.pending_picture = None;
                }
            }
            "ui.quit" => self.quit_requested = true,
            // Buttons that act through their own click handler (menus, layout inserts).
            "ui.menu" => {}
            other => return Err(format!("unknown command '{other}'")),
        }
        Ok(Value::Null)
    }

    /// Adds the data downloads that have landed (Add Data › From URL). The bytes go to the session
    /// as they are; a download of a project that has since been closed is dropped.
    fn poll_downloads(&mut self) {
        let mut landed = Vec::new();
        self.ui.downloads.retain(|(id, url, p, slot)| match slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Some(r) => {
                landed.push((*id, url.clone(), p.clone(), r));
                false
            }
            None => true,
        });
        for (id, url, mut p, r) in landed {
            match r {
                Ok(bytes) => {
                    if !self.session.download_landed(id, bytes) {
                        self.session.notify(
                            i18n::fmt("{url}: downloaded after its project was closed, so it wasn't added", &[("url", &url)]),
                            false,
                        );
                        continue;
                    }
                    p["id"] = json!(id);
                    p["url"] = json!(url);
                    let zoom = p.get("zoom").and_then(Value::as_bool) == Some(true);
                    if zoom && self.ui.link_zoomed {
                        p["zoom"] = json!(false);
                    }
                    match self.run("layer.addDownload", p) {
                        Ok(r) => {
                            if r.get("zoomed").and_then(Value::as_bool) == Some(true) {
                                self.ui.link_zoomed = true;
                            }
                            self.map.invalidate();
                        }
                        Err(e) => self.session.notify(if e.starts_with(&url) { e } else { format!("{url}: {e}") }, true),
                    }
                }
                Err(e) => {
                    self.session.download_failed(id);
                    self.session.notify(if e.starts_with(&url) { e } else { format!("{url}: {e}") }, true);
                }
            }
        }
    }

    /// Lists a service's layers once its capabilities have arrived.
    fn poll_service(&mut self) {
        let Some(conn) = &mut self.ui.service else { return };
        let Some(r) = conn.slot.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        match r {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                let mut p = json!({"url": conn.url, "text": text});
                if conn.kind != "auto" {
                    p["type"] = json!(conn.kind);
                }
                conn.text = Some(text);
                // Not `self.run`: an error belongs in the dialog, not the notices.
                match self.session.run("service.layers", &p) {
                    Ok(v) => {
                        if let Some(c) = &mut self.ui.service {
                            c.listing = Some(v);
                        }
                    }
                    Err(e) => {
                        if let Some(c) = &mut self.ui.service {
                            c.error = Some(e);
                        }
                    }
                }
            }
            Err(e) => conn.error = Some(if e.starts_with(&conn.url) { e } else { format!("{}: {e}", conn.url) }),
        }
    }

    /// Per-frame work that doesn't draw (theme, fonts, files that arrived).
    pub fn logic(&mut self, ctx: &egui::Context) {
        // Fonts installed last frame are active now.
        self.fonts_pending = false;
        i18n::set_current(self.lang());
        rustgis_engine::plugins::set_language(self.lang().code());
        panes::poll_geolibre(self, ctx);
        self.poll_downloads();
        self.poll_service();
        pluginsui::poll(self, ctx);
        assistantpane::poll(self, ctx);
        self.ensure_script_font();
        // Fonts downloaded by the web app.
        let fonts: Vec<(String, Vec<u8>)> = match &self.services.font_inbox {
            Some(inbox) => std::mem::take(&mut *inbox.lock().unwrap_or_else(|e| e.into_inner())),
            None => Vec::new(),
        };
        for (url, bytes) in fonts {
            use rustgis_engine::render::fonts::{CjkRegion, cjk_font_url, valid_font};
            let region =
                [CjkRegion::SimplifiedChinese, CjkRegion::Japanese, CjkRegion::Korean].into_iter().find(|r| cjk_font_url(*r) == url);
            if bytes.is_empty() || !valid_font(&bytes, 0) {
                // A failed download: say so; picking the language again retries.
                log::error!("{url}: no usable font downloaded");
                // Name the language whose font failed (not the current one), in English: its own
                // script has no font yet.
                let language = i18n::Lang::all()
                    .find(|l| region.is_some_and(|r| Self::script_region(l.script()) == Some(r)))
                    .unwrap_or(self.chosen_lang());
                let msg = i18n::fmt(
                    "Couldn't download the font for {language}; pick the language again to retry.",
                    &[("language", language.english())],
                );
                self.session.notify(msg, true);
                if let Some(r) = region {
                    let name = format!("cjk-{r:?}");
                    self.fonts_requested.retain(|n| *n != name);
                    self.fonts_failed.push(name);
                }
                continue;
            }
            let name = format!("web-{}", url.rsplit('/').next().unwrap_or("font"));
            self.extra_fonts.retain(|(n, _, _)| *n != name);
            self.extra_fonts.push((name, std::sync::Arc::new(egui::FontData::from_owned(bytes)), region));
            self.fonts_dirty = true;
        }
        if self.fonts_dirty {
            self.fonts_dirty = false;
            self.installed_region = self.current_region();
            theme::install_fonts(ctx, &self.extra_fonts, self.installed_region);
            self.fonts_pending = true;
            self.map.invalidate();
        }
        // Files from the async picker or browser drops.
        let arrived: Vec<(String, Vec<u8>)> = match &self.services.inbox {
            Some(inbox) => std::mem::take(&mut *inbox.lock().unwrap_or_else(|e| e.into_inner())),
            None => Vec::new(),
        };
        for (name, bytes) in arrived {
            let _ = self.open_picked(&name, &bytes);
        }
        // Files dropped on the desktop window arrive as paths. (The web shell reads browser drops
        // asynchronously and passes them through the inbox instead.)
        if !cfg!(target_arch = "wasm32") {
            let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
            for f in dropped {
                let p = f.path().display().to_string();
                if p.is_empty() {
                    continue;
                }
                if p.to_ascii_lowercase().ends_with(".rgplugin") {
                    match std::fs::read(f.path()) {
                        Ok(bytes) => pluginsui::received(self, bytes, None),
                        Err(e) => self.session.notify(format!("{p}: {e}"), true),
                    }
                    continue;
                }
                let cmd = if rustgis_engine::project::is_project_file(&p) { "project.open" } else { "layer.addData" };
                let _ = self.run(cmd, json!({"path": p}));
                self.map.invalidate();
            }
        }
        if self.fonts_frames == 0 {
            self.add_cjk_fallback();
            self.installed_region = self.current_region();
            theme::install_fonts(ctx, &self.extra_fonts, self.installed_region);
            self.fonts_pending = true;
        }
        // Switch the drawn language once its font can draw it; until then keep the previous one.
        let chosen = self.chosen_lang();
        if self.shown_lang != chosen {
            if self.can_draw(chosen) {
                self.shown_lang = chosen;
                self.map.invalidate();
            } else if self.fonts_pending {
                ctx.request_repaint();
            }
        }
        if self.ui.dark_follows_system
            && let Some(system) = ctx.system_theme()
        {
            self.ui.dark = system == egui::Theme::Dark;
        }
        if self.applied_dark != Some(self.ui.dark) {
            let t = if self.ui.dark { theme::Tokens::dark() } else { theme::Tokens::light() };
            theme::apply(ctx, &t);
            self.applied_dark = Some(self.ui.dark);
            self.map.invalidate();
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // Fonts installed by `logic` take effect on the next frame.
        if self.fonts_frames < 2 {
            self.fonts_frames += 1;
            ctx.request_repaint();
            return;
        }
        i18n::set_current(self.lang());
        let t = theme::Tokens::get(&ctx);
        chrome::shortcuts(self, &ctx);
        if self.ui.backstage {
            backstage::show(self, ui);
        } else {
            chrome::header(self, ui);
            ribbon::show(self, ui);
            chrome::status_bar(self, ui);
            panes::show(self, ui);
            table::show(self, ui);
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.canvas)).show(ui, |ui| {
                mapview::show(self, ui);
            });
        }
        dialogs::show(self, &ctx);
        pluginsui::show(self, &ctx);
        update::show(self, &ctx);
        chrome::command_search(self, &ctx);
        chrome::about(self, &ctx);
        chrome::notices(self, &ctx);
        if let Some(text) = self.ui.copy_out.take() {
            ctx.copy_text(text);
        }
        if let Some(img) = self.ui.copy_image.take() {
            ctx.copy_image(img);
        }
        if let Some(url) = self.ui.open_url.take() {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        let title = format!("{}{} - RustGIS", self.session.project.name, if self.session.dirty { " •" } else { "" });
        if title != self.sent_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.sent_title = title;
        }
    }
}
