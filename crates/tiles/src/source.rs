//! From a tile layer's address to a style ready to draw.
//!
//! Every kind of tile layer becomes a MapLibre [`Style`]: raster tiles are a style with one
//! raster source and layer, vector tiles get the built-in style ([`DEFAULT_STYLE`]), and a style
//! document is read as it is. Sources given by a TileJSON address are filled in from it. Each
//! step fetches through the cache, so resolving answers [`Resolve::Pending`] until the
//! documents have arrived, then the same resolved style every time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Value, json};

use crate::cache::{self, Payload, State, Want};
use crate::style::{Source, Style};
use crate::{address, mbtiles, pmtiles, template};

/// The kind of address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Raster,
    Vector,
    Style,
    /// A PMTiles archive (raster or vector, as its header says).
    PmTiles,
    /// An MBTiles archive (raster or vector, as its metadata says): a file, or an upload.
    MbTiles,
}

/// A tile layer, as far as fetching and drawing it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub kind: Kind,
    pub url: String,
    pub subdomains: Vec<String>,
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub tile_size: u32,
    pub tms: bool,
    pub attribution: String,
}

impl Spec {
    fn key(&self) -> String {
        format!(
            "{:?}|{}|{}|{}|{}|{}|{}|{}",
            self.kind,
            self.url,
            self.subdomains.join(","),
            self.min_zoom,
            self.max_zoom,
            self.tile_size,
            self.tms,
            self.attribution
        )
    }
}

/// A style whose sources all have tile addresses.
pub struct Resolved {
    pub style: Style,
    /// The style's sprite (an address without `.json`/`.png`), when it has one RustGIS can
    /// fetch.
    pub sprite: Option<String>,
    /// Credits from the layer and its sources, without duplicates.
    pub attribution: Vec<String>,
}

pub enum Resolve {
    Ready(Arc<Resolved>),
    Pending,
    Failed(String),
}

fn resolved() -> &'static Mutex<HashMap<String, Arc<Resolved>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<Resolved>>>> = OnceLock::new();
    R.get_or_init(Mutex::default)
}

/// The most resolved styles kept.
const MAX_RESOLVED: usize = 64;

/// Forgets the resolved styles, so their documents are read again (with [`crate::cache::clear`]).
pub(crate) fn forget() {
    resolved().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// The built-in style for vector tiles without one: it knows the OpenMapTiles, Protomaps and
/// Shortbread layer names, and draws water, land use, parks, buildings, roads, boundaries and
/// place names from whichever of them the tiles have.
pub const DEFAULT_STYLE: &str = include_str!("default_style.json");

/// The style to draw `spec` with, once the documents it needs have been fetched.
pub fn resolve(spec: &Spec) -> Resolve {
    let key = spec.key();
    // A PMTiles archive on this computer is a path, not a web address.
    let local = spec.kind == Kind::PmTiles && pmtiles::is_local(&spec.url);
    let checked = if spec.kind == Kind::MbTiles {
        mbtiles::check(&spec.url)
    } else if local {
        pmtiles::check_local(&spec.url)
    } else {
        address::check(&spec.url, None)
    };
    if let Err(e) = checked {
        return Resolve::Failed(e);
    }
    if let Some(r) = resolved().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Resolve::Ready(r.clone());
    }
    let doc = match spec.kind {
        Kind::Raster => raster_style(spec),
        Kind::Vector => match vector_style(spec) {
            Some(d) => d,
            None => return Resolve::Failed("the built-in vector style could not be read".into()),
        },
        Kind::Style => match fetch_json(&spec.url) {
            Ok(Some(d)) => d,
            Ok(None) => return Resolve::Pending,
            Err(e) => return Resolve::Failed(e),
        },
        Kind::PmTiles => match pmtiles::info(&spec.url) {
            Ok(Some(info)) => {
                let archive = Archive {
                    tiles: pmtiles::template(&spec.url),
                    vector: info.tile_type.is_vector(),
                    min_zoom: info.min_zoom,
                    max_zoom: info.max_zoom,
                    attribution: info.attribution(),
                    layers: info.vector_layers(),
                };
                match archive_style(spec, archive) {
                    Some(d) => d,
                    None => return Resolve::Failed("the built-in vector style could not be read".into()),
                }
            }
            Ok(None) => return Resolve::Pending,
            Err(e) => return Resolve::Failed(format!("{}: {e}", spec.url)),
        },
        Kind::MbTiles => match mbtiles::info(&spec.url) {
            Ok(info) => {
                let archive = Archive {
                    tiles: mbtiles::template(&spec.url),
                    vector: info.format.is_vector(),
                    min_zoom: info.min_zoom,
                    max_zoom: info.max_zoom,
                    attribution: info.attribution.clone(),
                    layers: info.vector_layers(),
                };
                match archive_style(spec, archive) {
                    Some(d) => d,
                    None => return Resolve::Failed("the built-in vector style could not be read".into()),
                }
            }
            Err(e) => return Resolve::Failed(e),
        },
    };
    let mut style = match Style::parse(&doc) {
        Ok(s) => s,
        Err(e) => return Resolve::Failed(format!("{}: {e}", spec.url)),
    };
    // Sources given by a TileJSON address.
    let base = spec.url.clone();
    let mut pending = false;
    for (_, src) in &mut style.sources {
        let (tiles, url) = match src {
            Source::Vector { tiles, url, .. } | Source::Raster { tiles, url, .. } => (tiles, url),
            Source::Other => continue,
        };
        if tiles.is_empty()
            && let Some(u) = url.clone()
        {
            if u.starts_with("mapbox://") {
                return Resolve::Failed(format!("{u}: mapbox:// addresses need Mapbox's own library"));
            }
            let u = template::join(&base, &u);
            // An ArcGIS vector tile service answers its TileJSON-like description as JSON only
            // when asked (`?f=json`); its styles point at the service itself.
            let u = if u.split(['?', '#']).next().unwrap_or(&u).trim_end_matches('/').to_ascii_lowercase().ends_with("/vectortileserver")
                && !u.contains("f=json")
            {
                // With the slash, so the tile paths it gives resolve inside the service.
                format!("{}/?f=json", u.split(['?', '#']).next().unwrap_or(&u).trim_end_matches('/'))
            } else {
                u
            };
            // A style's TileJSON is the style's choice; a vector layer's is the user's.
            if let Err(e) = address::check(&u, (spec.kind == Kind::Style).then_some(base.as_str())) {
                return Resolve::Failed(e);
            }
            match fetch_json(&u) {
                Ok(Some(tj)) => {
                    src.apply_tilejson(&tj);
                    // Tile addresses relative to the TileJSON document, which chose them.
                    if let Source::Vector { tiles, .. } | Source::Raster { tiles, .. } = src {
                        for t in tiles.iter_mut() {
                            *t = template::join(&u, t);
                            if let Err(e) = address::check(t, Some(&u)) {
                                return Resolve::Failed(e);
                            }
                        }
                    }
                }
                Ok(None) => pending = true,
                Err(e) => return Resolve::Failed(e),
            }
        } else if let Source::Vector { tiles, .. } | Source::Raster { tiles, .. } = src {
            for t in tiles.iter_mut() {
                // Tiles from an archive: read from the archive the layer names.
                if (spec.kind == Kind::PmTiles && pmtiles::is_tile(t)) || (spec.kind == Kind::MbTiles && mbtiles::is_tile(t)) {
                    continue;
                }
                *t = template::join(&base, t);
                // A style's tile addresses are the style's choice; a raster or vector layer's
                // template is the user's.
                if let Err(e) = address::check(t, (spec.kind == Kind::Style).then_some(base.as_str())) {
                    return Resolve::Failed(e);
                }
            }
        }
    }
    if pending {
        return Resolve::Pending;
    }
    let mut attribution: Vec<String> = Vec::new();
    let mut credit = |a: &str| {
        let a = plain_text(a);
        if !a.is_empty() && !attribution.contains(&a) {
            attribution.push(a);
        }
    };
    credit(&spec.attribution);
    for (_, src) in &style.sources {
        if let Source::Vector { attribution, .. } | Source::Raster { attribution, .. } = src {
            credit(attribution);
        }
    }
    // The sprite is the style's choice; one that can't be fetched only loses the icons.
    let sprite = style
        .sprite
        .as_deref()
        .filter(|s| !s.starts_with("mapbox://"))
        .map(|s| template::join(&base, s))
        .filter(|s| address::check(s, Some(&base)).is_ok());
    let r = Arc::new(Resolved { style, attribution, sprite });
    let mut all = resolved().lock().unwrap_or_else(|e| e.into_inner());
    // A few layers are in use at a time: past that many, start again rather than grow.
    if all.len() >= MAX_RESOLVED {
        all.clear();
    }
    all.insert(key, r.clone());
    Resolve::Ready(r)
}

/// The document at `url`: `Ok(None)` while it is on its way.
fn fetch_json(url: &str) -> Result<Option<Value>, String> {
    match cache::get(url, Want::Json) {
        State::Ready(p) => match &*p {
            Payload::Json(v) => Ok(Some(v.clone())),
            _ => Err(format!("{url}: not a JSON document")),
        },
        State::Pending => Ok(None),
        State::Failed(e) => Err(format!("{url}: {e}")),
    }
}

/// The tile addresses for a template with `{s}` expanded over its subdomains, and TMS rows
/// written as `{-y}`.
fn templates(spec: &Spec) -> Vec<String> {
    let url = if spec.tms { spec.url.replace("{y}", "{-y}") } else { spec.url.clone() };
    if url.contains("{s}") {
        let subs: Vec<String> = if spec.subdomains.is_empty() { vec!["a".into(), "b".into(), "c".into()] } else { spec.subdomains.clone() };
        subs.iter().map(|s| url.replace("{s}", s)).collect()
    } else {
        vec![url]
    }
}

fn raster_style(spec: &Spec) -> Value {
    json!({
        "version": 8,
        "sources": {"raster": {
            "type": "raster", "tiles": templates(spec), "tileSize": spec.tile_size,
            "minzoom": spec.min_zoom, "maxzoom": spec.max_zoom,
        }},
        "layers": [{"id": "raster", "type": "raster", "source": "raster"}],
    })
}

/// A tile archive (PMTiles or MBTiles), as far as styling it goes.
struct Archive {
    /// The template of its tile addresses.
    tiles: String,
    vector: bool,
    min_zoom: u8,
    max_zoom: u8,
    attribution: String,
    /// The vector layers its metadata lists.
    layers: Vec<String>,
}

/// The style for a tile archive: a raster layer for picture tiles; for vector tiles the
/// built-in style when the archive has the layers it knows (a basemap), otherwise a layer for each
/// of the archive's vector layers (points, lines and polygons each in their own colour).
fn archive_style(spec: &Spec, info: Archive) -> Option<Value> {
    let tiles = vec![info.tiles];
    let (minzoom, maxzoom) = (info.min_zoom, info.max_zoom);
    let attribution = if spec.attribution.is_empty() { info.attribution } else { spec.attribution.clone() };
    if !info.vector {
        return Some(json!({
            "version": 8,
            "sources": {"raster": {
                "type": "raster", "tiles": tiles, "tileSize": spec.tile_size,
                "minzoom": minzoom, "maxzoom": maxzoom, "attribution": attribution,
            }},
            "layers": [{"id": "raster", "type": "raster", "source": "raster"}],
        }));
    }
    let source = json!({"type": "vector", "tiles": tiles, "minzoom": minzoom, "maxzoom": maxzoom, "attribution": attribution});
    let names = info.layers;
    let known = [
        "water",
        "roads",
        "transportation",
        "buildings",
        "building",
        "landuse",
        "landcover",
        "boundaries",
        "boundary",
        "places",
        "place",
        "earth",
        "streets",
    ];
    if names.iter().filter(|n| known.contains(&n.as_str())).count() >= 3 {
        let mut doc: Value = serde_json::from_str(DEFAULT_STYLE).ok()?;
        *doc.get_mut("sources")?.get_mut("vector")? = source;
        return Some(doc);
    }
    const COLOURS: [&str; 8] = ["#e6550d", "#3182bd", "#31a354", "#756bb1", "#d6616b", "#8c6d31", "#17becf", "#bcbd22"];
    let mut layers = Vec::new();
    // Each archive starts the palette somewhere of its own, so two archives look apart.
    let start = spec.url.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(usize::from(b)));
    for (i, name) in names.iter().take(64).enumerate() {
        let c = COLOURS.get(start.wrapping_add(i) % COLOURS.len()).copied().unwrap_or("#3182bd");
        layers.push(json!({"id": format!("{name}-fill"), "type": "fill", "source": "vector", "source-layer": name,
            "filter": ["==", "$type", "Polygon"], "paint": {"fill-color": c, "fill-opacity": 0.35}}));
        layers.push(json!({"id": format!("{name}-line"), "type": "line", "source": "vector", "source-layer": name,
            "filter": ["in", "$type", "LineString", "Polygon"], "paint": {"line-color": c, "line-width": 1}}));
        layers.push(json!({"id": format!("{name}-point"), "type": "circle", "source": "vector", "source-layer": name,
            "filter": ["==", "$type", "Point"], "paint": {"circle-color": c, "circle-radius": 3, "circle-stroke-color": "#ffffff", "circle-stroke-width": 1}}));
    }
    Some(json!({"version": 8, "sources": {"vector": source}, "layers": layers}))
}

fn vector_style(spec: &Spec) -> Option<Value> {
    let mut doc: Value = serde_json::from_str(DEFAULT_STYLE).ok()?;
    let source = if spec.url.contains("{z}") {
        json!({"type": "vector", "tiles": templates(spec), "minzoom": spec.min_zoom, "maxzoom": spec.max_zoom})
    } else {
        // A TileJSON address.
        json!({"type": "vector", "url": spec.url})
    };
    *doc.get_mut("sources")?.get_mut("vector")? = source;
    Some(doc)
}

/// Attribution HTML as plain text: tags dropped, entities decoded.
pub fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&copy;", "©")
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: Kind, url: &str) -> Spec {
        Spec {
            kind,
            url: url.into(),
            subdomains: Vec::new(),
            min_zoom: 0,
            max_zoom: 19,
            tile_size: 256,
            tms: false,
            attribution: "&copy; <a href=\"x\">OSM</a>".into(),
        }
    }

    #[test]
    fn resolves_raster_and_vector_templates_at_once() {
        let Resolve::Ready(r) = resolve(&spec(Kind::Raster, "https://{s}.tile.example.com/{z}/{x}/{y}.png")) else { panic!("not ready") };
        assert_eq!(r.attribution, vec!["© OSM".to_string()]);
        match r.style.sources.first().map(|s| &s.1) {
            Some(Source::Raster { tiles, .. }) => assert_eq!(tiles.len(), 3),
            _ => panic!("no raster source"),
        }
        let Resolve::Ready(v) = resolve(&spec(Kind::Vector, "https://tiles.example.com/{z}/{x}/{y}.pbf")) else { panic!("not ready") };
        assert!(v.style.layers.len() > 10);
    }

    #[test]
    fn refuses_forbidden_addresses() {
        assert!(matches!(resolve(&spec(Kind::Raster, "http://169.254.169.254/{z}/{x}/{y}.png")), Resolve::Failed(_)));
        // A self-hosted server the user names is fine.
        assert!(matches!(resolve(&spec(Kind::Raster, "http://localhost:8080/{z}/{x}/{y}.png")), Resolve::Ready(_)));
    }

    #[test]
    fn plain_attribution() {
        assert_eq!(plain_text("<a href='x'>&copy; OpenStreetMap</a>  contributors &amp; CARTO"), "© OpenStreetMap contributors & CARTO");
    }
}
