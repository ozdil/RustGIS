//! Web map and feature services: connect to a WMS, WMTS, WFS, OGC API Features or ArcGIS REST
//! service, list what it offers (from its capabilities), and add a layer — map layers as tile
//! layers, feature types and collections as data downloaded as GeoJSON.
//!
//! The app fetches the capabilities and features in the background and passes the capabilities
//! here as `"text"`; the command line, MCP and the desktop assistant let these commands fetch
//! (waiting for the answer).

use rustgis_data::ogc::{Offer, Service};
use serde_json::{Value, json};

use super::ps;
use crate::{CmdResult, CommandSpec as C, Session};

pub fn commands() -> Vec<C> {
    vec![
        C::new("service.layers", "Service Layers", "Home › Add", layers).view().params(
            r#"{"url": "a WMS, WMTS or WFS address (GetCapabilities is added), an OGC API /collections address, or an ArcGIS FeatureServer, MapServer, ImageServer or VectorTileServer", "type"?: "wms"|"wmts"|"wfs"|"ogcapi"|"arcgis" (guessed from the address), "text"?: "the capabilities document, when already fetched"}"#,
        ),
        C::new("service.add", "Add Service Layer", "Home › Add", add).params(
            r#"{"url": "as service.layers", "type"?, "layer": "the layer's name (service.layers lists them)", "name"?: text, "text"?: "the capabilities document, when already fetched"}"#,
        ),
    ]
}

fn url_of(p: &Value) -> Result<&str, String> {
    let url = ps(p, "url").map(str::trim).filter(|u| !u.is_empty()).ok_or("url is required")?;
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(format!("{url}: a service is given by its http:// or https:// address"));
    }
    Ok(url)
}

/// The service at `p`'s address: from its capabilities in `"text"`, or fetched (waiting for them).
fn capabilities(p: &Value) -> Result<Service, String> {
    let url = url_of(p)?;
    let caps = rustgis_data::ogc::capabilities_url(url, ps(p, "type"));
    let svc = match ps(p, "text") {
        Some(text) => rustgis_data::ogc::parse(&caps, text.as_bytes()).map_err(|e| format!("{url}: {e}"))?,
        None => fetch_capabilities(url, &caps)?,
    };
    Ok(vet(svc, &caps))
}

#[cfg(target_arch = "wasm32")]
fn fetch_capabilities(url: &str, _caps: &str) -> Result<Service, String> {
    Err(format!("{url}: in the web app, use Home › Add Data › From Service"))
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_capabilities(url: &str, caps: &str) -> Result<Service, String> {
    let bytes = crate::render::rustgis_tiles::cache::download_blocking(caps).map_err(|e| format!("{url}: {e}"))?;
    rustgis_data::ogc::parse(caps, &bytes).map_err(|e| format!("{url}: {e}"))
}

/// Leaves out the layers whose addresses a document on the web may not send RustGIS to: the
/// capabilities are hostile input, and their endpoints and links must not reach into the local
/// network (unless the service itself is there) or a forbidden address.
fn vet(mut svc: Service, caps: &str) -> Service {
    let before = svc.layers.len();
    svc.layers.retain(|l| {
        let u = match &l.offer {
            Offer::Tiles { template } => template,
            Offer::Features { url } | Offer::Style { url } => url,
        };
        crate::render::rustgis_tiles::address::check(u, Some(caps)).is_ok()
    });
    let dropped = before - svc.layers.len();
    if dropped > 0 {
        svc.notes.push(format!("{dropped} layers point to addresses RustGIS won't fetch for a service on the web and are left out"));
    }
    svc
}

/// A service's GeoJSON answer made ready to read: latitude-first coordinates turned round, and a
/// cut-off answer pointed out.
pub fn features_answer(s: &mut Session, name: &str, bytes: Vec<u8>) -> Vec<u8> {
    if let Some(note) = rustgis_data::ogc::partial_note(&bytes) {
        s.notify(format!("{name}: {note}"), false);
    }
    rustgis_data::ogc::fix_axes(&bytes).unwrap_or(bytes)
}

/// A service as the app and other callers see it.
pub fn service_json(svc: &Service) -> Value {
    let layers: Vec<Value> = svc
        .layers
        .iter()
        .map(|l| {
            let offer = match &l.offer {
                Offer::Tiles { template } => json!({"type": "tiles", "url": template}),
                Offer::Features { url } => json!({"type": "features", "url": url}),
                Offer::Style { url } => json!({"type": "style", "url": url}),
            };
            json!({"name": l.name, "title": l.title, "bbox": l.bbox, "offer": offer})
        })
        .collect();
    json!({"kind": svc.kind, "title": svc.title, "count": layers.len(), "layers": layers, "notes": svc.notes})
}

fn layers(_s: &mut Session, p: &Value) -> CmdResult {
    Ok(service_json(&capabilities(p)?))
}

/// Adds one of a service's layers: map layers as tiles (an overlay), a style as a style, features
/// downloaded.
fn add(s: &mut Session, p: &Value) -> CmdResult {
    let url = url_of(p)?.to_string();
    let want = ps(p, "layer").map(str::trim).filter(|l| !l.is_empty()).ok_or("layer is required")?;
    let svc = capabilities(p)?;
    let layer = svc
        .layers
        .iter()
        .find(|l| l.name == want)
        .or_else(|| svc.layers.iter().find(|l| l.name.eq_ignore_ascii_case(want) || l.title.eq_ignore_ascii_case(want)))
        .ok_or_else(|| format!("{url}: no layer '{want}' (service.layers lists them)"))?;
    let name = ps(p, "name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&layer.title).to_string();
    let zoom = |s: &mut Session| {
        if let Some([w, so, e, n]) = layer.bbox {
            s.zoom_to(&rustgis_geom::Extent::new(w, so, e, n));
        }
    };
    match &layer.offer {
        Offer::Tiles { template } => {
            let r = super::tiles::add(s, &json!({"url": template, "kind": "raster", "name": name, "overlay": true}))?;
            zoom(s);
            Ok(r)
        }
        Offer::Style { url: style } => {
            let r = super::tiles::add(s, &json!({"url": style, "kind": "style", "name": name}))?;
            zoom(s);
            Ok(r)
        }
        Offer::Features { url: items } => {
            #[cfg(target_arch = "wasm32")]
            {
                let _ = items;
                Err(format!("{url}: in the web app, Home › Add Data › From Service downloads the features"))
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                // `download_blocking` is bounded: MAX_DOWNLOAD bytes and BLOCKING_TIMEOUT.
                let bytes = crate::render::rustgis_tiles::cache::download_blocking(items).map_err(|e| format!("{items}: {e}"))?;
                let bytes = features_answer(s, &name, bytes);
                let r = super::raster::add_named_bytes(s, &json!({"name": name}), format!("{name}.geojson"), bytes)?;
                zoom(s);
                Ok(r)
            }
        }
    }
}
