//! Find Place and Geocode Addresses: geocoding off the frame, one request at a time (as the
//! public servers ask), through the tile cache's downloads (a thread natively, fetch on the web).

use egui::vec2;
use rustgis_engine::render::rustgis_tiles::{
    cache,
    geocode::{Geocoder, Place},
};
use serde_json::{Map, Value, json};
use web_time::Instant;

use crate::{RustGisApp, Slot, i18n};

type Answer = Slot<Result<Vec<u8>, String>>;

/// Searches and geocoding runs under way.
#[derive(Default)]
pub struct State {
    /// A Find Place search waiting for its answer.
    search: Option<(Geocoder, Answer)>,
    /// The places the last search found.
    pub places: Vec<Place>,
    /// Whether a search is under way.
    pub searching: bool,
    job: Option<Job>,
}

/// A table's addresses being geocoded.
struct Job {
    g: Geocoder,
    /// `layer.geocode`'s parameters, given the answers when the last lands.
    params: Value,
    queue: Vec<String>,
    next: usize,
    found: Map<String, Value>,
    waiting: Option<(String, Answer)>,
    sent: Option<Instant>,
}

fn fetch(url: &str) -> Answer {
    let slot: Answer = Slot::default();
    let land = slot.clone();
    cache::download(url, Box::new(move |r| *land.lock().unwrap_or_else(|e| e.into_inner()) = Some(r)));
    slot
}

fn take(slot: &Answer) -> Option<Result<Vec<u8>, String>> {
    slot.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Lands searches and moves geocoding runs along.
pub fn poll(app: &mut RustGisApp, ctx: &egui::Context) {
    let st = &mut app.ui.geocode;
    if let Some((g, slot)) = &st.search {
        match take(slot) {
            Some(r) => {
                let r = r.and_then(|b| g.parse(&b));
                st.search = None;
                st.searching = false;
                match r {
                    Ok(p) if p.is_empty() => app.dialogs.error = Some(tl!("No place found").to_string()),
                    Ok(p) => {
                        app.dialogs.error = None;
                        st.places = p;
                    }
                    Err(e) => app.dialogs.error = Some(e),
                }
            }
            None => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
        }
    }
    let Some(job) = &mut app.ui.geocode.job else { return };
    if let Some((q, slot)) = &job.waiting {
        let Some(r) = take(slot) else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        let place = r.and_then(|b| job.g.parse(&b)).ok().and_then(|p| p.into_iter().next());
        job.found.insert(q.clone(), place.map(|p| json!({"lon": p.lon, "lat": p.lat, "name": p.name})).unwrap_or(Value::Null));
        job.waiting = None;
    }
    if let Some(q) = job.queue.get(job.next) {
        let due = job.sent.is_none_or(|t| t.elapsed().as_millis() as u64 >= job.g.spacing_ms());
        if due {
            job.waiting = Some((q.clone(), fetch(&job.g.url(q, 1))));
            job.sent = Some(Instant::now());
            job.next += 1;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        return;
    }
    // Every answer is in: the layer.
    let Some(job) = app.ui.geocode.job.take() else { return };
    let mut p = job.params;
    p["places"] = Value::Object(job.found);
    if app.run("layer.geocode", p).is_ok() {
        app.map.invalidate();
    }
}

/// Find Place: a search box, then the places found; one picked is zoomed to.
pub fn find_place(app: &mut RustGisApp, ui: &mut egui::Ui) -> bool {
    let mut go = false;
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut app.dialogs.a).hint_text(tl!("A place or an address")).desired_width(330.0));
        r.request_focus();
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.add_enabled(!app.ui.geocode.searching, egui::Button::new(tl!("Search"))).clicked() || enter {
            go = true;
        }
    });
    provider(app, ui);
    if go && !app.dialogs.a.trim().is_empty() {
        match Geocoder::new(Some(app.dialogs.mode), Some(app.dialogs.b.as_str())) {
            Ok(g) => {
                let slot = fetch(&g.url(&app.dialogs.a, 8));
                app.ui.geocode.search = Some((g, slot));
                app.ui.geocode.searching = true;
                app.ui.geocode.places.clear();
                app.dialogs.error = None;
            }
            Err(e) => app.dialogs.error = Some(e),
        }
    }
    if app.ui.geocode.searching {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tl!("Searching…"));
        });
    }
    let mut picked = None;
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for (i, p) in app.ui.geocode.places.iter().enumerate() {
            if ui.add(egui::Button::new(&p.name).wrap().min_size(vec2(ui.available_width(), 0.0))).clicked() {
                picked = Some(i);
            }
        }
    });
    let attribution = egui::RichText::new(tl!("Search results © OpenStreetMap contributors")).small().weak();
    ui.label(attribution);
    if let Some(p) = picked.and_then(|i| app.ui.geocode.places.get(i)).cloned() {
        rustgis_engine::cmd::geocode::go_to_place(&mut app.session, &p);
        app.map.invalidate();
        return true;
    }
    false
}

/// The geocoder choice shared by both dialogs: OpenStreetMap's Nominatim, Photon, or a server of
/// one's own.
fn provider(app: &mut RustGisApp, ui: &mut egui::Ui) {
    if !matches!(app.dialogs.mode, "nominatim" | "photon") {
        app.dialogs.mode = "nominatim";
    }
    egui::Grid::new("geocoder").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        ui.label(tl!("Geocoder"));
        egui::ComboBox::from_id_salt("geocoder_kind")
            .selected_text(if app.dialogs.mode == "photon" { "Photon" } else { "Nominatim" })
            .width(200.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut app.dialogs.mode, "nominatim", "Nominatim");
                ui.selectable_value(&mut app.dialogs.mode, "photon", "Photon");
            });
        ui.end_row();
        ui.label(tl!("Server"));
        ui.add(egui::TextEdit::singleline(&mut app.dialogs.b).hint_text(tl!("OpenStreetMap's public server")).desired_width(280.0));
        ui.end_row();
    });
}

/// Geocode Addresses: a table or layer, its address fields and the geocoder; the run goes on
/// after the dialog closes, a request at a time.
pub fn geocode(app: &mut RustGisApp, ui: &mut egui::Ui) -> bool {
    ui.label(egui::RichText::new(tl!("Turns a table's addresses into points: each row's address fields are joined with commas (such as street, city, country) and looked up. The public servers take up to 100 addresses a run, one a second; a server of your own takes more.")).weak());
    let tables: Vec<String> = app.session.project.tables.iter().map(|t| t.name.clone()).collect();
    let layers: Vec<String> = app.session.map().layers.iter().filter(|l| !l.is_raster()).map(|l| l.name.clone()).collect();
    if app.dialogs.layer.is_empty() {
        app.dialogs.layer = tables.first().or(layers.first()).cloned().unwrap_or_default();
    }
    egui::Grid::new("geocode").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        ui.label(tl!("Table or layer"));
        egui::ComboBox::from_id_salt("geocode_source").selected_text(app.dialogs.layer.clone()).width(280.0).show_ui(ui, |ui| {
            for n in tables.iter().chain(&layers) {
                ui.selectable_value(&mut app.dialogs.layer, n.clone(), n);
            }
        });
        ui.end_row();
        ui.label(tl!("Address fields"));
        ui.add(egui::TextEdit::singleline(&mut app.dialogs.a).hint_text(tl!("street, city, country")).desired_width(280.0));
        ui.end_row();
    });
    provider(app, ui);
    if app.ui.geocode.job.is_some() {
        ui.label(egui::RichText::new(tl!("Another run is under way.")).weak());
    }
    crate::dialogs::buttons(app, ui, "Geocode", |app| {
        if app.ui.geocode.job.is_some() {
            return Err(tl!("Another run is under way.").to_string());
        }
        let source = app.dialogs.layer.clone();
        let key = if tables.contains(&source) && !layers.contains(&source) { "table" } else { "layer" };
        let mut params = json!({key: source, "address": app.dialogs.a.trim(), "provider": app.dialogs.mode});
        if !app.dialogs.b.trim().is_empty() {
            params["url"] = json!(app.dialogs.b.trim());
        }
        let g = Geocoder::new(Some(app.dialogs.mode), Some(app.dialogs.b.as_str()))?;
        let (_, queue) = rustgis_engine::cmd::geocode::addresses(&app.session, &params)?;
        if queue.is_empty() {
            return Err(tl!("The address fields are empty in every row.").to_string());
        }
        rustgis_engine::cmd::geocode::check_batch(&g, queue.len())?;
        app.session.notify(i18n::fmt("Geocoding {n} addresses…", &[("n", &queue.len().to_string())]), false);
        app.ui.geocode.job = Some(Job { g, params, queue, next: 0, found: Map::new(), waiting: None, sent: None });
        Ok(())
    })
}
