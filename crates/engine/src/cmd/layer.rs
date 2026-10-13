//! Home › Add (Add Data), Layers pane actions, and the Feature Layer / Labeling / Data tabs.

use std::path::Path;

use rustgis_data::GeometryType;
use rustgis_map::{ClassMethod, Marker, Ramp, Renderer, Rgba};
use serde_json::{Value, json};

use super::{b, f, need_feature_layer, need_layer, ps};
use crate::{CmdResult, CommandSpec as C, Session};

pub fn commands() -> Vec<C> {
    vec![
        C::new("layer.addData", "Add Data", "Home › Add", add_data)
            .params(r#"{"path": string, "name"?: string, "layer"?: "GeoPackage, GDAL or DuckDB layer", "stream"?: bool, "crs"?: "EPSG:… (or WKT) for a CSV's coordinates, or a GDAL or DuckDB file that doesn't say", "x"?: "CSV x/longitude column", "y"?: "CSV y/latitude column", "wkt"?: "CSV WKT geometry column", "delimiter"?: ","|";"|"\\t"|"|"}"#),
        C::new("info.fileLayers", "List Layers in File", "Home › Add › Add Data", file_layers)
            .view()
            .params(r#"{"path": string}"#),
        C::new("layer.addSql", "Add Data From SQL", "Home › Add", add_sql).params(
            r#"{"sql": "one DuckDB SELECT; DuckDB Spatial's ST_ functions, read_parquet('…'), web addresses", "name"?: string, "database"?: "a .duckdb file whose tables the query names", "crs"?: "EPSG:… of a geometry that doesn't say", "stream"?: bool}"#,
        ),
        C::new("layer.loadAll", "Load All Features", "Layers", load_all).when(need_streamed).params(r#"{"layer"?: name|id}"#),
        C::new("layer.addUrl", "Add Data From URL", "Home › Add", add_url).params(
            r#"{"url": "https://…/data.geojson|.csv|.parquet|.fgb|.gpkg|.zip|.kml|.kmz|.gpx|.tif (the format from the address, else the contents) | a .pmtiles archive", "name"?: text, "layer"?: "one layer of a zip, KML or GPX"}"#,
        ),
        C::new("layer.addDownload", "Add Downloaded Data", "Home › Add", add_download)
            .params(r#"{"id": a download the app finished (Add Data › From URL), "url": its address, "name"?: text, "layer"?: text}"#),
        C::new("layer.addEncodedPolyline", "Add Encoded Polyline", "Home › Add", add_encoded_polyline).params(
            r#"{"polyline": "encoded polyline text" | [texts] (one line each), "precision"?: 5 (Google) | 6 (OSRM, Valhalla), "name"?: text}"#,
        ),
        C::new("layer.addDataText", "Add Data (from text)", "Home › Add", add_data_text).params(r#"{"fileName": "name.geojson|.csv|.tsv", "text": string, "name"?: text, "x"?, "y"?, "wkt"?, "crs"?, "delimiter"? (as layer.addData, for delimited text)}"#),
        C::new("layer.addSample", "Add Sample Data", "Home › Add › Add Data", add_sample).params(r#"{"name": "countries"|"cities"|"rivers"|"lakes"}"#),
        C::new("layer.remove", "Remove", "Layers", remove).key("Delete").when(need_layer).params(r#"{"layer"?: name|id}"#),
        C::new("layer.select", "Select Layer", "Layers", |s, p| {
            s.active_layer = Some(s.layer_id(p)?);
            Ok(Value::Null)
        })
        .view(),
        C::new("layer.rename", "Rename", "Layers", |s, p| {
            let n = s_req(p, "name")?.trim().chars().take(200).collect::<String>();
            if n.is_empty() {
                return Err("name is required".into());
            }
            let old = s.map().layer(s.layer_id(p)?).map(|l| l.name.clone()).unwrap_or_default();
            let r = with(s, p, |l| {
                l.name = n.clone();
                l.dataset.name = n.clone();
            })?;
            // Layers masked by this one follow the new name.
            for l in &mut s.map_mut().layers {
                if l.mask.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(&old)) {
                    l.mask = Some(n.clone());
                }
            }
            Ok(r)
        })
        .when(need_layer),
        C::new("layer.visibility", "Visibility", "Layers", |s, p| {
            let on = b(p, "visible");
            with(s, p, |l| l.visible = on.unwrap_or(!l.visible))
        })
        .when(need_layer),
        C::new("layer.moveUp", "Move Up", "Layers", |s, p| move_layer(s, p, -1)).when(need_layer),
        C::new("layer.moveDown", "Move Down", "Layers", |s, p| move_layer(s, p, 1)).when(need_layer),
        C::new("layer.zoomTo", "Zoom To Layer", "Layers", |s, p| {
            let id = s.layer_id(p)?;
            let e = s.map().layer(id).map(|l| l.extent()).unwrap_or_default();
            if e.is_empty() {
                return Err("the layer has no features".into());
            }
            s.zoom_to(&e);
            Ok(Value::Null)
        })
        .view()
        .when(need_layer),
        C::new("layer.symbology", "Symbology", "Style › Drawing", symbology)
            .when(need_feature_layer)
            .params(r#"{"layer"?, "type": "single"|"unique"|"graduated"|"proportional", "field"?, "classes"?: 1..12, "method"?: "naturalBreaks"|"equalInterval"|"quantile", "ramp"?: string}"#),
        C::new("layer.ramp", "Color Scheme", "Style › Drawing", |s, p| {
            let r = Ramp::from_name(s_req(p, "ramp")?).ok_or("unknown colour scheme")?;
            with(s, p, |l| match &mut l.raster {
                Some(rl) => rl.style.set_ramp(r),
                None => {
                    let g = l.geometry_type();
                    l.renderer.apply_ramp(r, g)
                }
            })
        })
        .when(need_layer),
        C::new("layer.color", "Symbol Color", "Style › Drawing", |s, p| {
            let c = Rgba::from_hex(s_req(p, "color")?).ok_or("color must be #RRGGBB or #RRGGBBAA")?;
            with(s, p, |l| {
                let g = l.geometry_type();
                match &mut l.renderer {
                    Renderer::Single(sym) => *sym = sym.with_fill(c, g),
                    Renderer::Proportional { base, .. } => *base = base.with_fill(c, g),
                    other => *other = Renderer::Single(other.base_symbol().with_fill(c, g)),
                }
            })
        })
        .when(need_feature_layer)
        .params(r##"{"layer"?, "color": "#RRGGBB"}"##),
        C::new("layer.outline", "Outline", "Style › Drawing", |s, p| {
            let c = s.get_color(p, "color")?;
            let w = f(p, "width").map(|w| w.clamp(0.0, 20.0) as f32);
            with(s, p, |l| {
                l.renderer.map_symbols(|sym| {
                    if let Some(c) = c {
                        sym.stroke = c;
                    }
                    if let Some(w) = w {
                        sym.stroke_width = w;
                    }
                })
            })
        })
        .when(need_feature_layer)
        .params(r##"{"layer"?, "color"?: "#RRGGBB", "width"?: number}"##),
        C::new("layer.marker", "Marker", "Style › Drawing", |s, p| {
            let shape = ps(p, "shape").map(|n| Marker::ALL.into_iter().find(|m| m.label().eq_ignore_ascii_case(n)).ok_or("unknown marker shape")).transpose()?;
            let size = f(p, "size").map(|v| v.clamp(1.0, 80.0) as f32);
            with(s, p, |l| {
                l.renderer.map_symbols(|sym| {
                    if let Some(m) = shape {
                        sym.marker = m;
                    }
                    if let Some(sz) = size {
                        sym.size = sz;
                    }
                })
            })
        })
        .when(need_feature_layer)
        .params(r#"{"layer"?, "shape"?: "circle"|"square"|"triangle"|"diamond"|"star", "size"?: number}"#),
        C::new("layer.transparency", "Transparency", "Style › Effects", |s, p| {
            let t = f(p, "value").ok_or("value (0–100) is required")?.clamp(0.0, 100.0) as f32;
            with(s, p, |l| l.transparency = t)
        })
        .when(need_layer)
        .params(r#"{"layer"?, "value": 0..100}"#),
        C::new("layer.scaleRange", "Visibility Range", "Style › Visibility Range", |s, p| {
            let min = f(p, "min").filter(|v| *v > 0.0);
            let max = f(p, "max").filter(|v| *v > 0.0);
            with(s, p, |l| {
                l.min_scale = min;
                l.max_scale = max;
            })
        })
        .when(need_layer)
        .params(r#"{"layer"?, "min"?: scale (out beyond), "max"?: scale (in beyond)}"#),
        C::new("layer.selectable", "Selectable", "Layers", |s, p| {
            let on = b(p, "on");
            with(s, p, |l| l.selectable = on.unwrap_or(!l.selectable))
        })
        .when(need_feature_layer),
        C::new("layer.expand", "Expand Legend", "Layers", |s, p| {
            let on = b(p, "on");
            with(s, p, |l| l.expanded = on.unwrap_or(!l.expanded))
        })
        .view()
        .when(need_layer),
        C::new("labels.enable", "Label", "Style › Labels", |s, p| {
            let on = b(p, "on");
            with(s, p, |l| l.labels.enabled = on.unwrap_or(!l.labels.enabled))
        })
        .when(need_feature_layer)
        .params(r#"{"layer"?, "on"?: bool}"#),
        C::new("labels.field", "Label Field", "Style › Label Class", |s, p| {
            let name = s_req(p, "field")?.to_string();
            let id = s.layer_id(p)?;
            let fname = s.map().layer(id).and_then(|l| l.dataset.field_index(&name).and_then(|i| l.dataset.fields.get(i)).map(|f| f.name.clone()));
            let fname = fname.ok_or_else(|| format!("no field named '{name}'"))?;
            with(s, p, |l| {
                l.labels.field = fname.clone();
                l.labels.enabled = true;
            })
        })
        .when(need_feature_layer),
        C::new("labels.style", "Text Symbol", "Style › Text Symbol", |s, p| {
            let size = f(p, "size").map(|v| v.clamp(4.0, 72.0) as f32);
            let color = s.get_color(p, "color")?;
            let (halo, bold) = (b(p, "halo"), b(p, "bold"));
            with(s, p, |l| {
                if let Some(v) = size {
                    l.labels.size = v;
                }
                if let Some(c) = color {
                    l.labels.color = c;
                }
                if let Some(h) = halo {
                    l.labels.halo = h;
                }
                if let Some(bb) = bold {
                    l.labels.bold = bb;
                }
            })
        })
        .when(need_feature_layer)
        .params(r##"{"layer"?, "size"?: number, "color"?: "#RRGGBB", "halo"?: bool, "bold"?: bool}"##),
        C::new("layer.definitionQuery", "Definition Query", "Data › Definition Query", |s, p| {
            let q = ps(p, "where").map(str::trim).unwrap_or("").to_string();
            let id = s.layer_id(p)?;
            if !q.is_empty()
                && let Some(l) = s.map().layer(id)
            {
                rustgis_data::query::parse(&q, &l.dataset)?;
            }
            with(s, p, |l| l.definition_query = if q.is_empty() { None } else { Some(q.clone()) })
        })
        .when(need_feature_layer)
        .params(r#"{"layer"?, "where": "SQL clause, empty to clear"}"#),
        C::new("table.open", "Attribute Table", "Data › Table", |s, p| {
            s.table_layer = Some(s.layer_id(p)?);
            Ok(Value::Null)
        })
        .view()
        .key("Ctrl+T")
        .when(need_feature_layer),
        C::new("table.close", "Close Table", "Data › Table", |s, _| {
            s.table_layer = None;
            Ok(Value::Null)
        })
        .view(),
        C::new("layer.fields", "Fields", "Data › Data Design", |s, p| {
            let id = s.layer_id(p)?;
            let l = s.map().layer(id).ok_or("no layer")?;
            Ok(json!(l.dataset.fields.iter().map(|f| json!({"name": f.name, "type": f.ty.label(), "alias": f.alias})).collect::<Vec<_>>()))
        })
        .view()
        .when(need_feature_layer),
    ]
}

impl Session {
    fn get_color(&self, p: &Value, k: &str) -> Result<Option<Rgba>, String> {
        match ps(p, k) {
            Some(c) => Rgba::from_hex(c).map(Some).ok_or_else(|| format!("{k} must be #RRGGBB or #RRGGBBAA")),
            None => Ok(None),
        }
    }
}

fn s_req<'a>(p: &'a Value, k: &str) -> Result<&'a str, String> {
    ps(p, k).ok_or_else(|| format!("{k} is required"))
}

/// Applies `f` to the layer named by `p` (default: active layer).
pub(crate) fn with(s: &mut Session, p: &Value, f: impl FnOnce(&mut rustgis_map::Layer)) -> CmdResult {
    let id = s.layer_id(p)?;
    let l = s.map_mut().layer_mut(id).ok_or("no such layer")?;
    f(l);
    Ok(Value::Null)
}

/// Adds a dataset to the active map, selects it and returns its id.
pub fn add_dataset(s: &mut Session, mut ds: rustgis_data::Dataset) -> u32 {
    // Unique layer names keep name-based parameters unambiguous.
    let base = ds.name.clone();
    let mut i = 1;
    while s.map().layer_by_name(&ds.name).is_some() {
        i += 1;
        ds.name = format!("{base} ({i})");
    }
    let id = s.map_mut().add_dataset(ds);
    s.active_layer = Some(id);
    id
}

fn add_data(s: &mut Session, p: &Value) -> CmdResult {
    let path = s_req(p, "path")?;
    if rustgis_data::raster::is_raster_name(path) {
        return super::raster::add_path(s, path, ps(p, "name").map(str::trim).filter(|n| !n.is_empty()));
    }
    if path.to_ascii_lowercase().ends_with(".rgislyr") {
        return add_layer_file(s, path);
    }
    if rustgis_data::source::is_source_name(path) {
        return add_source(s, p, Origin::Path(path));
    }
    // A PMTiles archive is a tile layer (raster or vector, as its header says).
    if path.to_ascii_lowercase().ends_with(".pmtiles") {
        if cfg!(target_arch = "wasm32") {
            return Err(format!("{path}: the web app reads PMTiles archives from web addresses (Home › Add › Basemap › Add Tile Layer)"));
        }
        let full = std::path::absolute(path).map(|p| p.display().to_string()).map_err(|e| format!("{path}: {e}"))?;
        let mut q = json!({"url": full, "kind": "pmtiles"});
        if let Some(n) = ps(p, "name") {
            q["name"] = json!(n);
        }
        return super::tiles::add(s, &q);
    }
    // So is an MBTiles archive, read in place a tile at a time.
    if rustgis_data::mbtiles::is_mbtiles_name(path) {
        if cfg!(target_arch = "wasm32") {
            return Err(format!("{path}: in the web app, add the MBTiles file from this computer (Home › Add Data)"));
        }
        let full = std::path::absolute(path).map(|p| p.display().to_string()).map_err(|e| format!("{path}: {e}"))?;
        // Read now, so a file that isn't an archive is said at once rather than when it draws.
        crate::render::rustgis_tiles::mbtiles::info(&full)?;
        let mut q = json!({"url": full, "kind": "mbtiles"});
        if let Some(n) = ps(p, "name") {
            q["name"] = json!(n);
        }
        return super::tiles::add(s, &q);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if rustgis_data::source::gdal::is_gdal_name(path) {
        return add_gdal(s, p, path);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if rustgis_data::source::duckdb::is_duckdb_name(path) {
        return add_duckdb(s, p, path);
    }
    // A spreadsheet: one sheet ("layer"), read like delimited text.
    if rustgis_data::sheets::is_sheet_name(path) {
        let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
        if meta.len() > super::raster::MAX_UPLOAD_BYTES as u64 {
            return Err(format!("{path} is too large to open"));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        return add_sheet(s, p, path, &bytes);
    }
    if rustgis_data::io::is_multi_layer_name(path) {
        let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
        if meta.len() > super::raster::MAX_UPLOAD_BYTES as u64 {
            return Err(format!("{path} is too large to open"));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let full = std::path::absolute(path).map(|p| p.display().to_string()).unwrap_or_else(|_| path.to_string());
        return add_layers(s, p, &full, &bytes);
    }
    let delimited = read_delimited(p, path, || {
        let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
        // The limit the text readers keep without options too.
        if meta.len() > rustgis_data::io::MAX_BYTES {
            return Err(format!("{path} is too large to open"));
        }
        std::fs::read(path).map_err(|e| format!("{path}: {e}"))
    })?;
    let read = match delimited {
        Some(read) => read,
        None => rustgis_data::io::read_path(Path::new(path)).map_err(|e| format!("{path}: {e}"))?,
    };
    add_read(s, p, read)
}

/// Adds every layer of a file that holds several: the Shapefiles (and other files) of a zip, a KML
/// or KMZ's points, lines and polygons drawn in its styles, a GPX's waypoints, routes, tracks and
/// track points. `"layer"` picks one by name; a name given applies to the first.
pub(crate) fn add_layers(s: &mut Session, p: &Value, file_name: &str, bytes: &[u8]) -> CmdResult {
    // A LandXML file in a coordinate system it doesn't say.
    let crs = ps(p, "crs").map(str::trim).filter(|c| !c.is_empty());
    let landxml = rustgis_data::landxml::is_landxml_name(file_name)
        || (file_name.to_ascii_lowercase().ends_with(".xml")
            && rustgis_data::landxml::looks_like_landxml(&String::from_utf8_lossy(bytes.get(..bytes.len().min(4096)).unwrap_or(bytes))));
    let mut reads = match crs.filter(|_| landxml) {
        Some(c) => {
            let crs = rustgis_data::raster::Crs::parse(c)?;
            rustgis_data::landxml::read(file_name, bytes, Some(crs))
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|(dataset, report)| rustgis_data::io::LayerRead { dataset, report, styles: Vec::new() })
                .collect()
        }
        None => rustgis_data::io::read_layers(file_name, bytes).map_err(|e| e.to_string())?,
    };
    if let Some(want) = ps(p, "layer").map(str::trim).filter(|t| !t.is_empty()) {
        let names: Vec<String> = reads.iter().map(|r| r.dataset.name.clone()).collect();
        // By its full name or its last word ("tracks" for "walk tracks").
        let pick = |n: &str| n.eq_ignore_ascii_case(want) || n.rsplit(' ').next().is_some_and(|w| w.eq_ignore_ascii_case(want));
        reads.retain(|r| pick(&r.dataset.name));
        if reads.is_empty() {
            return Err(format!("{file_name}: no layer '{want}' (layers: {})", names.join(", ")));
        }
    }
    let mut added = Vec::new();
    let mut failed = Vec::new();
    let mut q = p.clone();
    for read in reads {
        let name = read.dataset.name.clone();
        let styles = read.styles;
        // A layer that can't be added is skipped and said; the others are still added. A name
        // given goes to the first layer actually added.
        match add_read(s, &q, (read.dataset, read.report)) {
            Ok(r) => {
                if let Some(id) = r.get("layer").and_then(Value::as_u64).and_then(|i| u32::try_from(i).ok()) {
                    apply_styles(s, id, &styles);
                }
                added.push(r);
                if let Some(o) = q.as_object_mut() {
                    o.remove("name");
                }
            }
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    if added.is_empty() {
        return Err(failed.join("; "));
    }
    for f in &failed {
        s.notify(format!("{file_name}: {f}"), true);
    }
    let layers: Vec<Value> = added.iter().filter_map(|r| r.get("layer").cloned()).collect();
    let tables: Vec<Value> = added.iter().filter_map(|r| r.get("table").cloned()).collect();
    let features: u64 = added.iter().filter_map(|r| r.get("features").and_then(Value::as_u64)).sum();
    let mut out = json!({"layers": layers, "tables": tables, "features": features});
    // The first layer, as the result of adding a single file says it.
    if let Some(first) = layers.first() {
        out["layer"] = first.clone();
    }
    Ok(out)
}

/// Draws a layer in the styles its file declares (KML): unique values of its `style` field, each
/// style's colours and widths, the layer's default symbol for anything a style leaves unset.
fn apply_styles(s: &mut Session, id: u32, styles: &[(String, rustgis_data::kml::Style)]) {
    use rustgis_map::Symbol;
    // A legend of hundreds of styles helps no one; such files keep the default symbol.
    if styles.is_empty() || styles.len() > 64 {
        return;
    }
    let Some(layer) = s.map_mut().layer_mut(id) else { return };
    let g = layer.dataset.geometry_type;
    let base = match &layer.renderer {
        Renderer::Single(sym) => *sym,
        _ => Symbol::default_for(g, id),
    };
    let rgba = |c: [u8; 4]| Rgba::new(c[0], c[1], c[2], c[3]);
    let classes = styles
        .iter()
        .map(|(name, st)| {
            let mut sym = base;
            match g {
                GeometryType::Point => {
                    if let Some(c) = st.icon {
                        sym.fill = rgba(c);
                    }
                    if let Some(k) = st.scale {
                        sym.size = (base.size * k).clamp(2.0, 40.0);
                    }
                }
                GeometryType::Polyline => {
                    if let Some(c) = st.stroke {
                        sym.stroke = rgba(c);
                        sym.fill = rgba(c);
                    }
                    if let Some(w) = st.width {
                        sym.stroke_width = w.clamp(0.3, 20.0);
                    }
                }
                _ => {
                    if let Some(c) = st.fill {
                        sym.fill = rgba(c);
                    }
                    if !st.filled {
                        sym.fill = Rgba::TRANSPARENT;
                    }
                    if let Some(c) = st.stroke {
                        sym.stroke = rgba(c);
                    }
                    if let Some(w) = st.width {
                        sym.stroke_width = w.clamp(0.0, 20.0);
                    }
                    if !st.outlined {
                        sym.stroke = Rgba::TRANSPARENT;
                    }
                }
            }
            (name.clone(), sym)
        })
        .collect();
    layer.renderer = Renderer::Unique { field: "style".into(), classes, other: base, ramp: Default::default() };
}

/// The layers a file holds, without adding them: a GeoPackage's feature tables, a GDAL dataset's
/// layers, or the one layer of other files. `layer.addData` takes a name with `"layer"`.
fn file_layers(_s: &mut Session, p: &Value) -> CmdResult {
    let path = s_req(p, "path")?;
    let err = |e: rustgis_data::DataError| format!("{path}: {e}");
    #[cfg(not(target_arch = "wasm32"))]
    if rustgis_data::source::gdal::is_gdal_name(path) {
        let layers = rustgis_data::source::gdal::layers(Path::new(path)).map_err(err)?;
        return Ok(json!({"count": layers.len(), "layers": layers}));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if rustgis_data::source::duckdb::is_duckdb_name(path) {
        let tables = rustgis_data::source::duckdb::tables(Path::new(path)).map_err(err)?;
        let layers: Vec<String> = tables.iter().map(|t| t.label()).collect();
        return Ok(json!({"count": layers.len(), "layers": layers}));
    }
    if rustgis_data::source::is_source_name(path) {
        let layers = rustgis_data::source::layers(Path::new(path)).map_err(err)?;
        return Ok(json!({"count": layers.len(), "layers": layers}));
    }
    if !Path::new(path).is_file() {
        return Err(format!("{path}: no such file"));
    }
    if rustgis_data::sheets::is_sheet_name(path) {
        let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
        if meta.len() > super::raster::MAX_UPLOAD_BYTES as u64 {
            return Err(format!("{path} is too large to open"));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let layers = rustgis_data::sheets::sheet_names(&bytes).map_err(err)?;
        return Ok(json!({"count": layers.len(), "layers": layers}));
    }
    if rustgis_data::io::is_multi_layer_name(path) {
        let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
        if meta.len() > super::raster::MAX_UPLOAD_BYTES as u64 {
            return Err(format!("{path} is too large to open"));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let layers: Vec<String> = rustgis_data::io::read_layers(path, &bytes).map_err(err)?.into_iter().map(|l| l.dataset.name).collect();
        return Ok(json!({"count": layers.len(), "layers": layers}));
    }
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !rustgis_data::io::READ_EXTENSIONS.contains(&ext.as_str()) && !rustgis_data::raster::is_raster_name(path) {
        return Err(format!("{path}: not a data file RustGIS reads"));
    }
    let name = Path::new(path).file_stem().and_then(|n| n.to_str()).unwrap_or("layer");
    Ok(json!({"count": 1, "layers": [name]}))
}

/// Where a source file is: on disk, or uploaded (its name and bytes).
pub(crate) enum Origin<'a> {
    Path(&'a str),
    Bytes(&'a str, std::sync::Arc<Vec<u8>>),
}

/// Most GeoPackage tables added at once.
const MAX_TABLES: usize = 32;

/// Most skipped tables named in `layer.addData`'s result (`skippedCount` counts them all, and
/// `info.fileLayers` lists them).
const MAX_SKIPPED_NAMED: usize = 100;

/// Adds a GeoPackage, GeoParquet or FlatGeobuf file: each feature table becomes a layer. Large
/// sources (or any, with `"stream": true`) are streamed, read in place a view at a time; smaller
/// ones are loaded whole, so they can be edited.
pub(crate) fn add_source(s: &mut Session, p: &Value, origin: Origin<'_>) -> CmdResult {
    use rustgis_data::source;
    let label = match &origin {
        Origin::Path(p) | Origin::Bytes(p, _) => p.to_string(),
    };
    let err = |e: rustgis_data::DataError| format!("{label}: {e}");
    // Tables past the cap, said in the result as well as in a notice, so the CLI, the MCP server
    // and the AI Assistant know the file holds more than was added.
    let mut skipped: Vec<String> = Vec::new();
    let mut total = None;
    let tables: Vec<Option<String>> = match (ps(p, "layer").map(str::trim).filter(|t| !t.is_empty()), &origin) {
        (Some(t), _) => vec![Some(t.to_string())],
        (None, Origin::Path(path)) if path.to_ascii_lowercase().ends_with(".gpkg") => {
            let mut all = source::layers(Path::new(path)).map_err(err)?;
            if all.len() > MAX_TABLES {
                total = Some(all.len());
                skipped = all.split_off(MAX_TABLES);
            }
            all.into_iter().map(Some).collect()
        }
        // In the browser an uploaded GeoPackage adds every table, listed by the pure-Rust reader.
        // (The desktop opens uploads with SQLite, one temporary copy per table, so it adds the
        // first; files on disk add them all.)
        #[cfg(target_arch = "wasm32")]
        (None, Origin::Bytes(name, bytes)) if name.to_ascii_lowercase().ends_with(".gpkg") => {
            let mut all = source::gpkg_mem::layers(source::Shared(bytes.clone())).map_err(err)?;
            if all.len() > MAX_TABLES {
                total = Some(all.len());
                skipped = all.split_off(MAX_TABLES);
            }
            all.into_iter().map(Some).collect()
        }
        _ => vec![None],
    };
    if tables.is_empty() {
        return Err(format!("{label}: no feature tables"));
    }
    let empty_before = s.map().layers.is_empty();
    let mut ids = Vec::new();
    let mut streamed = false;
    let mut features = 0u64;
    for table in tables {
        let src = match &origin {
            Origin::Path(path) => source::open_path(Path::new(path), table.as_deref()),
            Origin::Bytes(name, bytes) => source::open_shared(name, bytes.clone(), table.as_deref()),
        }
        .map_err(err)?;
        let info = src.info().clone();
        let stream = b(p, "stream").unwrap_or(info.count.is_none_or(|c| c > rustgis_map::source::STREAM_ABOVE));
        // A name given applies to a single table.
        let name = match ps(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) if ids.is_empty() => n.to_string(),
            _ => info.name.clone(),
        };
        if stream {
            let layer = match &origin {
                Origin::Path(path) => {
                    let full = std::path::absolute(path).map(|p| p.display().to_string()).unwrap_or_else(|_| path.to_string());
                    rustgis_map::source::SourceLayer::from_path(full, table.clone(), src)
                }
                Origin::Bytes(file, bytes) => rustgis_map::source::SourceLayer::from_bytes(*file, bytes.clone(), table.clone(), src),
            };
            let unique = unique_name(s, &name);
            for n in &info.notes {
                s.notify(format!("{unique}: {n}"), false);
            }
            let id = s.map_mut().add_streamed(unique.clone(), layer);
            s.active_layer = Some(id);
            ids.push(id);
            streamed = true;
            features += info.count.unwrap_or(0);
            s.notify(
                format!(
                    "{unique}: {} features, read in place a view at a time (Load All Features loads them into memory)",
                    info.count.map(|c| c.to_string()).unwrap_or_else(|| "many".into())
                ),
                false,
            );
        } else {
            let (mut ds, report) = source::materialize(src.as_ref()).map_err(err)?;
            ds.name = name;
            ds.source = Some(label.clone());
            features += ds.features.len() as u64;
            let id = add_dataset(s, ds);
            ids.push(id);
            for m in report.messages {
                s.notify(m, false);
            }
        }
    }
    if empty_before {
        let e = ids.iter().filter_map(|id| s.map().layer(*id)).fold(rustgis_geom::Extent::EMPTY, |e, l| e.union(&l.extent()));
        if !e.is_empty() {
            s.zoom_to(&e);
        }
    }
    let first = ids.first().copied();
    let mut out = json!({"layer": first, "layers": ids, "features": features, "streamed": streamed});
    // Said once the tables are in: a failure above is the error instead.
    if let Some(total) = total {
        s.notify(
            format!(
                "{label}: added the first {MAX_TABLES} of {total} tables; add others by name with \"layer\" (info.fileLayers lists them)"
            ),
            false,
        );
        out["tables"] = json!(total);
        out["skippedCount"] = json!(skipped.len());
        // Named up to a bound, so a file of thousands of tables can't flood an agent's context.
        out["skipped"] = json!(skipped.iter().take(MAX_SKIPPED_NAMED).collect::<Vec<_>>());
    }
    Ok(out)
}

/// Adds a file GDAL reads (KML, GPX, File Geodatabase, DXF…): each layer is converted once into an
/// indexed FlatGeobuf in the cache, then added like one (streamed when large). A layer mixing
/// geometry types becomes a layer per type.
#[cfg(not(target_arch = "wasm32"))]
fn add_gdal(s: &mut Session, p: &Value, path: &str) -> CmdResult {
    let crs = ps(p, "crs").map(str::trim).filter(|c| !c.is_empty());
    let (converted, failed) =
        rustgis_data::source::gdal::convert(Path::new(path), ps(p, "layer"), crs).map_err(|e| format!("{path}: {e}"))?;
    // Layers GDAL couldn't convert are said, so the file isn't taken for whole.
    for f in failed {
        s.notify(format!("{path}: {f}"), true);
    }
    let original = std::path::absolute(path).map(|p| p.display().to_string()).unwrap_or_else(|_| path.to_string());
    let parts = converted
        .into_iter()
        .map(|c| Converted {
            path: c.path,
            name: c.name,
            unprojected: c.unprojected,
            origin: ConvertedOrigin::Gdal(rustgis_map::source::GdalOrigin {
                path: original.clone(),
                layer: c.layer,
                family: c.family,
                crs: crs.map(str::to_string),
            }),
        })
        .collect();
    add_converted(s, p, path, &original, parts)
}

/// Adds the tables of a DuckDB database that have a geometry column (or the one `layer` names),
/// each converted once into GeoParquet in the cache.
#[cfg(not(target_arch = "wasm32"))]
fn add_duckdb(s: &mut Session, p: &Value, path: &str) -> CmdResult {
    use rustgis_data::source::duckdb;
    let crs = ps(p, "crs").map(str::trim).filter(|c| !c.is_empty());
    let db = Path::new(path);
    let err = |e: rustgis_data::DataError| format!("{path}: {e}");
    let tables = match ps(p, "layer").map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => vec![duckdb::find_table(db, l).map_err(err)?],
        None => duckdb::tables(db).map_err(err)?,
    };
    if tables.is_empty() {
        return Err(format!("{path}: no tables with a geometry column"));
    }
    // Tables past the cap, said in the result as well as in a notice (as for a GeoPackage).
    let total = tables.len();
    let mut tables = tables;
    let skipped: Vec<String> =
        if total > duckdb::MAX_TABLES { tables.split_off(duckdb::MAX_TABLES).iter().map(|t| t.label()).collect() } else { Vec::new() };
    let original = std::path::absolute(path).map(|p| p.display().to_string()).unwrap_or_else(|_| path.to_string());
    let mut parts = Vec::new();
    let single = tables.len() == 1;
    for t in tables {
        let out = match duckdb::convert_table(db, &t, crs) {
            Ok(o) => o,
            // A table that fails doesn't stop the others.
            Err(e) if !single => {
                s.notify(format!("{path}: {}: {e}", t.label()), true);
                continue;
            }
            Err(e) => return Err(err(e)),
        };
        let origin = |family| rustgis_map::source::DuckdbOrigin {
            sql: None,
            database: Some(original.clone()),
            table: Some(t.label()),
            crs: crs.map(str::to_string),
            family,
        };
        parts.extend(duckdb_parts(s, &t.label(), out, origin));
    }
    let mut out = add_converted(s, p, path, &original, parts)?;
    // Said once the tables are in: a failure above is the error instead.
    if !skipped.is_empty() {
        s.notify(
            format!(
                "{path}: added the first {} of {total} tables; add others by name with \"layer\" (info.fileLayers lists them)",
                duckdb::MAX_TABLES
            ),
            false,
        );
        out["tables"] = json!(total);
        out["skippedCount"] = json!(skipped.len());
        out["skipped"] = json!(skipped.iter().take(MAX_SKIPPED_NAMED).collect::<Vec<_>>());
    }
    Ok(out)
}

/// Runs a DuckDB query and adds its result as a layer (a layer per geometry type it mixes).
fn add_sql(s: &mut Session, p: &Value) -> CmdResult {
    let sql = s_req(p, "sql")?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, sql);
        Err("SQL queries run through DuckDB in the desktop app".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rustgis_data::source::duckdb;
        let crs = ps(p, "crs").map(str::trim).filter(|c| !c.is_empty());
        let database = ps(p, "database").map(str::trim).filter(|d| !d.is_empty());
        let full = database.map(|d| std::path::absolute(d).map(|p| p.display().to_string()).unwrap_or_else(|_| d.to_string()));
        let q = duckdb::Query { sql, database: database.map(Path::new), crs };
        let out = duckdb::run(&q).map_err(|e| e.to_string())?;
        let name = ps(p, "name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Query");
        let origin = |family| rustgis_map::source::DuckdbOrigin {
            sql: Some(sql.to_string()),
            database: full.clone(),
            table: None,
            crs: crs.map(str::to_string),
            family,
        };
        let parts = duckdb_parts(s, name, out, origin);
        // The name given names the layer, or the layers by geometry type.
        let mut q = p.clone();
        if let Some(o) = q.as_object_mut() {
            o.remove("name");
        }
        add_converted(s, &q, "the query", full.as_deref().unwrap_or("DuckDB query"), parts)
    }
}

/// The layers of a DuckDB result: named `name`, or `name (points)` and so on when it mixes
/// geometry types.
#[cfg(not(target_arch = "wasm32"))]
fn duckdb_parts(
    s: &mut Session,
    name: &str,
    out: rustgis_data::source::duckdb::Output,
    origin: impl Fn(rustgis_data::source::Family) -> rustgis_map::source::DuckdbOrigin,
) -> Vec<Converted> {
    if out.skipped > 0 {
        s.notify(format!("{name}: {} rows without a geometry were left out", out.skipped), false);
    }
    let mixed = out.parts.len() > 1;
    out.parts
        .into_iter()
        .map(|part| Converted {
            path: part.path,
            name: if mixed { format!("{name} ({})", part.family.label()) } else { name.to_string() },
            // A geometry that says no coordinate system is usually longitude/latitude: only
            // coordinates past it are pointed out (in add_converted).
            unprojected: false,
            origin: ConvertedOrigin::Duckdb(origin(part.family)),
        })
        .collect()
}

/// A layer another program (GDAL, DuckDB) converted into the cache.
#[cfg(not(target_arch = "wasm32"))]
struct Converted {
    path: std::path::PathBuf,
    name: String,
    /// Its coordinates are the source's own, in a coordinate system it doesn't say.
    unprojected: bool,
    origin: ConvertedOrigin,
}

#[cfg(not(target_arch = "wasm32"))]
enum ConvertedOrigin {
    Gdal(rustgis_map::source::GdalOrigin),
    Duckdb(rustgis_map::source::DuckdbOrigin),
}

/// Adds converted layers like the files they are (streamed when large), remembering where they came
/// from so a project can convert them again. `label` names the source in messages, `original` is
/// the layers' source.
#[cfg(not(target_arch = "wasm32"))]
fn add_converted(s: &mut Session, p: &Value, label: &str, original: &str, converted: Vec<Converted>) -> CmdResult {
    let empty_before = s.map().layers.is_empty();
    let single = converted.len() == 1;
    let (mut ids, mut features, mut streamed) = (Vec::new(), 0u64, false);
    for c in converted {
        let mut q = json!({"name": c.name});
        if let Some(n) = ps(p, "name").filter(|_| single) {
            q["name"] = json!(n);
        }
        if let Some(v) = p.get("stream") {
            q["stream"] = v.clone();
        }
        let file = c.path.display().to_string();
        // One layer that can't be added doesn't stop the others.
        let r = match add_source(s, &q, Origin::Path(&file)) {
            Ok(r) => r,
            Err(e) if !single => {
                s.notify(format!("{}: {e}", c.name), true);
                continue;
            }
            Err(e) => return Err(e),
        };
        features += r["features"].as_u64().unwrap_or(0);
        streamed |= r["streamed"].as_bool().unwrap_or(false);
        for id in r["layers"].as_array().into_iter().flatten().filter_map(Value::as_u64).filter_map(|i| u32::try_from(i).ok()) {
            if let Some(l) = s.map_mut().layer_mut(id) {
                l.dataset.source = Some(original.to_string());
                if let Some(src) = &mut l.source {
                    match &c.origin {
                        ConvertedOrigin::Gdal(g) => src.gdal = Some(g.clone()),
                        ConvertedOrigin::Duckdb(d) => src.duckdb = Some(d.clone()),
                    }
                }
            }
            // The source couldn't be reprojected, or its coordinates go past longitude/latitude:
            // its coordinate system is unknown.
            let e = s.map().layer(id).map(|l| l.extent()).unwrap_or_default();
            let outside = !e.is_empty() && (e.min_x < -181.0 || e.max_x > 181.0 || e.min_y < -91.0 || e.max_y > 91.0);
            if outside || (c.unprojected && matches!(c.origin, ConvertedOrigin::Gdal(_))) {
                s.notify(
                    format!("{}: the data doesn't say its coordinate system; add it again with \"crs\" (such as EPSG:2263)", c.name),
                    true,
                );
            }
            ids.push(id);
        }
    }
    if empty_before {
        let e = ids.iter().filter_map(|id| s.map().layer(*id)).fold(rustgis_geom::Extent::EMPTY, |e, l| e.union(&l.extent()));
        if !e.is_empty() {
            s.zoom_to(&e);
        }
    }
    if ids.is_empty() {
        return Err(format!("{label}: none of its layers could be added"));
    }
    Ok(json!({"layer": ids.first(), "layers": ids, "features": features, "streamed": streamed}))
}

fn unique_name(s: &Session, base: &str) -> String {
    let mut name = base.to_string();
    let mut i = 1;
    while s.map().layer_by_name(&name).is_some() {
        i += 1;
        name = format!("{base} ({i})");
    }
    name
}

/// The active (or named) layer is streamed.
fn need_streamed(s: &Session) -> Option<&'static str> {
    match s.active_layer.and_then(|id| s.map().layer(id)) {
        Some(l) if l.is_streamed() => None,
        Some(_) => Some("this layer's features are already in memory"),
        None => Some("select a layer in the Layers pane first"),
    }
}

/// Loads a streamed layer's features into memory, so it can be edited like any other layer.
fn load_all(s: &mut Session, p: &Value) -> CmdResult {
    let id = s.layer_id(p)?;
    let l = s.map().layer(id).ok_or("no such layer")?;
    let ds = l.load_all()?;
    let n = ds.features.len();
    let l = s.map_mut().layer_mut(id).ok_or("no such layer")?;
    l.dataset = ds;
    l.dataset.windowed = false;
    l.source = None;
    l.selection.clear();
    l.touch_geometry();
    Ok(json!({"layer": id, "features": n}))
}

/// Adds a layer file: a layer with its data, symbology and labels.
fn add_layer_file(s: &mut Session, path: &str) -> CmdResult {
    let len = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?.len();
    if len > 512 << 20 {
        return Err("the layer file is too large".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    add_layer_text(s, &text)
}

/// Adds a layer from a layer file's JSON.
pub(crate) fn add_layer_text(s: &mut Session, text: &str) -> CmdResult {
    let mut layer: rustgis_map::Layer = serde_json::from_str(text).map_err(|e| format!("not a RustGIS layer file: {e}"))?;
    // A fresh id and a unique name in this map; the rest goes through project repair.
    let base = layer.name.clone();
    let mut i = 1;
    while s.map().layer_by_name(&layer.name).is_some() {
        i += 1;
        layer.name = format!("{base} ({i})");
    }
    let id = s.map_mut().add_layer(layer);
    s.project.normalize();
    s.active_layer = Some(id);
    Ok(json!({"layer": id}))
}

/// The most polylines added at once.
const MAX_POLYLINES: usize = 10_000;

/// Adds encoded polylines (routes from Google, OSRM or Valhalla) as a line layer.
fn add_encoded_polyline(s: &mut Session, p: &Value) -> CmdResult {
    use rustgis_data::{Dataset, Field, FieldType, Geometry, Value as V};
    let texts: Vec<&str> = match p.get("polyline") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).take(MAX_POLYLINES + 1).collect(),
        _ => return Err("polyline is required (text, or a list of texts)".into()),
    };
    if texts.len() > MAX_POLYLINES {
        return Err(format!("at most {MAX_POLYLINES} polylines at once"));
    }
    let precision = match f(p, "precision") {
        Some(v) if v.fract() == 0.0 && (1.0..=10.0).contains(&v) => v as u32,
        Some(v) => return Err(format!("precision {v}: give a whole number from 1 to 10 (5 for Google and OSRM, 6 for Valhalla)")),
        None => 5,
    };
    let name = ps(p, "name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Polyline");
    let mut ds =
        Dataset::new(name, GeometryType::Polyline, vec![Field::new("index", FieldType::Integer), Field::new("points", FieldType::Integer)]);
    for (i, t) in texts.iter().enumerate() {
        let pts = rustgis_data::interop::decode_polyline(t, precision).map_err(|e| format!("polyline {}: {e}", i + 1))?;
        if pts.len() < 2 {
            return Err(format!("polyline {}: a line needs at least two points", i + 1));
        }
        let n = pts.len() as i64;
        ds.push_next(Some(Geometry::Line(vec![pts])), vec![V::Int(i as i64 + 1), V::Int(n)]);
    }
    add_read(s, p, (ds, rustgis_data::ReadReport::default()))
}

/// Adds a sheet of a spreadsheet (`"layer"` names it; the first by default), read like delimited
/// text with the same options.
pub(crate) fn add_sheet(s: &mut Session, p: &Value, file_name: &str, bytes: &[u8]) -> CmdResult {
    let stem = Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("sheet");
    let opts = csv_options(p)?.unwrap_or_default();
    let sheet = ps(p, "layer").map(str::trim).filter(|l| !l.is_empty());
    let read = rustgis_data::sheets::read(bytes, stem, sheet, &opts).map_err(|e| format!("{file_name}: {e}"))?;
    add_read(s, p, read)
}

/// The `x`, `y`, `wkt`, `crs` and `delimiter` parameters for a delimited text file, when any is
/// given.
pub(crate) fn csv_options(p: &Value) -> Result<Option<rustgis_data::io::CsvOptions>, String> {
    let text = |k: &str| ps(p, k).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let (x, y, wkt, crs, delimiter) = (text("x"), text("y"), text("wkt"), text("crs"), ps(p, "delimiter"));
    if x.is_none() && y.is_none() && wkt.is_none() && crs.is_none() && delimiter.is_none() {
        return Ok(None);
    }
    let crs = crs.map(|c| rustgis_data::raster::Crs::parse(&c)).transpose()?;
    let delimiter = match delimiter {
        None => None,
        Some("\\t" | "tab" | "\t") => Some(b'\t'),
        Some(d) if d.len() == 1 && d.is_ascii() && !d.starts_with(['"', '\n', '\r']) => d.bytes().next(),
        Some(d) => return Err(format!("delimiter '{d}': give one character, such as ; or \\t")),
    };
    Ok(Some(rustgis_data::io::CsvOptions { x, y, wkt, crs, delimiter }))
}

/// Whether a file name is delimited text (CSV, TSV, TXT).
fn is_delimited_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".csv") || n.ends_with(".tsv") || n.ends_with(".txt")
}

/// Reads delimited text with the options `p` gives, or `None` when it gives none (or the file
/// isn't delimited text).
fn read_delimited(
    p: &Value,
    name: &str,
    bytes: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<Option<(rustgis_data::Dataset, rustgis_data::ReadReport)>, String> {
    if !is_delimited_name(name) {
        return Ok(None);
    }
    let Some(opts) = csv_options(p)? else { return Ok(None) };
    let bytes = bytes()?;
    let text = std::str::from_utf8(&bytes).map_err(|_| format!("{name} is not UTF-8 text"))?;
    let stem = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or("layer");
    rustgis_data::io::read_csv_with(text, stem, &opts).map(Some).map_err(|e| format!("{name}: {e}"))
}

/// The file name for data downloaded from `url`: its last path segment when that has an extension
/// RustGIS reads, else `<segment or "data">.<ext>` with the extension its contents suggest
/// (an error when they suggest none).
pub fn url_file_name(url: &str, bytes: &[u8]) -> Result<String, String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let path = path.split_once("://").map_or(path, |(_, rest)| rest.split_once('/').map_or("", |(_, p)| p));
    let last: String = path
        .rsplit('/')
        .find(|p| !p.is_empty())
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '\\' | ':'))
        .take(120)
        .collect();
    let ext = Path::new(&last).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    let known = rustgis_data::io::READ_EXTENSIONS.contains(&ext.as_str()) || rustgis_data::raster::is_raster_name(&last) || ext == "tsv";
    if known && ext != "json" {
        return Ok(last);
    }
    let stem = Path::new(&last).file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).unwrap_or("data");
    match rustgis_data::io::sniff_extension(bytes) {
        Some(ext) => Ok(format!("{stem}.{ext}")),
        None => {
            Err("not a data format RustGIS reads (GeoJSON, CSV, GeoParquet, FlatGeobuf, GeoPackage, zipped Shapefiles, KML, GPX, GeoTIFF)"
                .into())
        }
    }
}

/// Downloads data from a web address and adds it (the command line, MCP and the assistant; the
/// app downloads off the frame and adds the bytes).
pub(crate) fn add_url(s: &mut Session, p: &Value) -> CmdResult {
    let url = ps(p, "url").map(str::trim).filter(|u| !u.is_empty()).ok_or("url is required")?;
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(format!("{url}: give an http:// or https:// address (Add Data opens files)"));
    }
    // A PMTiles archive is read a tile at a time, never downloaded whole.
    if lower.split(['?', '#']).next().is_some_and(|p| p.ends_with(".pmtiles")) {
        let mut q = json!({"url": url, "kind": "pmtiles"});
        if let Some(n) = ps(p, "name") {
            q["name"] = json!(n);
        }
        return super::tiles::add(s, &q);
    }
    #[cfg(target_arch = "wasm32")]
    {
        Err(format!("{url}: in the web app, use Home › Add Data › From URL"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Errors about the address itself name it already.
        let named = |e: String| if e.starts_with(url) { e } else { format!("{url}: {e}") };
        let bytes = crate::render::rustgis_tiles::cache::download_blocking(url).map_err(named)?;
        let name = url_file_name(url, &bytes).map_err(named)?;
        super::raster::add_named_bytes(s, p, name, bytes)
    }
}

/// Adds a download the app finished (its bytes handed to the session, not passed as text).
fn add_download(s: &mut Session, p: &Value) -> CmdResult {
    let id = f(p, "id").filter(|v| *v >= 0.0 && v.fract() == 0.0).map(|v| v as u64).ok_or("id is required")?;
    let url = ps(p, "url").unwrap_or("data").to_string();
    let i = s.downloaded.iter().position(|(d, _)| *d == id).ok_or("no such download (it was dropped when another project opened)")?;
    let (_, bytes) = s.downloaded.remove(i);
    let name = url_file_name(&url, &bytes).map_err(|e| format!("{url}: {e}"))?;
    super::raster::add_named_bytes(s, p, name, bytes)
}

pub(crate) fn add_data_text(s: &mut Session, p: &Value) -> CmdResult {
    let name = s_req(p, "fileName")?;
    let text = s_req(p, "text")?;
    if let Some(read) = read_delimited(p, name, || Ok(text.as_bytes().to_vec()))? {
        return add_read(s, p, read);
    }
    let read = rustgis_data::io::read_bytes(name, text.as_bytes()).map_err(|e| format!("{name}: {e}"))?;
    add_read(s, p, read)
}

pub(crate) fn add_read(s: &mut Session, p: &Value, read: (rustgis_data::Dataset, rustgis_data::ReadReport)) -> CmdResult {
    let (mut ds, report) = read;
    if let Some(n) = ps(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        ds.name = n.to_string();
    }
    if ds.geometry_type == GeometryType::None {
        let name = ds.name.clone();
        s.project.tables.push(ds);
        return Ok(json!({"table": name}));
    }
    let n = ds.features.len();
    let empty_before = s.map().layers.is_empty();
    let id = add_dataset(s, ds);
    for m in report.messages {
        s.notify(m, false);
    }
    if empty_before && let Some(e) = s.map().layer(id).map(|l| l.extent()) {
        s.zoom_to(&e);
    }
    Ok(json!({"layer": id, "features": n}))
}

fn add_sample(s: &mut Session, p: &Value) -> CmdResult {
    use crate::sample::*;
    let name = s_req(p, "name")?.to_ascii_lowercase();
    let (text, title) = match name.as_str() {
        "countries" => (countries(), "Countries"),
        "cities" => (CITIES, "Cities"),
        "rivers" => (RIVERS, "Rivers"),
        "lakes" => (LAKES, "Lakes"),
        _ => return Err(format!("no sample '{name}'")),
    };
    let (ds, _) = rustgis_data::io::read_geojson(text, title).map_err(|e| e.to_string())?;
    let id = add_dataset(s, ds);
    Ok(json!({"layer": id}))
}

fn remove(s: &mut Session, p: &Value) -> CmdResult {
    let id = s.layer_id(p)?;
    let m = s.map_mut();
    let i = m.layer_index(id).ok_or("no such layer")?;
    m.layers.remove(i);
    let next = m.layers.get(i.min(m.layers.len().saturating_sub(1))).map(|l| l.id);
    s.active_layer = next;
    Ok(Value::Null)
}

fn move_layer(s: &mut Session, p: &Value, d: i32) -> CmdResult {
    let id = s.layer_id(p)?;
    let m = s.map_mut();
    let i = m.layer_index(id).ok_or("no such layer")?;
    let j = i as i64 + d as i64;
    if j < 0 || j as usize >= m.layers.len() {
        return Err("the layer can't move further".into());
    }
    m.layers.swap(i, j as usize);
    Ok(Value::Null)
}

fn symbology(s: &mut Session, p: &Value) -> CmdResult {
    let id = s.layer_id(p)?;
    let kind = ps(p, "type").unwrap_or("single").to_ascii_lowercase();
    let ramp = ps(p, "ramp").and_then(Ramp::from_name);
    let method = ps(p, "method").and_then(ClassMethod::from_name).unwrap_or_default();
    let classes = f(p, "classes").map(|c| c.clamp(1.0, 12.0) as usize).unwrap_or(5);
    let l = s.map().layer(id).ok_or("no such layer")?;
    let g = l.geometry_type();
    let base = l.renderer.base_symbol();
    let base = if matches!(l.renderer, Renderer::Graduated { .. } | Renderer::Unique { .. }) {
        rustgis_map::Symbol { ..rustgis_map::Symbol::default_for(g, id) }.with_fill(base.fill, g)
    } else {
        base
    };
    let field = || -> Result<String, String> {
        if let Some(x) = ps(p, "field") {
            return Ok(x.to_string());
        }
        l.renderer
            .field()
            .map(str::to_string)
            .or_else(|| l.dataset.fields.iter().find(|f| f.ty.is_numeric()).map(|f| f.name.clone()))
            .ok_or_else(|| "field is required".into())
    };
    let r = match kind.as_str() {
        "single" | "singlesymbol" => Renderer::Single(base),
        "unique" | "uniquevalues" => Renderer::unique(&l.dataset, &field()?, ramp.unwrap_or(Ramp::Harbour), base)?,
        "graduated" | "graduatedcolors" => Renderer::graduated(&l.dataset, &field()?, classes, method, ramp.unwrap_or(Ramp::Ember), base)?,
        "proportional" | "proportionalsymbols" => {
            if g != GeometryType::Point {
                return Err("Proportional Symbols needs a point layer".into());
            }
            Renderer::proportional(&l.dataset, &field()?, base)?
        }
        other => return Err(format!("unknown symbology '{other}'")),
    };
    with(s, p, |l| l.renderer = r)
}
