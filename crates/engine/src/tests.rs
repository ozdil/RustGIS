use serde_json::{Value, json};

use crate::{Project, Session, sample};

/// Keeps tests away from the user's installed plugins.
fn isolate_plugins() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("rustgis-test-plugins-{}", std::process::id()));
        rustgis_plugins::store::set_dir(Some(dir));
    });
}

fn world() -> Session {
    isolate_plugins();
    let mut s = Session::new(Project::default());
    s.run("project.sample", &Value::Null).unwrap();
    s
}

#[test]
fn worldview_swaps_only_untouched_sample_countries() {
    use sample::{Worldview, swap_countries};
    let countries = |s: &Session| s.map().layer_by_name("Countries").unwrap().dataset.features.len();
    let mut s = world();
    s.dirty = false;
    let n = countries(&s);
    // The layer holds the default countries, not China's.
    assert!(!swap_countries(&mut s, Worldview::China, Worldview::Default));
    assert!(swap_countries(&mut s, Worldview::Default, Worldview::China));
    assert_ne!(countries(&s), n);
    assert!(!s.dirty);
    assert!(swap_countries(&mut s, Worldview::China, Worldview::Default));
    assert_eq!(countries(&s), n);
    // Not in a project with changes, or a saved one.
    s.dirty = true;
    assert!(!swap_countries(&mut s, Worldview::Default, Worldview::China));
    s.dirty = false;
    s.path = Some("atlas.rgis".into());
    assert!(!swap_countries(&mut s, Worldview::Default, Worldview::China));
    // Not someone else's layer named Countries.
    let mut t = world();
    t.dirty = false;
    t.run("edit.deleteField", &json!({"layer": "Countries", "field": "pop_est"})).unwrap();
    t.dirty = false;
    assert!(!swap_countries(&mut t, Worldview::Default, Worldview::China));
}

#[test]
fn sample_project_loads() {
    let s = world();
    let names: Vec<&str> = s.map().layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Cities", "Rivers", "Lakes", "Countries"]);
    assert!(s.active_layer.is_some());
    assert!(!sample::world_project().map().bookmarks.is_empty());
}

#[test]
fn select_symbolize_undo_redo() {
    let mut s = world();
    let r = s.run("select.byAttributes", &json!({"layer": "Countries", "where": "continent = 'Africa'"})).unwrap();
    assert!(r["selected"].as_u64().unwrap() > 40);
    s.run("layer.symbology", &json!({"layer": "Countries", "type": "unique", "field": "continent"})).unwrap();
    assert_eq!(s.map().layer_by_name("Countries").unwrap().renderer.kind_label(), "Unique Values");
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.map().layer_by_name("Countries").unwrap().renderer.kind_label(), "Graduated Colors");
    s.run("edit.redo", &Value::Null).unwrap();
    assert_eq!(s.map().layer_by_name("Countries").unwrap().renderer.kind_label(), "Unique Values");
}

#[test]
fn geoprocessing_adds_layers_and_history() {
    let mut s = world();
    s.run("gp.run", &json!({"tool": "buffer", "params": {"input": "Cities", "distance": 200}})).unwrap();
    assert!(s.map().layer_by_name("Cities_Buffer").is_some());
    s.run("gp.run", &json!({"tool": "summaryStatistics", "params": {"input": "Countries", "field": "pop_est", "case": "continent"}}))
        .unwrap();
    assert_eq!(s.project.tables.len(), 1);
    assert!(s.run("gp.run", &json!({"tool": "buffer", "params": {"input": "Nope"}})).is_err());
    assert_eq!(s.project.gp_history.len(), 2, "a failed run restores the project");
}

#[test]
fn editing_features_and_fields() {
    let mut s = world();
    s.run(
        "edit.addFeature",
        &json!({"layer": "Cities", "geometry": {"type": "Point", "coordinates": [1.0, 2.0]}, "attributes": {"name": "Null Island"}}),
    )
    .unwrap();
    let n = s.map().layer_by_name("Cities").unwrap().dataset.features.len();
    assert!(
        s.run("edit.addFeature", &json!({"layer": "Cities", "geometry": {"type": "LineString", "coordinates": [[0,0],[1,1]]}})).is_err()
    );
    s.run("edit.addField", &json!({"layer": "Cities", "name": "score", "type": "double"})).unwrap();
    s.run("edit.calculateField", &json!({"layer": "Cities", "field": "score", "expression": "pop_max / 1000"})).unwrap();
    s.run("edit.delete", &Value::Null).unwrap();
    assert_eq!(s.map().layer_by_name("Cities").unwrap().dataset.features.len(), n - 1);
}

#[test]
fn project_save_and_open_roundtrip() {
    let mut s = world();
    let dir = std::env::temp_dir().join(format!("rustgis-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("atlas.rgis");
    s.run("project.saveAs", &json!({"path": path.display().to_string()})).unwrap();
    let mut t = Session::new(Project::default());
    t.run("project.open", &json!({"path": path.display().to_string()})).unwrap();
    assert_eq!(t.map().layers.len(), 4);
    let png = dir.join("map.png");
    t.run("share.exportMap", &json!({"path": png.display().to_string(), "width": 320, "height": 200})).unwrap();
    assert!(std::fs::metadata(&png).unwrap().len() > 1000);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn project_extension_is_rgis_and_legacy_rgisx_opens() {
    let mut s = world();
    let dir = std::env::temp_dir().join(format!("rustgis-ext-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    s.run("project.saveAs", &json!({"path": dir.join("atlas").display().to_string()})).unwrap();
    let saved = dir.join("atlas.rgis");
    assert!(saved.exists(), "a path without an extension gets .rgis");
    let legacy = dir.join("old.rgisx");
    std::fs::copy(&saved, &legacy).unwrap();
    let mut t = Session::new(Project::default());
    t.run("project.open", &json!({"path": legacy.display().to_string()})).unwrap();
    assert_eq!(t.map().layers.len(), 4);
    for (name, is) in [("a.rgis", true), ("A.RGISX", true), ("a.rgismap", false), ("a.rgispkg", false), ("rgis", false)] {
        assert_eq!(crate::project::is_project_file(name), is, "{name}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn save_clears_dirty_and_open_clears_history() {
    let mut s = world();
    assert!(!s.dirty && !s.can_undo(), "a fresh sample is clean");
    s.run("layer.color", &json!({"layer": "Countries", "color": "#336699"})).unwrap();
    assert!(s.dirty && s.can_undo());
    let dir = std::env::temp_dir().join(format!("rustgis-dirty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.rgis");
    s.run("project.saveAs", &json!({"path": path.display().to_string()})).unwrap();
    assert!(!s.dirty, "saving clears the dirty marker");
    assert!(s.can_undo(), "saving keeps the edit history");
    s.run("project.open", &json!({"path": path.display().to_string()})).unwrap();
    assert!(!s.dirty && !s.can_undo() && !s.can_redo(), "opening starts a fresh history");
    assert!(s.run("edit.undo", &Value::Null).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn degenerate_geometry_and_deleted_fields() {
    let mut s = world();
    for g in [
        json!({"type": "LineString", "coordinates": [[0, 0]]}),
        json!({"type": "MultiPoint", "coordinates": []}),
        json!({"type": "MultiPolygon", "coordinates": []}),
    ] {
        let layer = if g["type"] == "LineString" {
            "Rivers"
        } else if g["type"] == "MultiPoint" {
            "Cities"
        } else {
            "Countries"
        };
        assert!(s.run("edit.addFeature", &json!({"layer": layer, "geometry": g})).is_err(), "{g}");
    }
    s.run("layer.definitionQuery", &json!({"layer": "Countries", "where": "pop_est > 1000"})).unwrap();
    s.run("edit.deleteField", &json!({"layer": "Countries", "field": "pop_est"})).unwrap();
    let l = s.map().layer_by_name("Countries").unwrap();
    assert_eq!(l.renderer.kind_label(), "Single Symbol");
    assert!(l.definition_query.is_none());
    s.run("edit.deleteField", &json!({"layer": "Countries", "field": "name"})).unwrap();
    assert!(!s.map().layer_by_name("Countries").unwrap().labels.enabled);
}

#[test]
fn open_from_text() {
    let mut s = world();
    let text = serde_json::to_string(&s.project).unwrap();
    s.run("layer.addDataText", &json!({"fileName": "pts.csv", "text": "name,lon,lat\nA,1,2\n"})).unwrap();
    assert!(s.map().layer_by_name("pts").is_some());
    assert!(s.run("layer.addDataText", &json!({"fileName": "x.shp", "text": ""})).is_err());
    assert!(s.run("layer.addDataText", &json!({"fileName": "x.geojson", "text": "{"})).is_err());
    assert!(s.run("project.openText", &json!({"text": "not json"})).is_err());
    s.run("project.openText", &json!({"text": text})).unwrap();
    assert!(s.map().layer_by_name("pts").is_none() && !s.dirty && !s.can_undo());
}

#[test]
fn command_ids_are_unique() {
    let s = Session::new(Project::default());
    let mut ids: Vec<&str> = s.registry.commands.iter().map(|c| c.id).collect();
    ids.sort_unstable();
    let n = ids.len();
    ids.dedup();
    assert_eq!(n, ids.len());
}

/// A 3-band 8×6 UTM 17N GeoTIFF near 81°W, 42°N, as base64.
fn utm_tiff_b64() -> String {
    use base64::Engine;
    use rustgis_data::raster::{ColorHint, Crs, GeoTransform, Raster, geotiff};
    let gt = GeoTransform { origin_x: 497_000.0, origin_y: 4_651_000.0, pixel_width: 500.0, pixel_height: -500.0 };
    let bands = (0..3).map(|b| (0..48).map(|i| ((i * 5 + b * 40) % 256) as f32).collect()).collect();
    let r = Raster::new("utm", 8, 6, bands, gt, Crs::from_epsg(32617).unwrap(), None, ColorHint::Rgb { alpha: false }, "UInt8").unwrap();
    base64::engine::general_purpose::STANDARD.encode(geotiff::write(&r, 0).unwrap())
}

/// The sample world with a streamed layer active.
fn with_streamed(path: &std::path::Path) -> Session {
    let mut s = world();
    s.run("layer.addData", &json!({"path": path.display().to_string(), "stream": true, "name": "Streamed"})).unwrap();
    s
}

fn with_raster() -> Session {
    let mut s = world();
    s.run("layer.addDataBytes", &json!({"fileName": "scene.tif", "data": utm_tiff_b64()})).unwrap();
    s
}

#[test]
fn globe_projection_toggles_renders_and_saves() {
    let mut s = with_raster();
    assert_eq!(s.run("map.projection", &Value::Null).unwrap()["projection"], "globe");
    assert_eq!(s.map().projection, rustgis_map::Projection::Globe);
    // The globe draws vectors, rasters and labels, from any side.
    for (lon, lat, res) in [(-81.0, 42.0, 30_000.0), (100.0, -60.0, 60_000.0), (-81.0, 42.0, 50.0), (0.0, 89.0, 160_000.0)] {
        s.map_mut().camera = rustgis_map::Camera { center: rustgis_geom::lonlat_to_merc(rustgis_geom::Coord::new(lon, lat)), res };
        let png = crate::cmd::share::map_png(&mut s, Some(400.0), Some(300.0), None).unwrap();
        assert!(png.len() > 1000);
    }
    assert!(s.run("map.projection", &json!({"type": "nope"})).is_err());
    s.run("map.projection", &json!({"type": "mercator"})).unwrap();
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.map().projection, rustgis_map::Projection::Globe, "undo restores the globe");
    let json = serde_json::to_string(&s.project).unwrap();
    assert!(Project::from_json(&json).unwrap().map().projection == rustgis_map::Projection::Globe);
}

#[test]
fn custom_projections_switch_keep_scale_query_and_render() {
    let mut s = with_raster();
    s.run("map.goToXY", &json!({"lon": 10.0, "lat": 50.0, "scale": 5_000_000})).unwrap();
    let before = s.scale();
    let r = s.run("map.projection", &json!({"type": "europeEqualArea"})).unwrap();
    assert_eq!(r["projection"], "europeEqualArea");
    assert!((s.scale() / before - 1.0).abs() < 0.02, "scale kept: {before} vs {}", s.scale());
    let c = s.map().center_lonlat();
    assert!((c.x - 10.0).abs() < 1e-6 && (c.y - 50.0).abs() < 1e-6, "{c:?}");
    // Identify and box select work in the projection's coordinates.
    let v = s.run("identify", &json!({"lon": 2.35, "lat": 48.85, "tolerancePx": 5})).unwrap();
    assert!(!v.is_null(), "something is under Paris");
    let sel = s.run("select.byRect", &json!({"minLon": 5.0, "minLat": 45.0, "maxLon": 15.0, "maxLat": 55.0})).unwrap();
    assert!(sel["selected"].as_u64().unwrap() > 0);
    assert!(crate::cmd::share::map_png(&mut s, Some(400.0), Some(300.0), None).unwrap().len() > 1000);
    // An EPSG code; the raster (UTM 17N) in the map's own system draws without reprojection.
    s.run("map.projection", &json!({"type": "EPSG:32617"})).unwrap();
    s.run("layer.zoomTo", &json!({"layer": "scene"})).unwrap();
    assert!(crate::cmd::share::map_png(&mut s, Some(400.0), Some(300.0), None).unwrap().len() > 1000);
    assert!(s.run("map.projection", &json!({"type": "EPSG:4326"})).is_err(), "degrees are refused");
    assert!(s.run("map.projection", &json!({"type": "nope"})).is_err());
    assert!(s.run("map.goToXY", &json!({"lon": 100.0, "lat": 0.0})).is_err(), "folded far side of the UTM zone");
    // Undo and save/reload keep the projection.
    s.run("map.projection", &json!({"type": "mollweide"})).unwrap();
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.map().projection.code(), "EPSG:32617");
    let json = serde_json::to_string(&s.project).unwrap();
    assert_eq!(Project::from_json(&json).unwrap().map().projection.code(), "EPSG:32617");
    assert!(s.run("map.projections", &Value::Null).unwrap().as_array().unwrap().len() >= 10);
    // A project naming a projection this version can't use opens in Web Mercator.
    let broken = json.replace("EPSG:32617", "EPSG:9");
    let reopened = Project::from_json(&broken).unwrap();
    assert_eq!(reopened.map().projection, rustgis_map::Projection::WebMercator);
    assert_eq!(reopened.map().camera, rustgis_map::Camera::world(), "UTM metres aren't read as Mercator ones");
    // Bookmarks follow a projection switch, and the extent history doesn't replay old cameras.
    s.run("map.projection", &json!({"type": "mercator"})).unwrap();
    s.run("map.goToXY", &json!({"lon": 10.0, "lat": 50.0, "scale": 5_000_000})).unwrap();
    s.run("bookmark.add", &json!({"name": "Europe"})).unwrap();
    s.run("map.goToXY", &json!({"lon": -100.0, "lat": 40.0})).unwrap();
    assert!(s.can_extent_back());
    s.run("map.projection", &json!({"type": "europeEqualArea"})).unwrap();
    assert!(!s.can_extent_back(), "history from Web Mercator is dropped");
    s.run("bookmark.go", &json!({"name": "Europe"})).unwrap();
    let c = s.map().center_lonlat();
    assert!((c.x - 10.0).abs() < 1e-6 && (c.y - 50.0).abs() < 1e-6, "{c:?}");
    assert!((s.scale() / 5_000_000.0 - 1.0).abs() < 0.02, "{}", s.scale());
    // An export at another size measures its scale bar with its own camera.
    let wide = s.map().ground_res_at(&rustgis_map::Camera { res: s.map().camera.res * 3.0, ..s.map().camera });
    assert!((wide / s.map().ground_res() - 3.0).abs() < 1e-9);
    // The built-in world projections (and Equal Earth by its EPSG code) draw the whole world.
    for p in ["robinson", "equalEarth", "naturalEarth", "winkelTripel", "eckertIV", "EPSG:8857"] {
        s.run("map.projection", &json!({"type": p})).unwrap_or_else(|e| panic!("{p}: {e}"));
        s.run("map.zoomFull", &Value::Null).unwrap();
        let c = s.map().center_lonlat();
        assert!(c.x.abs() < 1.0 && c.y.abs() < 30.0, "{p}: {c:?}");
        let v = s.run("identify", &json!({"lon": 2.35, "lat": 48.85, "tolerancePx": 5})).unwrap();
        assert!(!v.is_null(), "{p}: something is under Paris");
        assert!(crate::cmd::share::map_png(&mut s, Some(400.0), Some(300.0), None).unwrap().len() > 1000, "{p}");
    }
}

#[test]
fn raster_layers_open_style_identify_and_round_trip() {
    let mut s = with_raster();
    let l = s.map().layers.last().unwrap();
    assert!(l.is_raster(), "rasters go under the vector layers");
    assert_eq!(l.name, "scene");
    assert_eq!(s.active_layer, Some(l.id));
    let e = l.extent();
    assert!((e.center().x + 81.0).abs() < 0.1 && (e.center().y - 42.0).abs() < 0.1, "{e:?}");
    // Rendering, stretches and enhancement.
    for p in [
        json!({"type": "stretch", "band": 2, "ramp": "Terrain"}),
        json!({"type": "classify", "classes": 4, "method": "quantile"}),
        json!({"type": "unique"}),
        json!({"type": "shadedRelief", "azimuth": 270}),
        json!({"type": "rgb"}),
    ] {
        s.run("raster.symbology", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    }
    assert!(s.run("raster.symbology", &json!({"type": "colormap"})).is_err());
    assert!(s.run("raster.symbology", &json!({"band": 9})).is_err());
    s.run("raster.bands", &json!({"red": 3, "green": 2, "blue": 1})).unwrap();
    s.run("raster.symbology", &json!({"type": "rgb"})).unwrap(); // keeps the combination
    assert!(s.run("raster.bands", &json!({"preset": "colorInfrared"})).is_err(), "no band 4");
    s.run("raster.stretch", &json!({"type": "standardDeviation", "n": 2.5})).unwrap();
    s.run("raster.dra", &json!({"on": true})).unwrap();
    s.run("raster.resampling", &json!({"type": "bilinear"})).unwrap();
    s.run("raster.enhance", &json!({"brightness": 20, "contrast": -10, "gamma": 1.4})).unwrap();
    s.run("layer.ramp", &json!({"ramp": "Aurora"})).unwrap();
    let st = s.map().layers.last().unwrap().raster.as_ref().unwrap().style.clone();
    assert_eq!(st.renderer, rustgis_map::RasterRenderer::Rgb { red: 2, green: 1, blue: 0 });
    assert!(st.dra && st.gamma == 1.4);
    // Feature-only commands are off for rasters, and layer conditions follow a named layer.
    assert!(s.run("table.open", &Value::Null).is_err());
    assert!(s.run("labels.enable", &json!({"layer": "scene"})).is_err());
    s.run("layer.symbology", &json!({"layer": "Countries", "type": "single"})).unwrap();
    s.active_layer = s.map().layer_by_name("Countries").map(|l| l.id);
    s.run("raster.info", &json!({"layer": "scene"})).unwrap();
    s.active_layer = s.map().layer_by_name("scene").map(|l| l.id);
    // Identify reports features first (Lake Erie is here), then pixel values.
    let v = s.run("identify", &json!({"lon": -81.0, "lat": 42.0})).unwrap();
    assert_eq!(v["layer"], "Lakes", "{v}");
    for l in ["Cities", "Rivers", "Lakes", "Countries"] {
        s.run("layer.visibility", &json!({"layer": l, "visible": false})).unwrap();
    }
    let v = s.run("identify", &json!({"lon": -81.0, "lat": 42.0})).unwrap();
    assert_eq!(v["raster"], true, "{v}");
    assert!(v["attributes"]["Band 1"].is_string());
    let info = s.run("raster.info", &Value::Null).unwrap();
    assert_eq!(info["crs"], "EPSG:32617");
    assert_eq!(info["bands"].as_array().map(Vec::len), Some(3));
    // Export, then reopen from the file.
    let dir = std::env::temp_dir().join(format!("rustgis-raster-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let tif = dir.join("out.tif");
    s.run("share.exportRaster", &json!({"path": tif.display().to_string()})).unwrap();
    s.run("layer.addData", &json!({"path": tif.display().to_string()})).unwrap();
    assert!(s.map().layer_by_name("out").is_some_and(|l| l.raster.as_ref().is_some_and(|r| r.path.is_some())));
    // Save and reload the project: the upload is embedded, the file is reopened by path.
    let proj = dir.join("p.rgis");
    s.run("project.saveAs", &json!({"path": proj.display().to_string()})).unwrap();
    s.run("project.open", &json!({"path": proj.display().to_string()})).unwrap();
    for name in ["scene", "out"] {
        let l = s.map().layer_by_name(name).unwrap();
        assert!(l.raster.as_ref().unwrap().data.is_some(), "{name} reloaded");
    }
    assert!(s.map().layer_by_name("scene").unwrap().raster.as_ref().unwrap().style.dra, "style kept");
    // Undo after a style change restores the previous style.
    s.active_layer = s.map().layer_by_name("scene").map(|l| l.id);
    s.run("raster.dra", &json!({"on": false})).unwrap();
    s.run("edit.undo", &Value::Null).unwrap();
    assert!(s.map().layer_by_name("scene").unwrap().raster.as_ref().unwrap().style.dra);
    let _ = std::fs::remove_dir_all(&dir);
    // The map renders with rasters.
    assert!(crate::cmd::share::map_png(&mut s, Some(320.0), Some(200.0), None).is_ok());
}

/// Every command with junk parameters returns Ok or Err, never panics, and leaves a usable session.
#[test]
fn hostile_params_never_panic() {
    let junk = [
        Value::Null,
        json!({}),
        json!({"layer": 99999, "path": "/nonexistent/x.geojson", "name": "", "where": "((", "tool": "buffer", "params": 7}),
        json!({"layer": "Countries", "row": 1e30, "field": "nope", "value": [1], "scale": -1, "lon": 1e300, "lat": "x", "classes": 1e9, "type": "graduated"}),
        json!({"layer": "Cities", "rows": [1, 99999999], "geometry": {"type": "Polygon", "coordinates": [[]]}, "width": 1e9, "height": -3, "expression": "'"}),
        json!({"band": -1, "red": 1e12, "green": "x", "blue": f64::MAX, "preset": "", "ramp": "nope", "classes": -5, "n": 1e308, "low": -50, "high": 900, "gamma": 0, "zFactor": -1, "fileName": "a.tif", "data": "!!!"}),
        json!({"fileName": "a.tif", "data": "AAAA", "type": "unique", "on": "maybe"}),
    ];
    let ids: Vec<&str> = Session::new(Project::default()).registry.commands.iter().map(|c| c.id).collect();
    let streamed = grid_fgb("hostile", 20);
    for id in ids {
        if matches!(
            id,
            "project.save"
                | "project.saveAs"
                | "share.exportMap"
                | "share.exportLayer"
                | "share.exportTable"
                | "share.exportRaster"
                // Download from the network.
                | "gp.installGeolibre"
                | "plugin.registry"
                | "plugin.installFromRegistry"
        ) {
            continue;
        }
        for p in &junk {
            let mut s = world();
            let _ = s.run(id, p);
            let _ = s.run("info.project", &Value::Null);
            // Again with a raster layer active.
            let mut s = with_raster();
            let _ = s.run(id, p);
            let _ = s.run("info.project", &Value::Null);
            // And with a streamed layer active.
            let mut s = with_streamed(&streamed);
            let _ = s.run(id, p);
            let _ = s.run("info.project", &Value::Null);
        }
    }
}

#[test]
fn every_tool_survives_an_environment_extent() {
    // An extent holding some features, and one holding none, with every tool and junk params.
    for extent in [json!([0, 40, 20, 55]), json!([-179.9, -89, -179.8, -88.9])] {
        for t in rustgis_analysis::tools().filter(|t| !rustgis_analysis::is_external(t)) {
            let mut s = world();
            s.run("gp.env", &json!({"extent": extent})).unwrap();
            for params in [json!({"input": "Countries"}), json!({"input": "Cities", "distance": 1e9, "clip": "Countries"})] {
                let _ = s.run("gp.run", &json!({"tool": t.id, "params": params}));
            }
            let _ = s.run("info.project", &Value::Null);
        }
    }
    let mut s = world();
    s.run("gp.env", &json!({"extent": [-179.9, -89, -179.8, -88.9]})).unwrap();
    let e = s.run("gp.run", &json!({"tool": "buffer", "params": {"input": "Cities", "distance": 10}})).unwrap_err();
    assert!(e.contains("inside the environment"), "{e}");
}

#[cfg(feature = "geolibre")]
#[test]
fn geolibre_tools_run_on_layers_and_add_their_outputs() {
    let mut s = with_raster();
    let st = s.run("gp.geolibre", &Value::Null).unwrap();
    assert_eq!(st["supported"], true);
    assert!(s.run("gp.setGeolibre", &json!({"path": "/no/such/geolibre"})).is_err(), "not a runner");
    let undo = s.undo_label();
    s.run("gp.setGeolibre", &json!({"path": ""})).unwrap();
    assert_eq!(s.undo_label(), undo, "choosing a runner isn't an edit");
    if st["available"] != true {
        eprintln!("skipped: no GeoLibre runner (set RUSTGIS_GEOLIBRE)");
        return;
    }
    let tools = s.run("gp.tools", &Value::Null).unwrap();
    assert!(tools.as_array().unwrap().iter().any(|t| t["id"] == "geolibre:slope"));
    let before = s.map().layers.len();
    let r = s.run("gp.run", &json!({"tool": "geolibre:slope", "params": {"input": "scene"}})).unwrap();
    assert_eq!(r["layers"].as_array().map(Vec::len), Some(1), "{r}");
    assert_eq!(s.map().layers.len(), before + 1);
    let slope = s.map().layer_by_name("Slope").unwrap();
    assert!(slope.is_raster() && slope.raster.as_ref().is_some_and(|r| r.embedded.is_some()), "kept in the project");
    // It saves with the project, and undo takes it away.
    let json = serde_json::to_string(&s.project).unwrap();
    assert!(
        Project::from_json(&json)
            .unwrap()
            .map()
            .layer_by_name("Slope")
            .is_some_and(|l| l.raster.as_ref().is_some_and(|r| r.data.is_some()))
    );
    s.run("edit.undo", &Value::Null).unwrap();
    assert!(s.map().layer_by_name("Slope").is_none());
    // A vector where a raster belongs is an error, not a crash.
    let err = s.run("gp.run", &json!({"tool": "geolibre:slope", "params": {"input": "Cities"}})).unwrap_err();
    assert!(err.starts_with("Slope:"), "{err}");
}

/// The browser's way: prepare a run, let a runner (here, pretend) produce the files, then
/// apply it with `gp.finish`, which keeps undo and history.
#[cfg(feature = "geolibre")]
#[test]
fn geolibre_runs_can_finish_asynchronously() {
    use crate::cmd::gp::{external_complete, external_start};
    let mut s = with_raster();
    assert!(s.run("gp.finish", &json!({"id": 99})).is_err(), "nothing pending");
    assert!(external_complete(&mut s, 99, Ok((vec![], vec![]))).is_err(), "nothing started");
    if s.run("gp.geolibre", &Value::Null).unwrap()["available"] != true {
        eprintln!("skipped: no GeoLibre runner (set RUSTGIS_GEOLIBRE)");
        return;
    }
    assert!(external_start(&mut s, "geolibre:slope", &json!({})).unwrap_err().contains("required"));
    assert!(external_start(&mut s, "buffer", &json!({})).is_err(), "only GeoLibre tools");
    let run = external_start(&mut s, "geolibre:slope", &json!({"input": "scene"})).unwrap();
    assert_eq!((run.tool.as_str(), run.files.len()), ("slope", 1), "the embedded raster is staged");
    assert!(run.argv.iter().any(|a| a.starts_with("--output=/work/")));
    // The "runner" writes the input back as the output.
    let out = run.files[0].1.clone();
    external_complete(&mut s, run.id, Ok((vec![("output.tif".into(), out)], vec![]))).unwrap();
    let before = s.map().layers.len();
    let r = s.run("gp.finish", &json!({"id": run.id})).unwrap();
    assert_eq!(r["layers"].as_array().map(Vec::len), Some(1), "{r}");
    assert_eq!(s.map().layers.len(), before + 1);
    assert!(s.project.gp_history.last().is_some_and(|h| h.ok && h.tool == "Slope"));
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.map().layers.len(), before, "undo removes it");
    assert!(s.run("gp.finish", &json!({"id": run.id})).is_err(), "applied once");
    // A runner error is reported like a failed gp.run.
    let run = external_start(&mut s, "geolibre:slope", &json!({"input": "scene"})).unwrap();
    external_complete(&mut s, run.id, Err("trap".into())).unwrap();
    assert_eq!(s.run("gp.finish", &json!({"id": run.id})).unwrap_err(), "Slope: trap");
    // A run of a project that was closed meanwhile doesn't land in the new one.
    let run = external_start(&mut s, "geolibre:slope", &json!({"input": "scene"})).unwrap();
    s.run("project.new", &Value::Null).unwrap();
    assert!(external_complete(&mut s, run.id, Ok((vec![], vec![]))).unwrap_err().contains("project was closed"));
}

#[test]
fn clipboard_copies_cuts_and_pastes_features() {
    let mut s = world();
    assert!(s.run("edit.copy", &Value::Null).is_err(), "nothing selected");
    s.run("select.byAttributes", &json!({"layer": "Cities", "where": "name = 'Paris'"})).unwrap();
    let r = s.run("edit.copy", &Value::Null).unwrap();
    assert_eq!(r["copied"], 1);
    let text = r["text"].as_str().unwrap().to_string();
    assert!(text.contains("Paris"));
    // Into the same layer, then as a new layer, then from GeoJSON text.
    let n = s.map().layer_by_name("Cities").unwrap().dataset.features.len();
    s.run("edit.paste", &json!({"layer": "Cities"})).unwrap();
    assert_eq!(s.map().layer_by_name("Cities").unwrap().dataset.features.len(), n + 1);
    assert!(s.run("edit.paste", &json!({"layer": "Countries"})).is_err(), "points don't go in polygons");
    let r = s.run("edit.pasteSpecial", &json!({"name": "Copied"})).unwrap();
    assert_eq!(r["pasted"], 1);
    assert_eq!(s.map().layer_by_name("Copied").unwrap().dataset.features.len(), 1);
    s.run("edit.pasteSpecial", &json!({"text": text, "name": "FromText"})).unwrap();
    assert!(s.map().layer_by_name("FromText").is_some());
    assert!(s.run("edit.paste", &json!({"layer": "Cities", "text": "not json"})).is_err());
    // Cut removes the features.
    s.run("select.byAttributes", &json!({"layer": "Cities", "where": "name = 'Paris'"})).unwrap();
    let cut = s.run("edit.cut", &Value::Null).unwrap();
    assert_eq!(cut["copied"], 2);
    assert_eq!(s.map().layer_by_name("Cities").unwrap().dataset.features.len(), n - 1);
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.map().layer_by_name("Cities").unwrap().dataset.features.len(), n + 1);
    // The sample layers live in the project, not in files.
    assert!(s.run("layer.copyPath", &json!({"layer": "Cities"})).is_err());
}

#[test]
fn popup_modes_choose_what_identify_reports() {
    let mut s = world();
    let at = json!({"lon": 2.35, "lat": 48.85, "tolerancePx": 5});
    let top = s.run("identify", &at).unwrap();
    assert_eq!(top["layer"], "Cities");
    s.run("map.popupMode", &json!({"mode": "visible"})).unwrap();
    let all = s.run("identify", &at).unwrap();
    let layers: Vec<&str> = all["results"].as_array().unwrap().iter().map(|r| r["layer"].as_str().unwrap()).collect();
    assert_eq!(layers, ["Cities", "Countries"]);
    s.run("map.popupMode", &json!({"mode": "selectedInContents"})).unwrap();
    s.run("layer.select", &json!({"layer": "Countries"})).unwrap();
    assert_eq!(s.run("identify", &at).unwrap()["layer"], "Countries");
    s.run("map.popupMode", &json!({"mode": "none"})).unwrap();
    assert!(s.run("identify", &at).unwrap().is_null());
    assert!(s.run("map.popupMode", &json!({"mode": "bogus"})).is_err());
    // The mode is saved with the project.
    let text = serde_json::to_string(&s.project).unwrap();
    assert!(text.contains("\"popup_mode\":\"None\""));
}

#[test]
fn label_range_placement_lock_and_convert() {
    use crate::render::View;
    let mut s = world();
    s.run("labels.enable", &json!({"layer": "Countries", "on": true})).unwrap();
    s.run("map.goToXY", &json!({"lon": 10.0, "lat": 50.0, "scale": 20_000_000})).unwrap();
    let view = |s: &Session| View { width: 800.0, height: 600.0, pixels_per_point: 1.0, camera: s.map().camera, dark: false };
    let count = |s: &mut Session| {
        let v = view(s);
        crate::render::render(s.map_mut(), &v).unwrap().labels.len()
    };
    let placed = count(&mut s);
    assert!(placed > 5);
    // Out beyond 1:5,000,000: no labels at 1:20,000,000.
    s.run("labels.scaleRange", &json!({"layer": "Countries", "min": 5_000_000})).unwrap();
    let n = count(&mut s);
    assert!(n < placed, "{n} < {placed}");
    s.run("labels.scaleRange", &json!({"layer": "Countries", "clear": true})).unwrap();
    assert_eq!(count(&mut s), placed);
    s.run("labels.distanceRange", &json!({"layer": "Countries", "farthest": 1000})).unwrap();
    assert!(count(&mut s) < placed);
    s.run("labels.distanceRange", &json!({"layer": "Countries", "farthest": null})).unwrap();
    assert!(s.run("labels.scaleRange", &json!({"layer": "Countries", "min": -4})).is_err());
    // Pause hides every label; unplaced labels are reported when asked for.
    s.run("labels.pause", &json!({"on": true})).unwrap();
    assert_eq!(count(&mut s), 0);
    s.run("labels.pause", &json!({"on": false})).unwrap();
    s.run("labels.viewUnplaced", &json!({"on": true})).unwrap();
    let v = view(&s);
    let r = crate::render::render(s.map_mut(), &v).unwrap();
    assert!(!r.unplaced.is_empty() && r.unplaced.iter().all(|l| l.color == crate::render::UNPLACED));
    s.run("labels.viewUnplaced", &json!({"on": false})).unwrap();
    // Placement moves labels.
    s.run("labels.placement", &json!({"layer": "Cities", "position": "right"})).unwrap();
    assert_eq!(s.map().layer_by_name("Cities").unwrap().labels.placement, crate::map::LabelPlacement::Right);
    assert!(s.run("labels.placement", &json!({"position": "sideways"})).is_err());
    // Lock keeps the labelled set as the map moves.
    s.run("labels.lock", &json!({"on": true})).unwrap();
    let locked = count(&mut s);
    assert!(s.map().label_lock.is_some());
    s.run("map.zoomOut", &Value::Null).unwrap();
    assert!(count(&mut s) <= locked);
    s.run("labels.lock", &json!({"on": false})).unwrap();
    count(&mut s);
    assert!(s.map().label_lock.is_none());
    // Convert makes an annotation layer and turns the source's labels off.
    let r = s.run("labels.convert", &json!({"layer": "Countries"})).unwrap();
    assert!(r["annotations"].as_u64().unwrap() > 100);
    let a = s.map().layer_by_name("Countries Annotation").unwrap();
    assert!(a.labels.enabled && a.labels.field == "TEXT");
    assert!(!s.map().layer_by_name("Countries").unwrap().labels.enabled);
}

#[test]
fn graphics_draw_shapes_text_and_pictures() {
    use crate::render::View;
    use base64::Engine;
    let mut s = world();
    s.run("map.goToXY", &json!({"lon": 10.0, "lat": 50.0, "scale": 20_000_000})).unwrap();
    // The first graphic makes a graphics layer.
    let r = s.run("graphics.add", &json!({"shape": "rectangle", "points": [[0, 45], [10, 55]], "fill": "#FF000040"})).unwrap();
    let id = r["layer"].as_u64().unwrap() as u32;
    assert!(s.map().layer(id).unwrap().graphics);
    for (shape, pts) in [
        ("circle", json!([[5, 50], [8, 50]])),
        ("ellipse", json!([[0, 40], [20, 48]])),
        ("line", json!([[0, 40], [5, 45], [10, 40]])),
        ("curve", json!([[0, 40], [5, 45], [10, 40]])),
        ("freehand", json!([[0, 40], [1, 41], [2, 40]])),
        ("polygon", json!([[0, 40], [5, 45], [10, 40]])),
        ("lasso", json!([[0, 40], [5, 45], [10, 40], [5, 38]])),
        ("point", json!([[2, 48]])),
    ] {
        s.run("graphics.add", &json!({"shape": shape, "points": pts})).unwrap_or_else(|e| panic!("{shape}: {e}"));
    }
    assert!(s.run("graphics.add", &json!({"shape": "polygon", "points": [[0, 0], [1, 1]]})).is_err());
    assert!(s.run("graphics.add", &json!({"shape": "rectangle", "points": [[0, 0], [0, 1]]})).is_err());
    assert!(s.run("graphics.add", &json!({"shape": "text", "points": [[0, 0]]})).is_err(), "text needs text");
    assert!(s.run("graphics.add", &json!({"shape": "blob", "points": [[0, 0]]})).is_err());
    assert!(s.run("graphics.add", &json!({"shape": "point", "points": [[0, 0]], "layer": "Cities"})).is_err(), "not a graphics layer");
    // Dynamic text expands when drawn.
    s.run("graphics.add", &json!({"shape": "text", "points": [[10, 50]], "text": "{map} {scale}", "fontSize": 20})).unwrap();
    let view = View { width: 800.0, height: 600.0, pixels_per_point: 1.0, camera: s.map().camera, dark: false };
    let r = crate::render::render(s.map_mut(), &view).unwrap();
    assert!(
        r.labels.iter().any(|l| l.text.starts_with("World 1:") && l.size == 20.0),
        "{:?}",
        r.labels.iter().map(|l| &l.text).collect::<Vec<_>>()
    );
    // A picture: a 2×2 PNG.
    let mut png = tiny_skia::Pixmap::new(2, 2).unwrap();
    png.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
    let b64 = base64::engine::general_purpose::STANDARD.encode(png.encode_png().unwrap());
    s.run("graphics.add", &json!({"shape": "picture", "points": [[0, 45], [10, 55]], "image": b64})).unwrap();
    assert!(s.run("graphics.add", &json!({"shape": "picture", "points": [[0, 45], [10, 55]], "image": "bm90IGFuIGltYWdl"})).is_err());
    let v = View { width: 800.0, height: 600.0, pixels_per_point: 1.0, camera: s.map().camera, dark: false };
    let r = crate::render::render(s.map_mut(), &v).unwrap();
    let (x, y) = v.to_screen(rustgis_geom::lonlat_to_merc(rustgis_geom::Coord::new(5.0, 50.0)));
    let px = r.pixmap.pixel(x as u32, y as u32).unwrap();
    assert!(px.red() > 200 && px.green() < 60, "the picture covers its rectangle: {px:?}");
    // Restyle the selection; keep the tool.
    s.run("select.rows", &json!({"layer": id, "rows": [0]})).unwrap();
    s.run("graphics.style", &json!({"fill": "#00FF00", "width": 3})).unwrap();
    assert_eq!(rustgis_map::graphics::symbol(&s.map().layer(id).unwrap().dataset, 0).fill, rustgis_map::Rgba::rgb(0, 255, 0));
    assert!(s.run("graphics.style", &json!({})).is_err(), "nothing to change");
    s.run("graphics.style", &json!({"text": "not on a rectangle"})).unwrap();
    assert!(rustgis_map::graphics::label(&s.map().layer(id).unwrap().dataset, 0).is_none(), "text only goes on text graphics");
    assert_eq!(rustgis_render::picture_size(&b64), Some((2, 2)));
    assert!(s.run("graphics.keepTool", &Value::Null).unwrap()["keepTool"].as_bool().unwrap());
    s.run("tool.set", &json!({"tool": "graphicRectangle"})).unwrap();
    assert_eq!(s.tool.code(), "graphicRectangle");
    // Graphics round-trip through a project file.
    let text = serde_json::to_string(&s.project).unwrap();
    let p: crate::Project = serde_json::from_str(&text).unwrap();
    assert!(p.map().layer(id).unwrap().graphics);
    s.run("graphics.newLayer", &json!({"name": "Notes"})).unwrap();
    assert!(s.map().layer_by_name("Notes").unwrap().graphics);
}

#[test]
fn layouts_compose_export_and_round_trip() {
    use base64::Engine;
    let dir = std::env::temp_dir().join(format!("rustgis-layout-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = world();
    s.run("map.goToXY", &json!({"lon": 10.0, "lat": 50.0, "scale": 20_000_000})).unwrap();
    let r = s.run("layout.new", &json!({"name": "Europe", "page": "a4", "orientation": "landscape"})).unwrap();
    assert_eq!(r["layout"], 0);
    assert_eq!(s.active_layout, Some(0));
    let l = &s.project.layouts[0];
    assert!((l.width - 297.0).abs() < 1e-9 && l.elements.len() == 6, "the map template adds a frame and five surrounds");
    let frame = l.first_frame().unwrap();
    // Frames, surrounds and the rest.
    for shape in ["circle", "ellipse", "polygon", "lasso"] {
        s.run("layout.addFrame", &json!({"shape": shape, "points": [[0, 0], [1, 0], [0.5, 1]]})).unwrap();
    }
    assert!(s.run("layout.addFrame", &json!({"shape": "polygon", "points": [[0, 0], [1, 0]]})).is_err());
    let mut png = tiny_skia::Pixmap::new(4, 4).unwrap();
    png.fill(tiny_skia::Color::from_rgba8(0, 128, 255, 255));
    let b64 = base64::engine::general_purpose::STANDARD.encode(png.encode_png().unwrap());
    for p in [
        json!({"kind": "table", "layer": "Countries", "rows": 5}),
        json!({"kind": "chart", "layer": "Countries", "field": "pop_est", "label": "name"}),
        json!({"kind": "rectangle"}),
        json!({"kind": "dynamicText", "text": "Page {page} of {pages} · {scale}"}),
        json!({"kind": "picture", "image": b64}),
        json!({"kind": "grid", "step": 10}),
    ] {
        s.run("layout.addElement", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    }
    // A chart moved to a layer without its field charts that layer's first numeric field.
    let chart = s.project.layouts[0].elements.iter().find(|e| e.kind.code() == "chart").unwrap().id;
    s.run("layout.update", &json!({"element": chart, "layer": "Cities"})).unwrap();
    let Some(rustgis_map::layout::ElementKind::Chart { layer, field, .. }) = s.project.layouts[0].element(chart).map(|e| &e.kind) else {
        panic!()
    };
    let cities = &s.map().layer_by_name("Cities").unwrap().dataset;
    assert_eq!(layer, "Cities");
    assert!(cities.field_index(field).is_some_and(|i| cities.fields[i].ty.is_numeric()), "{field}");
    s.run("layout.update", &json!({"element": chart, "layer": "Countries", "field": "pop_est"})).unwrap();
    let inset = s.run("layout.addFrame", &json!({"shape": "rectangle", "x": 200, "y": 150, "w": 60, "h": 40})).unwrap()["element"]
        .as_u64()
        .unwrap();
    s.run("layout.update", &json!({"element": inset, "scale": 200_000_000})).unwrap();
    s.run("layout.addElement", &json!({"kind": "extentIndicator", "frame": inset, "of": frame})).unwrap();
    assert!(s.run("layout.addElement", &json!({"kind": "extentIndicator", "frame": inset, "of": inset})).is_err());
    assert!(s.run("layout.addElement", &json!({"kind": "sparkles"})).is_err());
    // Frame zooms and edits.
    s.run("layout.frameZoom", &json!({"element": frame, "to": "layer", "layer": "Lakes"})).unwrap();
    s.run("layout.update", &json!({"element": frame, "x": 12, "w": 180, "center": [10, 50]})).unwrap();
    assert!(s.run("layout.update", &json!({"element": 9999, "x": 1})).is_err());
    s.run("layout.update", &json!({"element": frame, "name": "Main map"})).unwrap();
    assert!(s.run("layout.update", &json!({"element": frame, "x": 99, "center": "Paris"})).is_err());
    assert!((s.project.layouts[0].element(frame).unwrap().x - 12.0).abs() < 1e-9, "a bad update changes nothing");
    let chart = s.project.layouts[0].elements.iter().find(|e| matches!(e.kind, rustgis_map::layout::ElementKind::Chart { .. })).unwrap().id;
    assert!(s.run("layout.update", &json!({"element": chart, "field": "no_such_field"})).is_err());
    assert!(s.run("layout.update", &json!({"element": chart, "layer": "Atlantis"})).is_err());
    s.run("layout.update", &json!({"element": chart, "field": "gdp_md"})).unwrap();
    assert_eq!(s.project.layouts[0].element(frame).unwrap().name, "Main map");
    // A map series over the lakes: one page each.
    let r = s.run("layout.series", &json!({"layer": "Lakes", "nameField": "name"})).unwrap();
    let pages = r["pages"].as_u64().unwrap();
    assert!(pages > 5);
    let next = s.run("layout.seriesPage", &json!({"page": "next"})).unwrap();
    assert_eq!(next["page"], 1);
    s.run("layout.seriesRefresh", &Value::Null).unwrap();
    // A manual zoom of the series frame survives exports.
    s.run("layout.update", &json!({"element": frame, "scale": 3_000_000})).unwrap();
    let scale_of = |s: &Session| match &s.project.layouts[0].element(frame).unwrap().kind {
        rustgis_map::layout::ElementKind::MapFrame { scale, .. } => *scale,
        _ => 0.0,
    };
    // Export PNG and a PDF of every page.
    let png_path = dir.join("europe.png");
    s.run("layout.export", &json!({"path": png_path.display().to_string(), "dpi": 72})).unwrap();
    let bytes = std::fs::read(&png_path).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));
    let pdf_path = dir.join("europe.pdf");
    s.run("layout.export", &json!({"path": pdf_path.display().to_string(), "dpi": 36})).unwrap();
    let pdf = std::fs::read(&pdf_path).unwrap();
    let count = format!("/Count {pages}");
    assert!(pdf.windows(count.len()).any(|w| w == count.as_bytes()), "one PDF page per series page");
    assert_eq!(s.project.layouts[0].series.as_ref().unwrap().page, 1, "the shown page is restored");
    assert_eq!(scale_of(&s), 3_000_000.0, "and so is the frame's zoom");
    assert!(s.run("layout.export", &json!({"path": dir.join("x.svg").display().to_string()})).is_err());
    // Layout files.
    let file = dir.join("europe.rgislayout");
    s.run("layout.save", &json!({"path": file.display().to_string()})).unwrap();
    s.run("layout.import", &json!({"path": file.display().to_string()})).unwrap();
    assert_eq!(s.project.layouts.len(), 2);
    assert_eq!(s.project.layouts[1].name, "Europe 2");
    assert!(s.run("layout.import", &json!({"text": "{}"})).is_err());
    // Removing the frame removes its surrounds; undo brings them back.
    let before = s.project.layouts[1].elements.len();
    let first = s.project.layouts[1].first_frame().unwrap();
    s.run("layout.removeElement", &json!({"layout": 1, "element": first})).unwrap();
    assert!(s.project.layouts[1].elements.len() < before - 3);
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.project.layouts[1].elements.len(), before);
    // Layouts are saved with the project.
    let text = serde_json::to_string(&s.project).unwrap();
    let p = crate::Project::from_json(&text).unwrap();
    assert_eq!(p.layouts.len(), 2);
    let r = s.run("layout.rename", &json!({"name": "Europe 2", "newName": "europe"})).unwrap();
    assert_eq!(r["name"], "europe 2", "renaming keeps names unique");
    s.run("layout.rename", &json!({"name": "europe 2", "newName": "Europe 2"})).unwrap();
    s.run("layout.close", &Value::Null).unwrap();
    assert_eq!(s.active_layout, None);
    s.run("layout.remove", &json!({"name": "Europe 2"})).unwrap();
    assert_eq!(s.project.layouts.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_items_files_styles_packages_tasks_and_3d() {
    let dir = std::env::temp_dir().join(format!("rustgis-items-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |n: &str| dir.join(n).display().to_string();
    let mut s = world();
    // Folder connections.
    s.run("project.addFolder", &json!({"path": dir.display().to_string()})).unwrap();
    assert!(s.run("project.addFolder", &json!({"path": path("missing")})).is_err());
    assert_eq!(s.run("project.connections", &Value::Null).unwrap()["folders"][0]["available"], true);
    // Map and layer files.
    s.run("map.save", &json!({"path": path("world.rgismap")})).unwrap();
    s.run("map.import", &json!({"path": path("world.rgismap")})).unwrap();
    assert_eq!(s.project.maps.len(), 2);
    assert_eq!(s.project.maps[1].name, "World (2)");
    s.run("layer.symbology", &json!({"layer": "Cities", "type": "single", "color": "#AA00FF"})).ok();
    s.run("layer.save", &json!({"layer": "Cities", "path": path("cities.rgislyr")})).unwrap();
    let n = s.map().layers.len();
    s.run("layer.addData", &json!({"path": path("cities.rgislyr")})).unwrap();
    assert_eq!(s.map().layers.len(), n + 1);
    assert!(s.map().layer_by_name("Cities (2)").is_some());
    // Styles.
    s.run("style.new", &json!({"layer": "Countries", "name": "Choropleth"})).unwrap();
    s.run("style.save", &json!({"path": path("styles.rgisstyle")})).unwrap();
    s.run("style.remove", &json!({"name": "Choropleth"})).unwrap();
    s.run("style.add", &json!({"path": path("styles.rgisstyle")})).unwrap();
    assert!(s.run("style.apply", &json!({"name": "Choropleth", "layer": "Cities"})).is_err(), "polygon style on points");
    s.run("layer.symbology", &json!({"layer": "Countries", "type": "single"})).unwrap();
    s.run("style.apply", &json!({"name": "Choropleth", "layer": "Countries"})).unwrap();
    assert_eq!(s.map().layer_by_name("Countries").unwrap().renderer.kind_label(), "Graduated Colors");
    let r = s.run("style.import", &json!({"path": path("world.rgismap")})).unwrap();
    assert!(r["imported"].as_u64().unwrap() >= 4);
    // Templates keep the setup, not the features.
    s.run("project.saveTemplate", &json!({"path": path("atlas.rgistemplate")})).unwrap();
    let mut t = Session::new(Project::default());
    t.run("project.fromTemplate", &json!({"path": path("atlas.rgistemplate"), "name": "From template"})).unwrap();
    assert_eq!(t.project.name, "From template");
    assert!(t.map().layer_by_name("Countries").unwrap().dataset.features.is_empty());
    assert!(!t.project.styles.is_empty());
    // Packages open again.
    s.run("project.package", &json!({"path": path("atlas.rgispkg")})).unwrap();
    let mut o = Session::new(Project::default());
    o.run("project.open", &json!({"path": path("atlas.rgispkg")})).unwrap();
    assert_eq!(o.project.maps.len(), 2);
    assert!(o.path.is_none(), "a package opens unsaved");
    s.run("map.package", &json!({"path": path("world.rgismpk"), "mobile": true, "tolerance": 0.05})).unwrap();
    s.run("map.import", &json!({"path": path("world.rgismpk")})).unwrap();
    let full: usize = s.project.maps[0]
        .layer_by_name("Countries")
        .unwrap()
        .dataset
        .features
        .iter()
        .filter_map(|f| f.geometry.as_ref())
        .map(|g| g.coord_count())
        .sum();
    let mobile: usize = s
        .project
        .maps
        .last()
        .unwrap()
        .layer_by_name("Countries")
        .unwrap()
        .dataset
        .features
        .iter()
        .filter_map(|f| f.geometry.as_ref())
        .map(|g| g.coord_count())
        .sum();
    assert!(mobile < full, "mobile packages simplify: {mobile} < {full}");
    assert!(s.run("gp.package", &json!({"path": path("results.rgisgpk")})).is_err(), "no runs yet");
    s.run("map.activate", &json!({"name": "World"})).unwrap();
    s.run("gp.run", &json!({"tool": "buffer", "params": {"input": "Cities", "distance": 100}})).unwrap();
    s.run("gp.package", &json!({"path": path("results.rgisgpk")})).unwrap();
    let entries = rustgis_data::zip::read(&std::fs::read(path("results.rgisgpk")).unwrap()).unwrap();
    assert!(entries.iter().any(|(n, _)| n == "history.json") && entries.iter().any(|(n, _)| n.starts_with("results/")));
    // Layer templates.
    let id = s.run("layer.template", &json!({"template": "places"})).unwrap()["layer"].as_u64().unwrap() as u32;
    assert_eq!(s.map().layer(id).unwrap().dataset.fields.len(), 4);
    assert!(s.run("layer.template", &json!({"template": "unicorns"})).is_err());
    // Tasks.
    s.run("task.new", &json!({"name": "Explore", "steps": [{"title": "Zoom to Europe", "command": "map.goToXY", "params": {"lon": 10, "lat": 50, "scale": 20000000}}, {"title": "Select big countries", "command": "select.byAttributes", "params": {"layer": "Countries", "where": "pop_est > 50000000"}}]})).unwrap();
    assert!(s.run("task.new", &json!({"steps": [{"title": "x", "command": "no.such"}]})).is_err());
    let r = s.run("task.run", &json!({"task": "Explore"})).unwrap();
    assert_eq!(r["ran"].as_array().unwrap().len(), 2);
    assert!(s.map().selection_count() > 5);
    s.run("task.addStep", &json!({"task": "Explore", "title": "Clear", "command": "select.clear"})).unwrap();
    s.run("task.run", &json!({"task": "Explore", "step": 2})).unwrap();
    assert_eq!(s.map().selection_count(), 0);
    s.run("task.new", &json!({"name": "Replay", "fromHistory": true})).unwrap();
    assert!(s.trusted_tasks.contains("replay"), "history run in this session");
    // History that came with an opened project is someone else's.
    s.history_from_file = 1;
    s.run("task.new", &json!({"name": "Replay file", "fromHistory": true})).unwrap();
    assert!(!s.trusted_tasks.contains("replay file"));
    s.history_from_file = 0;
    assert_eq!(s.project.tasks.last().unwrap().steps.len(), 1);
    s.run("task.save", &json!({"task": "Explore", "path": path("explore.rgistask")})).unwrap();
    s.run("task.import", &json!({"path": path("explore.rgistask")})).unwrap();
    assert_eq!(s.project.tasks.last().unwrap().name, "Explore 2");
    // A task file may not touch files or run scripts.
    let sneaky =
        json!({"name": "Sneaky", "steps": [{"title": "Overwrite", "command": "share.exportMap", "params": {"path": path("x.png")}}]});
    assert!(s.run("task.import", &json!({"text": sneaky.to_string()})).is_err());
    let nested = json!({"name": "Nested", "steps": [{"title": "Run", "command": "script.run", "params": {"script": "map.zoomFull"}}]});
    assert!(s.run("task.import", &json!({"text": nested.to_string()})).is_err());
    // A task arriving inside a project file is vetted when it runs.
    s.project.tasks.push(crate::project::Task {
        name: "From a file".into(),
        description: String::new(),
        steps: vec![crate::project::TaskStep {
            title: "Export".into(),
            instructions: String::new(),
            command: "share.exportMap".into(),
            params: json!({"path": path("y.png")}),
        }],
    });
    assert!(s.run("task.run", &json!({"task": "From a file"})).is_err());
    assert!(!std::path::Path::new(&path("y.png")).exists());
    // A step naming a feature layer runs even when a raster is selected.
    s.run("task.new", &json!({"name": "Layered", "steps": [{"title": "Zoom", "command": "layer.zoomTo", "params": {"layer": "Lakes"}}]}))
        .unwrap();
    s.run("task.run", &json!({"task": "Layered"})).unwrap();
    // 3D export: polygons extruded by a field.
    s.run("share.export3d", &json!({"layer": "Countries", "path": path("countries.obj"), "heightField": "pop_est", "zScale": 0.00001}))
        .unwrap();
    let obj = std::fs::read_to_string(path("countries.obj")).unwrap();
    assert!(obj.lines().filter(|l| l.starts_with("v ")).count() > 1000 && obj.contains("\nf "));
    // Caps are triangles, and every face refers to a written vertex.
    let verts = obj.lines().filter(|l| l.starts_with("v ")).count();
    assert!(obj.lines().filter(|l| l.starts_with("f ")).all(|l| {
        let ids: Vec<usize> = l[2..].split_whitespace().map(|x| x.parse().unwrap()).collect();
        (3..=4).contains(&ids.len()) && ids.iter().all(|i| (1..=verts).contains(i))
    }));
    let (name, bytes) = crate::cmd::items::item_bytes(&mut s, "obj", &json!({"layer": "Cities"})).unwrap();
    assert!(name.ends_with(".obj") && !bytes.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn data_design_rules_hold_on_every_edit() {
    let mut s = world();
    s.run("layer.template", &json!({"template": "routes", "name": "Roads"})).unwrap();
    s.run("edit.addField", &json!({"layer": "Roads", "name": "kind", "type": "long"})).unwrap();
    s.run("edit.addField", &json!({"layer": "Roads", "name": "lanes", "type": "long"})).unwrap();
    // Domains.
    s.run("data.domain", &json!({"layer": "Roads", "field": "surface", "coded": [["paved", "Paved"], ["gravel", "Gravel"]]})).unwrap();
    s.run("data.domain", &json!({"layer": "Roads", "field": "lanes", "min": 1, "max": 8})).unwrap();
    assert!(s.run("data.domain", &json!({"layer": "Roads", "field": "name", "min": 1, "max": 2})).is_err(), "text takes coded values");
    let line = json!({"type": "LineString", "coordinates": [[0, 0], [1, 1]]});
    s.run("edit.addFeature", &json!({"layer": "Roads", "geometry": line, "attributes": {"surface": "paved", "lanes": 2}})).unwrap();
    assert!(s.run("edit.addFeature", &json!({"layer": "Roads", "geometry": line, "attributes": {"surface": "ice"}})).is_err());
    assert!(s.run("edit.setValue", &json!({"layer": "Roads", "row": 0, "field": "lanes", "value": 12})).is_err());
    assert_eq!(s.map().layer_by_name("Roads").unwrap().dataset.features.len(), 1, "the failed edits were undone");
    // Subtypes fill defaults on new features.
    s.run("data.subtypes", &json!({"layer": "Roads", "field": "kind", "subtypes": [{"code": 1, "name": "Highway", "defaults": {"lanes": 4}}, {"code": 2, "name": "Street", "defaults": {"lanes": 1}}]})).unwrap();
    assert!(s.run("data.subtypes", &json!({"layer": "Roads", "field": "name", "subtypes": [{"code": 1, "name": "x"}]})).is_err());
    let r = s.run("edit.addFeature", &json!({"layer": "Roads", "geometry": line, "attributes": {"kind": 1}})).unwrap();
    let row = r["row"].as_u64().unwrap() as usize;
    let lanes = |s: &Session, row: usize| {
        let l = s.map().layer_by_name("Roads").unwrap();
        l.dataset.value(row, l.dataset.field_index("lanes").unwrap()).clone()
    };
    assert_eq!(lanes(&s, row).as_f64(), Some(4.0));
    // Attribute rules: a calculation and a constraint.
    s.run(
        "data.attributeRule",
        &json!({"layer": "Roads", "name": "Length", "kind": "calculation", "field": "length_km", "expression": "lanes * 10"}),
    )
    .unwrap();
    s.run(
        "data.attributeRule",
        &json!({"layer": "Roads", "name": "Named highways", "kind": "constraint", "expression": "kind <> 1 OR name IS NOT NULL"}),
    )
    .unwrap();
    assert!(
        s.run("edit.addFeature", &json!({"layer": "Roads", "geometry": line, "attributes": {"kind": 1}})).is_err(),
        "highways need names"
    );
    let r = s.run("edit.addFeature", &json!({"layer": "Roads", "geometry": line, "attributes": {"kind": 1, "name": "A1"}})).unwrap();
    let row = r["row"].as_u64().unwrap() as usize;
    let l = s.map().layer_by_name("Roads").unwrap();
    assert_eq!(l.dataset.value(row, l.dataset.field_index("length_km").unwrap()).as_f64(), Some(40.0));
    assert!(s.run("data.attributeRule", &json!({"layer": "Roads", "name": "Bad", "kind": "constraint", "expression": "(("})).is_err());
    s.run("data.attributeRule", &json!({"layer": "Roads", "name": "Named highways", "remove": true})).unwrap();
    // Deleting a field a rule uses drops the rule, so editing isn't blocked.
    s.run("edit.addField", &json!({"layer": "Roads", "name": "temp", "type": "double"})).unwrap();
    s.run("data.attributeRule", &json!({"layer": "Roads", "name": "Temp", "kind": "constraint", "expression": "temp IS NULL OR temp > 0"}))
        .unwrap();
    let r = s.run("edit.deleteField", &json!({"layer": "Roads", "field": "temp"})).unwrap();
    assert_eq!(r["rulesRemoved"], 1);
    s.run("edit.addFeature", &json!({"layer": "Roads", "geometry": line, "attributes": {"kind": 2}})).unwrap();
    // Contingent values.
    s.run("data.contingentValues", &json!({"layer": "Roads", "name": "Surface by kind", "fields": ["kind", "surface"], "combos": [[1, "paved"], [2, "paved"], [2, "gravel"]]})).unwrap();
    assert!(
        s.run("edit.addFeature", &json!({"layer": "Roads", "geometry": line, "attributes": {"kind": 1, "surface": "gravel"}})).is_err()
    );
    s.run("edit.addFeature", &json!({"layer": "Roads", "geometry": line, "attributes": {"kind": 2, "surface": "gravel"}})).unwrap();
    // Templates.
    s.run("edit.template", &json!({"layer": "Roads", "name": "Gravel street", "defaults": {"kind": 2, "surface": "gravel"}})).unwrap();
    let r = s.run("edit.addFeature", &json!({"layer": "Roads", "geometry": line, "template": "Gravel street"})).unwrap();
    assert_eq!(lanes(&s, r["row"].as_u64().unwrap() as usize).as_f64(), Some(1.0), "template, then subtype defaults");
    // Archive and restore.
    s.run("data.archive", &json!({"layer": "Roads"})).unwrap();
    let t0 = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap().as_secs_f64() - 0.001;
    let n = s.map().layer_by_name("Roads").unwrap().dataset.features.len();
    s.run("edit.addFeature", &json!({"layer": "Roads", "geometry": line, "template": "Gravel street"})).unwrap();
    s.run("edit.setValue", &json!({"layer": "Roads", "row": 0, "field": "name", "value": "Main"})).unwrap();
    let h = s.run("data.archiveHistory", &json!({"layer": "Roads"})).unwrap();
    assert_eq!(h["edits"], 2);
    s.run("data.archiveRestore", &json!({"layer": "Roads", "time": t0})).unwrap();
    let l = s.map().layer_by_name("Roads").unwrap();
    assert_eq!(l.dataset.features.len(), n);
    assert!(l.dataset.value(0, l.dataset.field_index("name").unwrap()).is_null());
    // Edit status, and turning editing off.
    let st = s.run("edit.status", &Value::Null).unwrap();
    assert!(st["layers"].as_array().unwrap().iter().any(|l| l["layer"] == "Roads" && l["added"].as_u64().unwrap() >= 5));
    s.run("edit.enable", &json!({"on": false})).unwrap();
    assert!(s.run("edit.addFeature", &json!({"layer": "Roads", "geometry": line})).is_err());
    s.run("edit.enable", &json!({"on": true})).unwrap();
}

#[test]
fn modify_tools_snapping_topology_relationships_and_elevation() {
    let mut s = with_raster();
    s.run("layer.template", &json!({"template": "lines", "name": "Paths"})).unwrap();
    let add = |s: &mut Session, coords: Value| {
        s.run("edit.addFeature", &json!({"layer": "Paths", "geometry": {"type": "LineString", "coordinates": coords}})).unwrap()["row"]
            .as_u64()
            .unwrap() as usize
    };
    let a = add(&mut s, json!([[0, 0], [1, 0]]));
    // Move one degree east (about 111 km at the equator), then rotate and scale.
    s.run("select.rows", &json!({"layer": "Paths", "rows": [a]})).unwrap();
    s.run("edit.move", &json!({"dx": 111_319.5, "dy": 0})).unwrap();
    let first = |s: &Session, row: usize| match s.map().layer_by_name("Paths").unwrap().dataset.features[row].geometry.as_ref().unwrap() {
        rustgis_data::Geometry::Line(p) => p[0][0],
        _ => panic!("not a line"),
    };
    assert!((first(&s, a).x - 1.0).abs() < 1e-3, "{:?}", first(&s, a));
    s.run("edit.rotate", &json!({"angle": 90})).unwrap();
    s.run("edit.scale", &json!({"factor": 2})).unwrap();
    s.run("edit.flip", &Value::Null).unwrap();
    // Split, then merge back, then explode.
    let b = add(&mut s, json!([[10, 10], [12, 10]]));
    s.run("select.rows", &json!({"layer": "Paths", "rows": [b]})).unwrap();
    s.run("edit.split", &json!({"lon": 11, "lat": 10.2})).unwrap();
    let n = s.map().layer_by_name("Paths").unwrap().dataset.features.len();
    s.run("select.rows", &json!({"layer": "Paths", "rows": [b, n - 1]})).unwrap();
    // A merge the rules reject leaves the layer as it was.
    s.run("data.attributeRule", &json!({"layer": "Paths", "name": "Never", "kind": "constraint", "expression": "name = 'never'"})).unwrap();
    let before = s.map().layer_by_name("Paths").unwrap().dataset.features.clone();
    assert!(s.run("edit.merge", &Value::Null).is_err());
    assert_eq!(s.map().layer_by_name("Paths").unwrap().dataset.features, before);
    s.run("data.attributeRule", &json!({"layer": "Paths", "name": "Never", "remove": true})).unwrap();
    s.run("select.rows", &json!({"layer": "Paths", "rows": [b, n - 1]})).unwrap();
    s.run("edit.merge", &Value::Null).unwrap();
    assert_eq!(s.map().layer_by_name("Paths").unwrap().dataset.features.len(), n - 1);
    s.run("edit.explode", &Value::Null).unwrap();
    assert_eq!(s.map().layer_by_name("Paths").unwrap().dataset.features.len(), n);
    assert!(s.run("edit.setGeometry", &json!({"layer": "Paths", "row": 0, "geometry": {"type": "Point", "coordinates": [0, 0]}})).is_err());
    s.run("edit.setGeometry", &json!({"layer": "Paths", "row": 0, "geometry": {"type": "LineString", "coordinates": [[0, 0], [0, 1]]}}))
        .unwrap();
    // Traverse: two legs from a start, ground to grid off and on.
    s.run("layer.select", &json!({"layer": "Paths"})).unwrap();
    let r = s
        .run("edit.traverse", &json!({"start": [20, 0], "legs": [{"distance": 1000, "bearing": 90}, {"distance": 1000, "bearing": 0}]}))
        .unwrap();
    let row = r["row"].as_u64().unwrap() as usize;
    let end = |s: &Session, row: usize| match s.map().layer_by_name("Paths").unwrap().dataset.features[row].geometry.as_ref().unwrap() {
        rustgis_data::Geometry::Line(p) => *p[0].last().unwrap(),
        _ => panic!("not a line"),
    };
    let e = end(&s, row);
    assert!((e.x - 20.008983).abs() < 1e-5 && (e.y - 0.009043).abs() < 1e-4, "{e:?}");
    s.run("edit.groundToGrid", &json!({"scale": 1.5, "rotation": 0})).unwrap();
    let r = s.run("edit.traverse", &json!({"start": [20, 0], "legs": [{"distance": 1000, "bearing": 90}]})).unwrap();
    assert!((end(&s, r["row"].as_u64().unwrap() as usize).x - 20.013475).abs() < 1e-5);
    assert!(s.run("edit.groundToGrid", &json!({"scale": 9})).is_err());
    s.run("edit.groundToGrid", &json!({"on": false})).unwrap();
    // Snapping to a vertex near the pointer.
    s.run("map.goToXY", &json!({"lon": 20, "lat": 0, "scale": 50_000})).unwrap();
    let off = s.run("edit.snap", &json!({"lon": 20.0001, "lat": 0.0001})).unwrap();
    assert_eq!(off["snapped"], false, "snapping is off");
    s.run("edit.snapping", &json!({"on": true, "tolerancePx": 20})).unwrap();
    let on = s.run("edit.snap", &json!({"lon": 20.0001, "lat": 0.0001})).unwrap();
    assert_eq!(on["snapped"], true);
    assert!((on["lon"].as_f64().unwrap() - 20.0).abs() < 1e-9 && on["lat"].as_f64().unwrap().abs() < 1e-9, "{on}");
    // Topology: overlapping countries? Lakes inside countries? Dangling paths.
    s.run("topology.rules", &json!({"add": {"rule": "mustNotHaveDangles", "layer": "Paths"}})).unwrap();
    s.run("topology.rules", &json!({"add": {"rule": "mustBeCoveredBy", "layer": "Cities", "by": "Countries"}})).unwrap();
    assert!(s.run("topology.rules", &json!({"add": {"rule": "mustNotOverlap", "layer": "Paths"}})).is_err(), "overlap is for polygons");
    let v = s.run("topology.validate", &Value::Null).unwrap();
    assert!(v["errors"].as_u64().unwrap() > 0);
    assert_eq!(s.topology_errors.len() as u64, v["errors"].as_u64().unwrap());
    // Relationships: cities to their country by ISO code.
    s.run("data.relate", &json!({"name": "Country of city", "origin": "Countries", "originKey": "name", "destination": "Cities", "destinationKey": "adm0name"})).unwrap();
    assert!(
        s.run(
            "data.relate",
            &json!({"name": "x", "origin": "Countries", "originKey": "nope", "destination": "Cities", "destinationKey": "adm0name"})
        )
        .is_err()
    );
    s.run("select.byAttributes", &json!({"layer": "Countries", "where": "name = 'France'"})).unwrap();
    let rel = s.run("data.related", &Value::Null).unwrap();
    assert!(rel["related"].as_u64().unwrap() >= 1, "{rel}");
    // Elevation from the raster surface goes in z.
    s.run("edit.elevation", &json!({"mode": "constant", "z": 125})).unwrap();
    s.run("layer.template", &json!({"template": "points", "name": "Spots"})).unwrap();
    s.run("edit.addFeature", &json!({"layer": "Spots", "geometry": {"type": "Point", "coordinates": [0, 0]}})).unwrap();
    let l = s.map().layer_by_name("Spots").unwrap();
    assert_eq!(l.dataset.value(0, l.dataset.field_index("z").unwrap()).as_f64(), Some(125.0));
    assert!(s.run("edit.elevation", &json!({"mode": "surface", "surface": "Cities"})).is_err(), "not a raster");
    let tools = s.run("edit.tools", &Value::Null).unwrap();
    assert!(tools.as_array().unwrap().len() >= 9);
}

/// A 40×40 lon/lat DEM near (10°E, 45°N): a hill in the middle, with a wall to its east.
fn dem_b64() -> String {
    use base64::Engine;
    use rustgis_data::raster::{ColorHint, Crs, GeoTransform, Raster, geotiff};
    let gt = GeoTransform { origin_x: 10.0, origin_y: 45.04, pixel_width: 0.001, pixel_height: -0.001 };
    let z: Vec<f32> = (0..1600)
        .map(|i| {
            let (c, r) = ((i % 40) as f32 - 20.0, (i / 40) as f32 - 20.0);
            let hill = 100.0 * (-(c * c + r * r) / 60.0).exp();
            if i % 40 == 30 { 400.0 } else { 200.0 + hill }
        })
        .collect();
    let r = Raster::new("dem", 40, 40, vec![z], gt, Crs::LonLat, None, ColorHint::Gray, "Float32").unwrap();
    base64::engine::general_purpose::STANDARD.encode(geotiff::write(&r, 0).unwrap())
}

#[test]
fn analysis_workflows_models_scripts_and_raster_functions() {
    let mut s = world();
    s.run("layer.addDataBytes", &json!({"fileName": "dem.tif", "data": dem_b64()})).unwrap();
    let dem = s.map().layers.iter().find(|l| l.is_raster()).unwrap().name.clone();
    // Raster functions and a chain.
    for (func, extra) in [
        ("slope", json!({})),
        ("aspect", json!({})),
        ("hillshade", json!({"azimuth": 300})),
        ("threshold", json!({"value": 250})),
        ("bandMath", json!({"expression": "b1 * 2 - 100"})),
    ] {
        let mut p = json!({"layer": dem, "function": func});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        s.run("raster.function", &p).unwrap_or_else(|e| panic!("{func}: {e}"));
    }
    assert!(s.run("raster.function", &json!({"layer": dem, "function": "warp"})).is_err());
    assert!(s.run("raster.function", &json!({"layer": dem, "function": "bandMath", "expression": "b2"})).is_err());
    let r = s
        .run(
            "raster.functionChain",
            &json!({"layer": dem, "steps": [{"function": "slope"}, {"function": "threshold", "value": 10}], "name": "Steep"}),
        )
        .unwrap();
    let steep = s.map().layer(r["layer"].as_u64().unwrap() as u32).unwrap();
    assert_eq!(steep.name, "Steep");
    let data = steep.raster.as_ref().unwrap().data.as_ref().unwrap();
    assert!(data.bands[0].stats.max == 1.0 && data.bands[0].stats.min == 0.0);
    // Rasters made by functions save with the project.
    let text = serde_json::to_string(&s.project).unwrap();
    let back = crate::Project::from_json(&text).unwrap();
    assert!(back.map().layer_by_name("Steep").unwrap().raster.as_ref().unwrap().data.is_some());
    // Suitability: high and flat.
    let slope_name = format!("{dem}_slope");
    let r = s
        .run(
            "analysis.suitability",
            &json!({"criteria": [
        {"layer": dem, "ranges": [[0, 250, 1], [250, 1000, 10]], "weight": 2},
        {"layer": slope_name, "ranges": [[0, 5, 10], [5, 90, 1]], "weight": 1}]}),
        )
        .unwrap();
    assert!(r["layer"].as_u64().is_some());
    // Viewshed and line of sight: the wall at column 30 hides what's beyond it.
    // On the flat northern edge: everything west of the wall is in view.
    let v = s.run("analysis.viewshed", &json!({"layer": dem, "lon": 10.0055, "lat": 45.0395, "height": 2})).unwrap();
    assert!(v["visibleCells"].as_u64().unwrap() > 100, "{v}");
    let los = s.run("analysis.lineOfSight", &json!({"layer": dem, "from": [10.0205, 45.0395], "to": [10.0355, 45.0395]})).unwrap();
    assert_eq!(los["visible"], false);
    let los = s.run("analysis.lineOfSight", &json!({"layer": dem, "from": [10.0205, 45.0395], "to": [10.0055, 45.0395]})).unwrap();
    assert_eq!(los["visible"], true);
    // IDW of city populations, with an environment extent and cell size.
    s.run("gp.env", &json!({"extent": [-10, 35, 30, 60], "cellSize": 0.5})).unwrap();
    let r = s.run("analysis.interpolate", &json!({"layer": "Cities", "field": "pop_max"})).unwrap();
    let idw = s.map().layer(r["layer"].as_u64().unwrap() as u32).unwrap().raster.as_ref().unwrap().data.clone().unwrap();
    assert_eq!((idw.width, idw.height), (80, 50));
    // The environment extent limits tools to the features meeting it.
    s.run("gp.run", &json!({"tool": "buffer", "params": {"input": "Cities", "distance": 10, "output": "Europe buffers"}})).unwrap();
    let europe = s.map().layer_by_name("Europe buffers").unwrap().dataset.features.len();
    let all = s.map().layer_by_name("Cities").unwrap().dataset.features.len();
    assert!(europe > 10 && europe < all / 2, "{europe} of {all}");
    s.run("gp.env", &json!({"extent": null, "cellSize": null})).unwrap();
    // Network route along the rivers: two points on the same river.
    let rivers = s.map().layer_by_name("Rivers").unwrap();
    let (a, b2) = match rivers.dataset.features.iter().filter_map(|f| f.geometry.as_ref()).find_map(|g| match g {
        rustgis_data::Geometry::Line(p) if p[0].len() > 10 => Some((p[0][0], p[0][p[0].len() - 1])),
        _ => None,
    }) {
        Some(x) => x,
        None => panic!("no river"),
    };
    let r = s.run("analysis.route", &json!({"layer": "Rivers", "from": [a.x, a.y], "to": [b2.x, b2.y]})).unwrap();
    assert!(r["lengthKm"].as_f64().unwrap() > 10.0, "{r}");
    // Neighbourhood, simulation, profile, exploration, similarity, conversion.
    let n = s
        .run("analysis.neighborhood", &json!({"lon": 2.35, "lat": 48.85, "radius": 300000, "layers": ["Cities"], "select": true}))
        .unwrap();
    assert!(n["layers"][0]["count"].as_u64().unwrap() >= 2);
    s.run("select.byAttributes", &json!({"layer": "Countries", "where": "name = 'France'"})).unwrap();
    let sim = s.run("analysis.simulate", &json!({"layer": "Countries", "count": 20, "seed": 3})).unwrap();
    assert_eq!(sim["points"], 20);
    s.run("select.clear", &Value::Null).unwrap();
    let prof = s.run("data.profile", &json!({"layer": "Countries"})).unwrap();
    assert!(prof["fields"].as_array().unwrap().iter().any(|f| f["field"] == "pop_est" && f["min"].as_f64().is_some()));
    let ex = s.run("data.explore", &json!({"layer": "Countries", "fields": ["pop_est", "gdp_md"]})).unwrap();
    assert!(ex["correlation"][0][1].as_f64().unwrap() > 0.3, "population and GDP correlate");
    let fr = s.map().layer_by_name("Countries").unwrap().dataset.features.iter().position(|_| true).unwrap();
    let sm = s.run("data.similar", &json!({"layer": "Countries", "row": fr, "fields": ["pop_est", "gdp_md"], "count": 3})).unwrap();
    assert_eq!(sm["similar"].as_array().unwrap().len(), 3);
    for fmt in ["kml", "gpx", "wkt", "geojson", "csv"] {
        let t = s.run("data.convert", &json!({"layer": "Cities", "format": fmt})).unwrap();
        assert!(t["text"].as_str().unwrap().len() > 1000, "{fmt}");
    }
    // Models chain tools; scripts run commands.
    s.run(
        "model.new",
        &json!({"name": "Buffer then dissolve", "steps": [
        {"tool": "buffer", "params": {"input": "Cities", "distance": 50}},
        {"tool": "dissolve", "params": {"input": "%1"}}]}),
    )
    .unwrap();
    let r = s.run("model.run", &json!({"name": "Buffer then dissolve"})).unwrap();
    assert_eq!(r["ran"].as_array().unwrap().len(), 2);
    assert!(s.run("model.new", &json!({"steps": [{"tool": "nope", "params": {}}]})).is_err());
    // A model carried in a file can't touch files.
    s.project.models.push(crate::project::Model {
        name: "From a file".into(),
        steps: vec![crate::project::ModelStep {
            tool: "buffer".into(),
            params: json!({"input": "Cities", "distance": 5, "output": "/tmp/x.shp"}),
        }],
    });
    let e = s.run("model.run", &json!({"name": "From a file"})).unwrap_err();
    assert!(e.contains("from a file"), "{e}");
    let m = s.run("model.new", &json!({"name": "Replay", "fromHistory": true})).unwrap();
    assert!(m["steps"].as_u64().unwrap() >= 2);
    let dissolve = s.project.models.last().unwrap().steps.iter().find(|st| st.tool == "dissolve").unwrap();
    assert!(dissolve.params["input"].as_str().unwrap().starts_with('%'), "history inputs wire to earlier steps");
    let r = s.run("script.run", &json!({"script": "# zoom then select\nmap.goToXY {\"lon\": 2, \"lat\": 48, \"scale\": 5000000}\nselect.byAttributes {\"layer\": \"Cities\", \"where\": \"pop_max > 1000000\"}"})).unwrap();
    assert_eq!(r["ran"].as_array().unwrap().len(), 2);
    assert!(s.run("script.run", &json!({"script": "no.such {}"})).is_err());
    let g = s.run("gp.gallery", &json!({"kind": "raster"})).unwrap();
    assert!(g.as_array().unwrap().iter().any(|t| t["id"] == "raster.function"));
}

#[test]
fn layer_effects_and_imagery_tools() {
    use crate::render::View;
    let mut s = world();
    s.run("layer.addDataBytes", &json!({"fileName": "dem.tif", "data": dem_b64()})).unwrap();
    let dem = s.map().layers.iter().find(|l| l.is_raster()).unwrap().name.clone();
    s.run("map.goToXY", &json!({"lon": 10.0, "lat": 48.0, "scale": 20_000_000})).unwrap();
    let view = |s: &Session| View { width: 400.0, height: 300.0, pixels_per_point: 1.0, camera: s.map().camera, dark: false };
    let png = |s: &mut Session| {
        let v = view(s);
        crate::render::render(s.map_mut(), &v).unwrap().pixmap
    };
    // Every effect renders.
    s.run("layer.blend", &json!({"layer": "Countries", "mode": "multiply"})).unwrap();
    assert!(s.run("layer.blend", &json!({"layer": "Countries", "mode": "dodge"})).is_err());
    s.run("layer.mask", &json!({"layer": "Cities", "mask": "Countries"})).unwrap();
    assert!(s.run("layer.mask", &json!({"layer": "Countries", "mask": "Countries"})).is_err());
    assert!(s.run("layer.mask", &json!({"layer": "Countries", "mask": "Cities"})).is_err(), "masks are polygons");
    s.run("layer.swipe", &json!({"layer": "Countries", "position": 0.5})).unwrap();
    let swiped = png(&mut s);
    // Right of the divider the countries are gone: only water and basemap land remain.
    s.run("layer.swipe", &json!({"layer": "Countries", "off": true})).unwrap();
    let full = png(&mut s);
    assert_ne!(swiped.data(), full.data());
    s.run("layer.displayFilters", &json!({"layer": "Cities", "filters": [{"name": "Big at small scales", "minScale": 1e12, "maxScale": 5_000_000, "where": "pop_max > 5000000"}]})).unwrap();
    assert!(s.run("layer.displayFilters", &json!({"layer": "Cities", "filters": [{"where": "(("}]})).is_err());
    s.run("layer.aggregation", &json!({"layer": "Cities", "shape": "hexagon", "size": 30})).unwrap();
    assert!(s.run("layer.aggregation", &json!({"layer": "Countries", "shape": "hexagon"})).is_err());
    png(&mut s);
    s.run("layer.aggregation", &json!({"layer": "Cities", "shape": "square"})).unwrap();
    png(&mut s);
    s.run("layer.extrusion", &json!({"layer": "Countries", "type": "field", "field": "pop_est", "factor": 0.002})).unwrap();
    s.run("layer.faces", &json!({"layer": "Countries", "culling": false, "lighting": true})).unwrap();
    s.run("layer.lightShadow", &json!({"layer": "Countries", "shadows": true})).unwrap();
    let extruded = png(&mut s);
    assert_ne!(extruded.data(), full.data());
    assert!(s.run("layer.faces", &json!({"layer": "Lakes", "culling": true})).is_err(), "no extrusion yet");
    s.run("raster.rotation", &json!({"layer": dem, "angle": 30})).unwrap();
    s.run("map.goToXY", &json!({"lon": 10.02, "lat": 45.02, "scale": 50_000})).unwrap();
    png(&mut s);
    assert!(s.run("raster.rotation", &json!({"layer": "Countries", "angle": 30})).is_err());
    s.run("raster.rotation", &json!({"layer": dem, "angle": 0})).unwrap();
    // Imagery.
    let r = s.run("raster.georeference", &json!({"layer": dem, "points": [[0, 0, 20.0, 50.0], [40, 40, 20.4, 49.6]]})).unwrap();
    let e = r["extent"].as_array().unwrap();
    assert!((e[0].as_f64().unwrap() - 20.0).abs() < 1e-6 && (e[3].as_f64().unwrap() - 50.0).abs() < 1e-6, "{r}");
    assert!(
        s.run("raster.georeference", &json!({"layer": dem, "points": [[0, 0, 20.0, 50.0], [40, 40, 19.0, 51.0]]})).is_err(),
        "north up only"
    );
    s.run("raster.georeference", &json!({"layer": dem, "extent": [10.0, 45.0, 10.04, 45.04]})).unwrap();
    let sp = s.run("raster.spectral", &json!({"layer": dem, "lon": 10.0205, "lat": 45.0195})).unwrap();
    assert!(sp["values"][0].as_f64().unwrap() > 290.0);
    s.run("raster.index", &json!({"layer": dem, "index": "ndvi", "nir": 1, "red": 1})).unwrap();
    let r = s.run("raster.classify", &json!({"layer": dem, "classes": 3})).unwrap();
    // Forms send numbers as floats; whole ones are accepted.
    s.run("raster.classify", &json!({"layer": dem, "classes": 3.0, "name": "float_classes"})).unwrap();
    assert!(s.run("raster.classify", &json!({"layer": dem, "classes": 2.5})).is_err());
    assert!(r["layer"].as_u64().is_some());
    s.run("raster.function", &json!({"layer": dem, "function": "bandMath", "expression": "b1 + 10", "name": "dem_later"})).unwrap();
    let ch = s.run("raster.change", &json!({"before": dem, "after": "dem_later", "method": "difference"})).unwrap();
    let ch_layer = s.map().layer(ch["layer"].as_u64().unwrap() as u32).unwrap().raster.as_ref().unwrap().data.clone().unwrap();
    assert!((ch_layer.bands[0].stats.mean - 10.0).abs() < 0.01);
    s.run("raster.process", &json!({"layer": dem, "operation": "clip", "extent": [10.005, 45.005, 10.02, 45.02]})).unwrap();
    let r = s.run("raster.process", &json!({"layer": dem, "operation": "resample", "factor": 4})).unwrap();
    let small = s.map().layer(r["layer"].as_u64().unwrap() as u32).unwrap().raster.as_ref().unwrap().data.clone().unwrap();
    assert_eq!((small.width, small.height), (10, 10));
    let r = s
        .run(
            "raster.editPixels",
            &json!({"layer": dem, "polygon": [[10.0, 45.0], [10.01, 45.0], [10.01, 45.01], [10.0, 45.01]], "value": 0}),
        )
        .unwrap();
    assert_eq!(r["pixels"], 100);
    // Supervised classification from two training squares.
    s.run("layer.template", &json!({"template": "polygons", "name": "Training"})).unwrap();
    for (name, x) in [("low", 10.002), ("high", 10.018)] {
        s.run("edit.addFeature", &json!({"layer": "Training", "geometry": {"type": "Polygon", "coordinates": [[[x, 45.018], [x + 0.004, 45.018], [x + 0.004, 45.022], [x, 45.022], [x, 45.018]]]}, "attributes": {"name": name}})).unwrap();
    }
    let r = s.run("raster.classifySupervised", &json!({"layer": dem, "training": "Training", "classField": "name"})).unwrap();
    assert_eq!(r["classes"].as_array().unwrap().len(), 2, "{r}");
    // Mensuration.
    let m = s.run("raster.measure", &json!({"layer": dem, "kind": "height", "points": [[10.0015, 45.0385], [10.0205, 45.0195]]})).unwrap();
    assert!(m["metres"].as_f64().unwrap() > 50.0, "{m}");
    s.run(
        "raster.measure",
        &json!({"layer": dem, "kind": "volume", "points": [[10.015, 45.015], [10.025, 45.015], [10.025, 45.025], [10.015, 45.025]]}),
    )
    .unwrap();
    s.run("raster.measure", &json!({"kind": "area", "points": [[0, 0], [1, 0], [1, 1]]})).unwrap();
    assert_eq!(s.run("raster.measurements", &Value::Null).unwrap().as_array().unwrap().len(), 3);
    // Workspaces: an orthomosaic of two rasters, a DSM mosaic and its mesh.
    s.run("imagery.workspace", &json!({"kind": "ortho", "name": "Valley", "images": [dem, "dem_later"]})).unwrap();
    s.run("imagery.workspaceOptions", &json!({"name": "Valley", "cellSize": 0.002})).unwrap();
    let r = s.run("imagery.mosaic", &json!({"name": "Valley"})).unwrap();
    let mo = s.map().layer(r["layer"].as_u64().unwrap() as u32).unwrap().raster.as_ref().unwrap().data.clone().unwrap();
    assert_eq!((mo.width, mo.height), (20, 20));
    s.run("imagery.workspace", &json!({"kind": "reality", "images": ["dem_later"]})).unwrap();
    let r = s.run("imagery.mosaic", &json!({"name": "Reality Mapping"})).unwrap();
    let dsm = s.map().layer(r["layer"].as_u64().unwrap() as u32).unwrap().name.clone();
    let mesh = s.run("imagery.mesh", &json!({"layer": dsm})).unwrap();
    assert!(mesh["faces"].as_u64().unwrap() > 100);
    // Meshes go to .obj files; workspace methods are first or mean, checked on creation too.
    assert!(s.run("imagery.mesh", &json!({"layer": dsm, "path": "/tmp/dsm.png"})).is_err());
    assert!(s.run("imagery.workspace", &json!({"kind": "ortho", "images": [dem], "method": "median"})).is_err());
    assert!(s.run("imagery.workspace", &json!({"kind": "ortho", "images": [dem], "cellSize": 20})).is_err());
    // A mean mosaic keeps fractional values.
    s.run("imagery.workspaceOptions", &json!({"name": "Valley", "method": "mean"})).unwrap();
    let r = s.run("imagery.mosaic", &json!({"name": "Valley"})).unwrap();
    let mo = s.map().layer(r["layer"].as_u64().unwrap() as u32).unwrap().raster.as_ref().unwrap().data.clone().unwrap();
    assert_eq!(mo.data_type, "Float32");
    assert!(s.run("imagery.workspace", &json!({"kind": "lidar", "images": [dem]})).is_err());
    // Effects and workspaces save with the project.
    let p = crate::Project::from_json(&serde_json::to_string(&s.project).unwrap()).unwrap();
    assert!(p.map().layer_by_name("Countries").unwrap().extrusion.is_some() && p.workspaces.len() == 2);
}

#[test]
fn view_effects_scene_and_navigation() {
    use crate::render::View;
    let mut s = world();
    s.run("map.goToXY", &json!({"lon": 10.0, "lat": 48.0, "scale": 20_000_000})).unwrap();
    let png = |s: &mut Session| {
        let v = View { width: 300.0, height: 200.0, pixels_per_point: 1.0, camera: s.map().camera, dark: false };
        crate::render::render(s.map_mut(), &v).unwrap().pixmap.data().to_vec()
    };
    let plain = png(&mut s);
    for p in [
        json!({"sunTime": 1_710_979_200.0}),
        json!({"weather": "fog"}),
        json!({"weather": "snow", "vignette": true, "edges": true}),
        json!({"tiltShift": true, "grain": true}),
        json!({"brightness": 20, "contrast": 10, "saturation": -50, "sepia": 30}),
    ] {
        s.run("map.effects", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(png(&mut s) != plain, "{p}");
    }
    assert!(s.run("map.effects", &json!({"weather": "hail"})).is_err());
    s.run("map.effects", &json!({"reset": true})).unwrap();
    assert!(png(&mut s) == plain, "reset restores the map");
    s.run("map.colorVision", &json!({"mode": "deuteranopia"})).unwrap();
    assert!(png(&mut s) != plain);
    // Exports aren't simulated.
    let export = crate::cmd::share::map_png(&mut s, Some(300.0), Some(200.0), None).unwrap();
    s.run("map.colorVision", &json!({"mode": "normal"})).unwrap();
    assert!(export == crate::cmd::share::map_png(&mut s, Some(300.0), Some(200.0), None).unwrap());
    s.run("map.drawingMode", &json!({"mode": "wireframe"})).unwrap();
    assert!(png(&mut s) != plain);
    s.run("map.drawingMode", &json!({"mode": "standard"})).unwrap();
    s.run("layer.priority", &json!({"layer": "Countries", "value": 5})).unwrap();
    assert!(png(&mut s) != plain, "countries now draw over the cities");
    s.run("layer.priority", &json!({"layer": "Countries", "value": 0})).unwrap();
    s.run("map.clipping", &json!({"mode": "distance", "distance": 500_000})).unwrap();
    assert!(png(&mut s) != plain);
    s.run("map.clipping", &json!({"mode": "none"})).unwrap();
    s.run("map.displayType", &json!({"mode": "speed"})).unwrap();
    assert!(png(&mut s) != plain, "no antialiasing");
    s.run("map.displayType", &json!({"mode": "quality"})).unwrap();
    assert!(png(&mut s) == plain);
    // Views and navigation.
    s.run("map.viewMode", &json!({"mode": "local"})).unwrap();
    assert!(s.map().projection.code().starts_with("local:"));
    png(&mut s);
    s.run("map.viewMode", &json!({"mode": "global"})).unwrap();
    assert!(s.map().projection.is_globe());
    s.run("map.navigation", &json!({"mode": "planar"})).unwrap();
    assert!(!s.map().projection.is_globe(), "planar navigation is flat");
    s.run("map.navigation", &json!({"mode": "firstPerson"})).unwrap();
    let before = s.map().center_lonlat();
    s.run("map.walk", &json!({"forward": 1})).unwrap();
    assert!(s.map().center_lonlat().y > before.y, "walking forward goes north");
    let cam = s.run("map.camera", &json!({"lon": 2.35, "lat": 48.85, "scale": 1_000_000})).unwrap();
    assert!((cam["lon"].as_f64().unwrap() - 2.35).abs() < 1e-6);
    assert!(s.run("map.navigator", &Value::Null).unwrap()["navigator"].as_bool().unwrap());
}

#[test]
fn workspace_links_thumbnails_animation_profiles_offline_and_cache() {
    let dir = std::env::temp_dir().join(format!("rustgis-ws-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = world();
    s.run("layer.addDataBytes", &json!({"fileName": "dem.tif", "data": dem_b64()})).unwrap();
    let dem = s.map().layers.iter().find(|l| l.is_raster()).unwrap().name.clone();
    // Linked views.
    s.run("map.new", &json!({"name": "Second"})).unwrap();
    s.run("map.activate", &json!({"name": "World"})).unwrap();
    s.run("view.linkViews", &json!({"on": true})).unwrap();
    s.run("map.goToXY", &json!({"lon": 12.5, "lat": 41.9, "scale": 2_000_000})).unwrap();
    let second = s.project.maps.iter().find(|m| m.name == "Second").unwrap();
    let c = second.center_lonlat();
    assert!((c.x - 12.5).abs() < 1e-6 && (second.scale() / 2_000_000.0 - 1.0).abs() < 0.01, "{c:?}");
    // Thumbnails.
    s.run("project.thumbnail", &json!({"create": true})).unwrap();
    assert!(s.project.thumbnail.as_ref().is_some_and(|t| t.len() > 100));
    s.run("map.thumbnail", &json!({"image": s.project.thumbnail.clone().unwrap()})).unwrap();
    assert!(s.run("map.thumbnail", &json!({"image": "bm90"})).is_err());
    s.run("project.thumbnail", &json!({"clear": true})).unwrap();
    assert!(s.project.thumbnail.is_none());
    // Jobs.
    let j = s.run("job.new", &json!({"name": "Check roads", "viewExtent": true})).unwrap()["job"].as_u64().unwrap();
    s.run("job.update", &json!({"job": j, "status": "in progress", "assigned": "field team"})).unwrap();
    assert!(s.run("job.update", &json!({"job": j, "status": "abandoned"})).is_err());
    assert_eq!(s.project.jobs[0].status, "in progress");
    // Animation: two keyframes, exported as frames.
    s.run("animation.add", &Value::Null).unwrap();
    s.run("map.goToXY", &json!({"lon": 2.35, "lat": 48.85, "scale": 4_000_000})).unwrap();
    s.run("animation.add", &json!({"seconds": 1})).unwrap();
    let path = dir.join("anim.zip");
    s.run("animation.export", &json!({"path": path.display().to_string(), "fps": 5, "width": 160, "height": 100})).unwrap();
    let frames = rustgis_data::zip::read(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(frames.len(), 6, "five frames for the second, then the last view");
    assert!(frames.iter().all(|(n, b)| n.ends_with(".png") && b.starts_with(b"\x89PNG")));
    s.run("animation.remove", &json!({"all": true})).unwrap();
    // Profiles over the hill.
    let p = s.run("profile.create", &json!({"layer": dem, "points": [[10.0015, 45.0195], [10.0385, 45.0195]]})).unwrap();
    assert!(p["max"].as_f64().unwrap() > 380.0 && p["gain"].as_f64().unwrap() > 150.0, "{}", p["max"]);
    let auto = s.run("profile.autoComplete", &json!({"layer": dem, "points": [[10.0015, 45.0195], [10.005, 45.0195]]})).unwrap();
    assert!(auto["length"].as_f64().unwrap() > 2500.0, "continues to the edge");
    let cross =
        s.run("profile.crossSection", &json!({"layer": dem, "points": [[10.0015, 45.0195], [10.0385, 45.0195]], "width": 2000})).unwrap();
    assert!((cross["length"].as_f64().unwrap() - 2000.0).abs() < 50.0);
    s.run("layer.template", &json!({"template": "lines", "name": "Trails"})).unwrap();
    s.run(
        "edit.addFeature",
        &json!({"layer": "Trails", "geometry": {"type": "LineString", "coordinates": [[10.002, 45.002], [10.038, 45.038]]}}),
    )
    .unwrap();
    let along = s.run("profile.alongLine", &json!({"layer": dem, "lines": "Trails"})).unwrap();
    assert!(along["samples"].as_array().unwrap().len() > 10);
    assert_eq!(s.run("profile.alongSelected", &json!({"layer": dem, "lines": "Trails"})).unwrap().as_array().unwrap().len(), 1);
    // Infographics and diagnostics.
    s.run("map.goToXY", &json!({"lon": 10.0, "lat": 48.0, "scale": 20_000_000})).unwrap();
    let info = s.run("info.infographic", &Value::Null).unwrap();
    assert!(info["layers"].as_array().unwrap().iter().any(|c| c["layer"] == "Cities" && c["count"].as_u64().unwrap() > 10));
    let d = s.run("info.diagnostics", &Value::Null).unwrap();
    assert!(d["features"].as_u64().unwrap() > 100 && d["rasterPixels"].as_u64().unwrap() == 1600);
    // Offline: download, edit offline, sync back.
    let r = s.run("offline.download", &json!({"name": "Paris", "extent": [1.0, 48.0, 4.0, 50.0]})).unwrap();
    assert!(r["features"].as_u64().unwrap() >= 2);
    s.run("offline.open", &json!({"name": "Paris"})).unwrap();
    assert_eq!(s.map().name, "Paris (offline)");
    s.run("select.byAttributes", &json!({"layer": "Cities", "where": "name = 'Paris'"})).unwrap();
    let row = s.map().layer_by_name("Cities").unwrap().selection[0];
    s.run("edit.setValue", &json!({"layer": "Cities", "row": row, "field": "pop_max", "value": 1})).unwrap();
    s.run(
        "edit.addFeature",
        &json!({"layer": "Cities", "geometry": {"type": "Point", "coordinates": [2.0, 49.0]}, "attributes": {"name": "Field camp"}}),
    )
    .unwrap();
    // Meanwhile France is edited in the source map; the area's untouched copy mustn't undo that.
    let set_france = |s: &mut Session, v: i64| {
        let world = s.project.maps.iter_mut().find(|m| m.name == "World").unwrap();
        let countries = world.layers.iter_mut().find(|l| l.name == "Countries").unwrap();
        let (ni, pi) = (countries.dataset.field_index("name").unwrap(), countries.dataset.field_index("pop_est").unwrap());
        let f = countries.dataset.features.iter_mut().find(|f| f.attrs[ni].to_string() == "France").unwrap();
        if v >= 0 {
            f.attrs[pi] = rustgis_data::Value::Int(v);
        }
        f.attrs[pi].clone()
    };
    set_france(&mut s, 12_345);
    let sync = s.run("offline.sync", &json!({"name": "Paris"})).unwrap();
    assert_eq!((sync["updated"].as_u64(), sync["added"].as_u64()), (Some(1), Some(1)), "{sync}");
    assert_eq!(set_france(&mut s, -1), rustgis_data::Value::Int(12_345), "the source edit survives the sync");
    let world_map = s.project.maps.iter().find(|m| m.name == "World").unwrap();
    let cities = world_map.layer_by_name("Cities").unwrap();
    assert!(cities.dataset.features.iter().any(|f| f.attrs.iter().any(|v| v.to_string() == "Field camp")));
    s.run("offline.remove", &json!({"name": "Paris"})).unwrap();
    assert!(s.project.maps.iter().all(|m| m.name != "Paris (offline)"));
    // Feature cache.
    s.run("map.activate", &json!({"name": "World"})).unwrap();
    s.run("cache.empty", &Value::Null).unwrap();
    assert_eq!(s.run("info.diagnostics", &Value::Null).unwrap()["cachedLayers"], 0);
    s.run("cache.fill", &Value::Null).unwrap();
    assert!(s.run("info.diagnostics", &Value::Null).unwrap()["cachedLayers"].as_u64().unwrap() >= 4);
    s.run("cache.auto", &json!({"on": true})).unwrap();
    s.run("map.projection", &json!({"type": "mollweide"})).unwrap();
    assert!(s.map().layers.iter().filter(|l| !l.is_raster()).all(|l| l.is_cached("mollweide")));
    s.run("cache.cancel", &Value::Null).unwrap();
    assert!(!s.auto_cache);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// plugins

/// Loads the test plugin as `id` with `perms`.
fn plugin(id: &str, perms: &[&str]) {
    let pkg = rustgis_plugins::Package::from_zip(&rustgis_plugins::testing::package(id, perms)).unwrap();
    rustgis_plugins::load(pkg, "test").unwrap();
}

/// Runs the test plugin's forwarding command: it sends `op` with `params` to the host.
fn forward(s: &mut Session, id: &str, op: &str, params: Value) -> crate::CmdResult {
    s.run(&format!("plugin:{id}/f"), &json!({"op": op, "params": params}))
}

#[test]
fn plugin_commands_run_like_commands() {
    let mut s = world();
    plugin("eng-view", &[]);
    assert_eq!(s.run("plugin:eng-view/h", &Value::Null).unwrap(), json!({"hello": "world"}));
    assert!(!s.can_undo(), "a plugin without 'edit' doesn't take undo snapshots");
    // Reading the map needs no permission.
    let layers = forward(&mut s, "eng-view", "host.layers", json!({})).unwrap();
    assert!(layers.as_array().unwrap().iter().any(|l| l["name"] == "Cities"));
    let fc = forward(&mut s, "eng-view", "host.features", json!({"layer": "Cities", "limit": 2})).unwrap();
    assert_eq!(fc["features"].as_array().unwrap().len(), 2);
    assert_eq!(fc["features"][1]["row"], 1);
    assert!(forward(&mut s, "eng-view", "map.zoomIn", json!({})).is_ok(), "view commands are fine");
    // Changing the project isn't.
    let e = forward(&mut s, "eng-view", "layer.remove", json!({"layer": "Cities"})).unwrap_err();
    assert!(e.contains("'edit' permission"), "{e}");
    let e = forward(&mut s, "eng-view", "host.addLayer", json!({"geojson": {"type": "Point", "coordinates": [1, 2]}})).unwrap_err();
    assert!(e.contains("'edit' permission"), "{e}");
    assert!(s.map().layer_by_name("Cities").is_some());
    // Listed with the built-in commands; unknown and disabled ones are errors.
    let all = s.run("info.commands", &Value::Null).unwrap();
    assert!(all.as_array().unwrap().iter().any(|c| c["id"] == "plugin:eng-view/h"));
    assert!(s.run("plugin:eng-view/nope", &Value::Null).unwrap_err().contains("unknown command"));
    s.run("plugin.enable", &json!({"id": "eng-view", "enabled": false})).unwrap();
    assert!(s.run("plugin:eng-view/h", &Value::Null).unwrap_err().contains("disabled"));
    rustgis_plugins::unload("eng-view");
}

#[test]
fn plugin_edits_are_one_undo_step() {
    let mut s = world();
    plugin("eng-edit", &["edit"]);
    let r =
        forward(&mut s, "eng-edit", "host.addLayer", json!({"name": "Pins", "geojson": {"type": "Point", "coordinates": [1, 2]}})).unwrap();
    assert!(r["layer"].as_u64().is_some());
    assert!(s.map().layer_by_name("Pins").is_some());
    assert_eq!(s.undo_label(), Some("Forward"));
    s.run("edit.undo", &Value::Null).unwrap();
    assert!(s.map().layer_by_name("Pins").is_none());
    forward(&mut s, "eng-edit", "layer.remove", json!({"layer": "Cities"})).unwrap();
    assert!(s.map().layer_by_name("Cities").is_none());
    // A failing command restores the project.
    let n = s.map().layers.len();
    assert!(forward(&mut s, "eng-edit", "layer.remove", json!({"layer": "Nope"})).is_err());
    assert_eq!(s.map().layers.len(), n);
    // Some things are off limits whatever the permissions.
    for op in ["script.run", "project.new", "edit.undo", "plugin.uninstall", "plugin:eng-edit/h", "gp.setGeolibre"] {
        assert!(forward(&mut s, "eng-edit", op, json!({})).is_err(), "{op}");
    }
    // Files need their own permission.
    let e = forward(&mut s, "eng-edit", "layer.addData", json!({"path": "/etc/passwd"})).unwrap_err();
    assert!(e.contains("'files' permission"), "{e}");
    for (op, params) in [
        ("share.exportMap", json!({})),
        ("project.save", json!({})),
        ("layer.rename", json!({"layer": "Cities", "name": "x", "output": 1})),
        ("layer.rename", json!({"layer": "Cities", "name": "https://example.com/x"})),
        ("layer.rename", json!({"layer": "Cities", "name": "../../x"})),
        ("layer.rename", json!({"layer": "Cities", "name": {"nested": {"url": "x"}}})),
    ] {
        let e = forward(&mut s, "eng-edit", op, params.clone()).unwrap_err();
        assert!(e.contains("'files' permission"), "{op} {params}: {e}");
    }
    assert!(forward(&mut s, "eng-edit", "layer.rename", json!({"layer": "Rivers", "name": "Streams"})).is_ok());
    rustgis_plugins::unload("eng-edit");
}

#[test]
fn plugin_tile_layers_need_network() {
    let mut s = world();
    let osm = json!({"url": "https://tile.openstreetmap.org/{z}/{x}/{y}.png", "attribution": "© OpenStreetMap contributors"});
    // Without `network`, a plugin can't add a tile layer, directly or by importing a map file.
    plugin("eng-tiles-edit", &["edit", "files"]);
    let e = forward(&mut s, "eng-tiles-edit", "map.addTiles", osm.clone()).unwrap_err();
    assert!(e.contains("'network' permission"), "{e}");
    let mut m = rustgis_map::Map::new("Imported");
    m.tiles.push(rustgis_map::TileLayer::new("Leak", rustgis_map::TileKind::Raster, "https://evil.example.com/{z}/{x}/{y}.png?d=secret"));
    let text = serde_json::to_string(&m).unwrap();
    let e = forward(&mut s, "eng-tiles-edit", "map.import", json!({"text": text})).unwrap_err();
    assert!(e.contains("'network' permission"), "{e}");
    assert!(s.project.maps.iter().all(|m| m.tiles.is_empty()));
    // Nor by changing how a layer it can see fetches (here, an imported map with the same
    // address as an existing layer but another subdomain list).
    s.run("map.addTiles", &json!({"url": "https://{s}.tiles.example.com/{z}/{x}/{y}.png"})).unwrap();
    let mut m = s.map().clone();
    m.name = "Copy".into();
    m.tiles[0].subdomains = vec!["evil".into()];
    let e = forward(&mut s, "eng-tiles-edit", "map.import", json!({"text": serde_json::to_string(&m).unwrap()})).unwrap_err();
    assert!(e.contains("'network' permission"), "{e}");
    assert!(s.project.maps.iter().flat_map(|m| &m.tiles).all(|t| t.subdomains.is_empty()));
    // Changing a layer's opacity is only an edit.
    forward(&mut s, "eng-tiles-edit", "map.setTiles", json!({"index": 0, "opacity": 0.5})).unwrap();
    assert_eq!(s.map().tiles[0].opacity, 0.5);
    s.run("map.removeTiles", &json!({})).unwrap();
    rustgis_plugins::unload("eng-tiles-edit");
    // With it (and `edit`), it can; it needs no `files` for an address.
    plugin("eng-tiles-net", &["edit", "network"]);
    forward(&mut s, "eng-tiles-net", "map.addTiles", osm).unwrap();
    assert_eq!(s.map().tiles.len(), 1);
    let listed = forward(&mut s, "eng-tiles-net", "map.tiles", json!({})).unwrap();
    assert_eq!(listed[0]["attribution"], "© OpenStreetMap contributors");
    forward(&mut s, "eng-tiles-net", "map.removeTiles", json!({})).unwrap();
    assert!(s.map().tiles.is_empty());
    rustgis_plugins::unload("eng-tiles-net");
}

#[test]
fn plugin_tools_run_in_the_geoprocessing_pane() {
    let mut s = world();
    plugin("eng-tools", &[]);
    let tools = s.run("gp.tools", &Value::Null).unwrap();
    assert!(tools.as_array().unwrap().iter().any(|t| t["id"] == "plugin:eng-tools/m" && t["toolbox"] == "Plugins › Test eng-tools"));
    s.run("gp.run", &json!({"tool": "plugin:eng-tools/m", "params": {"input": "Cities"}})).unwrap();
    assert_eq!(s.map().layer_by_name("Made").map(|l| l.dataset.features.len()), Some(1));
    let h = s.project.gp_history.last().unwrap();
    assert!(h.ok && h.messages == ["made"], "{h:?}");
    s.run("gp.run", &json!({"tool": "plugin:eng-tools/x", "params": {"input": "Cities"}})).unwrap();
    assert_eq!(s.map().layer_by_name("Cities").unwrap().selection, vec![0, 2]);
    assert!(s.run("gp.run", &json!({"tool": "plugin:eng-tools/m", "params": {}})).unwrap_err().contains("required"));
    rustgis_plugins::unload("eng-tools");
}

#[test]
fn plugins_install_and_uninstall_from_files() {
    isolate_plugins();
    let mut s = Session::new(Project::default());
    let dir = std::env::temp_dir().join(format!("rustgis-test-pkg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("eng-install.rgplugin");
    std::fs::write(&file, rustgis_plugins::testing::package("eng-install", &[])).unwrap();
    let info = s.run("plugin.install", &json!({"path": file.display().to_string()})).unwrap();
    assert_eq!(info["id"], "eng-install");
    let list = s.run("plugin.list", &Value::Null).unwrap();
    assert!(list["plugins"].as_array().unwrap().iter().any(|p| p["id"] == "eng-install"));
    let installed = rustgis_plugins::store::dir().unwrap().join("eng-install.rgplugin");
    assert!(installed.is_file(), "copied into the plugins folder");
    // Loaded again at startup, remembering that it was disabled.
    s.run("plugin.enable", &json!({"id": "eng-install", "enabled": false})).unwrap();
    rustgis_plugins::unload("eng-install");
    assert!(crate::cmd::plugins::load_installed().iter().all(|e| !e.contains("eng-install")));
    assert_eq!(rustgis_plugins::get("eng-install").map(|p| p.enabled), Some(false));
    s.run("plugin.uninstall", &json!({"id": "eng-install"})).unwrap();
    assert!(!installed.exists());
    assert!(rustgis_plugins::get("eng-install").is_none());
    // Uninstalling a development folder that shadows an installed package keeps the package.
    s.run("plugin.install", &json!({"path": file.display().to_string()})).unwrap();
    let folder = dir.join("eng-install-dev");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("plugin.json"), rustgis_plugins::testing::manifest("eng-install", &[])).unwrap();
    std::fs::write(folder.join("plugin.wasm"), rustgis_plugins::testing::wasm()).unwrap();
    s.run("plugin.install", &json!({"path": folder.display().to_string()})).unwrap();
    s.run("plugin.uninstall", &json!({"id": "eng-install"})).unwrap();
    assert!(installed.is_file(), "the installed package stays");
    assert!(crate::cmd::plugins::load_installed().iter().all(|e| !e.contains("eng-install")));
    assert_eq!(rustgis_plugins::get("eng-install").map(|p| p.source.clone()), Some(installed.display().to_string()));
    s.run("plugin.uninstall", &json!({"id": "eng-install"})).unwrap();
    assert!(!installed.exists());
    // A file that isn't a package isn't installed.
    std::fs::write(dir.join("bad.rgplugin"), b"nope").unwrap();
    assert!(s.run("plugin.install", &json!({"path": dir.join("bad.rgplugin").display().to_string()})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn registry_downloads_are_checked() {
    let bytes = rustgis_plugins::testing::package("eng-reg", &[]);
    let mut e = rustgis_plugins::registry::Entry {
        id: "eng-reg".into(),
        name: "Reg".into(),
        version: "1.0.0".into(),
        description: String::new(),
        author: String::new(),
        homepage: String::new(),
        license: String::new(),
        categories: Vec::new(),
        package: "https://example.com/x.rgplugin".into(),
        sha256: "0".repeat(64),
        size: 0,
        min_rustgis_version: String::new(),
        permissions: Vec::new(),
        i18n: Default::default(),
    };
    isolate_plugins();
    assert!(crate::cmd::plugins::install_checked(&e, &bytes).unwrap_err().contains("doesn't match"));
    e.sha256 = rustgis_plugins::package::sha256_hex(&bytes);
    e.id = "other".into();
    assert!(crate::cmd::plugins::install_checked(&e, &bytes).unwrap_err().contains("not 'other'"));
    e.id = "eng-reg".into();
    crate::cmd::plugins::install_checked(&e, &bytes).unwrap();
    assert_eq!(crate::cmd::plugins::entry_status(&e)["installed"], "1.0.0");
    let _ = rustgis_plugins::store::uninstall("eng-reg");
}
#[test]
fn reports_group_summarise_relate_export_and_reopen() {
    let dir = std::env::temp_dir().join(format!("rustgis-report-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = world();
    assert!(s.run("report.autofill", &Value::Null).is_err(), "no report yet");
    let cities = s.map().layer_by_name("Cities").unwrap();
    let fields: Vec<String> = cities.dataset.fields.iter().map(|f| f.name.clone()).collect();
    let numeric = cities.dataset.fields.iter().find(|f| f.name.to_ascii_lowercase().starts_with("pop")).unwrap().name.clone();
    let n = cities.dataset.features.len();
    s.run(
        "report.new",
        &json!({"layer": "Cities", "name": "Cities", "fields": ["name", numeric], "groups": ["adm0name"], "sort": numeric, "descending": true, "page": "a4"}),
    )
    .unwrap();
    assert_eq!((s.project.reports[0].width, s.project.reports[0].height), (210.0, 297.0));
    assert!(s.run("report.field", &json!({"field": "no_such_field"})).is_err());
    s.run("report.statistics", &json!({"field": numeric, "kind": "sum"})).unwrap();
    s.run("report.chart", &json!({"category": "adm0name"})).unwrap();
    s.run("report.supplementalPage", &json!({"title": "About", "text": "Cities of the world."})).unwrap();
    s.run("report.newTemplate", &json!({"end": true})).unwrap();
    assert!(s.active_layout.is_none(), "adding a layout page leaves the map view showing");
    // The blocks: a heading per country, the sum, every city once.
    let doc = crate::cmd::report::build(&s, 0).unwrap();
    use rustgis_map::report::Block;
    let rows = doc.blocks.iter().filter(|b| matches!(b, Block::Row(_))).count();
    assert_eq!(rows, n);
    assert!(doc.blocks.iter().any(|b| matches!(b, Block::Heading { level: 0, .. })));
    assert!(doc.blocks.iter().any(|b| matches!(b, Block::Stats(v) if v.iter().any(|l| l.starts_with("Sum of")))));
    assert!(doc.blocks.iter().any(|b| matches!(b, Block::Chart { bars, .. } if !bars.is_empty())));
    assert!(matches!(doc.blocks.last(), Some(Block::Layout(_))));
    // A report on countries with each country's cities beneath it, followed by the cities report.
    s.run("data.relate", &json!({"name": "Country of city", "origin": "Countries", "originKey": "name", "destination": "Cities", "destinationKey": "adm0name"}))
        .unwrap();
    s.run("report.new", &json!({"layer": "Countries", "name": "Countries", "fields": ["name"], "where": "name LIKE 'F%'"})).unwrap();
    s.run("report.subreport", &json!({"relationship": "Country of city", "fields": ["name"]})).unwrap();
    s.run("report.relatedReport", &json!({"related": "Cities"})).unwrap();
    assert!(s.run("report.relatedReport", &json!({"related": "Countries"})).is_err(), "not itself");
    let doc = crate::cmd::report::build(&s, 1).unwrap();
    assert!(doc.blocks.iter().any(|b| matches!(b, Block::SubRow(_))), "France's cities are listed under it");
    assert!(doc.blocks.iter().filter(|b| matches!(b, Block::Row(_))).count() > n, "the cities report follows");
    // Moving a report to the other end of its subreport's relationship drops the related fields
    // the new related layer lacks.
    s.run("report.new", &json!({"layer": "Countries", "name": "Swap"})).unwrap();
    s.run("report.subreport", &json!({"relationship": "Country of city", "fields": ["name", "adm0name"]})).unwrap();
    s.run("report.update", &json!({"layer": "Cities"})).unwrap();
    let swap = s.project.reports.iter().find(|r| r.name == "Swap").unwrap();
    assert!(swap.sections.iter().any(|x| matches!(x, rustgis_map::report::Section::Subreport { fields, .. } if fields == &["name"])));
    s.run("report.remove", &Value::Null).unwrap();
    // Picture paths are read relative to the saved project's folder only.
    let png = {
        let mut pm = tiny_skia::Pixmap::new(4, 4).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(200, 0, 0, 255));
        pm.encode_png().unwrap()
    };
    std::fs::write(dir.join("pic.png"), &png).unwrap();
    let pics = json!({"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"photo": "pic.png"}, "geometry": {"type": "Point", "coordinates": [0, 0]}},
        {"type": "Feature", "properties": {"photo": dir.join("pic.png").to_str().unwrap()}, "geometry": {"type": "Point", "coordinates": [1, 0]}},
        {"type": "Feature", "properties": {"photo": "../pic.png"}, "geometry": {"type": "Point", "coordinates": [2, 0]}}]});
    s.run("layer.addDataText", &json!({"fileName": "pics.geojson", "text": pics.to_string()})).unwrap();
    s.run("report.new", &json!({"layer": "pics", "name": "Pics"})).unwrap();
    s.run("report.dynamicPicture", &json!({"field": "photo"})).unwrap();
    let pi = s.project.reports.iter().position(|r| r.name == "Pics").unwrap();
    let count =
        |s: &Session| crate::cmd::report::build(s, pi).unwrap().blocks.iter().filter(|b| matches!(b, Block::Picture { .. })).count();
    assert_eq!(count(&s), 0, "an unsaved project reads no picture files");
    s.run("project.saveAs", &json!({"path": dir.join("p.rgis").to_str().unwrap()})).unwrap();
    assert_eq!(count(&s), 1, "only the relative path inside the project folder");
    // Renaming a report keeps links to it; exports are .pdf or .png only.
    s.run("report.update", &json!({"report": "Cities", "name": "World cities"})).unwrap();
    let countries = s.project.reports.iter().find(|r| r.name == "Countries").unwrap();
    assert!(countries.sections.iter().any(|x| matches!(x, rustgis_map::report::Section::Related { report } if report == "World cities")));
    assert!(s.run("report.export", &json!({"report": "Countries", "path": dir.join("c.jpg").to_str().unwrap()})).is_err());
    s.run("report.update", &json!({"report": "World cities", "name": "Cities"})).unwrap();
    // Sorting a column that mixes numbers and text (9, 10, "1a"…) is a total order, never a panic.
    let mixed = json!({"type": "FeatureCollection", "features": (0..200).map(|i| json!({"type": "Feature",
        "properties": {"v": if i % 3 == 0 { json!(format!("{}a", i % 7)) } else { json!(i % 11) }},
        "geometry": {"type": "Point", "coordinates": [i as f64 * 0.01, 0.0]}})).collect::<Vec<_>>()});
    s.run("layer.addDataText", &json!({"fileName": "mixed.geojson", "text": mixed.to_string()})).unwrap();
    s.run("report.new", &json!({"layer": "mixed", "name": "Mixed", "sort": "v", "groups": ["v"]})).unwrap();
    let i = s.project.reports.iter().position(|r| r.name == "Mixed").unwrap();
    assert!(crate::cmd::report::build(&s, i).is_ok());
    // A related report that loops back stops.
    s.run("report.relatedReport", &json!({"report": "Cities", "related": "Countries"})).unwrap();
    assert!(crate::cmd::report::build(&s, 1).is_ok());
    // Export: PDF pages and a PNG.
    let pdf = dir.join("countries.pdf");
    let r = s.run("report.export", &json!({"report": "Countries", "path": pdf.to_str().unwrap(), "dpi": 72})).unwrap();
    assert!(r["pages"].as_u64().unwrap() >= 3);
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
    let png = dir.join("cities.png");
    s.run("report.export", &json!({"report": "Cities", "path": png.to_str().unwrap(), "dpi": 72})).unwrap();
    assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
    // Report file and template, reopened.
    let file = dir.join("cities.rgisreport");
    let tmpl = dir.join("cities.rgisrpttemplate");
    s.run("report.save", &json!({"report": "Cities", "path": file.to_str().unwrap()})).unwrap();
    s.run("report.saveTemplate", &json!({"report": "Cities", "path": tmpl.to_str().unwrap()})).unwrap();
    let r = s.run("report.open", &json!({"path": file.to_str().unwrap()})).unwrap();
    assert_eq!(r["name"], "Cities 2");
    s.run("report.open", &json!({"path": tmpl.to_str().unwrap(), "layer": "Cities"})).unwrap();
    let t = s.project.reports.last().unwrap();
    assert_eq!(t.layer, "Cities");
    assert!(t.sections.iter().all(|x| matches!(x, rustgis_map::report::Section::Page { .. })));
    // Items, undo and fields kept.
    let (name, bytes) = crate::cmd::items::item_bytes(&mut s, "reportTemplate", &json!({"report": "Cities"})).unwrap();
    assert!(name.ends_with(".rgisrpttemplate") && !bytes.is_empty());
    assert!(fields.iter().any(|f| f == "adm0name"));
    let before = s.project.reports.len();
    s.run("report.remove", &json!({"report": "Cities 2"})).unwrap();
    assert_eq!(s.project.reports.len(), before - 1);
    s.undo();
    assert_eq!(s.project.reports.len(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn presentations_and_notebooks() {
    let dir = std::env::temp_dir().join(format!("rustgis-pres-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = world();
    assert!(s.run("presentation.mapPage", &Value::Null).is_err(), "no presentation yet");
    s.run("presentation.new", &json!({"name": "Tour", "title": "World tour"})).unwrap();
    s.run("map.goToXY", &json!({"lon": 12.5, "lat": 41.9, "scale": 5_000_000})).unwrap();
    s.run("presentation.mapPage", &json!({"title": "Rome"})).unwrap();
    s.run("presentation.blankPage", &json!({"title": "Facts", "text": "Rome is the capital of Italy.", "background": "#FFF8E0"})).unwrap();
    assert!(s.run("presentation.blankPage", &json!({"background": "beige"})).is_err());
    let png = s.run("project.thumbnail", &json!({"create": true})).map(|_| s.project.thumbnail.clone().unwrap()).unwrap();
    s.run("presentation.imagePage", &json!({"image": png, "caption": "The map", "at": 1})).unwrap();
    assert!(s.run("presentation.imagePage", &json!({"image": "bm90"})).is_err());
    s.run("presentation.videoPage", &json!({"source": "https://example.org/rome.mp4", "posterImage": png})).unwrap();
    s.run("presentation.videoPage", &json!({"source": "C:\\videos\\walk.webm"})).unwrap();
    let pres = &s.project.presentations[0];
    let kinds: Vec<&str> = pres.pages.iter().map(|p| p.kind.code()).collect();
    assert_eq!(kinds, ["blank", "image", "map", "blank", "video", "video"]);
    assert_eq!(pres.pages[5].title, "walk.webm");
    s.run("presentation.updatePage", &json!({"page": 2, "notes": "Zoom in", "seconds": 9, "moveTo": 0})).unwrap();
    assert_eq!(s.project.presentations[0].pages[0].title, "Rome");
    assert!(s.run("presentation.updatePage", &json!({"page": 99})).is_err());
    // The map page draws the map: not one flat colour.
    let img = crate::cmd::presentation::draw_page(&mut s, 0, 0, 40.0).unwrap();
    let first = img.pixels()[img.pixels().len() / 2];
    assert!(img.pixels().iter().any(|p| *p != first));
    // Exports.
    for (file, magic) in [("tour.pdf", &b"%PDF"[..]), ("tour.html", &b"<!doctype html>"[..]), ("tour.png", &b"\x89PNG"[..])] {
        let path = dir.join(file);
        let r = s.run("presentation.export", &json!({"path": path.to_str().unwrap(), "dpi": 48})).unwrap();
        assert_eq!(r["pages"], if file.ends_with(".png") { 1 } else { 6 });
        assert!(std::fs::read(&path).unwrap().starts_with(magic), "{file}");
    }
    assert!(s.run("presentation.export", &json!({"path": dir.join("deck.pptx").to_str().unwrap()})).is_err());
    let html = std::fs::read_to_string(dir.join("tour.html")).unwrap();
    use crate::cmd::presentation::playable;
    for (source, kept) in [
        ("https://a.org/v.mp4", true),
        ("clip.mp4", true),
        ("C:\\v.mp4", true),
        ("/v/a.mp4", true),
        ("file:///etc/v", false),
        ("data:x", false),
    ] {
        assert_eq!(playable(source).is_empty(), !kept, "{source}");
    }
    assert!(html.contains("<video") && html.contains("https://example.org/rome.mp4") && html.matches("<section").count() == 6);
    // Save and import.
    let file = dir.join("tour.rgispres");
    s.run("presentation.save", &json!({"path": file.to_str().unwrap()})).unwrap();
    let r = s.run("presentation.import", &json!({"path": file.to_str().unwrap()})).unwrap();
    assert_eq!(r["name"], "Tour 2");
    assert_eq!(s.project.presentations[1].pages.len(), 6);
    s.run("presentation.removePage", &json!({"page": 0})).unwrap();
    assert_eq!(s.project.presentations[1].pages.len(), 5);

    // Notebooks: command cells run and keep their output; Jupyter round trip.
    s.run("notebook.new", &json!({"name": "Analysis"})).unwrap();
    s.run("notebook.addCell", &json!({"source": "map.goToXY {\"lon\": 2.35, \"lat\": 48.85, \"scale\": 1000000}\nselect.byAttributes {\"layer\": \"Cities\", \"where\": \"name = 'Paris'\"}"}))
        .unwrap();
    s.run("notebook.addCell", &json!({"source": "no.such.command"})).unwrap();
    let r = s.run("notebook.run", &Value::Null).unwrap();
    assert_eq!(r["error"], 3);
    let nb = &s.project.notebooks[0];
    assert!(nb.cells[2].output.as_deref().is_some_and(|o| o.contains("\"selected\":1")), "{:?}", nb.cells[2].output);
    assert!(nb.cells[3].error);
    assert!((s.map().center_lonlat().x - 2.35).abs() < 1e-6);
    // A cell can't run notebooks (no recursion).
    s.run("notebook.editCell", &json!({"cell": 3, "source": "notebook.run {}"})).unwrap();
    s.run("notebook.run", &json!({"cell": 3})).unwrap();
    assert!(s.project.notebooks[0].cells[3].error);
    // Nor through a task that runs the notebook (no unbounded recursion).
    s.run(
        "task.new",
        &json!({"name": "Loop", "steps": [{"title": "again", "command": "notebook.run", "params": {"notebook": "Analysis"}}]}),
    )
    .unwrap();
    s.run("notebook.editCell", &json!({"cell": 3, "source": "task.run {\"task\": \"Loop\"}"})).unwrap();
    s.run("notebook.run", &json!({"cell": 3})).unwrap();
    let cell = &s.project.notebooks[0].cells[3];
    assert!(cell.error && cell.output.as_deref().is_some_and(|o| o.contains("already running")), "{:?}", cell.output);
    assert!(s.run("notebook.run", &json!({"cell": 2})).is_ok(), "the guard is cleared after a run");
    s.run("task.remove", &json!({"task": "Loop"})).unwrap();
    let ipynb = dir.join("analysis.ipynb");
    s.run("notebook.save", &json!({"path": ipynb.to_str().unwrap()})).unwrap();
    let v: Value = serde_json::from_slice(&std::fs::read(&ipynb).unwrap()).unwrap();
    assert_eq!(v["nbformat"], 4);
    assert_eq!(v["cells"].as_array().unwrap().len(), 4);
    s.run("notebook.open", &json!({"path": ipynb.to_str().unwrap()})).unwrap();
    let nb = s.project.notebooks.last().unwrap();
    assert_eq!(nb.name, "Analysis 2");
    assert_eq!(nb.cells.len(), 4);
    assert!(nb.cells[2].output.is_some());
    assert!(nb.cells[3].error, "a failed cell stays failed through Jupyter");
    // A notebook from a file can't touch files; one made here can.
    let png = dir.join("nb.png");
    let cell = format!("share.exportMap {{\"path\": {:?}}}", png.to_str().unwrap());
    s.run("notebook.addCell", &json!({"notebook": "Analysis 2", "source": cell})).unwrap();
    let r = s.run("notebook.run", &json!({"notebook": "Analysis 2", "cell": 4})).unwrap();
    assert_eq!(r["error"], 4);
    assert!(!png.exists());
    s.run("notebook.addCell", &json!({"notebook": "Analysis", "source": cell})).unwrap();
    let last = s.project.notebooks[0].cells.len() - 1;
    s.run("notebook.run", &json!({"notebook": "Analysis", "cell": last})).unwrap();
    assert!(png.exists());
    assert!(s.run("notebook.open", &json!({"text": "{\"cells\": 5}"})).is_err());
    // Trust left by an undone notebook doesn't pass to a file's notebook of the same name.
    s.run("notebook.new", &json!({"name": "Notebook"})).unwrap();
    s.run("edit.undo", &Value::Null).unwrap();
    let text = json!({"nbformat": 4, "cells": [{"cell_type": "code", "source": cell}]}).to_string();
    s.run("notebook.open", &json!({"text": text})).unwrap();
    let r = s.run("notebook.run", &json!({"notebook": "Notebook"})).unwrap();
    assert_eq!(r["error"], 0, "{r}");
    // Nor can it make a trusted notebook to run its lines later.
    let text = json!({"nbformat": 4, "cells": [{"cell_type": "code", "source": "notebook.new {\"name\": \"Laundered\"}"}]}).to_string();
    s.run("notebook.open", &json!({"text": text})).unwrap();
    let opened = s.project.notebooks.last().unwrap().name.clone();
    let r = s.run("notebook.run", &json!({"notebook": opened})).unwrap();
    assert_eq!(r["error"], 0, "{r}");
    assert!(!s.project.notebooks.iter().any(|nb| nb.name == "Laundered"));
    // Nor borrow trust from a notebook whose name differs only by non-ASCII case.
    let text = json!({"nbformat": 4, "metadata": {"title": "éa"}, "cells": [{"cell_type": "code", "source": cell}]}).to_string();
    s.run("notebook.open", &json!({"text": text})).unwrap();
    s.run("notebook.new", &json!({"name": "Éa"})).unwrap();
    let r = s.run("notebook.run", &json!({"notebook": "éa"})).unwrap();
    assert_eq!(r["error"], 0, "{r}");
    for name in ["Éa", "éa"] {
        let at = s.project.notebooks.iter().position(|nb| nb.name == name).unwrap();
        s.run("notebook.remove", &json!({"notebook": at})).unwrap();
    }
    // Or plant a cell in a trusted notebook.
    let plant = format!("notebook.addCell {{\"notebook\": \"Analysis\", \"source\": {:?}}}", "share.exportMap {}");
    s.run("notebook.editCell", &json!({"notebook": opened.as_str(), "cell": 0, "source": plant})).unwrap();
    let before = s.project.notebooks[0].cells.len();
    let r = s.run("notebook.run", &json!({"notebook": opened.as_str()})).unwrap();
    assert_eq!(r["error"], 0, "{r}");
    assert_eq!(s.project.notebooks[0].cells.len(), before);
    let last = s.project.notebooks.len() - 1;
    s.run("notebook.remove", &json!({"notebook": last})).unwrap();
    // A cell that removes an earlier notebook still writes its output to its own notebook.
    s.run("notebook.new", &json!({"name": "Runner"})).unwrap();
    s.run("notebook.addCell", &json!({"notebook": "Runner", "source": "notebook.remove {\"notebook\": 0}"})).unwrap();
    s.run("notebook.addCell", &json!({"notebook": "Runner", "source": "map.goToXY {\"lon\": 0, \"lat\": 0}"})).unwrap();
    let r = s.run("notebook.run", &json!({"notebook": "Runner"})).unwrap();
    assert!(r.get("error").is_none(), "{r}");
    let nb = s.project.notebooks.iter().find(|nb| nb.name == "Runner").unwrap();
    assert!(nb.cells[2].output.as_deref().is_some_and(|o| o.starts_with("notebook.remove")), "{:?}", nb.cells[2].output);
    assert!(nb.cells[3].output.is_some());
    assert_eq!(s.project.notebooks.len(), 3);
    // Notebooks and presentations from a project file get unique names, since they're found
    // (and notebooks trusted) by name.
    let mut v = serde_json::to_value(&s.project).unwrap();
    v["notebooks"][1]["name"] = json!("ANALYSIS 2");
    v["presentations"][1]["name"] = json!("tour");
    let p = Project::from_json(&v.to_string()).unwrap();
    assert_eq!(p.notebooks[1].name, "ANALYSIS 2 2");
    assert_eq!(p.presentations[1].name, "tour 2");
    // Many notebooks of one name: capped, and renamed quickly.
    v["notebooks"] = Value::Array((0..1500).map(|_| json!({"name": "A", "cells": []})).collect());
    let t0 = std::time::Instant::now();
    let p = Project::from_json(&v.to_string()).unwrap();
    assert_eq!(p.notebooks.len(), crate::project::MAX_ITEMS);
    assert_eq!(p.notebooks.last().unwrap().name, format!("A {}", crate::project::MAX_ITEMS));
    assert!(t0.elapsed().as_secs() < 5);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A three-band raster: water-like pixels on the left half, field-like on the right.
fn two_class_b64() -> String {
    use base64::Engine;
    use rustgis_data::raster::{ColorHint, Crs, GeoTransform, Raster, geotiff};
    let gt = GeoTransform { origin_x: 10.0, origin_y: 45.04, pixel_width: 0.001, pixel_height: -0.001 };
    let band = |water: f32, field: f32| -> Vec<f32> {
        (0..1600).map(|i| if i % 40 < 20 { water + (i % 7) as f32 } else { field + (i % 5) as f32 }).collect()
    };
    let r = Raster::new(
        "scene",
        40,
        40,
        vec![band(20.0, 120.0), band(80.0, 30.0), band(140.0, 60.0)],
        gt,
        Crs::LonLat,
        None,
        ColorHint::Rgb { alpha: false },
        "UInt8",
    )
    .unwrap();
    base64::engine::general_purpose::STANDARD.encode(geotiff::write(&r, 0).unwrap())
}

#[test]
fn deep_learning_hosted_imagery_and_sharing() {
    let dir = std::env::temp_dir().join(format!("rustgis-share-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = |f: &str| dir.join(f).to_str().unwrap().to_string();
    let mut s = world();
    s.run("layer.addDataBytes", &json!({"fileName": "scene.tif", "data": two_class_b64()})).unwrap();
    let training = json!({"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"class": "water"}, "geometry": {"type": "Polygon", "coordinates": [[[10.002, 45.002], [10.015, 45.002], [10.015, 45.035], [10.002, 45.035], [10.002, 45.002]]]}},
        {"type": "Feature", "properties": {"class": "field"}, "geometry": {"type": "Polygon", "coordinates": [[[10.025, 45.002], [10.038, 45.002], [10.038, 45.035], [10.025, 45.035], [10.025, 45.002]]]}}]});
    s.run("layer.addDataText", &json!({"fileName": "training.geojson", "text": training.to_string()})).unwrap();
    assert!(s.run("dl.package", &json!({"path": path("x.rgisdlpk")})).is_err(), "no model yet");
    // A class whose polygon misses the raster is named in the error.
    let lost = json!({"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"class": "water"}, "geometry": {"type": "Polygon", "coordinates": [[[10.002, 45.002], [10.015, 45.002], [10.015, 45.035], [10.002, 45.035], [10.002, 45.002]]]}},
        {"type": "Feature", "properties": {"class": "lost"}, "geometry": {"type": "Polygon", "coordinates": [[[-60.0, -10.0], [-59.0, -10.0], [-59.0, -9.0], [-60.0, -9.0], [-60.0, -10.0]]]}}]});
    s.run("layer.addDataText", &json!({"fileName": "lost.geojson", "text": lost.to_string()})).unwrap();
    let err = s.run("dl.train", &json!({"layer": "scene", "training": "lost", "classField": "class"})).unwrap_err();
    assert!(err.contains("'lost'"), "{err}");
    let r = s
        .run(
            "dl.classify",
            &json!({"layer": "scene", "training": "training", "classField": "class", "bands": [1, 2], "epochs": 30, "model": "landcover"}),
        )
        .unwrap();
    assert!(r["accuracy"].as_f64().unwrap() > 0.95, "{r}");
    let classes = r["classes"].as_array().unwrap();
    assert!(classes.iter().all(|c| c["pixels"].as_u64().unwrap() > 700), "both halves classified: {r}");
    // Package, remove, import, apply again.
    s.run("dl.package", &json!({"path": path("landcover.rgisdlpk")})).unwrap();
    s.run("dl.remove", &json!({"model": "landcover"})).unwrap();
    assert!(s.project.dl_models.is_empty());
    s.run("dl.import", &json!({"path": path("landcover.rgisdlpk")})).unwrap();
    s.run("dl.classify", &json!({"layer": "scene", "model": "landcover", "name": "again"})).unwrap();
    assert!(s.run("dl.import", &json!({"data": "UEsFBgAAAAAAAAAAAAAAAAAAAAAAAA=="})).is_err(), "an empty zip");
    // Importing again doesn't replace the model; retraining it by name does, and says so.
    let r = s.run("dl.import", &json!({"path": path("landcover.rgisdlpk")})).unwrap();
    assert_eq!(r["model"], "landcover 2");
    let r = s
        .run(
            "dl.train",
            &json!({"layer": "scene", "training": "training", "classField": "class", "bands": [1, 2], "epochs": 5, "model": "landcover"}),
        )
        .unwrap();
    assert_eq!(r["replaced"], true);
    assert_eq!(s.project.dl_models.len(), 2);
    // Hosted imagery: tiles in a zip, transparent outside the raster.
    let r = s.run("imagery.hostedImagery", &json!({"layer": "scene", "path": path("tiles.zip"), "minZoom": 10, "maxZoom": 12})).unwrap();
    assert!(r["tiles"].as_u64().unwrap() >= 3);
    let entries = rustgis_data::zip::read(&std::fs::read(path("tiles.zip")).unwrap()).unwrap();
    assert!(entries.iter().any(|e| e.0 == "tiles.json") && entries.iter().any(|e| e.0.starts_with("tiles/12/")));

    // Sharing to a data store.
    assert!(s.run("share.webMap", &Value::Null).is_err(), "no path and no data store");
    s.run("share.dataStore", &json!({"name": "out", "path": dir.to_str().unwrap()})).unwrap();
    assert!(s.run("share.dataStore", &json!({"path": path("missing")})).is_err());
    s.run("layer.symbology", &json!({"layer": "Countries", "type": "graduated", "field": "pop_est"})).unwrap();
    s.run("share.webMap", &Value::Null).unwrap();
    let html = std::fs::read_to_string(path("World.html")).unwrap();
    assert!(
        html.contains("<canvas") && html.contains("\"Countries\"") && html.contains("data:image/png;base64,"),
        "vector and raster layers"
    );
    s.run("share.webLayer", &json!({"layer": "Countries"})).unwrap();
    let z = rustgis_data::zip::read(&std::fs::read(path("Countries.zip")).unwrap()).unwrap();
    for f in ["collections.json", "conformance.json", "style.json", "index.html", "collections/countries/items.json"] {
        assert!(z.iter().any(|e| e.0 == f), "{f}");
    }
    s.run("share.webStyle", &json!({"layer": "Countries"})).unwrap();
    let style: Value = serde_json::from_slice(&std::fs::read(path("Countries.json")).unwrap()).unwrap();
    assert_eq!(style["version"], 8);
    assert_eq!(style["layers"][0]["paint"]["fill-color"][0], "step", "graduated colours as a step expression");
    s.run("share.locator", &json!({"layer": "Cities", "fields": ["name"]})).unwrap();
    let r = s.run("locator.find", &json!({"text": "paris", "zoom": true})).unwrap();
    assert_eq!(r["matches"][0]["text"], "Paris");
    assert!((s.map().center_lonlat().x - 2.35).abs() < 0.1);
    s.run("share.geodataService", &Value::Null).unwrap();
    s.run("model.new", &json!({"name": "Buffer", "steps": [{"tool": "buffer", "params": {"input": "Cities", "distance": 10}}]})).unwrap();
    s.run("share.webTool", &Value::Null).unwrap();
    s.run("model.import", &json!({"path": path("Buffer.rgistool")})).unwrap();
    assert_eq!(s.project.models.len(), 2);
    s.run("layer.extrusion", &json!({"layer": "Countries", "type": "height", "height": 100000})).unwrap();
    s.run("share.webScene", &Value::Null).unwrap();
    let g: Value = serde_json::from_slice(&std::fs::read(path("World.gltf")).unwrap()).unwrap();
    assert_eq!(g["asset"]["version"], "2.0");
    assert!(g["meshes"].as_array().unwrap().len() >= 2);
    assert!(s.run("share.linkChart", &Value::Null).is_err(), "no relationships yet");
    s.run("data.relate", &json!({"name": "Country of city", "origin": "Countries", "originKey": "name", "destination": "Cities", "destinationKey": "adm0name"})).unwrap();
    s.run("share.linkChart", &Value::Null).unwrap();
    assert!(std::fs::read_to_string(path("Relationships.html")).unwrap().contains("<svg"));
    // Manage: update in place; jobs list them.
    let before = s.project.shared.len();
    s.run("share.saveWebMap", &Value::Null).unwrap();
    s.run("share.replaceWebLayer", &json!({"layer": "Lakes"})).unwrap();
    s.run("share.saveWebScene", &Value::Null).unwrap();
    s.run("share.saveWebLinkChart", &Value::Null).unwrap();
    assert_eq!(s.project.shared.len(), before, "updated, not added");
    let z = rustgis_data::zip::read(&std::fs::read(path("Countries.zip")).unwrap()).unwrap();
    assert!(z.iter().any(|e| e.0 == "collections/lakes/items.json"), "the web layer now holds the lakes");
    s.run("share.upload", &json!({"path": path("landcover.rgisdlpk"), "store": "out"})).unwrap_err();
    let up = dir.join("upload_src");
    std::fs::create_dir_all(&up).unwrap();
    std::fs::write(up.join("notes.txt"), "hello").unwrap();
    s.run("share.upload", &json!({"path": up.join("notes.txt").to_str().unwrap()})).unwrap();
    assert!(dir.join("notes.txt").exists());
    let jobs = s.run("share.jobs", &Value::Null).unwrap();
    assert_eq!(jobs["items"].as_array().unwrap().len(), before + 1);
    // A second file of that name isn't written over the first.
    assert!(s.run("share.upload", &json!({"path": up.join("notes.txt").to_str().unwrap()})).is_err());
    // Shares and data stores listed by an opened project aren't written to (their paths come
    // from the file) until they're shared or registered again.
    s.project = Project::from_json(&serde_json::to_string(&s.project).unwrap()).unwrap();
    let err = s.run("share.saveWebMap", &Value::Null).unwrap_err();
    assert!(err.contains("came with the opened project"), "{err}");
    let err = s.run("share.webMap", &Value::Null).unwrap_err();
    assert!(err.contains("came with the opened project"), "{err}");
    s.run("share.dataStore", &json!({"name": "out", "path": dir.to_str().unwrap()})).unwrap();
    s.run("share.webMap", &Value::Null).unwrap();
    s.run("share.dataStore", &json!({"remove": "out"})).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A FlatGeobuf of a grid of points over the world (`n` × `n`), numbered.
fn grid_fgb(name: &str, n: usize) -> std::path::PathBuf {
    use rustgis_data::{Coord, Dataset, Field, FieldType, Geometry, GeometryType, Value as V};
    let mut ds = Dataset::new(name, GeometryType::Point, vec![Field::new("i", FieldType::Integer), Field::new("x", FieldType::Double)]);
    for i in 0..n {
        for j in 0..n {
            let c = Coord::new(-179.0 + 358.0 * i as f64 / (n - 1) as f64, -80.0 + 160.0 * j as f64 / (n - 1) as f64);
            ds.push(Some(Geometry::Point(c)), vec![V::Int((i * n + j) as i64), V::Float(c.x)]);
        }
    }
    let p = std::env::temp_dir().join(format!("rustgis-engine-{}-{name}.fgb", std::process::id()));
    rustgis_data::io::write_path(&ds, &p).unwrap();
    p
}

/// A view of a lon/lat box, 800 × 600 points.
fn view_of(s: &mut Session, e: rustgis_geom::Extent) -> crate::render::View {
    s.view_size = (800.0, 600.0);
    s.zoom_to(&e);
    crate::render::View { width: 800.0, height: 600.0, pixels_per_point: 1.0, camera: s.map().camera, dark: false }
}

#[test]
fn large_sources_stream_a_view_at_a_time() {
    let path = grid_fgb("big", 150);
    let mut s = Session::new(Project::default());
    let r = s.run("layer.addData", &json!({"path": path.display().to_string(), "stream": true})).unwrap();
    assert_eq!(r["streamed"], true, "{r}");
    let id = r["layer"].as_u64().unwrap() as u32;
    // A small view limit, to see sampling without a huge file.
    s.map_mut().layer_mut(id).unwrap().source.as_mut().unwrap().limit = 5_000;
    let layer = |s: &Session| s.map().layer(id).unwrap().clone();
    assert!(layer(&s).is_streamed());
    // The whole extent, not the part read.
    assert!(layer(&s).extent().width() > 350.0);

    // The world: a spread sample, capped.
    let v = view_of(&mut s, rustgis_geom::Extent::new(-180.0, -80.0, 180.0, 80.0));
    let png = crate::render::render_png(s.map_mut(), &v).unwrap();
    assert!(!png.is_empty());
    let l = layer(&s);
    let n = l.dataset.features.len();
    assert!(n > 2_000 && n <= 5_000, "{n}");
    assert!(l.source.as_ref().unwrap().sampled().is_some());
    assert!(l.dataset.features.iter().any(|f| f.geometry.as_ref().unwrap().extent().min_x > 150.0));

    // Zoomed in: exactly the features in view (padded), all of them.
    let small = rustgis_geom::Extent::new(10.0, 10.0, 14.0, 13.0);
    let v = view_of(&mut s, small);
    let lonlat = crate::render::view_lonlat(s.map(), &v);
    // Read in the background, as the screen does, then wait for it.
    let mut pending = s.map_mut().sync_sources(&lonlat, false);
    let start = std::time::Instant::now();
    while pending && start.elapsed() < std::time::Duration::from_secs(30) {
        std::thread::sleep(std::time::Duration::from_millis(5));
        pending = s.map_mut().sync_sources(&lonlat, false);
    }
    assert!(!pending);
    let l = layer(&s);
    assert!(l.source.as_ref().unwrap().sampled().is_none());
    let w = l.source.as_ref().unwrap().window().unwrap().extent;
    let inside =
        |e: &rustgis_geom::Extent| l.dataset.features.iter().filter(|f| e.contains(f.geometry.as_ref().unwrap().extent().center())).count();
    assert_eq!(inside(&w), l.dataset.features.len());
    assert!(inside(&lonlat) > 0);

    // A selection survives the next read (by feature id).
    let ids: Vec<u64> = l.dataset.features.iter().take(3).map(|f| f.id).collect();
    s.map_mut().layer_mut(id).unwrap().selection = vec![0, 1, 2];
    let v = view_of(&mut s, rustgis_geom::Extent::new(-40.0, -40.0, 60.0, 60.0));
    crate::render::render_png(s.map_mut(), &v).unwrap();
    let l = layer(&s);
    let kept: Vec<u64> = l.selection.iter().map(|r| l.dataset.features[*r].id).collect();
    assert!(kept.iter().all(|k| ids.contains(k)) && !kept.is_empty(), "{kept:?}");

    // Edits are refused; Load All Features brings them into memory.
    s.run("edit.enable", &json!({"on": true})).unwrap();
    s.active_layer = Some(id);
    let e = s.run("edit.delete", &json!({"layer": id})).unwrap_err();
    assert!(e.contains("read in place"), "{e}");
    let r = s.run("layer.loadAll", &json!({"layer": id})).unwrap();
    assert_eq!(r["features"], 22_500);
    let l = layer(&s);
    assert!(!l.is_streamed() && !l.dataset.windowed);
    s.map_mut().layer_mut(id).unwrap().selection = vec![0];
    s.run("edit.delete", &json!({"layer": id})).unwrap();
    assert_eq!(layer(&s).dataset.features.len(), 22_499);
    let _ = std::fs::remove_file(path);
}

#[test]
fn streamed_layers_save_their_source_not_their_features() {
    let path = grid_fgb("saved", 60);
    let mut s = Session::new(Project::default());
    s.run("layer.addData", &json!({"path": path.display().to_string(), "stream": true})).unwrap();
    let v = view_of(&mut s, rustgis_geom::Extent::new(-180.0, -80.0, 180.0, 80.0));
    crate::render::render_png(s.map_mut(), &v).unwrap();
    assert_eq!(s.map().layers[0].dataset.features.len(), 3600);
    let text = serde_json::to_string(&s.project).unwrap();
    // The window isn't in the project (3,600 features would be far more than this).
    assert!(text.len() < 20_000, "{}", text.len());
    let mut t = Session::new(Project::default());
    t.run("project.openText", &json!({"text": text})).unwrap();
    let l = &t.map().layers[0];
    assert!(l.is_streamed() && l.source.as_ref().unwrap().is_open());
    assert!(l.dataset.features.is_empty());
    let v = view_of(&mut t, rustgis_geom::Extent::new(-180.0, -80.0, 180.0, 80.0));
    crate::render::render_png(t.map_mut(), &v).unwrap();
    assert_eq!(t.map().layers[0].dataset.features.len(), 3600);
    // A missing file is reported, not a crash.
    // (The path as the project's JSON spells it: escaped backslashes on Windows.)
    let quoted = serde_json::to_string(&std::path::absolute(&path).unwrap().display().to_string()).unwrap();
    assert!(text.contains(quoted.trim_matches('"')), "{text}");
    let gone = text.replace(quoted.trim_matches('"'), "/nonexistent/gone.fgb");
    let mut u = Session::new(Project::default());
    u.run("project.openText", &json!({"text": gone})).unwrap();
    assert!(u.map().layers[0].source.as_ref().unwrap().error.is_some());
    crate::render::render_png(u.map_mut(), &v).unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn tools_read_streamed_layers_whole() {
    let path = grid_fgb("tools", 50);
    let mut s = Session::new(Project::default());
    s.run("layer.addData", &json!({"path": path.display().to_string(), "stream": true})).unwrap();
    // Only a corner is in view.
    let v = view_of(&mut s, rustgis_geom::Extent::new(0.0, 0.0, 20.0, 20.0));
    crate::render::render_png(s.map_mut(), &v).unwrap();
    let windowed = s.map().layers[0].dataset.features.len();
    assert!(windowed < 2500, "{windowed}");
    let r = s.run("gp.run", &json!({"tool": "buffer", "params": {"input": "tools", "distance": 1}})).unwrap();
    let out = s.map().layer(r["layer"].as_u64().unwrap() as u32).unwrap();
    assert_eq!(out.dataset.features.len(), 2500);
    // A tool updating the layer from a selection (made in the window) would replace the whole
    // layer with that part: refused. Without a selection it updates the whole source.
    let id = s.map().layer_by_name("tools").unwrap().id;
    s.map_mut().layer_mut(id).unwrap().selection = vec![0, 1];
    let e = s.run("gp.run", &json!({"tool": "calculateGeometry", "params": {"input": "tools"}})).unwrap_err();
    assert!(e.contains("Load All Features"), "{e}");
    assert!(s.map().layer(id).unwrap().is_streamed());
    s.map_mut().layer_mut(id).unwrap().selection.clear();
    s.run("gp.run", &json!({"tool": "calculateGeometry", "params": {"input": "tools"}})).unwrap();
    assert_eq!(s.map().layer(id).unwrap().dataset.features.len(), 2500);
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_streamed_layer_whose_file_goes_away_shows_an_error_not_stale_features() {
    let path = grid_fgb("vanishing", 40);
    let mut s = Session::new(Project::default());
    s.run("layer.addData", &json!({"path": path.display().to_string(), "stream": true})).unwrap();
    let v = view_of(&mut s, rustgis_geom::Extent::new(0.0, 0.0, 30.0, 30.0));
    crate::render::render_png(s.map_mut(), &v).unwrap();
    assert!(!s.map().layers[0].dataset.features.is_empty());
    std::fs::remove_file(&path).unwrap();
    let v = view_of(&mut s, rustgis_geom::Extent::new(-150.0, -60.0, -120.0, -30.0));
    crate::render::render_png(s.map_mut(), &v).unwrap();
    let l = &s.map().layers[0];
    assert!(l.dataset.features.is_empty());
    assert!(l.source.as_ref().unwrap().error.is_some());
}

#[test]
fn any_command_editing_a_streamed_layer_is_undone() {
    let path = grid_fgb("guarded", 30);
    let mut s = Session::new(Project::default());
    s.run("layer.addData", &json!({"path": path.display().to_string(), "stream": true})).unwrap();
    // A sample of the world, so a closer view reads again.
    s.map_mut().layers[0].source.as_mut().unwrap().limit = 300;
    let v = view_of(&mut s, rustgis_geom::Extent::new(-180.0, -80.0, 180.0, 80.0));
    crate::render::render_png(s.map_mut(), &v).unwrap();
    let before = s.map().layers[0].dataset.features.clone();
    // Whatever the command, an edit of the window is undone and refused.
    let r = s.guarded("Some Edit", true, |s| {
        if let Some(f) = s.map_mut().layers[0].dataset.features.get_mut(0) {
            f.attrs[0] = rustgis_data::Value::Int(-1);
        }
        Ok(Value::Null)
    });
    assert!(r.unwrap_err().contains("Load All Features"));
    assert_eq!(s.map().layers[0].dataset.features, before);
    // A command whose render reads the source again is fine.
    let r = s.guarded("Render", true, |s| {
        let v = view_of(s, rustgis_geom::Extent::new(0.0, 0.0, 20.0, 20.0));
        crate::render::render_png(s.map_mut(), &v).map(|_| Value::Null)
    });
    assert!(r.is_ok(), "{r:?}");
    assert_ne!(s.map().layers[0].dataset.features.len(), before.len());
    let _ = std::fs::remove_file(path);
}

/// A GDAL-only format (GeoJSON sequence), when GDAL is installed (skipped otherwise).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn gdal_formats_open_when_gdal_is_installed() {
    if !rustgis_data::source::gdal::available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("rustgis-engine-gdal-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let seq = dir.join("walk.geojsonl");
    std::fs::write(
        &seq,
        "{\"type\":\"Feature\",\"properties\":{\"name\":\"start\"},\"geometry\":{\"type\":\"Point\",\"coordinates\":[7,45]}}\n{\"type\":\"Feature\",\"properties\":{\"name\":\"end\"},\"geometry\":{\"type\":\"Point\",\"coordinates\":[7.1,45.1]}}\n",
    )
    .unwrap();
    let mut s = Session::new(Project::default());
    let r = s.run("layer.addData", &json!({"path": seq.display().to_string()})).unwrap();
    let l = s.map().layers.iter().find(|l| l.dataset.features.len() == 2).unwrap_or_else(|| panic!("{r}"));
    assert_eq!(l.dataset.source.as_deref(), Some(std::path::absolute(&seq).unwrap().display().to_string().as_str()));
    // Streamed, the layer remembers the original file.
    let r = s.run("layer.addData", &json!({"path": seq.display().to_string(), "stream": true})).unwrap();
    let id = r["layer"].as_u64().unwrap() as u32;
    assert!(s.map().layer(id).unwrap().source.as_ref().unwrap().gdal.is_some());
    let _ = std::fs::remove_dir_all(dir);
}

/// KML (with its styles), KMZ and GPX through the built-in readers: files on disk and uploads.
#[test]
fn kml_and_gpx_add_as_layers() {
    let dir = std::env::temp_dir().join(format!("rustgis-engine-kml-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let kml = r##"<kml xmlns="http://www.opengis.net/kml/2.2"><Document>
<Style id="red"><LineStyle><color>ff0000ff</color><width>4</width></LineStyle></Style>
<Style id="blue"><LineStyle><color>ffff0000</color></LineStyle></Style>
<Placemark><name>a</name><styleUrl>#red</styleUrl><LineString><coordinates>7,45 8,46</coordinates></LineString></Placemark>
<Placemark><name>b</name><styleUrl>#blue</styleUrl><LineString><coordinates>7,46 8,45</coordinates></LineString></Placemark>
<Placemark><name>p</name><Point><coordinates>7.5,45.5</coordinates></Point></Placemark>
</Document></kml>"##;
    let path = dir.join("roads.kml");
    std::fs::write(&path, kml).unwrap();
    let mut s = Session::new(Project::default());
    let listed = s.run("info.fileLayers", &json!({"path": path.display().to_string()})).unwrap();
    assert_eq!(listed["layers"], json!(["roads points", "roads lines"]));
    let r = s.run("layer.addData", &json!({"path": path.display().to_string()})).unwrap();
    assert_eq!(r["layers"].as_array().map(Vec::len), Some(2), "{r}");
    let lines = s.map().layer_by_name("roads lines").unwrap();
    match &lines.renderer {
        rustgis_map::Renderer::Unique { field, classes, .. } => {
            assert_eq!(field, "style");
            let red = classes.iter().find(|(k, _)| k == "red").unwrap().1;
            assert_eq!((red.stroke, red.stroke_width), (rustgis_map::Rgba::new(255, 0, 0, 255), 4.0));
        }
        other => panic!("{other:?}"),
    }
    // One layer by name.
    let one = s.run("layer.addData", &json!({"path": path.display().to_string(), "layer": "lines"})).unwrap();
    assert_eq!(one["layers"].as_array().map(Vec::len), Some(1));
    assert!(s.run("layer.addData", &json!({"path": path.display().to_string(), "layer": "nope"})).is_err());
    // A KMZ upload (the web app).
    {
        use base64::Engine;
        let kmz = rustgis_data::zip::write(&[("doc.kml".to_string(), kml.as_bytes().to_vec())]).unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(kmz);
        let r = s.run("layer.addDataBytes", &json!({"fileName": "roads.kmz", "data": data})).unwrap();
        assert_eq!(r["layers"].as_array().map(Vec::len), Some(2), "{r}");
    }
    // GPX: waypoints, tracks and track points.
    let gpx = dir.join("walk.gpx");
    std::fs::write(
        &gpx,
        r#"<?xml version="1.0"?><gpx version="1.1" creator="t" xmlns="http://www.topografix.com/GPX/1/1">
<wpt lat="45.0" lon="7.0"><name>start</name></wpt><wpt lat="45.1" lon="7.1"><name>end</name></wpt>
<trk><name>walk</name><trkseg><trkpt lat="45.0" lon="7.0"/><trkpt lat="45.05" lon="7.05"/><trkpt lat="45.1" lon="7.1"/></trkseg></trk></gpx>"#,
    )
    .unwrap();
    let mut s = Session::new(Project::default());
    s.run("layer.addData", &json!({"path": gpx.display().to_string()})).unwrap();
    let names: Vec<String> = s.map().layers.iter().map(|l| l.name.clone()).collect();
    for n in ["walk waypoints", "walk tracks", "walk track points"] {
        assert!(names.contains(&n.to_string()), "{names:?}");
    }
    assert_eq!(s.map().layer_by_name("walk waypoints").unwrap().dataset.features.len(), 2);
    let project = dir.join("p.rgis").display().to_string();
    s.run("project.saveAs", &json!({"path": project})).unwrap();
    s.run("project.open", &json!({"path": project})).unwrap();
    assert_eq!(s.map().layers.len(), 3);
    let _ = std::fs::remove_dir_all(dir);
}

/// SQL queries and DuckDB databases through DuckDB, when it is installed; without it, both say
/// how to get it.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn duckdb_queries_and_databases() {
    let mut s = Session::new(Project::default());
    let query = "SELECT 'a' AS name, ST_Point(7, 45) AS geom UNION ALL SELECT 'b', ST_GeomFromText('LINESTRING(7 45, 8 46)')";
    if !rustgis_data::source::duckdb::available() {
        let e = s.run("layer.addSql", &json!({"sql": query})).unwrap_err();
        assert!(e.contains("need DuckDB"), "{e}");
        let e = s.run("layer.addData", &json!({"path": "/nowhere/x.duckdb"})).unwrap_err();
        assert!(e.contains("need DuckDB"), "{e}");
        return;
    }
    // A result mixing geometry types: a layer per type.
    let r = s.run("layer.addSql", &json!({"sql": query, "name": "Trip"})).unwrap();
    let names: Vec<String> = s.map().layers.iter().map(|l| l.name.clone()).collect();
    assert_eq!(names, vec!["Trip (points)", "Trip (lines)"], "{r}");
    // Streamed, a layer remembers its query; a project never runs it again when the result is gone
    // (a project file is hostile input), and says how to.
    let r = s
        .run(
            "layer.addSql",
            &json!({"sql": "SELECT i AS id, ST_Point(i, i / 2) AS geom FROM range(50) t(i)", "name": "Pts", "stream": true}),
        )
        .unwrap();
    assert_eq!(r["features"], 50);
    let id = r["layer"].as_u64().unwrap() as u32;
    let src = s.map().layer(id).unwrap().source.clone().unwrap();
    let d = src.duckdb.clone().unwrap();
    assert!(d.sql.as_deref().is_some_and(|q| q.contains("range(50)")) && d.family == rustgis_data::source::Family::Points);
    let cached = src.path.clone().unwrap();
    let text = serde_json::to_string(&s.project).unwrap();
    std::fs::remove_file(&cached).unwrap();
    let mut t = Session::new(Project::default());
    t.run("project.openText", &json!({"text": text})).unwrap();
    let l = t.map().layer_by_name("Pts").unwrap();
    let again = l.source.as_ref().unwrap();
    assert!(!again.is_open());
    assert!(again.error.as_deref().is_some_and(|e| e.contains("run the query again")), "{:?}", again.error);
    // Errors are DuckDB's, and nothing is added.
    let before = s.map().layers.len();
    assert!(s.run("layer.addSql", &json!({"sql": "SELECT 1 AS a"})).unwrap_err().contains("no geometry column"));
    assert!(s.run("layer.addSql", &json!({"sql": "SELECT * FROM nowhere"})).unwrap_err().contains("nowhere"));
    assert_eq!(s.map().layers.len(), before);

    // A database file: its geometry tables become layers, or the one named.
    let dir = std::env::temp_dir().join(format!("rustgis-engine-duckdb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("places.duckdb");
    let ok = std::process::Command::new(rustgis_data::source::duckdb::program().unwrap())
        .arg(&db)
        .arg("-c")
        .arg("LOAD spatial; CREATE TABLE towns AS SELECT 'x' AS name, ST_Point(2.35, 48.85) AS geom; CREATE TABLE lakes AS SELECT ST_GeomFromText('POLYGON((6 46, 7 46, 7 47, 6 46))') AS geom; CREATE TABLE notes(t TEXT);")
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let mut s = Session::new(Project::default());
    let r = s.run("layer.addData", &json!({"path": db.display().to_string()})).unwrap();
    let mut names: Vec<String> = s.map().layers.iter().map(|l| l.name.clone()).collect();
    names.sort();
    assert_eq!(names, vec!["lakes", "towns"], "{r}");
    assert_eq!(
        s.map().layer_by_name("towns").unwrap().dataset.source.as_deref(),
        Some(std::path::absolute(&db).unwrap().display().to_string().as_str())
    );
    let r = s.run("layer.addData", &json!({"path": db.display().to_string(), "layer": "towns", "stream": true, "name": "T"})).unwrap();
    let id = r["layer"].as_u64().unwrap() as u32;
    let src = s.map().layer(id).unwrap().source.clone().unwrap();
    assert_eq!(src.duckdb.as_ref().unwrap().table.as_deref(), Some("towns"));
    // A table is converted again when its result is gone.
    let text = serde_json::to_string(&s.project).unwrap();
    std::fs::remove_file(src.path.as_deref().unwrap()).unwrap();
    let mut t = Session::new(Project::default());
    t.run("project.openText", &json!({"text": text})).unwrap();
    let again = t.map().layer_by_name("T").unwrap().source.as_ref().unwrap();
    assert!(again.is_open(), "{:?}", again.error);
    assert_eq!(again.info().and_then(|i| i.count), Some(1));
    assert!(s.run("layer.addData", &json!({"path": db.display().to_string(), "layer": "notes"})).is_err());
    // Its layers are listed without adding them.
    let listed = s.run("info.fileLayers", &json!({"path": db.display().to_string()})).unwrap();
    assert_eq!(listed, json!({"count": 2, "layers": ["lakes", "towns"]}));
    // A query over the database's tables.
    let r = s.run("layer.addSql", &json!({"sql": "SELECT * FROM towns", "database": db.display().to_string(), "name": "Q"})).unwrap();
    assert_eq!(r["features"], 1);
    // Projected coordinates that don't say their system are pointed out; "crs" converts them.
    let projected = "SELECT ST_Point(500000, 4200000) AS geom";
    s.run("layer.addSql", &json!({"sql": projected, "name": "Utm"})).unwrap();
    assert!(s.notices.iter().any(|n| n.error && n.text.starts_with("Utm: the data doesn't say its coordinate system")));
    let r = s.run("layer.addSql", &json!({"sql": projected, "name": "Utm2", "crs": "EPSG:32633"})).unwrap();
    let e = s.map().layer(r["layer"].as_u64().unwrap() as u32).unwrap().extent();
    assert!((e.min_x - 15.0).abs() < 0.01 && (e.min_y - 37.95).abs() < 0.05, "{e:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pmtiles_archives_become_tile_layers() {
    let mut s = Session::new(Project::default());
    // A file added as data is a tile layer of kind pmtiles (read when the map draws).
    let path = std::env::temp_dir().join("rustgis-test-archive.pmtiles");
    s.run("layer.addData", &json!({"path": path.display().to_string()})).unwrap();
    let t = &s.map().tiles[0];
    assert_eq!(t.kind, rustgis_map::TileKind::PmTiles);
    assert_eq!(t.name, "rustgis-test-archive");
    // A web address is guessed from its extension.
    s.run("map.addTiles", &json!({"url": "https://example.com/data/world.pmtiles"})).unwrap();
    assert!(s.map().tiles.iter().any(|t| t.kind == rustgis_map::TileKind::PmTiles && t.url.starts_with("https://")));
    // A relative path or another kind of file is refused.
    assert!(s.run("map.addTiles", &json!({"url": "world.pmtiles", "kind": "pmtiles"})).is_err());
    assert!(s.run("map.addTiles", &json!({"url": "/tmp/x.txt", "kind": "pmtiles"})).is_err());
    // A project keeps the layer.
    let text = serde_json::to_string(&s.project).unwrap();
    let mut t = Session::new(Project::default());
    t.run("project.openText", &json!({"text": text})).unwrap();
    assert_eq!(t.map().tiles.iter().filter(|x| x.kind == rustgis_map::TileKind::PmTiles).count(), 2);
}

/// An MBTiles file of one red picture tile: added by path or as an upload, it is a tile layer that
/// draws, survives a project round trip (by path), and a file that isn't an archive is refused.
#[test]
fn mbtiles_archives_become_tile_layers() {
    use crate::render::View;
    use base64::Engine;
    let dir = std::env::temp_dir().join(format!("rustgis-mbtiles-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("red.mbtiles");
    let _ = std::fs::remove_file(&path);
    let mut red = tiny_skia::Pixmap::new(256, 256).unwrap();
    red.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch(
        "CREATE TABLE metadata (name text, value text); CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
         INSERT INTO metadata VALUES ('name', 'Red'), ('format', 'png'), ('minzoom', '0'), ('maxzoom', '0'), ('attribution', '© Red');",
    )
    .unwrap();
    c.execute("INSERT INTO tiles VALUES (0, 0, 0, ?1)", [red.encode_png().unwrap()]).unwrap();
    drop(c);
    let p = path.display().to_string();

    let draws_red = |s: &mut Session| {
        s.run("map.goToXY", &json!({"lon": 10.0, "lat": 20.0, "scale": 50_000_000})).unwrap();
        let v = View { width: 300.0, height: 200.0, pixels_per_point: 1.0, camera: s.map().camera, dark: false };
        let mut px = None;
        for _ in 0..50 {
            let r = crate::render::render(s.map_mut(), &v).unwrap();
            px = r.pixmap.pixel(150, 100);
            if px.is_some_and(|p| p.red() > 200 && p.green() < 60) {
                return;
            }
            crate::render::rustgis_tiles::cache::wait(std::time::Duration::from_millis(100));
        }
        panic!("the tile layer didn't draw: {px:?}");
    };

    let mut s = Session::new(Project::default());
    s.run("layer.addData", &json!({"path": p})).unwrap();
    let t = &s.map().tiles[0];
    assert_eq!((t.kind, t.name.as_str()), (rustgis_map::TileKind::MbTiles, "red"));
    draws_red(&mut s);
    // A project keeps the layer by its path.
    let text = serde_json::to_string(&s.project).unwrap();
    let mut t = Session::new(Project::default());
    t.run("project.openText", &json!({"text": text})).unwrap();
    assert_eq!(t.map().tiles.first().map(|x| x.kind), Some(rustgis_map::TileKind::MbTiles));
    draws_red(&mut t);

    // An upload is held in memory.
    let b64 = base64::engine::general_purpose::STANDARD.encode(std::fs::read(&path).unwrap());
    let mut u = Session::new(Project::default());
    u.run("layer.addDataBytes", &json!({"fileName": "uploaded.mbtiles", "data": b64})).unwrap();
    assert_eq!(u.map().tiles[0].url, "upload:uploaded.mbtiles");
    draws_red(&mut u);

    // Not an archive, a relative path, a network share, an upload never made.
    let junk = dir.join("junk.mbtiles");
    std::fs::write(&junk, b"not sqlite").unwrap();
    assert!(s.run("layer.addData", &json!({"path": junk.display().to_string()})).is_err());
    let junk64 = base64::engine::general_purpose::STANDARD.encode(b"not sqlite");
    assert!(s.run("layer.addDataBytes", &json!({"fileName": "junk.mbtiles", "data": junk64})).is_err());
    for bad in ["red.mbtiles", r"\\attacker\share\x.mbtiles", "/tmp/x.pmtiles", "upload:"] {
        assert!(s.run("map.addTiles", &json!({"url": bad, "kind": "mbtiles"})).is_err(), "{bad}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A KMZ's ground overlays become raster layers kept in the project; a KML whose picture is a web
/// address or outside its folder says so.
#[test]
fn kml_ground_overlays_become_rasters() {
    use base64::Engine;
    let mut red = tiny_skia::Pixmap::new(8, 8).unwrap();
    red.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
    let doc = r#"<kml xmlns="http://www.opengis.net/kml/2.2"><Document>
<Placemark><name>P</name><Point><coordinates>10.5,40.5</coordinates></Point></Placemark>
<GroundOverlay><name>Red</name><Icon><href>files/red.png</href></Icon><LatLonBox><north>41</north><south>40</south><east>11</east><west>10</west><rotation>10</rotation></LatLonBox></GroundOverlay>
<GroundOverlay><name>Quad</name><Icon><href>files/red.png</href></Icon></GroundOverlay>
</Document></kml>"#;
    let kmz = rustgis_data::zip::write(&[("doc.kml".into(), doc.as_bytes().to_vec()), ("files/red.png".into(), red.encode_png().unwrap())])
        .unwrap();
    let mut s = Session::new(Project::default());
    let b64 = base64::engine::general_purpose::STANDARD.encode(&kmz);
    let r = s.run("layer.addDataBytes", &json!({"fileName": "site.kmz", "data": b64})).unwrap();
    assert_eq!(r["layers"].as_array().map(Vec::len), Some(2), "{r}");
    let overlay = s.map().layer_by_name("Red").unwrap();
    assert!(overlay.raster.as_ref().is_some_and(|r| r.data.is_some()));
    assert!(s.notices.iter().any(|n| n.text.contains("1 ground overlays without a picture or a box")));
    // The project keeps the picture.
    let text = serde_json::to_string(&s.project).unwrap();
    let mut t = Session::new(Project::default());
    t.run("project.openText", &json!({"text": text})).unwrap();
    assert!(t.map().layer_by_name("Red").and_then(|l| l.raster.as_ref()).is_some_and(|r| r.data.is_some()));
    // A KML of overlays only, whose pictures aren't readable from where it is.
    let dir = std::env::temp_dir().join(format!("rustgis-overlay-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let only = r#"<kml><GroundOverlay><name>Far</name><Icon><href>../secret.png</href></Icon><LatLonBox><north>1</north><south>0</south><east>1</east><west>0</west></LatLonBox></GroundOverlay>
<GroundOverlay><name>Web</name><Icon><href>https://example.com/x.png</href></Icon><LatLonBox><north>1</north><south>0</south><east>1</east><west>0</west></LatLonBox></GroundOverlay>
<GroundOverlay><name>Here</name><Icon><href>red.png</href></Icon><LatLonBox><north>1</north><south>0</south><east>1</east><west>0</west></LatLonBox></GroundOverlay></kml>"#;
    std::fs::write(dir.join("o.kml"), only).unwrap();
    std::fs::write(dir.join("red.png"), red.encode_png().unwrap()).unwrap();
    let mut u = Session::new(Project::default());
    u.run("layer.addData", &json!({"path": dir.join("o.kml").display().to_string()})).unwrap();
    assert!(u.map().layer_by_name("Here").is_some());
    assert!(u.map().layer_by_name("Far").is_none() && u.map().layer_by_name("Web").is_none());
    assert!(u.notices.iter().any(|n| n.text.contains("Far (its picture ../secret.png isn't in the file or beside it)")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pmtiles_from_network_shares_are_refused() {
    let mut s = Session::new(Project::default());
    for p in [r"\\attacker\share\x.pmtiles", "//attacker/share/x.pmtiles", "file://attacker/share/x.pmtiles"] {
        assert!(s.run("map.addTiles", &json!({"url": p, "kind": "pmtiles"})).is_err(), "{p}");
    }
    assert!(crate::render::rustgis_tiles::pmtiles::check_local(r"\\attacker\share\x.pmtiles").is_err());
    assert!(crate::render::rustgis_tiles::pmtiles::check_local("/data/x.pmtiles").is_ok());
}

/// A GeoPackage with more feature tables than are added at once says which were left out, in the
/// result and in a notice (issue #85); `info.fileLayers` lists them all.
#[test]
fn geopackage_table_cap_is_reported() {
    let dir = std::env::temp_dir().join(format!("rustgis-gpkg-cap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("many.gpkg");
    let _ = std::fs::remove_file(&path);
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch(
        "CREATE TABLE gpkg_spatial_ref_sys (srs_name TEXT, srs_id INTEGER PRIMARY KEY, organization TEXT, organization_coordsys_id INTEGER, definition TEXT, description TEXT);
         INSERT INTO gpkg_spatial_ref_sys VALUES ('WGS 84', 4326, 'EPSG', 4326, '', '');
         CREATE TABLE gpkg_contents (table_name TEXT PRIMARY KEY, data_type TEXT, identifier TEXT, description TEXT, last_change TEXT, min_x DOUBLE, min_y DOUBLE, max_x DOUBLE, max_y DOUBLE, srs_id INTEGER);
         CREATE TABLE gpkg_geometry_columns (table_name TEXT, column_name TEXT, geometry_type_name TEXT, srs_id INTEGER, z TINYINT, m TINYINT);",
    )
    .unwrap();
    let mut blob = b"GP\x00\x01".to_vec();
    blob.extend_from_slice(&4326i32.to_le_bytes());
    blob.extend(rustgis_data::wkb::encode(&rustgis_data::Geometry::Point(rustgis_data::Coord::new(2.35, 48.85))));
    for i in 1..=34 {
        let t = format!("t_{i:02}");
        c.execute_batch(&format!(
            "INSERT INTO gpkg_contents (table_name, data_type, srs_id) VALUES ('{t}', 'features', 4326);
             INSERT INTO gpkg_geometry_columns VALUES ('{t}', 'geom', 'POINT', 4326, 0, 0);
             CREATE TABLE {t} (fid INTEGER PRIMARY KEY AUTOINCREMENT, geom BLOB);"
        ))
        .unwrap();
        c.execute(&format!("INSERT INTO {t} (geom) VALUES (?1)"), [&blob]).unwrap();
    }
    drop(c);
    let p = path.display().to_string();

    let mut s = Session::new(Project::default());
    let listed = s.run("info.fileLayers", &json!({"path": p})).unwrap();
    assert_eq!(listed["count"], 34);
    assert_eq!(listed["layers"][33], "t_34");

    let mark = s.notice_mark();
    let r = s.run("layer.addData", &json!({"path": p})).unwrap();
    assert_eq!(r["layers"].as_array().map(Vec::len), Some(32));
    assert_eq!(r["tables"], 34);
    assert_eq!(r["skipped"], json!(["t_33", "t_34"]));
    assert_eq!(r["skippedCount"], 2);
    let said = s.notices_since(mark);
    assert!(said.iter().any(|n| n.text.contains("first 32 of 34 tables")), "{said:?}");
    // A table past the cap is added by name.
    let one = s.run("layer.addData", &json!({"path": p, "layer": "t_34"})).unwrap();
    assert!(one.get("skipped").is_none());
    assert_eq!(s.map().layers.len(), 33);
    // The pure-Rust reader the web app lists an upload's tables with sees them all.
    let listed =
        rustgis_data::source::gpkg_mem::layers(rustgis_data::source::Shared(std::sync::Arc::new(std::fs::read(&path).unwrap()))).unwrap();
    assert_eq!(listed.len(), 34);
    assert!(s.run("info.fileLayers", &json!({"path": dir.join("none.geojson").display().to_string()})).is_err());
    let notes = dir.join("notes.rgis");
    std::fs::write(&notes, "{}").unwrap();
    assert!(s.run("info.fileLayers", &json!({"path": notes.display().to_string()})).is_err(), "not a data file");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn notices_since_survives_the_cap() {
    let mut s = Session::new(Project::default());
    for i in 0..250 {
        s.notify(format!("n{i}"), false);
    }
    let mark = s.notice_mark();
    assert!(s.notices_since(mark).is_empty());
    s.notify("last", false);
    assert_eq!(s.notices_since(mark).iter().map(|n| n.text.as_str()).collect::<Vec<_>>(), ["last"]);
    assert_eq!(s.notices_since(0).len(), 200);
}

/// The web app's `?blank&projection=EPSG:3010` (discussion #81): an empty project starts in a
/// national grid, and that start is neither an edit to undo nor unsaved work.
#[test]
fn blank_project_starts_in_a_projected_crs() {
    isolate_plugins();
    let mut s = Session::new(Project::default());
    assert!(s.map().layers.is_empty());
    let r = s.run("map.projection", &json!({"type": "EPSG:3010"})).unwrap();
    assert_eq!(r["projection"], "EPSG:3010");
    assert!(s.dirty && s.can_undo());
    s.clear_history();
    assert!(!s.dirty && !s.can_undo());
    assert_eq!(s.map().projection.code(), "EPSG:3010");
    // It opens on the country, and reports that view back.
    s.run("map.zoomToExtent", &json!({"extent": [10.5, 55.0, 24.5, 69.5]})).unwrap();
    let e = s.run("map.extent", &Value::Null).unwrap()["extent"].clone();
    let n: Vec<f64> = e.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!(n[0] < 11.0 && n[2] > 24.0 && n[1] < 55.5 && n[3] > 69.0 && n[3] - n[1] < 25.0, "{n:?}");
    for bad in [json!(null), json!([1, 2, 3]), json!([20, 50, 10, 60]), json!([0, 0, 200, 10]), json!(["a", 0, 1, 1])] {
        assert!(s.run("map.zoomToExtent", &json!({"extent": bad})).is_err(), "{bad}");
    }
    assert!(crate::cmd::share::map_png(&mut s, Some(400.0), Some(300.0), None).unwrap().len() > 1000);
}

#[test]
fn layers_export_to_geopackage_geoparquet_shapefile_and_kmz() {
    let dir = std::env::temp_dir().join(format!("rustgis-engine-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = world();
    let want = s.map().layers.iter().find(|l| l.name == "Countries").unwrap().dataset.features.len();
    for ext in ["gpkg", "parquet", "shp", "kmz", "zip"] {
        let path = dir.join(format!("countries.{ext}")).display().to_string();
        let r = s.run("share.exportLayer", &json!({"layer": "Countries", "path": path})).unwrap();
        assert_eq!((r["features"].as_u64(), r["leftOut"].as_u64()), (Some(want as u64), Some(0)), "{ext}");
        if matches!(ext, "kmz" | "zip") {
            assert!(std::fs::metadata(&path).unwrap().len() > 0);
            continue;
        }
        // Added back, drawn and saved in a project that reopens.
        let r = s.run("layer.addData", &json!({"path": path})).unwrap();
        let id = r["layer"].as_u64().unwrap() as u32;
        let l = s.map().layer(id).unwrap();
        let n = if l.is_streamed() { l.load_all().unwrap().features.len() } else { l.dataset.features.len() };
        assert_eq!(n, want, "{ext}");
    }
    let project = dir.join("p.rgis").display().to_string();
    s.run("project.saveAs", &json!({"path": project})).unwrap();
    s.run("project.open", &json!({"path": project})).unwrap();
    s.run("share.exportMap", &json!({"path": dir.join("m.png").display().to_string()})).unwrap();
    // Not a format it writes.
    assert!(s.run("share.exportLayer", &json!({"layer": "Countries", "path": dir.join("x.dwg").display().to_string()})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn zipped_shapefiles_add_as_layers() {
    use base64::Engine;
    // The sample's countries and cities as Shapefiles in one zip, under a folder.
    let dir = std::env::temp_dir().join(format!("rustgis-engine-zip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut entries = Vec::new();
    for (name, text) in [("countries", crate::sample::countries()), ("cities", crate::sample::CITIES)] {
        let (ds, _) = rustgis_data::io::read_bytes(&format!("{name}.geojson"), text.as_bytes()).unwrap();
        let (zipped, _) = rustgis_data::io::write_bytes(&ds, "shp").unwrap();
        for (n, b) in rustgis_data::zip::read(&zipped).unwrap() {
            entries.push((format!("world/{n}"), b));
        }
    }
    let zip = rustgis_data::zip::write(&entries).unwrap();
    // Uploaded (the web app).
    let mut s = Session::new(Project::default());
    let data = base64::engine::general_purpose::STANDARD.encode(&zip);
    let r = s.run("layer.addDataBytes", &json!({"fileName": "world.zip", "data": data})).unwrap();
    assert_eq!(r["layers"].as_array().map(Vec::len), Some(2), "{r}");
    let names: Vec<&str> = s.map().layers.iter().map(|l| l.name.as_str()).collect();
    assert!(names.contains(&"cities") && names.contains(&"countries"), "{names:?}");
    // From disk (the desktop app), saved and reopened with the project.
    let path = dir.join("world.zip");
    std::fs::write(&path, &zip).unwrap();
    let mut s = Session::new(Project::default());
    let r = s.run("layer.addData", &json!({"path": path.display().to_string(), "name": "Towns"})).unwrap();
    assert!(r["features"].as_u64().unwrap() > 10, "{r}");
    assert_eq!(s.map().layers.iter().filter(|l| l.name == "Towns").count(), 1);
    let project = dir.join("p.rgis").display().to_string();
    s.run("project.saveAs", &json!({"path": project})).unwrap();
    s.run("project.open", &json!({"path": project})).unwrap();
    assert_eq!(s.map().layers.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
