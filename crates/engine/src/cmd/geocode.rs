//! Geocoding: finding a place by name, and turning a table's addresses into points.

use rustgis_data::{Feature, Field, FieldType, Geometry, GeometryType, Value as Attr};
use rustgis_geom::{Coord, Extent};
use serde_json::{Value, json};

use super::{f, ps};
use crate::render::rustgis_tiles::geocode::{Geocoder, Place};
use crate::{CmdResult, CommandSpec as C, Session};

pub fn commands() -> Vec<C> {
    vec![
        C::new("map.findPlace", "Find Place", "Home › Navigate", find_place).params(
            r#"{"query": "a place or an address", "provider"?: "nominatim" (OpenStreetMap, default) | "photon", "url"?: "a geocoding server of your own", "limit"?: 1–20, "go"?: bool (zooms to the first, default true)}"#,
        ),
        C::new("layer.geocode", "Geocode Addresses", "Analyze › Data", geocode).params(
            r#"{"table"?: "a standalone table (such as a CSV of addresses)", "layer"?: name|id (or the active layer), "address": field | [fields] (joined with commas, such as street, city, country), "provider"?: "nominatim" | "photon", "url"?: "a geocoding server of your own (more than 100 addresses)", "name"?: text}"#,
        ),
    ]
}

/// The geocoder a command names.
pub(crate) fn geocoder(p: &Value) -> Result<Geocoder, String> {
    Geocoder::new(ps(p, "provider"), ps(p, "url"))
}

/// A place as the commands return it.
pub fn place_json(p: &Place) -> Value {
    json!({"name": p.name, "lon": p.lon, "lat": p.lat, "bbox": p.bbox})
}

/// Zooms to a place: its extent, or around its point.
pub fn go_to_place(s: &mut Session, p: &Place) {
    let e = match p.bbox {
        // A city's extent, not a country's whole territory with its islands: no wider than 40°.
        Some([w, south, e, n]) if e > w && n > south && e - w < 40.0 && n - south < 40.0 => Extent::new(w, south, e, n),
        _ => Extent::new(p.lon - 0.01, p.lat - 0.01, p.lon + 0.01, p.lat + 0.01),
    };
    s.zoom_to(&e);
}

fn find_place(s: &mut Session, p: &Value) -> CmdResult {
    let query = ps(p, "query").map(str::trim).filter(|q| !q.is_empty()).ok_or("query is required")?;
    let g = geocoder(p)?;
    let limit = f(p, "limit").map(|l| l.clamp(1.0, 20.0) as usize).unwrap_or(5);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, query, g, limit);
        Err("in the web app, find places with Home › Navigate › Find Place".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let places = crate::render::rustgis_tiles::geocode::search_blocking(&g, query, limit)?;
        if super::b(p, "go").unwrap_or(true)
            && let Some(first) = places.first()
        {
            go_to_place(s, first);
        }
        Ok(json!({"count": places.len(), "places": places.iter().map(place_json).collect::<Vec<_>>()}))
    }
}

/// The table or layer whose addresses are geocoded: `table` names a standalone table; `layer` a
/// layer, or a table when no layer has its name.
fn source<'a>(s: &'a Session, p: &Value) -> Result<&'a rustgis_data::Dataset, String> {
    let table = |n: &str| s.project.tables.iter().find(|t| t.name == n);
    if let Some(n) = ps(p, "table") {
        return table(n).ok_or_else(|| format!("no table named '{n}'"));
    }
    if let Some(n) = ps(p, "layer")
        && s.map().layer_by_name(n).is_none()
        && let Some(t) = table(n)
    {
        return Ok(t);
    }
    let l = s.map().layer(s.layer_id(p)?).ok_or("no such layer")?;
    if l.is_raster() {
        return Err(format!("{}: geocoding reads a table or a feature layer, not a raster", l.name));
    }
    Ok(&l.dataset)
}

/// The address fields a command names.
fn address_fields(p: &Value) -> Result<Vec<String>, String> {
    let fields: Vec<String> = match p.get("address") {
        Some(Value::String(f)) => f.split(',').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect(),
        _ => Vec::new(),
    };
    if fields.is_empty() || fields.len() > 10 {
        return Err("address is a field, or up to 10 fields joined with commas".into());
    }
    Ok(fields)
}

/// Each row's address, from the layer's fields (empty for a row without one), and the distinct
/// addresses in their first order. The app geocodes those itself, then runs `layer.geocode` with
/// the answers in `"places"`.
pub fn addresses(s: &Session, p: &Value) -> Result<(Vec<String>, Vec<String>), String> {
    let ds = source(s, p)?;
    if ds.windowed {
        return Err(format!("{}: load all its features first (Layers › Load All Features)", ds.name));
    }
    let idx: Vec<usize> = address_fields(p)?
        .iter()
        .map(|f| ds.field_index(f).ok_or_else(|| format!("{}: no field '{f}'", ds.name)))
        .collect::<Result<_, _>>()?;
    let rows: Vec<String> = (0..ds.features.len())
        .map(|r| {
            idx.iter()
                .filter(|&&i| !ds.value(r, i).is_null())
                .map(|&i| ds.value(r, i).to_string().trim().to_string())
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect();
    let mut distinct: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for a in rows.iter().filter(|a| !a.is_empty()) {
        if seen.insert(a.as_str()) {
            distinct.push(a.clone());
        }
    }
    Ok((rows, distinct))
}

/// The addresses past what a run may send.
pub fn check_batch(g: &Geocoder, distinct: usize) -> Result<(), String> {
    if distinct > g.batch_limit() {
        return Err(if g.is_public() {
            format!(
                "{distinct} addresses: a public geocoder takes {} per run (its usage policy); give \"url\" of a Nominatim or Photon server of your own for more",
                g.batch_limit()
            )
        } else {
            format!("{distinct} addresses: up to {} per run", g.batch_limit())
        });
    }
    Ok(())
}

/// The answers the app passes in `"places"`: an object from address to `{lon, lat, name}` (or
/// null when nothing was found).
fn given_places(v: &Value) -> Result<std::collections::HashMap<String, Option<Place>>, String> {
    let o = v.as_object().ok_or("places is an object from address to {lon, lat, name}")?;
    Ok(o.iter()
        .take(rustgis_tiles_max())
        .map(|(k, v)| {
            let place = (|| {
                let (lon, lat) = (v.get("lon")?.as_f64()?, v.get("lat")?.as_f64()?);
                ((-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat)).then(|| Place {
                    name: v.get("name").and_then(Value::as_str).unwrap_or("").chars().take(500).collect(),
                    lon,
                    lat,
                    bbox: None,
                })
            })();
            (k.clone(), place)
        })
        .collect())
}

fn rustgis_tiles_max() -> usize {
    crate::render::rustgis_tiles::geocode::MAX_BATCH
}

fn geocode(s: &mut Session, p: &Value) -> CmdResult {
    let g = geocoder(p)?;
    let (rows, distinct) = addresses(s, p)?;
    if distinct.is_empty() {
        return Err("the address fields are empty in every row".into());
    }
    let found = match p.get("places") {
        Some(v) => given_places(v)?,
        None => {
            check_batch(&g, distinct.len())?;
            lookup(&g, &distinct)?
        }
    };
    let src = source(s, p)?;
    let name =
        ps(p, "name").map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| format!("{} (geocoded)", src.name));
    // The table's fields, and what the geocoder called each place.
    let mut fields = src.fields.clone();
    let mut col = "geocoded".to_string();
    while fields.iter().any(|f| f.name.eq_ignore_ascii_case(&col)) {
        col.push('_');
    }
    fields.push(Field::new(col, FieldType::Text));
    let mut ds = rustgis_data::Dataset::new(name, GeometryType::Point, fields);
    let (mut matched, mut unmatched) = (0usize, 0usize);
    for (i, (feat, address)) in src.features.iter().zip(&rows).enumerate() {
        let place = found.get(address).and_then(Option::as_ref);
        let mut attrs = feat.attrs.clone();
        attrs.resize(src.fields.len(), Attr::Null);
        attrs.push(place.map(|pl| Attr::Text(pl.name.clone())).unwrap_or(Attr::Null));
        if place.is_some() {
            matched += 1;
        } else {
            unmatched += 1;
        }
        // Rows not found stay in the table, without a point.
        ds.features.push(Feature { id: i as u64, geometry: place.map(|pl| Geometry::Point(Coord::new(pl.lon, pl.lat))), attrs });
    }
    if matched == 0 {
        return Err("none of the addresses was found".into());
    }
    let new = super::layer::add_dataset(s, ds);
    if unmatched > 0 {
        s.notify(format!("{unmatched} of {} rows weren't found; they are in the table without a point", rows.len()), false);
    }
    Ok(json!({"layer": new, "matched": matched, "unmatched": unmatched}))
}

/// Geocodes each address, waiting between requests as the server asks.
#[cfg(not(target_arch = "wasm32"))]
fn lookup(g: &Geocoder, distinct: &[String]) -> Result<std::collections::HashMap<String, Option<Place>>, String> {
    let mut out = std::collections::HashMap::new();
    let mut failures = 0usize;
    for (i, a) in distinct.iter().enumerate() {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(g.spacing_ms()));
        }
        match crate::render::rustgis_tiles::geocode::search_blocking(g, a, 1) {
            Ok(places) => {
                out.insert(a.clone(), places.into_iter().next());
            }
            // A server that refuses every request stops the run early.
            Err(e) => {
                failures += 1;
                if failures >= 3 && out.is_empty() {
                    return Err(e);
                }
                out.insert(a.clone(), None);
            }
        }
    }
    Ok(out)
}

#[cfg(target_arch = "wasm32")]
fn lookup(_: &Geocoder, _: &[String]) -> Result<std::collections::HashMap<String, Option<Place>>, String> {
    Err("in the web app, geocode addresses with Analyze › Geocode Addresses".into())
}
