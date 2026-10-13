//! Window chrome: the header (logo, Quick Access Toolbar, title, command search), the status bar,
//! keyboard shortcuts, the command search palette, About and notifications.

use egui::{Align2, CornerRadius, Key, KeyboardShortcut, Modifiers, Rect, Sense, Stroke, Ui, pos2, vec2};
use rustgis_engine::Tool;
use serde_json::{Value, json};

use crate::theme::{Tokens, regular, semibold};
use crate::widgets::{small, text_w};
use crate::{RustGisApp, icons};

pub fn header(app: &mut RustGisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("header")
        .exact_size(34.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.header).inner_margin(egui::Margin { left: 10, right: 10, top: 0, bottom: 0 }))
        .show(ui, |ui| {
            let full = ui.max_rect();
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                app.paint_logo(ui, r);
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tl!("RustGIS")).font(semibold(13.0)).color(t.text));
                ui.add_space(10.0);
                // Quick Access Toolbar.
                small(ui, app, "new", "", "New project", "project.new", Value::Null, false);
                small(ui, app, "open", "", "Open project", "ui.open", Value::Null, false);
                small(ui, app, "save", "", "Save project", "ui.save", Value::Null, false);
                small(
                    ui,
                    app,
                    "undo",
                    "",
                    app.session
                        .undo_label()
                        .map(|l| crate::i18n::fmt("Undo {action}", &[("action", tl!(l))]))
                        .unwrap_or(tl!("Undo").into())
                        .as_str(),
                    "edit.undo",
                    Value::Null,
                    false,
                );
                small(
                    ui,
                    app,
                    "redo",
                    "",
                    app.session
                        .redo_label()
                        .map(|l| crate::i18n::fmt("Redo {action}", &[("action", tl!(l))]))
                        .unwrap_or(tl!("Redo").into())
                        .as_str(),
                    "edit.redo",
                    Value::Null,
                    false,
                );
                for id in app.ui.quick_access.clone() {
                    let label = crate::ribbon::QUICK.iter().find(|q| q.0 == id).map_or(id.as_str(), |q| q.1).to_string();
                    small(ui, app, crate::ribbon::quick_icon(&id), "", &label, &id, Value::Null, false);
                }
                // Everything right of the Quick Access Toolbar is laid out by measuring, so on a
                // narrow window the search box shrinks to an icon and the title shortens or hides
                // instead of overlapping other controls.
                let qat_end = ui.cursor().min.x + 8.0;
                // Command search on the right: full width when there is room, a round icon button
                // otherwise. It never reaches into the toolbar.
                let label = tl!("Command Search");
                let hint = if cfg!(target_os = "macos") { "⌘K" } else { "Ctrl+K" };
                let right = full.max.x;
                let room = right - qat_end;
                let full_w = (text_w(ui, label, regular(12.0)) + text_w(ui, hint, regular(11.0)) + 70.0).clamp(200.0, 260.0);
                // Room kept for the title next to the search box.
                let reserve = (text_w(ui, &app.session.project.name, semibold(12.5)) + 16.0).min(160.0);
                let sw = if room >= full_w + reserve + 24.0 {
                    full_w
                } else if room >= 150.0 + reserve {
                    (room - reserve).clamp(150.0, full_w)
                } else {
                    26.0
                };
                let sr = Rect::from_min_max(pos2(right - sw, full.center().y - 12.0), pos2(right, full.center().y + 12.0));
                let resp = ui.interact(sr, egui::Id::new("command_search_box"), Sense::click());
                ui.painter().rect_filled(sr, CornerRadius::same(12), if resp.hovered() { t.hover } else { t.input });
                ui.painter().rect_stroke(sr, CornerRadius::same(12), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                let compact = sw < 100.0;
                let icon_x = if compact { sr.center().x } else { sr.min.x + 16.0 };
                icons::paint(
                    ui.painter(),
                    Rect::from_center_size(pos2(icon_x, sr.center().y), vec2(14.0, 14.0)),
                    "search",
                    t.text_dim,
                    t.accent,
                );
                if !compact {
                    let show_hint = sw >= full_w - 1.0;
                    let text_room = sr.width() - 40.0 - if show_hint { text_w(ui, hint, regular(11.0)) + 12.0 } else { 0.0 };
                    if text_w(ui, label, regular(12.0)) <= text_room {
                        ui.painter().text(pos2(sr.min.x + 30.0, sr.center().y), Align2::LEFT_CENTER, label, regular(12.0), t.text_dim);
                    }
                    if show_hint {
                        ui.painter().text(pos2(sr.max.x - 12.0, sr.center().y), Align2::RIGHT_CENTER, hint, regular(11.0), t.text_disabled);
                    }
                }
                let resp = if compact { resp.on_hover_text(format!("{label} ({hint})")) } else { resp };
                if resp.clicked() {
                    app.ui.command_search = Some(String::new());
                }
                // Title: centred when it fits, otherwise in the free gap; the map name and then the
                // project name drop out when there is no room.
                let title = format!("{}{}", app.session.project.name, if app.session.dirty { " •" } else { "" });
                let sub = format!("  —  {}", app.session.map().name);
                let (title_w, sub_w) = (text_w(ui, &title, semibold(12.5)), text_w(ui, &sub, regular(12.0)));
                let (a, b) = (qat_end + 8.0, sr.min.x - 16.0);
                if b > a {
                    let with_sub = title_w + sub_w <= b - a;
                    let tw = if with_sub { title_w + sub_w } else { title_w };
                    if tw <= b - a {
                        let cx = (full.center().x - tw / 2.0).min(b - tw).max(a);
                        ui.painter().text(pos2(cx, full.center().y), Align2::LEFT_CENTER, &title, semibold(12.5), t.text);
                        if with_sub {
                            ui.painter().text(pos2(cx + title_w, full.center().y), Align2::LEFT_CENTER, &sub, regular(12.0), t.text_dim);
                        }
                    }
                }
            });
        });
}

pub fn status_bar(app: &mut RustGisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::bottom("status")
        .exact_size(26.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.status_bar).inner_margin(egui::Margin { left: 10, right: 10, top: 0, bottom: 0 }))
        .show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().line_segment([pos2(r.min.x - 10.0, r.min.y), pos2(r.max.x + 10.0, r.min.y)], Stroke::new(1.0, t.border));
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 0.0);
                // Scale.
                let scale = app.session.scale();
                egui::ComboBox::from_id_salt("status_scale")
                    .selected_text(format!("1:{}", crate::ribbon::fmt_thousands(scale.round())))
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for s in [
                            5_000.0,
                            25_000.0,
                            100_000.0,
                            500_000.0,
                            1_000_000.0,
                            5_000_000.0,
                            10_000_000.0,
                            25_000_000.0,
                            50_000_000.0,
                            100_000_000.0,
                        ] {
                            if ui.selectable_label(false, format!("1:{}", crate::ribbon::fmt_thousands(s))).clicked() {
                                let _ = app.run("map.setScale", json!({"scale": s}));
                            }
                        }
                    });
                sep(ui, &t);
                // Pointer coordinates.
                let coord = app
                    .map
                    .pointer_lonlat
                    .map(|c| if app.map.dms { rustgis_engine::geom::format_dms(c) } else { rustgis_engine::geom::format_lonlat(c) });
                let resp = ui.add(
                    egui::Label::new(egui::RichText::new(coord.unwrap_or_else(|| "—".into())).monospace().size(11.5).color(t.text_dim))
                        .sense(Sense::click()),
                );
                if resp.on_hover_text(tl!("Click to switch between decimal degrees and DMS")).clicked() {
                    app.map.dms = !app.map.dms;
                }
                let projection = match &app.session.map().projection {
                    rustgis_engine::map::Projection::Globe => tl!("WGS 84 / Globe").to_string(),
                    rustgis_engine::map::Projection::WebMercator => tl!("WGS 84 / Web Mercator").to_string(),
                    p => tl!(p.label().as_str()).to_string(),
                };
                let resp = ui.add(egui::Label::new(egui::RichText::new(projection).size(11.0).color(t.text_dim)).sense(Sense::click()));
                if resp.on_hover_text(tl!("Change the map's coordinate system")).clicked() {
                    let _ = app.run("ui.dialog", json!({"name": "coordinateSystem"}));
                }
                sep(ui, &t);
                let n = app.session.map().selection_count();
                ui.label(
                    egui::RichText::new(crate::i18n::fmt("Selected Features: {n}", &[("n", &n.to_string())])).size(11.5).color(if n > 0 {
                        t.accent_text
                    } else {
                        t.text_dim
                    }),
                );
                if n > 0 {
                    let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
                    icons::paint(ui.painter(), r, "clear_sel", t.icon, t.accent);
                    if resp.on_hover_text(tl!("Clear selection")).clicked() {
                        let _ = app.run("select.clear", Value::Null);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("{} ms", app.map.render_ms)).size(11.0).color(t.text_disabled))
                        .on_hover_text(tl!("Last map render time"));
                    sep(ui, &t);
                    // Cut to the room left (the whole notice on hover), so a long one doesn't run
                    // over the selection count and coordinates.
                    if let Some(n) = app.session.notices.last() {
                        ui.add(
                            egui::Label::new(egui::RichText::new(&n.text).size(11.5).color(if n.error { t.red } else { t.text_dim }))
                                .truncate(),
                        )
                        .on_hover_text(&n.text);
                    }
                });
            });
        });
}

fn sep(ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 14.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, t.border_strong);
}

/// Global keyboard shortcuts.
pub fn shortcuts(app: &mut RustGisApp, ctx: &egui::Context) {
    let typing = ctx.egui_wants_keyboard_input();
    let cmd = Modifiers::COMMAND;
    let mut run: Vec<(&str, Value)> = Vec::new();
    ctx.input_mut(|i| {
        for (m, k, id) in [
            (cmd, Key::Z, "edit.undo"),
            (cmd, Key::Y, "edit.redo"),
            (cmd | Modifiers::SHIFT, Key::Z, "edit.redo"),
            (cmd, Key::S, "ui.save"),
            (cmd | Modifiers::SHIFT, Key::S, "ui.saveAs"),
            (cmd, Key::O, "ui.open"),
            (cmd, Key::N, "project.new"),
            (cmd, Key::K, "ui.commandSearch"),
            (cmd, Key::F1, "ui.ribbon"),
            (cmd, Key::T, "table.open"),
            (cmd, Key::Equals, "map.zoomIn"),
            (cmd, Key::Plus, "map.zoomIn"),
            (cmd, Key::Minus, "map.zoomOut"),
        ] {
            if i.consume_shortcut(&KeyboardShortcut::new(m, k)) {
                run.push((id, Value::Null));
            }
        }
        if !typing {
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                run.push(("tool.set", json!({"tool": "explore"})));
            }
            if i.consume_key(Modifiers::NONE, Key::Delete) {
                run.push(("edit.delete", Value::Null));
            }
            if i.consume_key(Modifiers::NONE, Key::Home) {
                run.push(("map.zoomFull", Value::Null));
            }
            // Clipboard keys arrive as events, not key presses.
            for e in &i.events {
                match e {
                    egui::Event::Copy => run.push(("ui.copy", Value::Null)),
                    egui::Event::Cut => run.push(("ui.cut", Value::Null)),
                    egui::Event::Paste(text) => run.push(("edit.paste", json!({"text": text}))),
                    _ => {}
                }
            }
        }
    });
    for (id, p) in run {
        if id == "tool.set" {
            app.map.cancel_sketch();
            app.layout.cancel_tool();
            if app.ui.command_search.is_some() {
                app.ui.command_search = None;
                continue;
            }
        }
        // In a layout, Delete removes the selected element (and never the map's hidden features).
        if id == "edit.delete"
            && let Some(li) = app.session.active_layout
        {
            if let Some(el) = app.layout.selected.take() {
                let _ = app.run("layout.removeElement", json!({"layout": li, "element": el}));
            }
            continue;
        }
        if matches!(id, "edit.delete" | "ui.copy" | "ui.cut") && app.session.map().selection_count() == 0 {
            continue;
        }
        // Pasting text that isn't features (a stray copy from elsewhere) does nothing.
        if id == "edit.paste" && !p.get("text").and_then(Value::as_str).is_some_and(|t| t.trim_start().starts_with('{')) {
            continue;
        }
        let _ = app.run(id, p);
    }
    if app.session.tool != Tool::Explore && app.map.sketch_len() == 0 {
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    }
}

/// The command palette: type to filter every command, Enter runs the first.
pub fn command_search(app: &mut RustGisApp, ctx: &egui::Context) {
    let Some(mut q) = app.ui.command_search.take() else { return };
    let t = Tokens::get(ctx);
    let mut keep = true;
    let mut chosen: Option<String> = None;
    // Fits small screens: never wider or taller than the window allows.
    let screen = ctx.content_rect();
    let width = (screen.width() - 48.0).clamp(220.0, 520.0);
    let list_h = (screen.height() - 190.0).clamp(120.0, 360.0);
    let modal = egui::Modal::new(egui::Id::new("command_search")).show(ctx, |ui| {
        ui.set_width(width);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            icons::paint(ui.painter(), r, "search", t.text_dim, t.accent);
            let te = ui.add(
                egui::TextEdit::singleline(&mut q)
                    .hint_text(tl!("Search commands — e.g. buffer, basemap, labels"))
                    .desired_width(width - 56.0)
                    .frame(egui::Frame::NONE),
            );
            te.request_focus();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
                }
                icons::paint(ui.painter(), r.shrink(5.0), "close", t.text_dim, t.accent);
                if resp.on_hover_text(tl!("Close (Esc)")).clicked() {
                    keep = false;
                }
            });
        });
        ui.separator();
        let ql = q.to_lowercase();
        let mut hits: Vec<(String, String, String)> = Vec::new();
        // Geoprocessing tools first when the query names one (a few dozen at most: the GeoLibre
        // suite has a thousand).
        for tool in rustgis_engine::analysis::tools() {
            if hits.len() >= 40 {
                break;
            }
            if !ql.is_empty()
                && (tool.name.to_lowercase().contains(&ql)
                    || tool.id.to_lowercase().contains(&ql)
                    || tl!(tool.name).to_lowercase().contains(&ql))
            {
                hits.push((
                    format!("gp:{}", tool.id),
                    crate::i18n::fmt("{name} (tool)", &[("name", tl!(tool.name))]),
                    crate::i18n::location(tool.toolbox),
                ));
            }
        }
        for c in &app.session.registry.commands {
            if ql.is_empty()
                || c.label.to_lowercase().contains(&ql)
                || c.id.to_lowercase().contains(&ql)
                || c.location.to_lowercase().contains(&ql)
            {
                if c.params.contains(':') && !c.params.contains("?") && c.params != "{}" {
                    continue; // needs parameters: not runnable from the palette
                }
                hits.push((c.id.to_string(), tl!(c.label).to_string(), crate::i18n::location(c.location)));
            }
        }
        let lang = app.lang().code();
        for c in rustgis_engine::plugins::commands() {
            let m = &c.plugin.manifest;
            let label = m.translate(lang, &c.decl.label);
            if ql.is_empty()
                || label.to_lowercase().contains(&ql)
                || c.decl.label.to_lowercase().contains(&ql)
                || m.name.to_lowercase().contains(&ql)
            {
                hits.push((c.id.clone(), label.to_string(), format!("{} › {}", tl!("Plugins"), m.translate(lang, &m.name))));
            }
        }
        for (id, label, loc) in [
            ("ui.addData", "Add Data…", "Home › Add"),
            ("ui.addDelimited", "Add Delimited Text…", "Home › Add"),
            ("ui.addUrl", "Add Data From URL…", "Home › Add"),
            ("ui.addService", "Add From Service…", "Home › Add"),
            ("ui.plugins", "Plugin Manager…", "Plugins"),
            ("ui.pane.geoprocessing", "Tools Pane", "View › Windows"),
            ("ui.pane.symbology", "Symbology Pane", "Style › Drawing"),
            ("ui.pane.assistant", "AI Assistant", "Analyze › AI"),
            ("ui.theme", "Toggle Dark Mode", "View"),
            ("ui.exportMap", "Export Map…", "Publish › Output"),
            ("ui.checkUpdate", "Check for Updates…", "Help › Help"),
        ] {
            // The web app always serves the latest release, and has no Check for Updates button.
            if id == "ui.checkUpdate" && cfg!(target_arch = "wasm32") {
                continue;
            }
            if ql.is_empty() || label.to_lowercase().contains(&ql) || tl!(label).to_lowercase().contains(&ql) {
                hits.push((id.into(), tl!(label).into(), crate::i18n::location(loc)));
            }
        }
        // The ribbon location is dropped when the palette is too narrow to show it.
        let show_loc = width >= 400.0;
        // A solid scroll bar takes its own gutter instead of floating over the locations.
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        egui::ScrollArea::vertical().max_height(list_h).show(ui, |ui| {
            let row_w = ui.available_width();
            for (i, (id, label, loc)) in hits.iter().take(40).enumerate() {
                let mut b = egui::Button::new(egui::RichText::new(label).size(13.0)).frame(false).min_size(vec2(row_w, 26.0));
                if show_loc {
                    b = b.right_text(egui::RichText::new(loc).size(11.0).weak());
                }
                let resp = ui.add(b);
                if resp.clicked() || (i == 0 && ui.input(|i| i.key_pressed(Key::Enter))) {
                    chosen = Some(id.clone());
                }
            }
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            keep = false;
        }
    });
    // A click outside the palette closes it too.
    if modal.backdrop_response.clicked() {
        keep = false;
    }
    if let Some(id) = chosen {
        keep = false;
        if let Some(tool) = id.strip_prefix("gp:") {
            let _ = app.run("ui.gp.open", json!({"tool": tool}));
        } else {
            let _ = app.run(&id, Value::Null);
        }
    }
    if keep {
        app.ui.command_search = Some(q);
    }
}

/// The project's website and source code, opened in a new tab in the browser build so the app
/// (and its map) stays open.
pub(crate) fn project_links(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add(egui::Hyperlink::from_label_and_url(tl!("Website"), "https://rustgis.app").open_in_new_tab(true));
        ui.label("·");
        ui.add(egui::Hyperlink::from_label_and_url(tl!("GitHub"), "https://github.com/opengeos/RustGIS").open_in_new_tab(true));
    });
}

pub fn about(app: &mut RustGisApp, ctx: &egui::Context) {
    if app.ui.about {
        let logo_tex = app.logo(ctx);
        let mut open = true;
        egui::Window::new(tl!("About RustGIS")).open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
                logo_tex.inspect(|id| {
                    ui.painter().image(*id, r, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
                });
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(tl!("RustGIS")).font(semibold(20.0)));
                    ui.label(egui::RichText::new(crate::i18n::fmt("Version {version}", &[("version", env!("CARGO_PKG_VERSION"))])).weak());
                });
            });
            ui.add_space(8.0);
            ui.label(tl!("A fast, free and open-source GIS written in pure Rust, for mapping, analyzing and sharing geospatial data on the desktop and in the browser."));
            ui.add_space(4.0);
            project_links(ui);
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tl!("Sample data: Natural Earth (public domain). Licensed under Apache-2.0.")).small().weak());
            let (fonts_from, _) = (rustgis_engine::render::fonts::ui_fonts().source.clone(), ());
            ui.label(egui::RichText::new(crate::i18n::fmt("Interface font: {font}", &[("font", &fonts_from)])).small().weak());
        });
        app.ui.about = open;
    }
    if app.ui.shortcuts {
        let mut open = true;
        egui::Window::new(tl!("Keyboard Shortcuts"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Grid::new("keys").striped(true).spacing(vec2(24.0, 6.0)).show(ui, |ui| {
                    for (k, d) in [
                        ("Ctrl+K", "Command Search"),
                        ("Ctrl+N / O / S", "New / Open / Save project"),
                        ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
                        ("Ctrl+T", "Attribute table"),
                        ("Ctrl+F1", "Collapse or expand the ribbon"),
                        ("Ctrl+= / Ctrl+-", "Zoom in / out"),
                        ("Home", "Full extent"),
                        ("Esc", "Back to Explore; cancel a sketch"),
                        ("Delete", "Delete selected features"),
                        ("Mouse wheel", "Zoom about the pointer"),
                        ("Drag / middle-drag", "Pan (Explore) · any tool"),
                        ("Shift+drag", "Zoom to a box (Explore)"),
                        ("Double-click", "Zoom in · finish a sketch"),
                    ] {
                        ui.label(egui::RichText::new(k).monospace());
                        ui.label(d);
                        ui.end_row();
                    }
                });
            });
        app.ui.shortcuts = open;
    }
}

/// Toasts for recent notices (errors stay longer).
pub fn notices(app: &mut RustGisApp, ctx: &egui::Context) {
    let t = Tokens::get(ctx);
    let now = ctx.input(|i| i.time);
    let total = app.session.notices.len();
    if total > app.map.notices_seen {
        for n in app.session.notices.iter().skip(app.map.notices_seen) {
            app.map.toasts.push((n.text.clone(), n.error, now));
        }
        app.map.notices_seen = total;
    }
    app.map.toasts.retain(|(_, err, at)| now - at < if *err { 6.0 } else { 3.0 });
    if app.map.toasts.is_empty() {
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
    let screen = ctx.content_rect();
    let mut y = screen.max.y - 40.0;
    for (i, (text, err, _)) in app.map.toasts.iter().rev().take(3).enumerate() {
        egui::Area::new(egui::Id::new(("toast", i))).fixed_pos(pos2(screen.max.x - 380.0, y - 44.0)).order(egui::Order::Tooltip).show(
            ctx,
            |ui| {
                egui::Frame::NONE
                    .fill(t.menu)
                    .stroke(Stroke::new(1.0, if *err { t.red } else { t.border_strong }))
                    .corner_radius(CornerRadius::same(8))
                    .shadow(egui::Shadow { offset: [0, 4], blur: 14, spread: 0, color: t.shadow })
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_width(340.0);
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 6.0, if *err { t.red } else { t.green });
                            ui.label(egui::RichText::new(text).size(12.0));
                        });
                    });
            },
        );
        y -= 52.0;
    }
}
