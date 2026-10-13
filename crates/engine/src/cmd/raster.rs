//! The Style and Data tabs for raster layers: Rendering (symbology, stretch, DRA, resampling, band
//! combination), Enhancement (brightness, contrast, gamma), plus raster info and export.

use std::path::Path;
use std::sync::Arc;

use rustgis_data::raster::{self, Raster};
use rustgis_map::raster::{RasterLayer, RasterRenderer, Resampling, Stretch, classify, unique_values};
use rustgis_map::{ClassMethod, Ramp};
use serde_json::{Value, json};

use super::{b, f, ps};
use crate::{CmdResult, CommandSpec as C, Session};

/// Largest file opened from its contents (a browser upload): 512 MiB, or 256 MiB in the browser.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_UPLOAD_BYTES: usize = 512 << 20;
#[cfg(target_arch = "wasm32")]
pub(crate) const MAX_UPLOAD_BYTES: usize = 256 << 20;

pub(crate) fn need_raster(s: &Session) -> Option<&'static str> {
    match s.active_layer.and_then(|id| s.map().layer(id)) {
        Some(l) if l.raster.as_ref().is_some_and(|r| r.data.is_some()) => None,
        Some(l) if l.is_raster() => Some("the raster's data couldn't be read"),
        _ => Some("select a raster layer in the Layers pane first"),
    }
}

pub fn commands() -> Vec<C> {
    vec![
        C::new("layer.addDataBytes", "Add Data (from file contents)", "Home › Add", add_bytes)
            .params(r#"{"fileName": "name.tif|.tiff|.asc|.geojson|.csv|.zip|.kml|.kmz|.gpx|.parquet|.fgb|.gpkg|.mbtiles", "data": base64, "layer"?: "one layer of a zip, KML or GPX by name", "name"?: text, "x"?, "y"?, "wkt"?, "crs"?, "delimiter"? (as layer.addData, for delimited text)}"#),
        C::new("raster.symbology", "Symbology", "Style › Rendering", symbology).when(need_raster).params(
            r#"{"layer"?, "type": "stretch"|"rgb"|"classify"|"unique"|"colormap"|"shadedRelief", "band"?: 1.., "ramp"?, "invert"?: bool, "classes"?: 1..32, "method"?, "azimuth"?, "altitude"?, "zFactor"?}"#,
        ),
        C::new("raster.stretch", "Stretch Type", "Style › Rendering", |s, p| {
            let code = ps(p, "type").ok_or("type is required")?;
            let st = Stretch::from_code(code, f(p, "low"), f(p, "high"), f(p, "n")).ok_or_else(|| format!("unknown stretch '{code}'"))?;
            with(s, p, |rl, _| {
                rl.style.stretch = st;
                Ok(())
            })
        })
        .when(need_raster)
        .params(r#"{"layer"?, "type": "none"|"minMax"|"percentClip"|"standardDeviation"|"histogramEqualize", "low"?: %, "high"?: %, "n"?: deviations}"#),
        C::new("raster.dra", "DRA", "Style › Rendering", |s, p| {
            let on = b(p, "on");
            with(s, p, |rl, _| {
                rl.style.dra = on.unwrap_or(!rl.style.dra);
                Ok(())
            })
        })
        .when(need_raster)
        .params(r#"{"layer"?, "on"?: bool}"#),
        C::new("raster.resampling", "Resampling Type", "Style › Rendering", |s, p| {
            let name = ps(p, "type").ok_or("type is required")?;
            let r = Resampling::from_name(name).ok_or_else(|| format!("unknown resampling '{name}'"))?;
            with(s, p, |rl, _| {
                rl.style.resampling = r;
                Ok(())
            })
        })
        .when(need_raster)
        .params(r#"{"layer"?, "type": "nearest"|"bilinear"}"#),
        C::new("raster.bands", "Band Combination", "Style › Rendering", bands)
            .when(need_raster)
            .params(r#"{"layer"?, "red"?: 1.., "green"?: 1.., "blue"?: 1.., "preset"?: "natural"|"colorInfrared"}"#),
        C::new("raster.enhance", "Enhancement", "Style › Enhancement", |s, p| {
            let (br, co, ga) = (f(p, "brightness"), f(p, "contrast"), f(p, "gamma"));
            with(s, p, |rl, _| {
                if let Some(v) = br {
                    rl.style.brightness = v.clamp(-100.0, 100.0);
                }
                if let Some(v) = co {
                    rl.style.contrast = v.clamp(-100.0, 100.0);
                }
                if let Some(v) = ga {
                    rl.style.gamma = v.clamp(0.1, 10.0);
                }
                Ok(())
            })
        })
        .when(need_raster)
        .params(r#"{"layer"?, "brightness"?: -100..100, "contrast"?: -100..100, "gamma"?: 0.1..10}"#),
        C::new("raster.info", "Raster Information", "Data › Information", info).view().when(need_raster).params(r#"{"layer"?}"#),
        C::new("share.exportRaster", "Export Raster", "Data › Information", export).view().when(need_raster).params(
            r#"{"layer"?, "path": "*.tif", "band"?: 1..}"#,
        ),
    ]
}

/// The raster layer named by `p` (default: active layer) and its data.
fn get(s: &Session, p: &Value) -> Result<(u32, Arc<Raster>), String> {
    let id = s.layer_id(p)?;
    let l = s.map().layer(id).ok_or("no such layer")?;
    let rl = l.raster.as_ref().ok_or_else(|| format!("{} isn't a raster layer", l.name))?;
    let data = rl.data.clone().ok_or_else(|| format!("{}: the raster's data couldn't be read", l.name))?;
    Ok((id, data))
}

/// Applies `f` to the raster layer named by `p`.
fn with(s: &mut Session, p: &Value, f: impl FnOnce(&mut RasterLayer, &Raster) -> Result<(), String>) -> CmdResult {
    let (id, data) = get(s, p)?;
    let rl = s.map_mut().layer_mut(id).and_then(|l| l.raster.as_mut()).ok_or("no such layer")?;
    f(rl, &data)?;
    Ok(Value::Null)
}

/// A 1-based band parameter as a 0-based index inside the raster.
fn band(p: &Value, k: &str, r: &Raster) -> Result<Option<usize>, String> {
    match f(p, k) {
        None => Ok(None),
        Some(v) if v >= 1.0 && v <= r.band_count() as f64 => Ok(Some(v as usize - 1)),
        Some(v) => Err(format!("{k} {v}: the raster has bands 1–{}", r.band_count())),
    }
}

/// Adds a raster as a new layer, selects it and returns its id.
pub fn add_raster(s: &mut Session, name: &str, layer: RasterLayer) -> CmdResult {
    let mut unique = name.to_string();
    let mut i = 1;
    while s.map().layer_by_name(&unique).is_some() {
        i += 1;
        unique = format!("{name} ({i})");
    }
    let (w, h, n, overview) = layer.data.as_ref().map(|d| (d.width, d.height, d.band_count(), d.overview)).unwrap_or_default();
    let empty_before = s.map().layers.is_empty();
    let id = s.map_mut().add_raster(unique.clone(), layer);
    s.active_layer = Some(id);
    if let Some((fw, fh)) = overview {
        s.notify(format!("{unique}: showing a {w} × {h} overview of the {fw} × {fh} raster"), false);
    }
    if empty_before && let Some(e) = s.map().layer(id).map(|l| l.extent()) {
        s.zoom_to(&e);
    }
    Ok(json!({"layer": id, "width": w, "height": h, "bands": n}))
}

/// Opens a raster file by path.
pub fn add_path(s: &mut Session, path: &str, name: Option<&str>) -> CmdResult {
    let r = raster::read_path(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    let name = name.map(str::to_string).unwrap_or_else(|| r.name.clone());
    // Absolute, so the project finds it again from anywhere (`absolute`, unlike `canonicalize`,
    // gives plain paths on Windows rather than `\\?\` ones).
    let full = std::path::absolute(path).map(|p| p.display().to_string()).unwrap_or_else(|_| path.to_string());
    add_raster(s, &name, RasterLayer::from_path(r, full))
}

fn add_bytes(s: &mut Session, p: &Value) -> CmdResult {
    use base64::Engine;
    let name = ps(p, "fileName").ok_or("fileName is required")?.to_string();
    let data = ps(p, "data").ok_or("data is required")?;
    // Base64 is 4 characters per 3 bytes; refuse files past the upload limit before decoding.
    if data.len() / 4 * 3 > MAX_UPLOAD_BYTES {
        return Err(format!("{name} is too large to open"));
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(|e| format!("data: {e}"))?;
    add_named_bytes(s, p, name, bytes)
}

/// Adds a file's contents (an upload, a download) as its name's extension says.
pub(crate) fn add_named_bytes(s: &mut Session, p: &Value, name: String, bytes: Vec<u8>) -> CmdResult {
    if raster::is_raster_name(&name) {
        let r = raster::read_bytes(&name, &bytes).map_err(|e| format!("{name}: {e}"))?;
        let title = ps(p, "name").map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| r.name.clone());
        return add_raster(s, &title, RasterLayer::from_bytes(r, name, bytes));
    }
    // An MBTiles archive: held in memory and drawn as a tile layer.
    if rustgis_data::mbtiles::is_mbtiles_name(&name) {
        let url = crate::render::rustgis_tiles::mbtiles::register(&name, &bytes)?;
        let mut q = json!({"url": url, "kind": "mbtiles"});
        if let Some(n) = ps(p, "name") {
            q["name"] = json!(n);
        }
        return super::tiles::add(s, &q);
    }
    // A spreadsheet's sheet.
    if rustgis_data::sheets::is_sheet_name(&name) {
        return super::layer::add_sheet(s, p, &name, &bytes);
    }
    // Files of several layers: zipped Shapefiles, KML/KMZ, GPX, GeoRSS, LandXML.
    if rustgis_data::io::is_multi_layer_name(&name) {
        return super::layer::add_layers(s, p, &name, &bytes);
    }
    // GeoParquet and FlatGeobuf uploads (binary vector files).
    if rustgis_data::source::is_source_name(&name) {
        return super::layer::add_source(s, p, super::layer::Origin::Bytes(&name, std::sync::Arc::new(bytes)));
    }
    let text = String::from_utf8(bytes).map_err(|_| format!("{name} is not a text file"))?;
    // The delimited-text and naming parameters carry over (not the data, already decoded).
    let mut q = json!({"fileName": name, "text": text});
    for k in ["name", "x", "y", "wkt", "crs", "delimiter"] {
        if let Some(v) = p.get(k) {
            q[k] = v.clone();
        }
    }
    super::layer::add_data_text(s, &q)
}

fn symbology(s: &mut Session, p: &Value) -> CmdResult {
    let (_, r) = get(s, p)?;
    let kind = ps(p, "type").unwrap_or("stretch").to_string();
    let ramp = ps(p, "ramp").map(|n| Ramp::from_name(n).ok_or_else(|| format!("unknown colour scheme '{n}'"))).transpose()?;
    let bnd = band(p, "band", &r)?;
    with(s, p, |rl, r| {
        let cur_band = rl.style.renderer.bands().first().copied().unwrap_or(0);
        let bi = bnd.unwrap_or(cur_band).min(r.band_count().saturating_sub(1));
        let cur_ramp = rl.style.renderer.ramp();
        rl.style.renderer = match kind.to_ascii_lowercase().replace([' ', '_', '-'], "").as_str() {
            "stretch" => {
                let ramp = ramp.or(cur_ramp.filter(|r| !r.is_qualitative())).unwrap_or(Ramp::Greys);
                let invert = b(p, "invert").unwrap_or(ramp == Ramp::Greys);
                RasterRenderer::Stretch { band: bi, ramp, invert }
            }
            "rgb" => {
                if r.band_count() < 3 {
                    return Err("RGB needs a raster with at least three bands".into());
                }
                // Keep a band combination already chosen.
                match rl.style.renderer {
                    RasterRenderer::Rgb { red, green, blue } => RasterRenderer::Rgb { red, green, blue },
                    _ => RasterRenderer::Rgb { red: 0, green: 1, blue: 2 },
                }
            }
            "classify" => {
                let classes = f(p, "classes").map(|c| c.clamp(1.0, 32.0) as usize).unwrap_or(5);
                let method = ps(p, "method").and_then(ClassMethod::from_name).unwrap_or_default();
                classify(r, bi, classes, method, ramp.or(cur_ramp.filter(|r| !r.is_qualitative())).unwrap_or(Ramp::Ember))
            }
            "unique" | "uniquevalues" => {
                if r.bands.get(bi).and_then(|b| b.stats.categories.as_ref()).is_none() {
                    return Err("Unique Values needs whole-number pixels with at most 256 distinct values".into());
                }
                unique_values(r, bi, ramp.unwrap_or(Ramp::Harbour))
            }
            "colormap" => {
                if !matches!(r.color, raster::ColorHint::Palette(_)) {
                    return Err("the raster has no colour table".into());
                }
                RasterRenderer::Colormap
            }
            "shadedrelief" | "hillshade" => RasterRenderer::ShadedRelief {
                band: bi,
                ramp: ramp.or(Some(Ramp::Terrain)).filter(|_| !b(p, "grey").unwrap_or(false)),
                azimuth: f(p, "azimuth").unwrap_or(315.0).rem_euclid(360.0),
                altitude: f(p, "altitude").unwrap_or(45.0).clamp(0.0, 90.0),
                z_factor: f(p, "zFactor").unwrap_or(1.0).clamp(0.001, 1000.0),
            },
            other => return Err(format!("unknown raster symbology '{other}'")),
        };
        if matches!(rl.style.renderer, RasterRenderer::Colormap | RasterRenderer::UniqueValues { .. } | RasterRenderer::Classify { .. }) {
            rl.style.stretch = Stretch::None;
        } else if rl.style.stretch == Stretch::None && r.data_type != "UInt8" {
            rl.style.stretch = Stretch::default();
        }
        Ok(())
    })
}

fn bands(s: &mut Session, p: &Value) -> CmdResult {
    let (_, r) = get(s, p)?;
    if r.band_count() < 3 {
        return Err("band combinations need a raster with at least three bands".into());
    }
    let (red, green, blue) = match ps(p, "preset").map(|v| v.to_ascii_lowercase().replace([' ', '_', '-'], "")) {
        Some(v) if v == "natural" || v == "naturalcolor" => (0, 1, 2),
        Some(v) if v == "colorinfrared" || v == "cir" => {
            if r.band_count() < 4 {
                return Err("Color Infrared needs a near-infrared band 4".into());
            }
            (3, 0, 1)
        }
        Some(v) => return Err(format!("unknown band combination '{v}'")),
        None => {
            let cur = match get(s, p)
                .ok()
                .and_then(|(id, _)| s.map().layer(id).and_then(|l| l.raster.as_ref()).map(|rl| rl.style.renderer.clone()))
            {
                Some(RasterRenderer::Rgb { red, green, blue }) => (red, green, blue),
                _ => (0, 1, 2),
            };
            (band(p, "red", &r)?.unwrap_or(cur.0), band(p, "green", &r)?.unwrap_or(cur.1), band(p, "blue", &r)?.unwrap_or(cur.2))
        }
    };
    with(s, p, |rl, _| {
        rl.style.renderer = RasterRenderer::Rgb { red, green, blue };
        Ok(())
    })
}

fn info(s: &mut Session, p: &Value) -> CmdResult {
    let (id, r) = get(s, p)?;
    let rl = s.map().layer(id).and_then(|l| l.raster.as_ref()).ok_or("no layer")?;
    let (cx, cy) = r.cell_size();
    Ok(json!({
        "name": r.name,
        "source": rl.path.clone().or(rl.file_name.clone()),
        "width": r.width, "height": r.height,
        "overviewOf": r.overview.map(|(w, h)| json!([w, h])),
        "bands": r.bands.iter().map(|b| json!({
            "name": b.name, "min": b.stats.min, "max": b.stats.max, "mean": b.stats.mean, "stdDev": b.stats.std_dev,
            "validPixels": b.stats.count, "distinctValues": b.stats.categories.as_ref().map(Vec::len),
        })).collect::<Vec<_>>(),
        "dataType": r.data_type,
        "noData": r.nodata,
        "crs": r.crs.label(),
        "cellSize": [cx, cy],
        "extent": {"west": r.extent().min_x, "south": r.extent().min_y, "east": r.extent().max_x, "north": r.extent().max_y},
        "renderer": rl.style.renderer.label(),
        "stretch": rl.style.stretch.label(),
    }))
}

/// GeoTIFF bytes of a raster layer (band is 1-based; RGB rasters keep their colour bands).
pub fn geotiff_bytes(s: &Session, p: &Value) -> Result<(String, Vec<u8>), String> {
    let (_, r) = get(s, p)?;
    let bi = band(p, "band", &r)?.unwrap_or(0);
    let bytes = raster::geotiff::write(&r, bi).map_err(|e| e.to_string())?;
    Ok((r.name.clone(), bytes))
}

fn export(s: &mut Session, p: &Value) -> CmdResult {
    let path = ps(p, "path").ok_or("path is required")?.to_string();
    let (_, bytes) = geotiff_bytes(s, p)?;
    rustgis_data::io::write_atomic(Path::new(&path), &bytes).map_err(|e| format!("{path}: {e}"))?;
    let (_, r) = get(s, p)?;
    let note = match r.overview {
        Some((fw, fh)) => format!(" (the {} × {} overview of the {fw} × {fh} raster)", r.width, r.height),
        None => String::new(),
    };
    s.notify(format!("Exported {path}{note}"), false);
    Ok(json!({"path": path}))
}

/// Pixel values of the topmost visible raster under a longitude/latitude point.
pub fn identify(s: &Session, lon: f64, lat: f64) -> Option<Value> {
    let scale = s.scale();
    let c = rustgis_geom::Coord::new(lon, lat);
    s.map().layers.iter().filter(|l| l.visible && l.in_scale_range(scale)).find_map(|l| {
        let r = l.raster.as_ref()?.data.as_ref()?;
        let (col, row) = r.pixel_at(c)?;
        let vals: Vec<Option<f32>> = (0..r.band_count()).map(|b| r.value(b, col, row)).collect();
        if vals.iter().all(Option::is_none) {
            return None;
        }
        let mut attrs = serde_json::Map::new();
        for (b, v) in r.bands.iter().zip(&vals) {
            attrs.insert(b.name.clone(), json!(v.map(|v| rustgis_map::symbology::fmt_num(v as f64)).unwrap_or_else(|| "NoData".into())));
        }
        attrs.insert("Pixel".into(), json!(format!("{col}, {row}")));
        Some(json!({"layer": l.name, "layerId": l.id, "raster": true, "attributes": attrs}))
    })
}
