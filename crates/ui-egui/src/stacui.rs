//! STAC Search: an API's collections, a search over the map's extent (or anywhere) with dates and
//! a cloud limit, the items found with their footprints, and an asset of one added by its web
//! address. Requests go off the frame through the tile cache's downloads (a thread natively,
//! fetch on the web).

use egui::vec2;
use rustgis_engine::render::rustgis_tiles::{
    cache,
    stac::{self, Item},
};
use serde_json::json;

use crate::{RustGisApp, Slot, i18n};

type Answer = Slot<Result<Vec<u8>, String>>;

/// What is being fetched.
enum Asked {
    Collections,
    /// A search, by its address (which the answer's links are resolved against).
    Search(String),
}

/// The dialog's choices and results.
pub struct State {
    pub api: String,
    /// Collections the API offers: id and title.
    pub collections: Vec<(String, String)>,
    pub collection: String,
    pub from: String,
    pub to: String,
    pub use_view: bool,
    pub max_cloud: String,
    pub items: Vec<Item>,
    /// The asset chosen for each item.
    pub chosen: Vec<String>,
    /// The footprints layer of the last search.
    footprints: Option<u32>,
    waiting: Option<(Asked, Answer)>,
    pub error: Option<String>,
    pub info: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        State {
            api: stac::EARTH_SEARCH.to_string(),
            collections: Vec::new(),
            collection: "sentinel-2-l2a".into(),
            from: String::new(),
            to: String::new(),
            use_view: true,
            max_cloud: "20".into(),
            items: Vec::new(),
            chosen: Vec::new(),
            footprints: None,
            waiting: None,
            error: None,
            info: None,
        }
    }
}

fn fetch(url: &str) -> Answer {
    let slot: Answer = Slot::default();
    let land = slot.clone();
    cache::download(url, Box::new(move |r| *land.lock().unwrap_or_else(|e| e.into_inner()) = Some(r)));
    slot
}

/// Lands collections and search results.
pub fn poll(app: &mut RustGisApp, ctx: &egui::Context) {
    let Some((_, slot)) = &app.ui.stac.waiting else { return };
    let Some(r) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        return;
    };
    let Some((asked, _)) = app.ui.stac.waiting.take() else { return };
    let st = &mut app.ui.stac;
    st.error = None;
    st.info = None;
    match asked {
        Asked::Collections => match r.and_then(|b| stac::parse_collections(&b)) {
            Ok(c) => {
                st.info = Some(i18n::fmt("{n} collections", &[("n", &c.len().to_string())]));
                st.collections = c.into_iter().map(|c| (c.id, c.title)).collect();
            }
            Err(e) => st.error = Some(e),
        },
        Asked::Search(url) => match r.and_then(|b| stac::parse_items(&b, &url)) {
            Ok((items, next)) => {
                st.info = Some(if next.is_some() {
                    i18n::fmt("{n} items (more match: narrow the search)", &[("n", &items.len().to_string())])
                } else {
                    i18n::fmt("{n} items", &[("n", &items.len().to_string())])
                });
                st.chosen = items.iter().map(|i| i.pick(None).map(|a| a.key.clone()).unwrap_or_default()).collect();
                st.items = items;
                show_footprints(app);
            }
            Err(e) => st.error = Some(e),
        },
    }
}

/// Replaces the last search's footprints with this one's, and frames them.
fn show_footprints(app: &mut RustGisApp) {
    if let Some(old) = app.ui.stac.footprints.take()
        && app.session.map().layer(old).is_some()
    {
        let _ = app.run("layer.remove", json!({"layer": old}));
    }
    let items = app.ui.stac.items.clone();
    match rustgis_engine::cmd::stac::add_footprints(&mut app.session, &items, "STAC footprints") {
        Ok(Some(id)) => {
            app.ui.stac.footprints = Some(id);
            if let Some(e) = app.session.map().layer(id).map(|l| l.extent()) {
                app.session.zoom_to(&e);
            }
        }
        Ok(None) => {}
        Err(e) => app.ui.stac.error = Some(e),
    }
    app.map.invalidate();
}

/// The dialog.
pub fn dialog(app: &mut RustGisApp, ui: &mut egui::Ui) {
    let busy = app.ui.stac.waiting.is_some();
    let mut list = false;
    let mut search = false;
    egui::Grid::new("stac").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        let st = &mut app.ui.stac;
        ui.label(tl!("STAC API"));
        ui.add(egui::TextEdit::singleline(&mut st.api).desired_width(380.0));
        ui.end_row();
        ui.label(tl!("Collection"));
        ui.horizontal(|ui| {
            if st.collections.is_empty() {
                ui.add(egui::TextEdit::singleline(&mut st.collection).hint_text(tl!("any")).desired_width(270.0));
            } else {
                let shown = if st.collection.is_empty() { tl!("any").to_string() } else { st.collection.clone() };
                egui::ComboBox::from_id_salt("stac_collection").selected_text(shown).width(270.0).height(320.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut st.collection, String::new(), tl!("any"));
                    for (id, title) in &st.collections {
                        let label = if title.is_empty() { id.clone() } else { format!("{id} — {title}") };
                        ui.selectable_value(&mut st.collection, id.clone(), label);
                    }
                });
            }
            list = ui.add_enabled(!busy, egui::Button::new(tl!("Collections"))).clicked();
        });
        ui.end_row();
        ui.label(tl!("Dates"));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut st.from).hint_text("2024-06-01").desired_width(110.0));
            ui.label("–");
            ui.add(egui::TextEdit::singleline(&mut st.to).hint_text("2024-06-30").desired_width(110.0));
        });
        ui.end_row();
        ui.label(tl!("Most cloud cover (%)"));
        ui.add(egui::TextEdit::singleline(&mut st.max_cloud).hint_text(tl!("any")).desired_width(110.0));
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut st.use_view, tl!("Only where the map is"));
        ui.end_row();
    });
    ui.horizontal(|ui| {
        search = ui.add_enabled(!busy, egui::Button::new(tl!("Find Items"))).clicked();
        if busy {
            ui.spinner();
        }
    });
    if list {
        match stac::api(Some(app.ui.stac.api.as_str())) {
            Ok(api) => app.ui.stac.waiting = Some((Asked::Collections, fetch(&stac::collections_url(&api)))),
            Err(e) => app.ui.stac.error = Some(e),
        }
    }
    if search {
        let st = &app.ui.stac;
        let datetime = match (st.from.trim(), st.to.trim()) {
            ("", "") => None,
            (a, "") => Some(format!("{a}/..")),
            ("", b) => Some(format!("../{b}")),
            (a, b) => Some(format!("{a}/{b}")),
        };
        let mut p = json!({"collections": st.collection.trim(), "limit": 50});
        if let Some(d) = datetime {
            p["datetime"] = json!(d);
        }
        if let Ok(c) = st.max_cloud.trim().parse::<f64>() {
            p["maxCloud"] = json!(c);
        }
        if st.use_view {
            p["bbox"] = json!("view");
        }
        let url = stac::api(Some(st.api.as_str()))
            .and_then(|api| rustgis_engine::cmd::stac::search_params(&app.session, &p).and_then(|s| stac::search_url(&api, &s)));
        match url {
            Ok(url) => {
                let slot = fetch(&url);
                app.ui.stac.waiting = Some((Asked::Search(url), slot));
            }
            Err(e) => app.ui.stac.error = Some(e),
        }
    }
    let t = crate::theme::Tokens::get(ui.ctx());
    if let Some(e) = &app.ui.stac.error {
        ui.label(egui::RichText::new(e).color(t.red));
    }
    if let Some(i) = &app.ui.stac.info {
        ui.label(egui::RichText::new(i).weak());
    }
    results(app, ui);
}

/// The items found: a click frames one; Add adds its chosen asset.
fn results(app: &mut RustGisApp, ui: &mut egui::Ui) {
    if app.ui.stac.items.is_empty() {
        return;
    }
    let (mut zoom, mut add) = (None, None);
    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        let st = &mut app.ui.stac;
        for (i, item) in st.items.iter().enumerate() {
            ui.horizontal(|ui| {
                let date = item.datetime.get(..10).unwrap_or(&item.datetime);
                let cloud = item.cloud.map(|c| format!(" · ☁ {c:.0}%")).unwrap_or_default();
                if ui.add(egui::Button::new(format!("{date}{cloud}  {}", item.id)).truncate().min_size(vec2(300.0, 0.0))).clicked() {
                    zoom = Some(i);
                }
                if let Some(key) = st.chosen.get_mut(i) {
                    egui::ComboBox::from_id_salt(("stac_asset", i)).selected_text(key.clone()).width(110.0).show_ui(ui, |ui| {
                        for a in &item.assets {
                            ui.selectable_value(key, a.key.clone(), &a.key);
                        }
                    });
                }
                if ui.button(tl!("Add")).clicked() {
                    add = Some(i);
                }
            });
        }
    });
    if let Some(e) = zoom.and_then(|i| app.ui.stac.items.get(i)).and_then(|i| i.bbox) {
        app.session.zoom_to(&rustgis_engine::geom::Extent::new(e[0], e[1], e[2], e[3]));
        app.map.invalidate();
    }
    if let Some(i) = add {
        let Some(item) = app.ui.stac.items.get(i) else { return };
        let key = app.ui.stac.chosen.get(i).cloned().unwrap_or_default();
        match item.asset_href(Some(key.as_str()).filter(|k| !k.is_empty())) {
            Ok((key, href)) => {
                let name = format!("{} {key}", item.id);
                if let Err(e) = app.run("ui.addUrl", json!({"url": href, "name": name})) {
                    app.ui.stac.error = Some(e);
                }
            }
            Err(e) => app.ui.stac.error = Some(e),
        }
    }
}
