//! Runs frames of the whole UI (no GPU) through every tab, pane, dialog and theme.

use egui_kittest::kittest::Queryable;
use rustgis_ui_egui::RustGisApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, RustGisApp> {
    egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 860.0)).with_max_steps(64).build_ui_state(
        |ui, app: &mut RustGisApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        RustGisApp::with_sample(),
    )
}

#[test]
fn every_tab_pane_and_dialog_renders() {
    let mut h = harness();
    for _ in 0..4 {
        h.step();
    }
    for tab in ["Home", "Data", "Style", "Edit", "Analyze", "View", "Publish", "Plugins", "Settings", "Help"] {
        h.state_mut().ui.tab = tab.into();
        h.step();
        h.step();
    }
    for (id, p) in [
        ("ui.pane.symbology", json!(null)),
        ("ui.pane.geoprocessing", json!(null)),
        ("ui.gp.open", json!({"tool": "buffer"})),
        ("ui.pane.history", json!(null)),
        ("ui.pane.details", json!(null)),
        ("ui.pane.catalog", json!(null)),
        ("table.open", json!(null)),
        ("ui.dialog", json!({"name": "selectByAttributes"})),
        ("ui.dialog", json!({"name": "goToXY"})),
        ("ui.dialog", json!({"name": "calculateField"})),
        ("ui.commandSearch", json!(null)),
        ("ui.about", json!(null)),
        ("ui.shortcuts", json!(null)),
        ("ui.theme", json!({"dark": true})),
        ("ui.backstage", json!({"open": true})),
    ] {
        h.state_mut().run(id, p).unwrap_or_else(|e| panic!("{id}: {e}"));
        h.step();
        h.step();
    }
    h.state_mut().ui.backstage = false;
    // Every language, the collapsed and peeked ribbon.
    for l in rustgis_ui_egui::i18n::Lang::all() {
        h.state_mut().run("ui.language", json!({"code": l.code()})).unwrap();
        h.step();
        h.step();
    }
    h.state_mut().run("ui.language", json!({"code": "auto"})).unwrap();
    assert!(h.state_mut().run("ui.language", json!({"code": "xx"})).is_err());
    h.state_mut().run("ui.ribbon", json!({"minimized": true})).unwrap();
    h.state_mut().ui.ribbon_peek = true;
    h.step();
    h.state_mut().run("ui.ribbon", json!({"minimized": false})).unwrap();
    h.step();
    for kind in ["unique", "graduated", "single"] {
        h.state_mut().run("layer.symbology", json!({"layer": "Countries", "type": kind, "field": "continent"})).ok();
        h.step();
    }
    h.state_mut().run("layer.select", json!({"layer": "Cities"})).unwrap();
    h.state_mut().run("layer.symbology", json!({"type": "proportional", "field": "pop_max"})).unwrap();
    h.state_mut().run("project.new", json!(null)).unwrap();
    for _ in 0..4 {
        h.step();
    }
    assert!(h.state().session.map().layers.is_empty());
}

/// Plugins are loaded for the whole process, so the tests that look at them take turns.
static PLUGINS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn a_ribbon_wider_than_the_window_scrolls_with_arrow_buttons() {
    let mut h = harness();
    h.set_size(egui::vec2(900.0, 600.0));
    h.state_mut().ui.tab = "View".into();
    for _ in 0..4 {
        h.step();
    }
    assert!(h.query_by_label("Scroll the ribbon left").is_none());
    h.get_by_label("Scroll the ribbon right").click();
    for _ in 0..4 {
        h.step();
    }
    let _ = h.get_by_label("Scroll the ribbon left");
    // A window wide enough for every group shows no arrows.
    h.set_size(egui::vec2(3000.0, 600.0));
    h.state_mut().ui.tab = "Help".into();
    for _ in 0..4 {
        h.step();
    }
    assert!(h.query_by_label("Scroll the ribbon right").is_none());
    assert!(h.query_by_label("Scroll the tabs right").is_none());
}

#[test]
fn a_tab_strip_wider_than_the_window_scrolls_with_arrow_buttons() {
    let mut h = harness();
    h.set_size(egui::vec2(560.0, 500.0));
    for _ in 0..4 {
        h.step();
    }
    assert!(h.query_by_label("Scroll the tabs left").is_none());
    h.get_by_label("Scroll the tabs right").click();
    h.step();
    h.step();
    let _ = h.get_by_label("Scroll the tabs left");
    // Scrolling to the end reaches the last tab.
    for _ in 0..20 {
        let Some(right) = h.query_by_label("Scroll the tabs right") else { break };
        right.click();
        h.step();
        h.step();
    }
    assert!(h.query_by_label("Scroll the tabs right").is_none());
    h.get_by_label("Help").click();
    h.step();
    h.step();
    assert_eq!(h.state().ui.tab, "Help");
    // A tab opened off screen scrolls into view, so the left arrow goes away.
    h.state_mut().ui.tab = "Home".into();
    for _ in 0..4 {
        h.step();
    }
    assert!(h.query_by_label("Scroll the tabs left").is_none());
}

#[test]
fn plugins_install_from_a_file_after_a_prompt() {
    let _turn = PLUGINS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("rustgis-ui-plugins-{}", std::process::id()));
    rustgis_ui_egui::rustgis_engine::plugins::store::set_dir(Some(dir.join("installed")));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("ui-test.rgplugin");
    std::fs::write(&file, rustgis_ui_egui::rustgis_engine::plugins::testing::package("ui-test", &["edit"])).unwrap();
    let mut h = harness();
    for _ in 0..4 {
        h.step();
    }
    h.state_mut().run("ui.installPlugin", json!({"path": file.display().to_string()})).unwrap();
    h.step();
    h.step();
    assert!(h.state().plugins.confirm.is_some(), "asks first");
    assert!(rustgis_ui_egui::rustgis_engine::plugins::get("ui-test").is_none());
    h.get_by_label("Install").click();
    h.step();
    h.step();
    assert!(rustgis_ui_egui::rustgis_engine::plugins::get("ui-test").is_some());
    // Its commands are on the Plugins tab, and run from there.
    h.state_mut().ui.tab = "Plugins".into();
    h.step();
    h.step();
    h.get_by_label("Hello").click();
    h.step();
    // Its tools: a button on the Plugins tab, and listed in the Tools pane.
    h.get_by_label("Make Points").click();
    h.step();
    h.step();
    assert_eq!(h.state().gp.tool, Some("plugin:ui-test/m"));
    h.state_mut().gp.tool = None;
    h.step();
    h.step();
    // On the ribbon and in the pane's Plugins section.
    assert_eq!(h.query_all_by_label("Select Two").count(), 2);
    for tab in ["installed", "browse", "settings"] {
        // An unreachable registry is an error in the window, not a crash.
        h.state_mut().run("ui.plugins", json!({"open": true, "tab": tab, "registry": "http://localhost:9/registry.json"})).unwrap();
        h.step();
        h.step();
    }
    h.state_mut().run("ui.plugins", json!({"open": true, "tab": "installed"})).unwrap();
    h.step();
    h.get_by_label("Uninstall").click();
    h.step();
    assert!(rustgis_ui_egui::rustgis_engine::plugins::get("ui-test").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plugins_can_have_a_tab_of_their_own() {
    let _turn = PLUGINS.lock().unwrap_or_else(|e| e.into_inner());
    use rustgis_ui_egui::rustgis_engine::plugins as rp;
    let mut m: serde_json::Value = serde_json::from_str(&rp::testing::manifest("ui-tab", &[])).unwrap();
    // Its own names, so the other tests (which share the loaded plugins) don't see them.
    m["commands"][0]["label"] = json!("Tab Hello");
    m["tools"][0]["name"] = json!("Tab Points");
    m["tools"][1]["name"] = json!("Tab Select");
    m["tab"] = json!({"label": "Hello Tab", "groups": [
        {"label": "Things", "items": ["h", "tool:m", {"menu": "More", "items": ["e", "tool:x"]}]}
    ]});
    let pkg = rp::testing::zip_of(&[("plugin.json", m.to_string().as_bytes()), ("plugin.wasm", &rp::testing::wasm())]);
    rp::load(rp::Package::from_zip(&pkg).unwrap(), "test").unwrap();
    let mut h = harness();
    for _ in 0..4 {
        h.step();
    }
    // Its tab is in the tab strip; its buttons are there, not on the Plugins tab.
    h.state_mut().ui.tab = "Plugins".into();
    h.step();
    h.step();
    assert_eq!(h.query_all_by_label("Tab Points").count(), 0);
    h.state_mut().ui.tab = "plugin:ui-tab".into();
    h.step();
    h.step();
    h.get_by_label("Tab Hello").click();
    h.step();
    h.get_by_label("Tab Points").click();
    h.step();
    h.step();
    assert_eq!(h.state().gp.tool, Some("plugin:ui-tab/m"));
    h.get_by_label("More").click();
    for _ in 0..4 {
        h.step();
    }
    h.get_by_label("Tab Select").click();
    h.step();
    assert_eq!(h.state().gp.tool, Some("plugin:ui-tab/x"));
    // Hidden, then shown again, from Help › Ribbon.
    h.state_mut().run("ui.customizeRibbon", json!({"tab": "plugin:ui-tab"})).unwrap();
    h.step();
    assert!(h.state().ui.hidden_tabs.contains(&"plugin:ui-tab".to_string()));
    h.state_mut().run("ui.customizeRibbon", json!({"tab": "plugin:ui-tab"})).unwrap();
    // A disabled plugin's tab goes, and the ribbon falls back to the Plugins tab.
    h.state_mut().ui.tab = "plugin:ui-tab".into();
    rp::set_enabled("ui-tab", false).unwrap();
    h.step();
    h.step();
    assert_eq!(h.state().ui.tab, "Plugins");
    rp::unload("ui-tab");
}

#[test]
fn browse_lists_thousands_of_plugins_quickly() {
    use rustgis_ui_egui::rustgis_engine::plugins::registry;
    let _turn = PLUGINS.lock().unwrap_or_else(|e| e.into_inner());
    let sha = "a".repeat(64);
    let list: Vec<serde_json::Value> = (0..5000)
        .map(|i| {
            let cat = ["Vector", "Raster", "Terrain"][i % 3];
            json!({"id": format!("fake-{i:04}"), "name": format!("Plugin {i}"), "version": "1.0.0",
                "description": format!("Does many things with data for plugin number {i}. ").repeat(3), "author": format!("Author {}", i % 97),
                "categories": [cat, "Utilities"], "package": format!("https://packages.example.com/fake-{i}.rgplugin"),
                "sha256": sha, "size": 123_456, "permissions": ["edit"]})
        })
        .collect();
    let entries = registry::parse(&json!({"plugins": list}).to_string(), "https://plugins.rustgis.app/registry.json").unwrap();
    assert_eq!(entries.len(), 5000);
    let mut h = harness();
    for _ in 0..4 {
        h.step();
    }
    h.state_mut().plugins.set_registry(Some(Ok(entries)));
    h.state_mut().run("ui.plugins", json!({"open": true, "tab": "browse"})).unwrap();
    h.step();
    let _ = h.get_by_label("5000 plugins");
    // Only the visible rows are laid out, so a frame stays cheap with thousands of plugins
    // (about 1 ms in a release build; it was over 100 ms when every card was drawn). The bound
    // is loose for debug builds on busy machines.
    let t0 = std::time::Instant::now();
    for _ in 0..10 {
        h.step();
    }
    let per_frame = t0.elapsed() / 10;
    assert!(per_frame < std::time::Duration::from_millis(100), "{per_frame:?} per frame");
    // Search and the category filter narrow the list.
    h.state_mut().plugins.filter = "plugin 4999".into();
    h.step();
    h.step();
    let _ = h.get_by_label("1 of 5000 plugins");
    h.state_mut().plugins.filter = String::new();
    h.state_mut().plugins.category = "Terrain".into();
    h.step();
    h.step();
    let _ = h.get_by_label("1666 of 5000 plugins");
    // A new registry of the same size replaces what is listed (the cache follows the registry,
    // not its allocation).
    h.state_mut().plugins.category = String::new();
    let two = |a: &str, b: &str| {
        let list: Vec<serde_json::Value> = [a, b]
            .iter()
            .map(|n| json!({"id": n.to_lowercase(), "name": n, "version": "1.0.0", "package": "https://packages.example.com/x.rgplugin", "sha256": sha}))
            .collect();
        registry::parse(&json!({"plugins": list}).to_string(), "https://plugins.rustgis.app/registry.json").unwrap()
    };
    h.state_mut().plugins.set_registry(Some(Ok(two("Alpha", "Beta"))));
    h.step();
    h.step();
    let _ = h.get_by_label("Alpha");
    h.state_mut().plugins.set_registry(Some(Ok(two("Gamma", "Delta"))));
    h.step();
    h.step();
    let _ = h.get_by_label("Gamma");
    assert!(h.query_by_label("Alpha").is_none());
    h.state_mut().plugins.set_registry(None);
    h.state_mut().plugins.open = false;
}

/// Switching to a CJK language keeps drawing the previous one for the frame its fonts are
/// installed in (`set_fonts` takes effect a frame late), so no frame shows missing-glyph boxes.
#[test]
fn language_switch_waits_for_its_font() {
    let mut h = harness();
    h.state_mut().run("ui.language", json!({"code": "en"})).unwrap();
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(h.state().lang().code(), "en");
    h.state_mut().run("ui.language", json!({"code": "zh"})).unwrap();
    assert_eq!(h.state().chosen_lang().code(), "zh");
    h.step();
    assert_eq!(h.state().lang().code(), "en", "fonts installed this frame aren't active yet");
    h.step();
    assert_eq!(h.state().lang().code(), "zh");
    // Back to an alphabetic language: nothing to wait for.
    h.state_mut().run("ui.language", json!({"code": "de"})).unwrap();
    h.step();
    assert_eq!(h.state().lang().code(), "de");
}

/// Settings › Start-up (discussion #81): the preferences are saved, a later start follows them,
/// a shell's overrides win, and the start is neither an edit to undo nor unsaved work.
#[test]
fn start_up_preferences() {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    let store: Arc<Mutex<HashMap<String, String>>> = Arc::default();
    let services = || {
        let (load, save) = (store.clone(), store.clone());
        rustgis_ui_egui::Services {
            load_pref: Some(Box::new(move |k| load.lock().unwrap().get(k).cloned())),
            save_pref: Some(Box::new(move |k, v| {
                save.lock().unwrap().insert(k.to_string(), v.to_string());
            })),
            ..Default::default()
        }
    };
    let mut app = RustGisApp::with_sample().with_services(services());
    assert!(app.run("ui.startup", json!({"projection": "nope"})).is_err());
    let r = app.run("ui.startup", json!({"blank": true, "projection": "epsg:3010"})).unwrap();
    assert_eq!(r, json!({"blank": true, "projection": "EPSG:3010", "extent": null}));
    assert_eq!(store.lock().unwrap().get("start_project").map(String::as_str), Some("blank"));

    let mut app = RustGisApp::with_sample().with_services(services());
    app.start(&Default::default()).unwrap();
    assert!(app.session.map().layers.is_empty());
    assert_eq!(app.session.map().projection.code(), "EPSG:3010");
    assert!(!app.session.dirty && !app.session.can_undo());

    // The web app's `?projection=robinson` without `?blank`: the sample still goes when the
    // preference says blank; an explicit `Some(false)` keeps it.
    let mut app = RustGisApp::with_sample().with_services(services());
    let start = |blank, projection: &str, extent: Option<&str>| rustgis_ui_egui::Start {
        blank,
        projection: Some(projection.into()),
        extent: extent.map(String::from),
    };
    app.start(&start(Some(false), "robinson", None)).unwrap();
    assert!(!app.session.map().layers.is_empty());
    assert_eq!(app.session.map().projection.code(), "robinson");
    assert!(app.start(&start(Some(false), "EPSG:4326", None)).is_err(), "degrees are refused");

    // A start extent: saved from the view, or given by the shell (`?extent=`).
    let mut app = RustGisApp::with_sample().with_services(services());
    app.start(&start(Some(true), "EPSG:3010", Some("10.5,55,24.5,69.5"))).unwrap();
    let view = app.session.run("map.extent", &json!(null)).unwrap();
    let e = rustgis_ui_egui::parse_extent(&view["extent"].as_array().unwrap().iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","))
        .unwrap();
    assert!(e[0] < 11.0 && e[2] > 24.0 && e[1] < 55.5 && e[3] > 69.0, "Sweden is in view: {e:?}");
    assert!(e[2] - e[0] < 90.0 && e[3] - e[1] < 25.0, "and not the world: {e:?}");
    assert!(app.start(&start(None, "EPSG:3010", Some("10,60,5,50"))).is_err(), "west after east");
    app.run("ui.startup", json!({"extent": "view"})).unwrap();
    assert!(store.lock().unwrap().get("start_extent").is_some_and(|v| v.split(',').count() == 4));
    let mut again = RustGisApp::with_sample().with_services(services());
    assert!(again.ui.start_extent.is_some());
    again.start(&Default::default()).unwrap();
    assert!(again.run("ui.startup", json!({"extent": [1, 2]})).is_err());
    again.run("ui.startup", json!({"extent": null})).unwrap();
    assert_eq!(store.lock().unwrap().get("start_extent").map(String::as_str), Some(""));

    // Web Mercator is the default, stored as nothing.
    app.run("ui.startup", json!({"blank": false, "projection": "mercator"})).unwrap();
    assert_eq!(store.lock().unwrap().get("start_projection").map(String::as_str), Some(""));
}

#[test]
fn old_and_unknown_tab_names_open_a_current_tab() {
    let mut h = harness();
    for (asked, opened) in [("Imagery", "Analyze"), ("Feature Layer", "Style"), ("Share", "Publish"), ("Map", "Home"), ("nope", "Home")] {
        let _ = h.state_mut().run("ui.tab", json!({"tab": asked}));
        assert_eq!(h.state().ui.tab, opened, "{asked}");
        h.step();
    }
}

/// Add Delimited Text in the web app: the dialog's options go with the file picked next (and
/// only that one), so a projected CSV lands in place.
#[test]
fn delimited_text_options_go_with_the_picked_file() {
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = asked.clone();
    let services = rustgis_ui_egui::Services {
        open_async: Some(Box::new(move |purpose: &str| log.lock().unwrap().push(purpose.to_string()))),
        ..Default::default()
    };
    let mut app = RustGisApp::with_sample().with_services(services);
    app.run("ui.addDelimited", json!(null)).unwrap();
    app.run("ui.addDelimited", json!({"x": "east", "y": "north", "crs": "EPSG:2263", "delimiter": ";"})).unwrap();
    assert_eq!(asked.lock().unwrap().as_slice(), ["text"]);
    // A drop (or another picker's file) doesn't take the options.
    let r = app.open_picked("dropped.csv", b"lon,lat\n3,4\n").unwrap();
    let e = app.session.map().layer(r["layer"].as_u64().unwrap() as u32).unwrap().extent();
    assert_eq!((e.min_x, e.min_y), (3.0, 4.0));
    let picked = format!("{}parks.csv", rustgis_ui_egui::DELIMITED_PICK);
    let r = app.open_picked(&picked, b"name;east;north\nCentral Park;992000;220000\n").unwrap();
    let id = r["layer"].as_u64().unwrap() as u32;
    let e = app.session.map().layer(id).unwrap().extent();
    assert!((e.min_x + 73.96).abs() < 0.05 && (e.min_y - 40.78).abs() < 0.05, "{e:?}");
    // The options were used up: the next pick is read as it is.
    let picked = format!("{}plain.csv", rustgis_ui_egui::DELIMITED_PICK);
    let r = app.open_picked(&picked, b"lon,lat\n1,2\n").unwrap();
    let e = app.session.map().layer(r["layer"].as_u64().unwrap() as u32).unwrap().extent();
    assert_eq!((e.min_x, e.min_y), (1.0, 2.0));
}

/// Add Data From URL downloads off the frame and adds the file when it lands; a failed download
/// is said.
#[test]
fn data_from_a_web_address_lands_later() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let body = br#"{"type": "Feature", "properties": {}, "geometry": {"type": "Point", "coordinates": [1, 2]}}"#;
        for stream in listener.incoming().take(1) {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf);
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(body);
        }
    });
    let mut h = harness();
    h.step();
    let before = h.state().session.map().layers.len();
    h.state_mut().run("ui.addUrl", json!(null)).unwrap();
    h.step();
    h.state_mut().run("ui.addUrl", json!({"url": format!("http://{addr}/pin"), "name": "Pin"})).unwrap();
    h.state_mut().run("ui.addUrl", json!({"url": "http://169.254.169.254/x.geojson"})).unwrap();
    for _ in 0..100 {
        h.step();
        if h.state().session.map().layer_by_name("Pin").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(h.state().session.map().layers.len(), before + 1);
    assert!(h.state().session.map().layer_by_name("Pin").is_some());
    assert!(h.state().session.notices.iter().any(|n| n.error && n.text.contains("never fetched")));
    assert!(h.state().ui.downloads.is_empty());
}

/// Add From Service connects in the background and lists the service's layers in the dialog.
#[test]
fn a_service_is_listed_when_its_capabilities_land() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let body = br#"<WMS_Capabilities version="1.3.0" xmlns="http://www.opengis.net/wms" xmlns:xlink="http://www.w3.org/1999/xlink"><Service><Title>Local</Title></Service><Capability><Request><GetMap><Format>image/png</Format><DCPType><HTTP><Get><OnlineResource xlink:href="http://127.0.0.1/wms?"/></Get></HTTP></DCPType></GetMap></Request><Layer><CRS>EPSG:3857</CRS><Layer><Name>a</Name><Title>A</Title></Layer></Layer></Capability></WMS_Capabilities>"#;
        for stream in listener.incoming().take(1) {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf);
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(body);
        }
    });
    let mut h = harness();
    h.state_mut().run("ui.addService", json!(null)).unwrap();
    h.step();
    h.state_mut().run("ui.addService", json!({"url": format!("http://{addr}/wms"), "type": "wms"})).unwrap();
    for _ in 0..100 {
        h.step();
        if h.state().ui.service.as_ref().is_some_and(|c| c.listing.is_some() || c.error.is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let conn = h.state().ui.service.as_ref().unwrap();
    assert_eq!(conn.error, None);
    assert_eq!(conn.listing.as_ref().unwrap()["layers"][0]["name"], "a");
    h.step();
}
