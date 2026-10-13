//! STAC (SpatioTemporal Asset Catalog) APIs: their collections, a search for items, and the
//! assets an item offers. Requests are built and answers read here; the fetching is the caller's
//! (blocking on the command line, off the frame in the app). Answers are hostile input: counts,
//! lengths and coordinates are bounded.

use serde_json::Value;

/// Element 84's Earth Search (Sentinel-2, Landsat, Copernicus DEM and more, on AWS), the default.
pub const EARTH_SEARCH: &str = "https://earth-search.aws.element84.com/v1";

/// The most items a search asks for.
pub const MAX_ITEMS: usize = 200;
/// The most collections read from an API.
const MAX_COLLECTIONS: usize = 2000;
/// The most assets read from an item.
const MAX_ASSETS: usize = 100;
/// The longest text kept (titles, ids, addresses).
const MAX_TEXT: usize = 2000;
/// The most coordinates a footprint keeps.
const MAX_FOOTPRINT: usize = 20_000;

/// A collection an API offers.
#[derive(Clone, Debug, PartialEq)]
pub struct Collection {
    pub id: String,
    pub title: String,
}

/// An asset of an item: a file, by its address.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub key: String,
    pub href: String,
    /// Its media type (`image/tiff; application=geotiff; profile=cloud-optimized`…).
    pub media_type: String,
    pub roles: Vec<String>,
    pub title: String,
}

/// An item: a scene (or tile) with its footprint, time and assets.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub collection: String,
    pub datetime: String,
    /// West, south, east, north.
    pub bbox: Option<[f64; 4]>,
    /// Its footprint, a GeoJSON geometry.
    pub geometry: Option<Value>,
    /// `eo:cloud_cover`, in percent.
    pub cloud: Option<f64>,
    pub assets: Vec<Asset>,
    /// Its own address (its `self` link), which relative asset addresses are resolved against.
    pub url: Option<String>,
}

/// What a search asks for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Search {
    pub collections: Vec<String>,
    /// West, south, east, north, in longitude/latitude.
    pub bbox: Option<[f64; 4]>,
    /// A date or a range: `2024-06-01`, `2024-06-01/2024-06-30`, `2024-06-01/..`, or RFC 3339.
    pub datetime: Option<String>,
    pub limit: usize,
    /// The most cloud cover, in percent.
    pub max_cloud: Option<f64>,
}

/// An API's address from what the user gave: without a trailing `/search` or `/collections`,
/// checked like any address the user names. Empty gives Earth Search.
pub fn api(url: Option<&str>) -> Result<String, String> {
    let Some(u) = url.map(str::trim).filter(|u| !u.is_empty()) else { return Ok(EARTH_SEARCH.to_string()) };
    crate::address::check(u, None)?;
    let u = u.split(['?', '#']).next().unwrap_or(u).trim_end_matches('/');
    Ok(u.strip_suffix("/search").or_else(|| u.strip_suffix("/collections")).unwrap_or(u).to_string())
}

/// The request for an API's collections.
pub fn collections_url(api: &str) -> String {
    format!("{api}/collections")
}

/// The request for a search (STAC API's `GET /search`).
pub fn search_url(api: &str, s: &Search) -> Result<String, String> {
    let mut q = vec![format!("limit={}", s.limit.clamp(1, MAX_ITEMS))];
    let ids: Vec<&str> = s.collections.iter().map(|c| c.trim()).filter(|c| !c.is_empty()).take(50).collect();
    if !ids.is_empty() {
        q.push(format!("collections={}", encode(&ids.join(","))));
    }
    if let Some([w, south, e, n]) = s.bbox {
        let ok = |x: f64, lim: f64| x.is_finite() && (-lim..=lim).contains(&x);
        if !(ok(w, 180.0) && ok(e, 180.0) && ok(south, 90.0) && ok(n, 90.0) && south < n) {
            return Err("bbox is [west, south, east, north] in longitude/latitude".into());
        }
        q.push(format!("bbox={w},{south},{e},{n}"));
    }
    if let Some(d) = s.datetime.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        q.push(format!("datetime={}", encode(&datetime(d)?)));
    }
    if let Some(c) = s.max_cloud.filter(|c| c.is_finite()) {
        let c = c.clamp(0.0, 100.0);
        q.push(format!("query={}", encode(&format!(r#"{{"eo:cloud_cover":{{"lte":{c}}}}}"#))));
    }
    Ok(format!("{api}/search?{}", q.join("&")))
}

/// A date range as STAC asks for it (RFC 3339): a bare date starts or ends its day.
fn datetime(d: &str) -> Result<String, String> {
    if d.len() > 100 || !d.chars().all(|c| c.is_ascii_digit() || "-:.TZtz+/ ".contains(c)) {
        return Err(format!("{d}: give a date (2024-06-01) or a range (2024-06-01/2024-06-30)"));
    }
    let bare = |s: &str| s.len() == 10 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(7) == Some(&b'-');
    let end = |s: &str, end: bool| -> String {
        let s = s.trim();
        if s.is_empty() || s == ".." {
            "..".to_string()
        } else if bare(s) {
            format!("{s}T{}Z", if end { "23:59:59" } else { "00:00:00" })
        } else {
            s.to_string()
        }
    };
    Ok(match d.split_once('/') {
        Some((a, b)) => format!("{}/{}", end(a, false), end(b, true)),
        // One day.
        None if bare(d.trim()) => format!("{}/{}", end(d, false), end(d, true)),
        None => d.trim().to_string(),
    })
}

/// Percent-encodes a query parameter.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").chars().take(MAX_TEXT).collect()
}

fn json(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|_| "the STAC API's answer isn't JSON".to_string())
}

/// The collections in an API's `/collections` answer.
pub fn parse_collections(bytes: &[u8]) -> Result<Vec<Collection>, String> {
    let v = json(bytes)?;
    let list = v.get("collections").and_then(Value::as_array).ok_or("the answer has no collections (is this a STAC API?)")?;
    Ok(list
        .iter()
        .take(MAX_COLLECTIONS)
        .filter_map(|c| {
            let id = text(c.get("id"));
            (!id.is_empty()).then(|| Collection { title: text(c.get("title")), id })
        })
        .collect())
}

/// The items of a search's answer (an ItemCollection), and the address of its next page. `from`
/// is the address the answer came from, which relative addresses are resolved against.
pub fn parse_items(bytes: &[u8], from: &str) -> Result<(Vec<Item>, Option<String>), String> {
    let v = json(bytes)?;
    let features = v.get("features").and_then(Value::as_array).ok_or("the answer isn't a list of STAC items")?;
    let items = features.iter().take(MAX_ITEMS).filter_map(|f| item(f, from)).collect();
    let next = link(&v, "next", from);
    Ok((items, next))
}

/// An item from its own document.
pub fn parse_item(bytes: &[u8], from: &str) -> Result<Item, String> {
    item(&json(bytes)?, from).ok_or_else(|| "the answer isn't a STAC item".to_string())
}

/// An item from its JSON (as STAC gives it, or as [`item_json`] wrote it).
pub fn item_from_json(v: &Value, from: &str) -> Option<Item> {
    item(v, from)
}

/// A link's address by its relation, resolved against `from`.
fn link(v: &Value, rel: &str, from: &str) -> Option<String> {
    let l = v.get("links")?.as_array()?.iter().take(100).find(|l| l.get("rel").and_then(Value::as_str) == Some(rel))?;
    // A POST link needs its body: only a plain GET is followed.
    if l.get("method").and_then(Value::as_str).is_some_and(|m| !m.eq_ignore_ascii_case("GET")) {
        return None;
    }
    resolve(from, l.get("href")?.as_str()?)
}

fn item(f: &Value, from: &str) -> Option<Item> {
    if f.get("type").and_then(Value::as_str) != Some("Feature") {
        return None;
    }
    let id = text(f.get("id"));
    if id.is_empty() {
        return None;
    }
    let url = link(f, "self", from);
    let base = url.clone().unwrap_or_else(|| from.to_string());
    let props = f.get("properties");
    let bbox = f.get("bbox").and_then(Value::as_array).and_then(|b| {
        let n: Vec<f64> = b.iter().filter_map(Value::as_f64).collect();
        // A 3D bbox gives six numbers: west, south, low, east, north, high.
        let [w, s, e, n] = match n.as_slice() {
            [w, s, e, n] => [*w, *s, *e, *n],
            [w, s, _, e, n, _] => [*w, *s, *e, *n],
            _ => return None,
        };
        let ok = |x: f64, lim: f64| x.is_finite() && (-lim..=lim).contains(&x);
        (ok(w, 180.0) && ok(e, 180.0) && ok(s, 90.0) && ok(n, 90.0) && s <= n).then_some([w, s, e, n])
    });
    let geometry = f.get("geometry").filter(|g| g.is_object() && footprint_size(g) <= MAX_FOOTPRINT).cloned();
    let cloud = props.and_then(|p| p.get("eo:cloud_cover")).and_then(Value::as_f64).filter(|c| c.is_finite());
    let datetime = props
        .and_then(|p| p.get("datetime").filter(|d| d.is_string()).or_else(|| p.get("start_datetime")))
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(40)
        .collect();
    let mut assets: Vec<Asset> = f
        .get("assets")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .take(MAX_ASSETS)
        .filter_map(|(k, a)| {
            Some(Asset {
                key: k.chars().take(200).collect(),
                href: resolve(&base, a.get("href")?.as_str()?)?,
                media_type: text(a.get("type")),
                roles: a
                    .get("roles")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .take(20)
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
                title: text(a.get("title")),
            })
        })
        .collect();
    assets.sort_by(|a, b| a.key.cmp(&b.key));
    Some(Item { id, collection: text(f.get("collection")), datetime, bbox, geometry, cloud, assets, url })
}

/// How many numbers a GeoJSON geometry holds (bounded by how deep it goes).
fn footprint_size(g: &Value) -> usize {
    fn count(v: &Value, depth: usize) -> usize {
        match v {
            Value::Array(a) if depth < 6 => a.iter().map(|x| count(x, depth + 1)).sum(),
            Value::Number(_) => 1,
            Value::Array(_) => usize::MAX / 4,
            _ => 0,
        }
    }
    g.get("coordinates").map(|c| count(c, 0)).unwrap_or(0)
}

/// An address relative to another, as a browser would resolve it (`s3://` and others are kept
/// as they are, so they can be refused by name).
pub fn resolve(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() || href.len() > MAX_TEXT || href.chars().any(char::is_control) {
        return None;
    }
    if href.contains("://") {
        return Some(href.to_string());
    }
    let (scheme, rest) = base.split_once("://")?;
    let host_end = rest.find('/').unwrap_or(rest.len());
    let origin = format!("{scheme}://{}", rest.get(..host_end)?);
    if let Some(abs) = href.strip_prefix("//") {
        return Some(format!("{scheme}://{abs}"));
    }
    if href.starts_with('/') {
        return Some(format!("{origin}{href}"));
    }
    // Relative to the base's folder, with `./` and `../` worked out.
    let path = rest.get(host_end..).unwrap_or("").split(['?', '#']).next().unwrap_or("");
    let mut parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if !path.ends_with('/') {
        parts.pop();
    }
    for seg in href.split('/') {
        match seg {
            "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    Some(format!("{origin}/{}", parts.join("/")))
}

impl Asset {
    /// Whether it is a GeoTIFF (cloud-optimised or not).
    pub fn is_geotiff(&self) -> bool {
        let t = self.media_type.to_ascii_lowercase();
        let path = self.href.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
        t.contains("geotiff") || t.starts_with("image/tiff") || path.ends_with(".tif") || path.ends_with(".tiff")
    }
}

impl Item {
    /// The asset `key` names, else the one a map most likely wants: `visual` (a true-colour
    /// image), else the first GeoTIFF, else the first data asset.
    pub fn pick(&self, key: Option<&str>) -> Result<&Asset, String> {
        if let Some(k) = key.map(str::trim).filter(|k| !k.is_empty()) {
            return self.assets.iter().find(|a| a.key == k).ok_or_else(|| {
                let keys: Vec<&str> = self.assets.iter().map(|a| a.key.as_str()).collect();
                format!("{}: no asset '{k}' (it has {})", self.id, keys.join(", "))
            });
        }
        // Among those on the web first (others are refused by name when nothing else is there).
        let web = |a: &&Asset| a.href.to_ascii_lowercase().starts_with("http");
        let choose = |only_web: bool| {
            let ok = |a: &&Asset| !only_web || web(a);
            self.assets
                .iter()
                .filter(ok)
                .find(|a| a.key == "visual")
                .or_else(|| self.assets.iter().filter(ok).find(|a| a.is_geotiff()))
                .or_else(|| self.assets.iter().filter(ok).find(|a| a.roles.iter().any(|r| r == "data")))
        };
        choose(true).or_else(|| choose(false)).ok_or_else(|| format!("{}: no asset RustGIS can add", self.id))
    }

    /// The address of the asset to add (see [`Item::pick`]), checked: a plain web address the item
    /// may point to, not one that needs signing or a cloud store's own protocol.
    pub fn asset_href(&self, key: Option<&str>) -> Result<(String, String), String> {
        let a = self.pick(key)?;
        let lower = a.href.to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            let scheme = lower.split("://").next().unwrap_or("");
            return Err(format!(
                "{}: asset '{}' is at {} (a {scheme}:// address); RustGIS adds assets by their web address",
                self.id, a.key, a.href
            ));
        }
        // Planetary Computer's assets are in Azure Blob Storage and need a signature.
        if lower.contains(".blob.core.windows.net/") && !lower.contains("sig=") {
            return Err(format!(
                "{}: asset '{}' needs signing (Microsoft Planetary Computer); sign it with the Planetary Computer's SAS API and add the signed address with From URL…",
                self.id, a.key
            ));
        }
        crate::address::check(&a.href, self.url.as_deref())?;
        Ok((a.key.clone(), a.href.clone()))
    }
}

/// An item as the commands return it (and read back in `stac.addAsset`).
pub fn item_json(i: &Item) -> Value {
    serde_json::json!({
        "type": "Feature",
        "id": i.id,
        "collection": i.collection,
        "bbox": i.bbox,
        "geometry": i.geometry,
        "properties": {"datetime": i.datetime, "eo:cloud_cover": i.cloud},
        "assets": i.assets.iter().map(|a| (a.key.clone(), serde_json::json!({"href": a.href, "type": a.media_type, "roles": a.roles, "title": a.title}))).collect::<serde_json::Map<_, _>>(),
        "links": i.url.as_ref().map(|u| vec![serde_json::json!({"rel": "self", "href": u})]).unwrap_or_default(),
    })
}

/// A GeoJSON FeatureCollection of items' footprints (their geometry, or their bbox), with the
/// id, collection, date, cloud cover and asset keys of each.
pub fn footprints_geojson(items: &[Item]) -> Value {
    let features: Vec<Value> = items
        .iter()
        .filter_map(|i| {
            let geometry = i.geometry.clone().or_else(|| {
                i.bbox.map(|[w, s, e, n]| serde_json::json!({"type": "Polygon", "coordinates": [[[w, s], [e, s], [e, n], [w, n], [w, s]]]}))
            })?;
            Some(serde_json::json!({
                "type": "Feature",
                "geometry": geometry,
                "properties": {
                    "id": i.id,
                    "collection": i.collection,
                    "datetime": i.datetime,
                    "cloud_cover": i.cloud,
                    "assets": i.assets.iter().map(|a| a.key.as_str()).collect::<Vec<_>>().join(", "),
                },
            }))
        })
        .collect();
    serde_json::json!({"type": "FeatureCollection", "features": features})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests() {
        assert_eq!(api(None).unwrap(), EARTH_SEARCH);
        assert_eq!(api(Some(" https://x.org/stac/v1/search?x=1 ")).unwrap(), "https://x.org/stac/v1");
        assert_eq!(api(Some("https://x.org/stac/collections/")).unwrap(), "https://x.org/stac");
        assert!(api(Some("file:///etc")).is_err() && api(Some("http://169.254.169.254/")).is_err());
        let s = Search {
            collections: vec!["sentinel-2-l2a".into(), " ".into()],
            bbox: Some([7.0, 46.0, 7.1, 46.1]),
            datetime: Some("2024-06-01/2024-06-30".into()),
            limit: 999,
            max_cloud: Some(20.0),
        };
        let u = search_url("https://x.org/v1", &s).unwrap();
        assert_eq!(
            u,
            "https://x.org/v1/search?limit=200&collections=sentinel-2-l2a&bbox=7,46,7.1,46.1&datetime=2024-06-01T00%3A00%3A00Z%2F2024-06-30T23%3A59%3A59Z&query=%7B%22eo%3Acloud_cover%22%3A%7B%22lte%22%3A20%7D%7D"
        );
        assert_eq!(datetime("2024-06-01").unwrap(), "2024-06-01T00:00:00Z/2024-06-01T23:59:59Z");
        assert_eq!(datetime("2024-06-01/..").unwrap(), "2024-06-01T00:00:00Z/..");
        assert!(datetime("yesterday").is_err());
        assert!(search_url("https://x.org", &Search { bbox: Some([0.0, 50.0, 1.0, 40.0]), ..Default::default() }).is_err());
    }

    #[test]
    fn answers() {
        let cols =
            br#"{"collections": [{"id": "sentinel-2-l2a", "title": "Sentinel-2 Level-2A"}, {"title": "no id"}, {"id": "cop-dem-glo-30"}]}"#;
        let c = parse_collections(cols).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].id.as_str(), c[0].title.as_str()), ("sentinel-2-l2a", "Sentinel-2 Level-2A"));
        assert!(parse_collections(b"[]").is_err() && parse_collections(b"<html>").is_err());
        let page = br#"{"type": "FeatureCollection", "features": [
            {"type": "Feature", "id": "S2A_1", "collection": "sentinel-2-l2a", "bbox": [7, 46, 8, 47],
             "geometry": {"type": "Polygon", "coordinates": [[[7, 46], [8, 46], [8, 47], [7, 47], [7, 46]]]},
             "properties": {"datetime": "2024-06-02T10:30:00Z", "eo:cloud_cover": 3.5},
             "links": [{"rel": "self", "href": "/v1/collections/sentinel-2-l2a/items/S2A_1"}],
             "assets": {"visual": {"href": "https://data.org/S2A_1/TCI.tif", "type": "image/tiff; application=geotiff; profile=cloud-optimized", "roles": ["visual"]},
                        "thumbnail": {"href": "./thumb.jpg", "type": "image/jpeg", "roles": ["thumbnail"]},
                        "b04": {"href": "s3://bucket/B04.tif", "type": "image/tiff; application=geotiff", "roles": ["data"]}}},
            {"type": "Feature", "id": "bad", "bbox": [0, 95, 1, 96], "properties": {}},
            {"type": "Nope", "id": "x"}],
            "links": [{"rel": "next", "href": "https://x.org/v1/search?token=abc"}]}"#;
        let (items, next) = parse_items(page, "https://x.org/v1/search?limit=2").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(next.as_deref(), Some("https://x.org/v1/search?token=abc"));
        let i = &items[0];
        assert_eq!((i.datetime.as_str(), i.cloud, i.bbox), ("2024-06-02T10:30:00Z", Some(3.5), Some([7.0, 46.0, 8.0, 47.0])));
        assert_eq!(i.url.as_deref(), Some("https://x.org/v1/collections/sentinel-2-l2a/items/S2A_1"));
        assert_eq!(
            i.assets.iter().find(|a| a.key == "thumbnail").unwrap().href,
            "https://x.org/v1/collections/sentinel-2-l2a/items/thumb.jpg"
        );
        assert_eq!(items[1].bbox, None, "a bbox off the globe is dropped");
        // The asset added: the one named, else visual; a cloud store's address is refused by name.
        assert_eq!(i.asset_href(None).unwrap().0, "visual");
        assert!(i.asset_href(Some("b04")).unwrap_err().contains("s3://"));
        assert!(i.asset_href(Some("nope")).unwrap_err().contains("visual"));
        let mut pc = i.clone();
        pc.assets = vec![Asset {
            key: "B04".into(),
            href: "https://sentinel2l2a01.blob.core.windows.net/x/B04.tif".into(),
            media_type: String::new(),
            roles: vec![],
            title: String::new(),
        }];
        assert!(pc.asset_href(None).unwrap_err().contains("signing"));
        // A web item can't point into the local network.
        let mut local = i.clone();
        local.assets[2].href = "http://192.168.1.2/x.tif".into();
        assert!(local.asset_href(Some("visual")).is_err());
        // Written out and read back.
        let again = item_from_json(&item_json(i), "https://x.org/").unwrap();
        assert_eq!(&again, i);
        assert_eq!(footprints_geojson(&items)["features"].as_array().unwrap().len(), 1);
        assert!(parse_items(b"{}", "https://x.org").is_err());
    }

    #[test]
    fn relative_addresses() {
        assert_eq!(resolve("https://x.org/a/b/item.json", "../c/d.tif").as_deref(), Some("https://x.org/a/c/d.tif"));
        assert_eq!(resolve("https://x.org/a/b/", "./d.tif").as_deref(), Some("https://x.org/a/b/d.tif"));
        assert_eq!(resolve("https://x.org/a/b", "/d.tif").as_deref(), Some("https://x.org/d.tif"));
        assert_eq!(resolve("https://x.org/a", "//y.org/d.tif").as_deref(), Some("https://y.org/d.tif"));
        assert_eq!(resolve("https://x.org/a", "s3://b/k").as_deref(), Some("s3://b/k"));
        assert_eq!(resolve("https://x.org/a", ""), None);
    }
}
