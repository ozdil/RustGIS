//! Home › Add › Basemap: web tile layers (XYZ/TMS/WMS raster tiles, vector tiles and
//! MapLibre styles) over the basemap.

use rustgis_map::tiles::{MAX_TILE_LAYERS, TileKind, TileLayer};
use serde_json::{Value, json};

use super::{b, f, ps};
use crate::{CmdResult, CommandSpec as C, Session};

/// Commands that make the map fetch from an address they're given (a plugin needs the
/// `network` permission to run them).
pub const NETWORK: &[&str] = &["map.addTiles", "layer.addUrl", "service.layers", "service.add"];

pub fn commands() -> Vec<C> {
    vec![
        C::new("map.addTiles", "Add Tile Layer", "Home › Add", add).params(
            r#"{"url": "https://…/{z}/{x}/{y}.png" | vector tiles ".pbf" or TileJSON | MapLibre style.json | a PMTiles archive (web address or file path) | an MBTiles file path, "kind"?: "raster"|"vector"|"style"|"pmtiles"|"mbtiles" (guessed from the url), "name"?: text, "attribution"?: text, "minZoom"?: number, "maxZoom"?: number, "tileSize"?: 256|512, "tms"?: bool, "subdomains"?: [text], "opacity"?: 0–1, "overlay"?: bool (a transparent layer kept on top), "replace"?: bool (removes the other tile basemaps, not overlays), "id"?: catalog id}"#,
        ),
        C::new("map.removeTiles", "Remove Tile Layer", "Home › Add", remove)
            .params(r#"{"name"?: text, "index"?: number, "id"?: catalog id} (removes every tile layer when none is given)"#),
        C::new("map.setTiles", "Tile Layer Properties", "Home › Add", set).params(
            r#"{"name"?: text | "index"?: number (0 = top), "visible"?: bool, "opacity"?: 0–1, "move"?: "up"|"down"|"top"|"bottom", "rename"?: text}"#,
        ),
        C::new("map.tiles", "Tile Layers", "Home › Add", |s, _| Ok(list(s))).view(),
    ]
}

fn list(s: &Session) -> Value {
    Value::Array(
        s.map()
            .tiles
            .iter()
            .enumerate()
            .map(|(i, t)| {
                json!({"index": i, "name": t.name, "id": t.id, "kind": t.kind.code(), "url": t.url, "visible": t.visible,
                    "opacity": t.opacity, "overlay": t.overlay, "attribution": t.attribution, "minZoom": t.min_zoom, "maxZoom": t.max_zoom})
            })
            .collect(),
    )
}

pub(crate) fn add(s: &mut Session, p: &Value) -> CmdResult {
    let url = ps(p, "url").map(str::trim).filter(|u| !u.is_empty()).ok_or("url is required")?;
    let kind = match ps(p, "kind") {
        Some(k) => TileKind::from_code(k).ok_or_else(|| format!("unknown kind '{k}' (raster, vector, style, pmtiles or mbtiles)"))?,
        None => TileKind::guess(url),
    };
    let mut t = TileLayer::new(ps(p, "name").unwrap_or_default(), kind, url);
    if let Some(a) = ps(p, "attribution") {
        t.attribution = a.to_string();
    }
    if let Some(z) = f(p, "minZoom") {
        t.min_zoom = z.clamp(0.0, 24.0) as u8;
    }
    if let Some(z) = f(p, "maxZoom") {
        t.max_zoom = z.clamp(0.0, 24.0) as u8;
    } else if kind != TileKind::Raster {
        t.max_zoom = 14;
    }
    if let Some(n) = f(p, "tileSize") {
        t.tile_size = n.clamp(0.0, 4096.0) as u32;
    }
    t.tms = b(p, "tms").unwrap_or(false);
    t.overlay = b(p, "overlay").unwrap_or(false);
    if let Some(o) = f(p, "opacity") {
        t.opacity = o as f32;
    }
    if let Some(subs) = p.get("subdomains") {
        t.subdomains = match subs {
            Value::Array(a) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            Value::String(s) => s.chars().map(String::from).collect(),
            _ => Vec::new(),
        };
    }
    if let Some(id) = ps(p, "id") {
        t.id = id.to_string();
    }
    t.repair()?;
    let replace = b(p, "replace").unwrap_or(false);
    let m = s.map_mut();
    if replace {
        m.tiles.retain(|o| o.overlay);
    }
    // The same address again replaces the old one.
    m.tiles.retain(|o| !(o.url == t.url && o.kind == t.kind));
    if m.tiles.len() >= MAX_TILE_LAYERS {
        return Err(format!("a map holds at most {MAX_TILE_LAYERS} tile layers"));
    }
    // Overlays go on top; basemaps go on top of the other basemaps, under the overlays.
    let at = if t.overlay { 0 } else { m.tiles.iter().position(|o| !o.overlay).unwrap_or(m.tiles.len()) };
    let out = json!({"name": t.name, "kind": t.kind.code(), "index": at});
    m.tiles.insert(at, t);
    Ok(out)
}

/// The tile layer a command names by `index`, `name` or `id`.
fn find(s: &Session, p: &Value) -> Result<Option<usize>, String> {
    let tiles = &s.map().tiles;
    if let Some(i) = f(p, "index") {
        let i = (i >= 0.0 && i.fract() == 0.0).then_some(i as usize).filter(|i| *i < tiles.len());
        return i.map(Some).ok_or_else(|| "no tile layer at that index".to_string());
    }
    if let Some(n) = ps(p, "name") {
        return tiles.iter().position(|t| t.name.eq_ignore_ascii_case(n.trim())).map(Some).ok_or_else(|| format!("no tile layer '{n}'"));
    }
    if let Some(id) = ps(p, "id") {
        return tiles.iter().position(|t| t.id == id).map(Some).ok_or_else(|| format!("no tile layer '{id}'"));
    }
    Ok(None)
}

fn remove(s: &mut Session, p: &Value) -> CmdResult {
    match find(s, p)? {
        Some(i) => {
            let t = s.map_mut().tiles.remove(i);
            Ok(json!({"removed": [t.name]}))
        }
        None => {
            let gone: Vec<String> = s.map_mut().tiles.drain(..).map(|t| t.name).collect();
            Ok(json!({"removed": gone}))
        }
    }
}

fn set(s: &mut Session, p: &Value) -> CmdResult {
    let i = find(s, p)?.ok_or("name or index is required")?;
    let m = s.map_mut();
    let n = m.tiles.len();
    let Some(t) = m.tiles.get_mut(i) else { return Err("no tile layer at that index".into()) };
    if let Some(v) = b(p, "visible") {
        t.visible = v;
    }
    if let Some(o) = f(p, "opacity") {
        t.opacity = if o.is_finite() { o.clamp(0.0, 1.0) as f32 } else { 1.0 };
    }
    if let Some(r) = ps(p, "rename").map(str::trim).filter(|r| !r.is_empty()) {
        t.name = r.chars().take(120).collect();
    }
    if let Some(mv) = ps(p, "move") {
        let to = match mv {
            "up" => i.saturating_sub(1),
            "down" => (i + 1).min(n - 1),
            "top" => 0,
            "bottom" => n - 1,
            other => return Err(format!("unknown move '{other}' (up, down, top or bottom)")),
        };
        let t = m.tiles.remove(i);
        m.tiles.insert(to, t);
    }
    Ok(list(s))
}

#[cfg(test)]
mod tests {
    use crate::{Project, Session};
    use serde_json::json;

    #[test]
    fn stacks_tile_layers() {
        let mut s = Session::new(Project::default());
        let osm = json!({"url": "https://tile.openstreetmap.org/{z}/{x}/{y}.png", "name": "OSM", "attribution": "© OpenStreetMap"});
        assert!(s.run("map.addTiles", &osm).is_ok());
        let labels = json!({"url": "https://x.example.com/labels/{z}/{x}/{y}.png", "name": "Labels", "overlay": true});
        assert!(s.run("map.addTiles", &labels).is_ok());
        let style = json!({"url": "https://tiles.openfreemap.org/styles/liberty", "name": "Liberty"});
        assert!(s.run("map.addTiles", &style).is_ok());
        let names: Vec<&str> = s.map().tiles.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Labels", "Liberty", "OSM"]);
        assert_eq!(s.map().tiles[1].kind, rustgis_map::TileKind::Style);
        // Replacing keeps the overlays.
        assert!(s.run("map.addTiles", &json!({"url": "https://y.example.com/{z}/{x}/{y}.jpg", "replace": true})).is_ok());
        let names: Vec<&str> = s.map().tiles.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Labels", "y.example.com"]);
        assert!(s.run("map.setTiles", &json!({"name": "labels", "opacity": 0.5, "move": "bottom"})).is_ok());
        assert_eq!(s.map().tiles[1].opacity, 0.5);
        assert!(s.run("edit.undo", &json!({})).is_ok());
        assert_eq!(s.map().tiles[0].name, "Labels");
        assert!(s.run("map.addTiles", &json!({"url": "file:///etc/passwd"})).is_err());
        assert!(s.run("map.addTiles", &json!({"url": "https://a/{z}", "kind": "nope"})).is_err());
        assert!(s.run("map.removeTiles", &json!({"index": 7})).is_err());
        assert!(s.run("map.removeTiles", &json!({})).is_ok());
        assert!(s.map().tiles.is_empty());
        // Saved and read back with the project.
        assert!(s.run("map.addTiles", &osm).is_ok());
        let text = serde_json::to_string(&s.project).unwrap();
        let p = Project::from_json(&text).unwrap();
        assert_eq!(p.map().tiles.len(), 1);
        assert_eq!(p.map().tiles[0].attribution, "© OpenStreetMap");
    }
}
