//! Plugins: install, list, enable and remove them, and run their commands against the session.
//!
//! A plugin command (`plugin:<plugin>/<command>`) runs through [`Session::run`] like any other:
//! one undo step, restored if it fails. While it runs, the plugin reaches the project through
//! the requests [`Bridge`] answers: RustGIS commands by id, and a few `host.*` requests for
//! reading layers. What it may run depends on the permissions it declared.

use std::sync::Arc;

use rustgis_plugins::{Host, Permission, Plugin};
use serde_json::{Value, json};

use super::{b, ps, uint};
use crate::{CmdResult, CommandSpec as C, Session};

pub fn commands() -> Vec<C> {
    vec![
        C::new("plugin.list", "Plugins", "Plugins › Manage", list).view(),
        C::new("plugin.install", "Install Plugin", "Plugins › Manage", install)
            .view()
            .params(r#"{"path": "a .rgplugin package, or a plugin folder (for development)"}"#),
        C::new("plugin.uninstall", "Uninstall Plugin", "Plugins › Manage", uninstall).view().params(r#"{"id": plugin id}"#),
        C::new("plugin.enable", "Enable Plugin", "Plugins › Manage", enable).view().params(r#"{"id": plugin id, "enabled"?: bool}"#),
        C::new("plugin.registry", "Browse Plugins", "Plugins › Manage", browse)
            .view()
            .params(r#"{"url"?: "registry.json (default: https://plugins.rustgis.app/registry.json)"}"#),
        C::new("plugin.installFromRegistry", "Install Plugin From Registry", "Plugins › Manage", install_from_registry)
            .view()
            .params(r#"{"id": plugin id, "url"?: registry.json}"#),
    ]
}

/// Loads the installed plugins (desktop and command line); returns what failed to load.
pub fn load_installed() -> Vec<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rustgis_plugins::store::load_installed()
    }
    #[cfg(target_arch = "wasm32")]
    {
        Vec::new()
    }
}

/// Installs a package's bytes: natively it is copied into the plugins folder too; in the
/// browser it is loaded for this session (the web app keeps the package itself).
pub fn install_bytes(bytes: &[u8]) -> Result<Arc<Plugin>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rustgis_plugins::store::install_bytes(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let pkg = rustgis_plugins::Package::from_zip(bytes)?;
        rustgis_plugins::load(pkg, "browser")
    }
}

fn list(_: &mut Session, _: &Value) -> CmdResult {
    #[cfg(not(target_arch = "wasm32"))]
    let folder = rustgis_plugins::store::dir().map(|d| d.display().to_string());
    #[cfg(target_arch = "wasm32")]
    let folder: Option<String> = None;
    Ok(json!({"plugins": rustgis_plugins::list().iter().map(|p| p.info()).collect::<Vec<_>>(), "folder": folder}))
}

fn install(_: &mut Session, p: &Value) -> CmdResult {
    let path = ps(p, "path").map(str::trim).filter(|s| !s.is_empty()).ok_or("path is required")?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let plugin = rustgis_plugins::store::install_path(std::path::Path::new(path))?;
        Ok(plugin.info())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        Err("in the browser, install plugins from the Plugin Manager".into())
    }
}

fn uninstall(_: &mut Session, p: &Value) -> CmdResult {
    let id = ps(p, "id").ok_or("id is required")?;
    #[cfg(not(target_arch = "wasm32"))]
    rustgis_plugins::store::uninstall(id)?;
    #[cfg(target_arch = "wasm32")]
    if !rustgis_plugins::unload(id) {
        return Err(format!("no plugin '{id}'"));
    }
    Ok(json!({"uninstalled": id}))
}

fn enable(_: &mut Session, p: &Value) -> CmdResult {
    let id = ps(p, "id").ok_or("id is required")?;
    let on = b(p, "enabled").unwrap_or(true);
    #[cfg(not(target_arch = "wasm32"))]
    rustgis_plugins::store::set_enabled(id, on)?;
    #[cfg(target_arch = "wasm32")]
    rustgis_plugins::set_enabled(id, on)?;
    Ok(json!({"id": id, "enabled": on}))
}

/// The registry at `url` with each plugin's installed version. Applies its blocklist.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_registry(url: &str) -> Result<Vec<rustgis_plugins::registry::Entry>, String> {
    use rustgis_plugins::registry;
    let text = registry::fetch(url, registry::MAX_REGISTRY_BYTES)?;
    let entries = registry::parse(&String::from_utf8_lossy(&text), url)?;
    // A registry without a blocklist blocks nothing; one that can't be fetched leaves the
    // cached list in place.
    if let Ok(b) = registry::fetch(&registry::blocklist_url(url), 4 * 1024 * 1024) {
        let _ = rustgis_plugins::store::save_blocklist(url, &String::from_utf8_lossy(&b));
    }
    Ok(entries)
}

/// A registry entry with what's installed, for the Plugin Manager and the command line.
pub fn entry_status(e: &rustgis_plugins::registry::Entry) -> Value {
    let installed = rustgis_plugins::get(&e.id).map(|p| p.manifest.version.clone());
    let update = installed.as_deref().is_some_and(|v| rustgis_plugins::manifest::version_cmp(&e.version, v).is_gt());
    let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.insert("installed".into(), json!(installed));
        o.insert("update".into(), json!(update));
        o.insert("blocked".into(), json!(rustgis_plugins::blocked(&e.id, &e.sha256).map(|b| b.reason)));
    }
    v
}

fn browse(_: &mut Session, p: &Value) -> CmdResult {
    let url = ps(p, "url").map(str::trim).filter(|s| !s.is_empty()).unwrap_or(rustgis_plugins::registry::DEFAULT_URL);
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(Value::Array(fetch_registry(url)?.iter().map(entry_status).collect()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = url;
        Err("in the browser, browse plugins in the Plugin Manager".into())
    }
}

/// Checks a downloaded package against its registry entry, then installs it.
pub fn install_checked(e: &rustgis_plugins::registry::Entry, bytes: &[u8]) -> Result<Arc<Plugin>, String> {
    let got = rustgis_plugins::package::sha256_hex(bytes);
    if !got.eq_ignore_ascii_case(&e.sha256) {
        return Err(format!("{}: the download doesn't match the registry (SHA-256 {got}); it wasn't installed", e.name));
    }
    let pkg = rustgis_plugins::Package::from_zip(bytes)?;
    if pkg.manifest.id != e.id {
        return Err(format!("{}: the package is '{}', not '{}'", e.name, pkg.manifest.id, e.id));
    }
    install_bytes(bytes)
}

fn install_from_registry(_: &mut Session, p: &Value) -> CmdResult {
    let id = ps(p, "id").ok_or("id is required")?;
    let url = ps(p, "url").map(str::trim).filter(|s| !s.is_empty()).unwrap_or(rustgis_plugins::registry::DEFAULT_URL);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let entries = fetch_registry(url)?;
        let e = entries.iter().find(|e| e.id == id).ok_or_else(|| format!("the registry has no plugin '{id}'"))?;
        let bytes = rustgis_plugins::registry::fetch(&e.package, rustgis_plugins::package::MAX_PACKAGE)?;
        Ok(install_checked(e, &bytes)?.info())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, url);
        Err("in the browser, install plugins from the Plugin Manager".into())
    }
}

/// A `'static` copy of a label (undo entries keep `&'static str`), shared between calls.
pub(crate) fn leak(s: &str) -> &'static str {
    static STRINGS: std::sync::Mutex<Option<std::collections::HashSet<&'static str>>> = std::sync::Mutex::new(None);
    let Ok(mut set) = STRINGS.lock() else { return Box::leak(s.to_string().into_boxed_str()) };
    let set = set.get_or_insert_with(Default::default);
    if let Some(&have) = set.get(s) {
        return have;
    }
    let new: &'static str = Box::leak(s.to_string().into_boxed_str());
    set.insert(new);
    new
}

/// Commands reserved to the user (see [`user_only`]); plugin management is too.
const NEVER: &[&str] = &[
    "gp.installGeolibre",
    "gp.setGeolibre",
    "script.run",
    "task.run",
    "model.run",
    "project.new",
    "project.open",
    "project.openText",
    "project.sample",
    "project.fromTemplate",
    "edit.undo",
    "edit.redo",
];

/// Commands that write files without naming one in their parameters.
const WRITES_FILES: &[&str] = &["project.save", "project.saveAs"];

/// Parameter keys that name a file, folder or address. (`data` and `text` carry content, which
/// plugins may pass: `layer.addDataBytes`, `map.import` from text.)
const PLACE_KEYS: &[&str] = &["path", "folder", "dir", "file", "url", "uri", "source", "output", "connection", "database"];

/// The command running an SQL query through DuckDB, which can read any file and web address.
pub const SQL: &str = "layer.addSql";

/// Commands that read a database, which can be a PostgreSQL server's as well as a file.
const DATABASES: &[&str] = &[SQL, "layer.addDatabase", "info.databaseTables"];

/// What a command reaches beyond the project when run with some parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Reads or writes files, or runs a native program (the GeoLibre suite) that does.
    Files,
    /// Fetches from an address (web tiles).
    Network,
}

/// Whether `id` is reserved to the user: plugin management, the GeoLibre runner, the user's own
/// automation (scripts, tasks, models run with the user's trust), and replacing the project or
/// stepping through its undo history. Plugins never run these; the AI assistant asks first.
pub fn user_only(id: &str) -> bool {
    id.starts_with("plugin.") || NEVER.contains(&id)
}

/// What running `spec` with `params` reaches beyond the project, if anything: by what the
/// parameters name, what the command's parameters can name, and what it is.
pub fn reaches(spec: &C, params: &Value) -> Option<Reach> {
    // External tools (the GeoLibre suite) run a native program that reads files a parameter
    // names, including bare file names.
    if spec.id == "gp.run" && ps(params, "tool").and_then(rustgis_analysis::tool).is_some_and(super::gp::is_geolibre_tool) {
        return Some(Reach::Files);
    }
    // Commands that fetch from an address read and write no files, so `files` doesn't come into
    // it (their addresses would look like paths to the check below).
    if super::tiles::NETWORK.contains(&spec.id) {
        return Some(Reach::Network);
    }
    if DATABASES.contains(&spec.id) {
        return Some(Reach::Files);
    }
    let files = super::items::names_place(params, PLACE_KEYS, 0)
        || PLACE_KEYS.iter().any(|k| spec.params.contains(&format!("\"{k}\"")))
        || WRITES_FILES.contains(&spec.id)
        || ["share.", "offline.", "cache.", "notebook."].iter().any(|p| spec.id.starts_with(p));
    files.then_some(Reach::Files)
}

/// Why a plugin may not run `id` with `params`, if it may not.
fn refused(plugin: &Plugin, spec: &C, params: &Value) -> Option<String> {
    let m = &plugin.manifest;
    if user_only(spec.id) {
        return Some(format!("plugins can't run '{}'", spec.id));
    }
    // An SQL query reads files and web addresses alike, and a database is a file or a server.
    if DATABASES.contains(&spec.id) && !(m.has(Permission::Files) && m.has(Permission::Network)) {
        return Some(format!("{} needs the 'files' and 'network' permissions to run '{}'", m.name, spec.id));
    }
    match reaches(spec, params) {
        Some(Reach::Files)
            if spec.id == "gp.run"
                && !m.has(Permission::Files)
                && ps(params, "tool").and_then(rustgis_analysis::tool).is_some_and(super::gp::is_geolibre_tool) =>
        {
            return Some(format!("{} needs the 'files' permission to run GeoLibre tools", m.name));
        }
        // The address could carry what the plugin read out of the project.
        Some(Reach::Network) if !m.has(Permission::Network) => {
            return Some(format!("{} needs the 'network' permission to run '{}'", m.name, spec.id));
        }
        Some(Reach::Files) if !m.has(Permission::Files) => {
            return Some(format!("{} needs the 'files' permission to run '{}'", m.name, spec.id));
        }
        _ => {}
    }
    if spec.mutates && !m.has(Permission::Edit) {
        return Some(format!("{} needs the 'edit' permission to run '{}'", m.name, spec.id));
    }
    None
}

/// Answers a running plugin's requests.
struct Bridge<'a> {
    s: &'a mut Session,
    plugin: Arc<Plugin>,
}

impl Bridge<'_> {
    fn need(&self, p: Permission, what: &str) -> Result<(), String> {
        if self.plugin.manifest.has(p) {
            Ok(())
        } else {
            Err(format!("{} needs the '{}' permission to {what}", self.plugin.manifest.name, p.code()))
        }
    }

    fn layers(&self) -> Value {
        let active = self.s.active_layer;
        Value::Array(
            self.s
                .map()
                .layers
                .iter()
                .map(|l| {
                    json!({
                        "id": l.id, "name": l.name, "raster": l.is_raster(), "visible": l.visible,
                        "geometry": format!("{:?}", l.dataset.geometry_type), "features": l.dataset.features.len(),
                        "selected": l.selection.len(), "active": Some(l.id) == active,
                        "fields": l.dataset.fields.iter().map(|f| json!({"name": f.name, "type": format!("{:?}", f.ty)})).collect::<Vec<_>>(),
                    })
                })
                .collect(),
        )
    }

    /// A layer's features as GeoJSON: all of them, or its selection (`"selected": true`), at
    /// most `limit`.
    fn features(&self, p: &Value) -> CmdResult {
        let id = self.s.layer_id(p)?;
        let l = self.s.map().layer(id).ok_or("no such layer")?;
        if l.is_raster() {
            return Err(format!("{} is a raster layer", l.name));
        }
        let limit = uint(p, "limit").and_then(|n| usize::try_from(n).ok()).unwrap_or(usize::MAX);
        let rows: Vec<usize> = if b(p, "selected") == Some(true) { l.selection.clone() } else { (0..l.dataset.features.len()).collect() };
        let ds = rustgis_data::Dataset {
            name: l.name.clone(),
            geometry_type: l.dataset.geometry_type,
            fields: l.dataset.fields.clone(),
            features: rows.iter().take(limit).filter_map(|&i| l.dataset.features.get(i).cloned()).collect(),
            source: None,
            windowed: false,
        };
        let mut gj = rustgis_data::io::to_geojson(&ds);
        // The row of each feature, for selecting or editing it afterwards.
        if let Some(fs) = gj.get_mut("features").and_then(Value::as_array_mut) {
            for (f, row) in fs.iter_mut().zip(rows.iter()) {
                if let Some(o) = f.as_object_mut() {
                    o.insert("row".into(), json!(row));
                }
            }
        }
        Ok(gj)
    }

    fn add_layer(&mut self, p: &Value) -> CmdResult {
        self.need(Permission::Edit, "add layers")?;
        let name = ps(p, "name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&self.plugin.manifest.name).to_string();
        let gj = p.get("geojson").ok_or("geojson is required")?;
        let (ds, report) = rustgis_data::io::read_geojson(&gj.to_string(), &name).map_err(|e| e.to_string())?;
        let id = super::layer::add_dataset(self.s, ds);
        Ok(json!({"layer": id, "skipped": report.skipped}))
    }

    fn command(&mut self, id: &str, params: &Value) -> CmdResult {
        if id.starts_with(rustgis_plugins::PREFIX) {
            return Err("plugins can't run other plugins' commands".into());
        }
        let spec = self.s.registry.get(id).ok_or_else(|| format!("unknown command '{id}'"))?;
        if let Some(why) = refused(&self.plugin, spec, params) {
            return Err(why);
        }
        let (enabled, run, label) = (spec.enabled, spec.run, spec.label);
        let named = params.get("layer").and_then(|_| self.s.layer_id(params).ok());
        let active = self.s.active_layer;
        if named.is_some() {
            self.s.active_layer = named;
        }
        let blocked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| enabled(self.s))).unwrap_or(Some("unavailable"));
        self.s.active_layer = active;
        if let Some(why) = blocked {
            return Err(format!("{label}: {why}"));
        }
        // Without `network`, no command may bring in a tile address (a map file read from text
        // carries its tile layers), or change where an existing layer fetches from: any such
        // layer is taken out again. Where a layer fetches from is decided by its address, kind,
        // subdomains and row order. Its name, attribution and opacity never reach a server, and
        // its tile size reaches one only as `{width}`/`{height}` (256 or 512).
        let fetched = |t: &rustgis_map::TileLayer| (t.url.clone(), t.kind, t.subdomains.clone(), t.tms);
        let tile_sources =
            |s: &Session| -> std::collections::HashSet<_> { s.project.maps.iter().flat_map(|m| &m.tiles).map(fetched).collect() };
        let before = (!self.plugin.manifest.has(Permission::Network)).then(|| tile_sources(self.s));
        // The plugin command took the undo snapshot: the commands it runs are part of it.
        let mut res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(self.s, params)))
            .unwrap_or_else(|_| Err(format!("{label} failed unexpectedly")));
        if let Some(before) = before {
            let mut dropped = false;
            for m in &mut self.s.project.maps {
                let n = m.tiles.len();
                m.tiles.retain(|t| before.contains(&fetched(t)));
                dropped |= m.tiles.len() != n;
            }
            if dropped {
                res = Err(format!("{} needs the 'network' permission to add tile layers", self.plugin.manifest.name));
            }
        }
        self.s.fix_active_layer();
        res
    }
}

impl Host for Bridge<'_> {
    fn call(&mut self, op: &str, params: &Value) -> Result<Value, String> {
        match op {
            "host.info" => Ok(json!({"rustgis": rustgis_plugins::manifest::RUSTGIS_VERSION, "abi": rustgis_plugins::manifest::ABI,
                "plugin": self.plugin.manifest.id, "permissions": self.plugin.manifest.permissions})),
            "host.layers" => Ok(self.layers()),
            "host.features" => self.features(params),
            "host.view" => {
                let c = &self.s.map().camera;
                let center = c.center_lonlat();
                Ok(json!({"center": [center.x, center.y], "scale": c.scale(), "projection": self.s.map().projection.code()}))
            }
            "host.notify" => {
                let text: String = ps(params, "text").unwrap_or_default().chars().take(500).collect();
                let error = b(params, "error").unwrap_or(false);
                self.s.notify(format!("{}: {text}", self.plugin.manifest.name), error);
                Ok(Value::Null)
            }
            "host.addLayer" => self.add_layer(params),
            _ if op.starts_with("host.") => Err(format!("unknown request '{op}'")),
            _ => self.command(op, params),
        }
    }

    fn log(&mut self, text: &str) {
        log::info!("{}: {text}", self.plugin.manifest.id);
    }
}

impl Session {
    /// Runs a plugin command (`plugin:<plugin>/<command>`) as one undoable step.
    pub(crate) fn run_plugin(&mut self, id: &str, params: &Value) -> CmdResult {
        let Some(cmd) = rustgis_plugins::command(id) else {
            return Err(match rustgis_plugins::split_id(id).and_then(|(p, _)| rustgis_plugins::get(p)) {
                Some(p) if !p.enabled => format!("the plugin {} is disabled", p.manifest.name),
                _ => format!("unknown command '{id}'"),
            });
        };
        let label = leak(&cmd.decl.label);
        let mutates = cmd.mutates();
        let plugin = cmd.plugin.clone();
        self.guarded(label, mutates, |s| rustgis_plugins::run_command(&cmd, &mut Bridge { s, plugin }, params))
    }
}
