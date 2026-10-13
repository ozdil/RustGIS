//! Dialogs (Select By Attributes, Go To XY, bookmarks, fields, definition query, rename) and
//! native file pickers.

use egui::{Align2, vec2};
use rustgis_engine::Session;
use serde_json::{Value, json};

use crate::RustGisApp;
use crate::theme::{Tokens, semibold};

#[derive(Default)]
pub struct Dialogs {
    pub open: Option<&'static str>,
    pub layer: String,
    pub text: String,
    pub a: String,
    pub b: String,
    pub c: String,
    pub mode: &'static str,
    pub flag: bool,
    pub error: Option<String>,
    pub info: Option<String>,
    /// Choices found for the dialog (Add From Database's tables).
    pub list: Vec<String>,
}

impl Dialogs {
    pub fn open(&mut self, which: &str, s: &Session) -> Result<(), String> {
        let active = s.active_layer.and_then(|id| s.map().layer(id));
        let name: &'static str = match which {
            "selectByAttributes" => "selectByAttributes",
            "goToXY" => "goToXY",
            "bookmark" => "bookmark",
            "addField" => "addField",
            "calculateField" => "calculateField",
            "definitionQuery" => "definitionQuery",
            "renameLayer" => "renameLayer",
            "coordinateSystem" => "coordinateSystem",
            "graphicText" => "graphicText",
            "camera" => "camera",
            "addTiles" => "addTiles",
            "addCsv" => "addCsv",
            "addDatabase" => "addDatabase",
            "exportLayer" => "exportLayer",
            other => return Err(format!("unknown dialog '{other}'")),
        };
        if matches!(name, "addField" | "calculateField" | "definitionQuery" | "renameLayer" | "exportLayer") && active.is_none() {
            return Err("select a layer in the Layers pane first".into());
        }
        if name == "exportLayer" && active.is_some_and(|l| l.is_raster()) {
            return Err("Export Features needs a vector layer (Data › Export Raster writes a raster)".into());
        }
        if name == "selectByAttributes" && s.map().layers.is_empty() {
            return Err("the map has no layers".into());
        }
        *self = Dialogs { open: Some(name), mode: "new", ..Dialogs::default() };
        self.layer = active.or_else(|| s.map().layers.first()).map(|l| l.name.clone()).unwrap_or_default();
        match name {
            "coordinateSystem" => {
                self.text = s.map().projection.code();
                // An EPSG code goes in the code box; presets are picked from the list.
                let is_code = self.text.to_ascii_lowercase().starts_with("epsg");
                self.a = if is_code { self.text.trim_start_matches(|c: char| !c.is_ascii_digit()).to_string() } else { String::new() };
            }
            "goToXY" => {
                let c = s.map().center_lonlat();
                self.a = format!("{:.4}", c.x);
                self.b = format!("{:.4}", c.y);
            }
            "bookmark" => self.text = format!("Bookmark {}", s.map().bookmarks.len() + 1),
            "camera" => {
                let c = s.map().center_lonlat();
                self.a = format!("{:.5}", c.x);
                self.b = format!("{:.5}", c.y);
                self.c = format!("{:.0}", s.scale());
            }
            "graphicText" => {
                self.text = "Text".into();
                self.c = "14".into();
            }
            "addField" => self.b = "Double".into(),
            "calculateField" => self.a = active.and_then(|l| l.dataset.fields.last()).map(|f| f.name.clone()).unwrap_or_default(),
            "definitionQuery" => self.text = active.and_then(|l| l.definition_query.clone()).unwrap_or_default(),
            "renameLayer" => self.text = active.map(|l| l.name.clone()).unwrap_or_default(),
            "exportLayer" => self.mode = "geojson",
            _ => {}
        }
        Ok(())
    }
}

pub fn show(app: &mut RustGisApp, ctx: &egui::Context) {
    let Some(which) = app.dialogs.open else { return };
    let title = match which {
        "selectByAttributes" => "Select By Attributes",
        "goToXY" => "Go To XY",
        "bookmark" => "New Bookmark",
        "addField" => "Add Field",
        "calculateField" => "Calculate Field",
        "definitionQuery" => "Definition Query",
        "coordinateSystem" => "Coordinate System",
        "graphicText" => "Add Text",
        "camera" => "Camera",
        "addTiles" => "Add Tile Layer",
        "addCsv" => "Add Delimited Text",
        "addDatabase" => "Add From Database",
        "exportLayer" => "Export Features",
        _ => "Rename Layer",
    };
    let t = Tokens::get(ctx);
    let mut open = true;
    let mut close = false;
    egui::Window::new(tl!(title)).id(egui::Id::new("dialog")).open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
        ui.set_width(match which {
            "coordinateSystem" => 660.0,
            "addTiles" => 560.0,
            _ => 440.0,
        });
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        match which {
            "selectByAttributes" => close = select_by_attributes(app, ui, &t),
            "goToXY" => {
                egui::Grid::new("xy").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                    ui.label(tl!("Longitude (X)"));
                    ui.add(egui::TextEdit::singleline(&mut app.dialogs.a).desired_width(200.0));
                    ui.end_row();
                    ui.label(tl!("Latitude (Y)"));
                    ui.add(egui::TextEdit::singleline(&mut app.dialogs.b).desired_width(200.0));
                    ui.end_row();
                    ui.label(tl!("Scale 1:"));
                    ui.add(egui::TextEdit::singleline(&mut app.dialogs.c).hint_text(tl!("keep current")).desired_width(200.0));
                    ui.end_row();
                });
                close = buttons(app, ui, "Go", |app| {
                    let mut p = json!({"lon": app.dialogs.a.trim(), "lat": app.dialogs.b.trim()});
                    if let Ok(s) = app.dialogs.c.trim().replace(',', "").parse::<f64>() {
                        p["scale"] = json!(s);
                    }
                    app.run("map.goToXY", p).map(|_| ())
                });
            }
            "coordinateSystem" => close = coordinate_system(app, ui, &t),
            "addTiles" => close = add_tiles(app, ui),
            "addCsv" => close = add_csv(app, ui),
            "addDatabase" => close = add_database(app, ui),
            "exportLayer" => close = export_layer(app, ui),
            "camera" => {
                egui::Grid::new("cam").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                    for (label, v) in [("Longitude", &mut app.dialogs.a), ("Latitude", &mut app.dialogs.b), ("Scale 1:", &mut app.dialogs.c)] {
                        ui.label(tl!(label));
                        ui.add(egui::TextEdit::singleline(v).desired_width(160.0));
                        ui.end_row();
                    }
                    ui.label(tl!("Projection"));
                    ui.label(app.session.map().projection.label());
                    ui.end_row();
                });
                close = buttons(app, ui, "Apply", |app| {
                    // Empty keeps the current value; anything else must be a number.
                    let n = |s: &str, what: &str| -> Result<Option<f64>, String> {
                        let s = s.trim();
                        if s.is_empty() {
                            return Ok(None);
                        }
                        s.parse::<f64>().ok().filter(|v| v.is_finite()).map(Some).ok_or_else(|| crate::i18n::fmt("{what} must be a number", &[("what", tl!(what))]))
                    };
                    let (lon, lat, scale) = (n(&app.dialogs.a, "Longitude")?, n(&app.dialogs.b, "Latitude")?, n(&app.dialogs.c, "Scale")?);
                    app.run("map.camera", json!({"lon": lon, "lat": lat, "scale": scale})).map(|_| ())
                });
            }
            "graphicText" => {
                ui.label(tl!("Text"));
                ui.add(egui::TextEdit::multiline(&mut app.dialogs.text).desired_rows(2).desired_width(f32::INFINITY));
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tl!("Dynamic text:")).small().weak());
                    for (token, what) in rustgis_engine::map::graphics::TOKENS {
                        if ui.small_button(*token).on_hover_text(tl!(*what)).clicked() {
                            app.dialogs.text.push_str(token);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(tl!("Size"));
                    ui.add(egui::TextEdit::singleline(&mut app.dialogs.c).desired_width(50.0));
                });
                close = buttons(app, ui, "Add", |app| {
                    let (lon, lat) = (app.dialogs.a.parse::<f64>().unwrap_or(f64::NAN), app.dialogs.b.parse::<f64>().unwrap_or(f64::NAN));
                    let size = app.dialogs.c.trim().parse::<f64>().unwrap_or(14.0);
                    let r = app.run("graphics.add", json!({"shape": "text", "points": [[lon, lat]], "text": app.dialogs.text, "fontSize": size}));
                    if r.is_ok() && !app.session.keep_tool {
                        app.session.tool = rustgis_engine::Tool::Explore;
                    }
                    r.map(|_| ())
                });
            }
            "bookmark" => {
                ui.label(tl!("Name"));
                ui.add(egui::TextEdit::singleline(&mut app.dialogs.text).desired_width(f32::INFINITY));
                close = buttons(app, ui, "Create", |app| app.run("bookmark.add", json!({"name": app.dialogs.text})).map(|_| ()));
            }
            "addField" => {
                egui::Grid::new("af").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                    ui.label(tl!("Field name"));
                    ui.add(egui::TextEdit::singleline(&mut app.dialogs.a).desired_width(220.0));
                    ui.end_row();
                    ui.label(tl!("Data type"));
                    egui::ComboBox::from_id_salt("af_type").selected_text(app.dialogs.b.clone()).show_ui(ui, |ui| {
                        for ty in ["Long", "Double", "Text", "Boolean"] {
                            ui.selectable_value(&mut app.dialogs.b, ty.to_string(), ty);
                        }
                    });
                    ui.end_row();
                });
                close = buttons(app, ui, "Add", |app| app.run("edit.addField", json!({"name": app.dialogs.a, "type": app.dialogs.b})).map(|_| ()));
            }
            "calculateField" => {
                let fields: Vec<String> = app.session.active_layer.and_then(|id| app.session.map().layer(id)).map(|l| l.dataset.fields.iter().map(|f| f.name.clone()).collect()).unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(tl!("Field"));
                    egui::ComboBox::from_id_salt("cf_field").selected_text(app.dialogs.a.clone()).width(220.0).show_ui(ui, |ui| {
                        for f in &fields {
                            ui.selectable_value(&mut app.dialogs.a, f.clone(), f);
                        }
                    });
                });
                ui.label(format!("{} =", app.dialogs.a));
                ui.add(egui::TextEdit::singleline(&mut app.dialogs.text).hint_text(tl!("e.g.  pop_est / 1000000   or   'Africa'   or   name")).desired_width(f32::INFINITY));
                ui.label(egui::RichText::new(tl!("A value, a field, or two of them joined by + − × ÷. Applies to the selected rows, or every row when none is selected.")).small().weak());
                close = buttons(app, ui, "Calculate", |app| app.run("edit.calculateField", json!({"field": app.dialogs.a, "expression": app.dialogs.text})).map(|_| ()));
            }
            "definitionQuery" => {
                ui.label(egui::RichText::new(crate::i18n::fmt("Only features matching this expression draw in {layer}.", &[("layer", &app.dialogs.layer)])).weak());
                ui.add(egui::TextEdit::multiline(&mut app.dialogs.text).hint_text(tl!("e.g. continent = 'Europe'")).desired_rows(3).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
                field_chips(app, ui);
                close = buttons(app, ui, "Apply", |app| app.run("layer.definitionQuery", json!({"where": app.dialogs.text})).map(|_| ()));
            }
            _ => {
                ui.label(tl!("Name"));
                ui.add(egui::TextEdit::singleline(&mut app.dialogs.text).desired_width(f32::INFINITY));
                close = buttons(app, ui, "Rename", |app| app.run("layer.rename", json!({"name": app.dialogs.text})).map(|_| ()));
            }
        }
        if let Some(e) = &app.dialogs.error {
            ui.label(egui::RichText::new(e).color(t.red));
        }
        if let Some(i) = &app.dialogs.info {
            ui.label(egui::RichText::new(i).color(t.green));
        }
    });
    if !open || close {
        app.dialogs.open = None;
    }
}

/// The formats Export Features writes: (name, extension). Format names aren't translated.
pub const EXPORT_FORMATS: &[(&str, &str)] = &[
    ("GeoJSON", "geojson"),
    ("GeoPackage", "gpkg"),
    ("GeoParquet", "parquet"),
    ("Shapefile", "shp"),
    ("FlatGeobuf", "fgb"),
    ("CSV", "csv"),
    ("KML", "kml"),
    ("KMZ", "kmz"),
    ("GPX", "gpx"),
];

/// Export Features: the active layer, in a format picked here (a file's type isn't changed by
/// the filter chosen in a save dialog, and the web app has no save dialog).
fn export_layer(app: &mut RustGisApp, ui: &mut egui::Ui) -> bool {
    let web = app.services.download.is_some();
    egui::Grid::new("export_layer").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        ui.label(tl!("Layer"));
        ui.label(&app.dialogs.layer);
        ui.end_row();
        ui.label(tl!("Format"));
        let shown = EXPORT_FORMATS.iter().find(|(_, e)| *e == app.dialogs.mode).map(|(l, _)| *l).unwrap_or("GeoJSON");
        egui::ComboBox::from_id_salt("export_format").selected_text(shown).width(220.0).show_ui(ui, |ui| {
            // GeoPackages are written with SQLite, which the web app doesn't have.
            for (label, ext) in EXPORT_FORMATS.iter().filter(|(_, e)| !(web && *e == "gpkg")) {
                ui.selectable_value(&mut app.dialogs.mode, *ext, *label);
            }
        });
        ui.end_row();
    });
    if app.dialogs.mode == "shp" && web {
        ui.label(egui::RichText::new(tl!("The Shapefile's files download as one zip.")).small().weak());
    }
    buttons(app, ui, if web { "Download" } else { "Export…" }, |app| {
        app.run("ui.exportLayer", json!({"format": app.dialogs.mode})).map(|_| ())
    })
}

/// Add Delimited Text: which columns hold the coordinates (or WKT geometry), their coordinate
/// system and the separator, then the file.
fn add_csv(app: &mut RustGisApp, ui: &mut egui::Ui) -> bool {
    if app.dialogs.mode == "new" {
        app.dialogs.mode = "auto";
    }
    ui.label(egui::RichText::new(tl!("Leave a box empty to find it from the file: x/y columns named like longitude/latitude or easting/northing, or a WKT column named like geometry.")).weak());
    egui::Grid::new("csv").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        for (label, hint, v) in [
            ("X column", "longitude, easting…", &mut app.dialogs.a),
            ("Y column", "latitude, northing…", &mut app.dialogs.b),
            ("WKT column", "wkt, geometry…", &mut app.dialogs.c),
            ("Coordinate system", "EPSG:4326", &mut app.dialogs.text),
        ] {
            ui.label(tl!(label));
            ui.add(egui::TextEdit::singleline(v).hint_text(tl!(hint)).desired_width(240.0));
            ui.end_row();
        }
        ui.label(tl!("Separator"));
        let seps: [(&'static str, &str); 5] = [("auto", "Automatic"), (",", "Comma"), (";", "Semicolon"), ("\t", "Tab"), ("|", "Pipe")];
        let shown = seps.iter().find(|(c, _)| *c == app.dialogs.mode).map(|(_, l)| tl!(*l).to_string()).unwrap_or_default();
        egui::ComboBox::from_id_salt("csv_sep").selected_text(shown).width(240.0).show_ui(ui, |ui| {
            for (code, label) in seps {
                ui.selectable_value(&mut app.dialogs.mode, code, tl!(label));
            }
        });
        ui.end_row();
    });
    buttons(app, ui, "Choose File…", |app| {
        let mut p = json!({});
        for (key, v) in [("x", &app.dialogs.a), ("y", &app.dialogs.b), ("wkt", &app.dialogs.c), ("crs", &app.dialogs.text)] {
            if !v.trim().is_empty() {
                p[key] = json!(v.trim());
            }
        }
        if app.dialogs.mode != "auto" {
            p["delimiter"] = json!(app.dialogs.mode);
        }
        app.run("ui.addDelimited", p).map(|_| ())
    })
}

/// Add From Database: a PostgreSQL/PostGIS connection (or a DuckDB file), its password (kept for
/// the session only) and one table or all of them.
fn add_database(app: &mut RustGisApp, ui: &mut egui::Ui) -> bool {
    ui.label(egui::RichText::new(tl!("A PostgreSQL connection (host=… port=… dbname=… user=…, or a postgresql:// address) or a DuckDB file. Read through DuckDB; the password is kept until RustGIS closes, never in the project.")).weak());
    // A bare connection string is a PostgreSQL one.
    let database = |a: &str| {
        let a = a.trim();
        if a.contains('=') && !a.contains("://") && !a.to_ascii_lowercase().starts_with("postgres:") {
            format!("postgres:{a}")
        } else {
            a.to_string()
        }
    };
    let mut list = false;
    egui::Grid::new("database").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        ui.label(tl!("Connection"));
        ui.add(egui::TextEdit::singleline(&mut app.dialogs.a).hint_text("host=localhost dbname=gis user=postgres").desired_width(280.0));
        ui.end_row();
        ui.label(tl!("Password"));
        ui.add(
            egui::TextEdit::singleline(&mut app.dialogs.b).password(true).hint_text(tl!("none, or from PGPASSWORD")).desired_width(280.0),
        );
        ui.end_row();
        ui.label(tl!("Table"));
        ui.horizontal(|ui| {
            let shown = if app.dialogs.c.is_empty() { tl!("All tables with geometry").to_string() } else { app.dialogs.c.clone() };
            egui::ComboBox::from_id_salt("db_table").selected_text(shown).width(190.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut app.dialogs.c, String::new(), tl!("All tables with geometry"));
                for t in app.dialogs.list.clone() {
                    ui.selectable_value(&mut app.dialogs.c, t.clone(), t);
                }
            });
            list = ui.button(tl!("List Tables")).clicked();
        });
        ui.end_row();
        ui.label(tl!("Coordinate system"));
        ui.add(egui::TextEdit::singleline(&mut app.dialogs.text).hint_text(tl!("of a geometry that doesn't say")).desired_width(280.0));
        ui.end_row();
    });
    if list {
        let p = json!({"database": database(&app.dialogs.a), "password": app.dialogs.b});
        match app.run("info.databaseTables", p) {
            Ok(v) => {
                app.dialogs.list = v["layers"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(str::to_string)).collect();
                app.dialogs.error = None;
                app.dialogs.info = Some(crate::i18n::fmt("{n} tables with geometry", &[("n", &app.dialogs.list.len().to_string())]));
            }
            Err(e) => {
                app.dialogs.info = None;
                app.dialogs.error = Some(e);
            }
        }
    }
    buttons(app, ui, "Add", |app| {
        let mut p = json!({"database": database(&app.dialogs.a), "password": app.dialogs.b});
        for (key, v) in [("layer", &app.dialogs.c), ("crs", &app.dialogs.text)] {
            if !v.trim().is_empty() {
                p[key] = json!(v.trim());
            }
        }
        app.run("layer.addDatabase", p).map(|_| ())
    })
}

/// Add Tile Layer: an XYZ/TMS/WMS template, vector tiles or a MapLibre style by address.
fn add_tiles(app: &mut RustGisApp, ui: &mut egui::Ui) -> bool {
    use rustgis_engine::map::TileKind;
    ui.label(egui::RichText::new(tl!("A tile service's address: raster tiles with {z}/{x}/{y} (or {-y}, {q}, {bbox-epsg-3857} for WMS), vector tiles (.pbf) or TileJSON, a MapLibre style, a PMTiles archive (a web address, or a file on this computer), or an MBTiles file on this computer.")).weak());
    egui::Grid::new("tiles").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        ui.label(tl!("Address"));
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.dialogs.text)
                    .hint_text("https://tile.openstreetmap.org/{z}/{x}/{y}.png")
                    .desired_width(if cfg!(target_arch = "wasm32") { 400.0 } else { 320.0 }),
            );
            // A PMTiles or MBTiles file on this computer (the web app reads PMTiles archives from
            // web addresses, and MBTiles files through Add Data).
            if !cfg!(target_arch = "wasm32")
                && ui.button(tl!("Tile Archive File…")).clicked()
                && let Some(path) = pick_open(&["pmtiles", "mbtiles"], tl!("Tile archive"))
            {
                app.dialogs.mode = if path.to_ascii_lowercase().ends_with(".mbtiles") { "mbtiles" } else { "pmtiles" };
                app.dialogs.text = path;
            }
        });
        ui.end_row();
        ui.label(tl!("Type"));
        let guessed = TileKind::guess(app.dialogs.text.trim());
        let shown = match app.dialogs.mode {
            "auto" | "new" => crate::i18n::fmt("Automatic ({kind})", &[("kind", tl!(guessed.label()))]),
            code => TileKind::from_code(code).map(|k| tl!(k.label()).to_string()).unwrap_or_default(),
        };
        egui::ComboBox::from_id_salt("tiles_kind").selected_text(shown).width(260.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut app.dialogs.mode, "auto", tl!("Automatic"));
            for k in TileKind::ALL {
                ui.selectable_value(&mut app.dialogs.mode, k.code(), tl!(k.label()));
            }
        });
        ui.end_row();
        ui.label(tl!("Name"));
        ui.add(egui::TextEdit::singleline(&mut app.dialogs.a).hint_text(tl!("from the address")).desired_width(400.0));
        ui.end_row();
        ui.label(tl!("Attribution"));
        ui.add(egui::TextEdit::singleline(&mut app.dialogs.b).hint_text("© OpenStreetMap contributors").desired_width(400.0));
        ui.end_row();
        ui.label(tl!("Maximum zoom"));
        ui.add(egui::TextEdit::singleline(&mut app.dialogs.c).hint_text(tl!("from the service")).desired_width(60.0));
        ui.end_row();
    });
    ui.checkbox(&mut app.dialogs.flag, tl!("Overlay (a transparent layer kept on top, such as labels or roads)"));
    buttons(app, ui, "Add", |app| {
        let mut p = json!({"url": app.dialogs.text.trim(), "overlay": app.dialogs.flag});
        if !matches!(app.dialogs.mode, "auto" | "new") {
            p["kind"] = json!(app.dialogs.mode);
        }
        for (key, v) in [("name", &app.dialogs.a), ("attribution", &app.dialogs.b)] {
            if !v.trim().is_empty() {
                p[key] = json!(v.trim());
            }
        }
        if let Ok(z) = app.dialogs.c.trim().parse::<f64>() {
            p["maxZoom"] = json!(z);
        }
        app.run("map.addTiles", p).map(|_| ())
    })
}

/// OK/Cancel row; returns true when the dialog should close.
fn buttons(app: &mut RustGisApp, ui: &mut egui::Ui, ok: &str, f: impl FnOnce(&mut RustGisApp) -> Result<(), String>) -> bool {
    let mut close = false;
    ui.add_space(4.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        let t = Tokens::get(ui.ctx());
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
        if crate::widgets::dialog_button(ui, tl!(ok), Some(t.accent), vec2(84.0, 28.0)).clicked() || enter {
            match f(app) {
                Ok(()) => close = true,
                Err(e) => app.dialogs.error = Some(e),
            }
        }
        if crate::widgets::dialog_button(ui, tl!("Cancel"), None, vec2(84.0, 28.0)).clicked() {
            close = true;
        }
    });
    close
}

fn field_chips(app: &mut RustGisApp, ui: &mut egui::Ui) {
    let fields: Vec<String> = app
        .session
        .map()
        .layer_by_name(&app.dialogs.layer)
        .map(|l| l.dataset.fields.iter().map(|f| f.name.clone()).collect())
        .unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(tl!("Fields:")).small().weak());
        for f in fields.iter().take(24) {
            if ui.small_button(f).clicked() {
                if !app.dialogs.text.is_empty() && !app.dialogs.text.ends_with(' ') {
                    app.dialogs.text.push(' ');
                }
                app.dialogs.text.push_str(f);
                app.dialogs.text.push(' ');
            }
        }
    });
    ui.horizontal(|ui| {
        for op in ["=", "<>", ">", "<", "LIKE", "AND", "OR", "IS NULL"] {
            if ui.small_button(op).clicked() {
                app.dialogs.text.push_str(&format!("{op} "));
            }
        }
    });
}

fn select_by_attributes(app: &mut RustGisApp, ui: &mut egui::Ui, t: &Tokens) -> bool {
    let layers: Vec<String> = app.session.map().layers.iter().map(|l| l.name.clone()).collect();
    egui::Grid::new("sba").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        ui.label(tl!("Input Rows"));
        egui::ComboBox::from_id_salt("sba_layer").selected_text(app.dialogs.layer.clone()).width(260.0).show_ui(ui, |ui| {
            for l in &layers {
                ui.selectable_value(&mut app.dialogs.layer, l.clone(), l);
            }
        });
        ui.end_row();
        ui.label(tl!("Selection Type"));
        egui::ComboBox::from_id_salt("sba_mode")
            .selected_text(match app.dialogs.mode {
                "add" => tl!("Add To Current Selection"),
                "remove" => tl!("Remove From Current Selection"),
                "subset" => tl!("Select Subset From Current Selection"),
                _ => tl!("New Selection"),
            })
            .width(260.0)
            .show_ui(ui, |ui| {
                for (m, l) in [
                    ("new", tl!("New Selection")),
                    ("add", tl!("Add To Current Selection")),
                    ("remove", tl!("Remove From Current Selection")),
                    ("subset", tl!("Select Subset From Current Selection")),
                ] {
                    ui.selectable_value(&mut app.dialogs.mode, m, l);
                }
            });
        ui.end_row();
    });
    ui.label(egui::RichText::new(tl!("Where")).font(semibold(12.5)));
    ui.add(
        egui::TextEdit::multiline(&mut app.dialogs.text)
            .hint_text(tl!("e.g. pop_est > 100000000 AND continent = 'Asia'"))
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .font(egui::TextStyle::Monospace),
    );
    field_chips(app, ui);
    // Live validation.
    let check =
        app.session.map().layer_by_name(&app.dialogs.layer).map(|l| rustgis_engine::data::query::select(&app.dialogs.text, &l.dataset));
    match (&check, app.dialogs.text.trim().is_empty()) {
        (_, true) => {}
        (Some(Ok(rows)), _) => {
            ui.label(
                egui::RichText::new(crate::i18n::fmt("Valid expression · {n} matching rows", &[("n", &rows.len().to_string())]))
                    .small()
                    .color(t.green),
            );
        }
        (Some(Err(e)), _) => {
            ui.label(egui::RichText::new(crate::i18n::fmt("Invalid: {error}", &[("error", e)])).small().color(t.red));
        }
        _ => {}
    }
    buttons(app, ui, "Apply", |app| {
        let layer = app.dialogs.layer.clone();
        app.run("select.byAttributes", json!({"layer": layer, "where": app.dialogs.text, "mode": app.dialogs.mode})).map(|_| ())
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn pick_open(exts: &[&str], label: &str) -> Option<String> {
    rfd::FileDialog::new().add_filter(label, exts).add_filter("All files", &["*"]).pick_file().map(|p| p.display().to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn pick_save(name: &str, exts: &[&str], label: &str) -> Option<String> {
    rfd::FileDialog::new().set_file_name(name).add_filter(label, exts).save_file().map(|p| p.display().to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn pick_open(_: &[&str], _: &str) -> Option<String> {
    None
}

#[cfg(target_arch = "wasm32")]
pub fn pick_save(_: &str, _: &[&str], _: &str) -> Option<String> {
    None
}

#[allow(dead_code)]
fn unused(_: Value) {}

/// The map's coordinate system: Web Mercator, the globe, a preset, or any projected EPSG code. The
/// list is on the left; the right shows what the selection is, where it can be used, its PROJ
/// definition, and a link to its page on epsg.io.
fn coordinate_system(app: &mut RustGisApp, ui: &mut egui::Ui, t: &Tokens) -> bool {
    let current = app.session.map().projection.code();
    ui.label(egui::RichText::new(tl!("The map is drawn in this coordinate system; layers are projected on the fly.")).weak());
    let mut choices: Vec<(String, String)> = vec![("mercator".into(), "Web Mercator".into()), ("globe".into(), "Globe".into())];
    choices.extend(rustgis_engine::map::projection::PRESETS.iter().map(|(id, label, _)| (id.to_string(), label.to_string())));
    // Two panes of a fixed height, so the dialog doesn't stretch to the window.
    let pane_h = 340.0;
    ui.allocate_ui_with_layout(vec2(ui.available_width(), pane_h), egui::Layout::left_to_right(egui::Align::Min), |ui| {
        ui.set_height(pane_h);
        ui.vertical(|ui| {
            ui.set_width(270.0);
            egui::ScrollArea::vertical().id_salt("crs_list").max_height(pane_h - 44.0).auto_shrink([false, false]).show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    for (id, label) in &choices {
                        let selected = app.dialogs.text.eq_ignore_ascii_case(id);
                        if ui.selectable_label(selected, tl!(label.as_str())).clicked() {
                            app.dialogs.text = id.clone();
                            app.dialogs.a.clear();
                        }
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(tl!("EPSG code"));
                let r = ui.add(egui::TextEdit::singleline(&mut app.dialogs.a).hint_text("3035").desired_width(110.0));
                if r.changed() && !app.dialogs.a.trim().is_empty() {
                    app.dialogs.text = format!("epsg:{}", app.dialogs.a.trim());
                }
            });
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.set_width(340.0);
            egui::ScrollArea::vertical().id_salt("crs_details").max_height(pane_h).show(ui, |ui| {
                crs_details(ui, &app.dialogs.text, &current, t);
            });
        });
    });
    ui.separator();
    let chosen = app.dialogs.text.clone();
    let check = rustgis_engine::map::Projection::from_code(&chosen);
    let same = check.as_ref().is_ok_and(|p| p.code() == current);
    let mut close = false;
    ui.horizontal(|ui| {
        if ui.add_enabled(check.is_ok() && !same, egui::Button::new(tl!("Apply"))).clicked() {
            match app.run("map.projection", json!({"type": chosen})) {
                Ok(_) => {
                    let _ = app.run("map.zoomFull", Value::Null);
                    close = true;
                }
                Err(e) => app.session.notify(e, true),
            }
        }
        if ui.button(tl!("Cancel")).clicked() {
            close = true;
        }
    });
    close
}

/// What a coordinate system is: its kind, area of use, definition and epsg.io page.
fn crs_details(ui: &mut egui::Ui, chosen: &str, current: &str, t: &Tokens) {
    use rustgis_engine::map::projection::{Domain, Projector, lookup};
    let projection = match rustgis_engine::map::Projection::from_code(chosen) {
        Ok(p) => p,
        Err(e) => {
            ui.label(egui::RichText::new(e).color(t.red));
            return;
        }
    };
    let code = projection.code();
    ui.label(egui::RichText::new(tl!(projection.label().as_str())).font(crate::theme::semibold(14.0)));
    if code == current {
        ui.label(egui::RichText::new(tl!("In use")).size(11.0).color(t.accent_text));
    }
    let epsg = code.to_ascii_lowercase().strip_prefix("epsg:").map(str::to_string);
    let (kind, page): (&str, Option<String>) = match code.as_str() {
        "mercator" => ("World, conformal: the usual web map; areas grow towards the poles.", Some("3857".into())),
        "globe" => ("A 3D globe seen from space.", None),
        "equirectangular" => ("World, cylindrical: equal steps of longitude and latitude.", Some("4087".into())),
        "miller" => ("World, cylindrical: a compromise between Mercator and equal steps.", Some("54003".into())),
        "mollweide" => ("World, equal-area: true sizes, shapes bend towards the edges.", Some("54009".into())),
        "cylindricalEqualArea" => ("World, equal-area, cylindrical: true sizes, flattened near the poles.", Some("54034".into())),
        "robinson" => ("World, compromise: neither equal-area nor conformal, balanced for wall maps.", Some("54030".into())),
        "equalEarth" => ("World, equal-area: true sizes with a Robinson-like look.", Some("8857".into())),
        "naturalEarth" => ("World, compromise: rounded corners, gentle shapes near the poles.", None),
        "winkelTripel" => ("World, compromise: low overall distortion, used for world atlases.", Some("54042".into())),
        "eckertIV" => ("World, equal-area: true sizes with rounded poles.", Some("54012".into())),
        "northPolar" => ("Polar, conformal: the Arctic and the northern lands.", None),
        "southPolar" => ("Polar, conformal: Antarctica and the Southern Ocean.", Some("3031".into())),
        "europeEqualArea" => ("Regional, equal-area: the European standard for statistics.", Some("3035".into())),
        "usaAlbers" => ("Regional, equal-area: the contiguous United States.", Some("5070".into())),
        _ => ("A projected coordinate system.", epsg.clone()),
    };
    ui.label(tl!(kind));
    if let Ok(p) = Projector::new(&code) {
        let area = match p.domain() {
            Domain::World => tl!("Area of use: the whole world").to_string(),
            Domain::Box(e) => crate::i18n::fmt(
                "Area of use: longitude {w}° to {e}°, latitude {s}° to {n}°",
                &[
                    ("w", &format!("{:.0}", e.min_x)),
                    ("e", &format!("{:.0}", e.max_x)),
                    ("s", &format!("{:.0}", e.min_y)),
                    ("n", &format!("{:.0}", e.max_y)),
                ],
            ),
            Domain::Cap { center, radius } => crate::i18n::fmt(
                "Area of use: within {r}° of {lon}°, {lat}°",
                &[("r", &format!("{radius:.0}")), ("lon", &format!("{:.0}", center.x)), ("lat", &format!("{:.0}", center.y))],
            ),
            Domain::Band { lon0, half } => crate::i18n::fmt(
                "Area of use: within {r}° of the {lon}° meridian",
                &[("r", &format!("{half:.0}")), ("lon", &format!("{lon0:.0}"))],
            ),
        };
        ui.label(egui::RichText::new(area).size(11.5).color(t.text_dim));
    }
    if let Ok((def, _)) = lookup(&code) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(tl!("PROJ definition")).size(11.0).color(t.text_dim));
        ui.add(egui::Label::new(egui::RichText::new(def).monospace().size(10.5)).wrap());
    }
    if let Some(n) = page {
        ui.add_space(4.0);
        // Five-digit codes from 53000 up are ESRI's world projections.
        let prefix = if n.len() == 5 && n.starts_with('5') { "esri" } else { "epsg" }.to_ascii_uppercase();
        let text = crate::i18n::fmt("Open {code} on epsg.io", &[("code", &format!("{prefix}:{n}"))]);
        // In the browser build the link must not replace the app (and the map) in its tab.
        ui.add(egui::Hyperlink::from_label_and_url(text, format!("https://epsg.io/{n}")).open_in_new_tab(true));
    }
}
