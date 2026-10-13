//! OGC web services: what a service offers, from its capabilities document — WMS and WMTS map
//! layers (as tile templates in Web Mercator), WFS feature types and OGC API Features collections
//! (as GeoJSON addresses) — and ArcGIS REST services (FeatureServer, MapServer, ImageServer,
//! VectorTileServer). The documents are hostile input: no DTD is read, nodes are capped, and
//! only layers RustGIS can draw are listed (the rest are counted in a note).

use roxmltree::{Document, Node, ParsingOptions};

/// What a layer of a service becomes.
#[derive(Clone, Debug, PartialEq)]
pub enum Offer {
    /// Map tiles: an XYZ template (`{z}/{x}/{y}`, or `{bbox-epsg-3857}` for WMS).
    Tiles { template: String },
    /// Features: an address answering GeoJSON.
    Features { url: String },
    /// A MapLibre style document (an ArcGIS vector tile service's).
    Style { url: String },
}

/// A layer a service offers.
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceLayer {
    /// The name the service knows it by.
    pub name: String,
    pub title: String,
    pub offer: Offer,
    /// Its longitude/latitude bounds (west, south, east, north), when the service says.
    pub bbox: Option<[f64; 4]>,
}

/// A service's capabilities: its kind (`WMS`, `WMTS`, `WFS`, `OGC API Features`), title, the
/// layers RustGIS can add, and notes on what it left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Service {
    pub kind: String,
    pub title: String,
    pub layers: Vec<ServiceLayer>,
    pub notes: Vec<String>,
}

/// Most layers listed from one service.
const MAX_LAYERS: usize = 2_000;
/// Features asked of a WFS in one request.
pub const FEATURE_LIMIT: usize = 50_000;
/// Features asked of an OGC API in one request: servers cap their pages (often at 10,000) and
/// some refuse a larger `limit` outright.
pub const OGC_API_LIMIT: usize = 10_000;

/// The address to fetch a service's capabilities from: OGC API landing pages and collection lists
/// are JSON; WMS, WMTS and WFS addresses get a GetCapabilities request unless they have one.
pub fn capabilities_url(url: &str, kind: Option<&str>) -> String {
    // ArcGIS REST: the service's JSON description.
    if kind.is_some_and(|k| k.eq_ignore_ascii_case("arcgis")) || (kind.is_none() && is_arcgis(url)) {
        return if query_value(url, "f").is_some_and(|f| f.eq_ignore_ascii_case("json") || f.eq_ignore_ascii_case("pjson")) {
            url.to_string()
        } else {
            with_query(url, "f=json")
        };
    }
    if query_value(url, "request").is_some_and(|r| r.eq_ignore_ascii_case("getcapabilities"))
        || base(url).to_ascii_lowercase().ends_with(".xml")
    {
        return url.to_string();
    }
    // The guess reads the path and query, never the host (`rapid.example.org` isn't an API).
    let rest = after_host(url).to_ascii_lowercase();
    let segments: Vec<&str> = base(&rest).split('/').filter(|s| !s.is_empty()).collect();
    let service = match kind.map(str::to_ascii_lowercase).as_deref() {
        Some("wms") => "WMS",
        Some("wmts") => "WMTS",
        Some("wfs") => "WFS",
        Some("ogcapi") | Some("features") => return with_query(url, "f=json"),
        _ if rest.contains("wmts") => "WMTS",
        _ if rest.contains("wfs") => "WFS",
        _ if rest.contains("wms") => "WMS",
        // An OGC API (`…/collections`, `…/ogc/features`, `…/api/…`): asked as JSON.
        _ if segments.iter().any(|s| matches!(*s, "collections" | "api" | "ogcapi" | "ogc")) => return with_query(url, "f=json"),
        // Anything else is taken as a WMS (the commonest service).
        _ => "WMS",
    };
    with_query(url, &format!("SERVICE={service}&REQUEST=GetCapabilities"))
}

/// Whether an address is an ArcGIS REST service (or its directory), not one of its OGC endpoints
/// (`…/MapServer/WMSServer`, `…/MapServer/WMTS`), which are read as WMS and WMTS.
fn is_arcgis(url: &str) -> bool {
    let b = base(url).trim_end_matches('/').to_ascii_lowercase();
    if ["/wmsserver", "/wfsserver", "/wmts"].iter().any(|k| b.contains(k)) {
        return false;
    }
    b.contains("/rest/services") || ["featureserver", "mapserver", "imageserver", "vectortileserver"].iter().any(|k| b.ends_with(k))
}

/// The part of an address after its host: the path, query and fragment.
fn after_host(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.find(['/', '?', '#']).and_then(|i| rest.get(i..)).unwrap_or("")
}

/// The scheme and host of an address (`https://x.org`).
fn origin(url: &str) -> &str {
    let host_at = url.find("://").map(|i| i + 3).unwrap_or(0);
    let end = url.get(host_at..).and_then(|r| r.find(['/', '?', '#'])).map(|i| host_at + i).unwrap_or(url.len());
    url.get(..end).unwrap_or(url)
}

/// The value of a query parameter, by its whole name (any case).
fn query_value<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let q = url.split('#').next()?.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        k.eq_ignore_ascii_case(key).then_some(v)
    })
}

/// Whether an address is a web address (`http://` or `https://`), not another scheme that only
/// starts with those letters.
fn is_web(url: &str) -> bool {
    let l = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

/// A link from a document at `from`, resolved: absolute, scheme-relative (`//host/…`),
/// host-relative (`/…`) or relative to the document's folder. `None` for other schemes.
fn resolve(from: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if is_web(href) {
        return Some(href.to_string());
    }
    if href.contains("://") || href.starts_with("data:") || href.starts_with("javascript:") || href.is_empty() {
        return None;
    }
    if let Some(rest) = href.strip_prefix("//") {
        let scheme = from.split_once("://").map(|(s, _)| s).unwrap_or("https");
        return Some(format!("{scheme}://{rest}"));
    }
    if href.starts_with('/') {
        return Some(format!("{}{href}", origin(from)));
    }
    let b = base(from);
    let dir = if after_host(b).is_empty() {
        format!("{b}/")
    } else {
        b.get(..b.rfind('/').map(|i| i + 1).unwrap_or(b.len())).unwrap_or(b).to_string()
    };
    Some(format!("{dir}{href}"))
}

fn with_query(url: &str, q: &str) -> String {
    let (base, frag) = url.split_once('#').map(|(b, f)| (b, Some(f))).unwrap_or((url, None));
    let sep = if base.contains('?') { if base.ends_with('?') || base.ends_with('&') { "" } else { "&" } } else { "?" };
    match frag {
        Some(f) => format!("{base}{sep}{q}#{f}"),
        None => format!("{base}{sep}{q}"),
    }
}

/// The part of an address before its query.
fn base(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// Reads a capabilities document (XML or OGC API JSON) fetched from `url`.
pub fn parse(url: &str, body: &[u8]) -> Result<Service, String> {
    let text = std::str::from_utf8(body).map_err(|_| "the service's answer isn't UTF-8 text".to_string())?;
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if text.starts_with('{') || text.starts_with('[') {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("the service's JSON: {e}"))?;
        if let Some(e) = v.get("error") {
            return Err(format!(
                "the service answered with an error: {}",
                e.get("message").and_then(|m| m.as_str()).unwrap_or("(no message)")
            ));
        }
        if is_arcgis(url) || v.get("currentVersion").is_some() {
            return arcgis(url, &v);
        }
        return ogc_api(url, &v);
    }
    // WMS 1.1.1 documents start with a DOCTYPE, so DTDs are allowed here; roxmltree still refuses
    // entity bombs and external entities, and the node count is capped.
    let opts = ParsingOptions { allow_dtd: true, nodes_limit: 5_000_000, ..ParsingOptions::default() };
    let doc = Document::parse_with_options(text, opts).map_err(|e| format!("the service's XML: {e}"))?;
    let root = doc.root_element();
    match local(&root) {
        "WMS_Capabilities" | "WMT_MS_Capabilities" => Ok(wms(url, &root)),
        "Capabilities" if child(&root, "Contents").is_some() => Ok(wmts(url, &root)),
        "WFS_Capabilities" => Ok(wfs(url, &root)),
        "ServiceExceptionReport" | "ExceptionReport" => {
            Err(format!("the service answered with an error: {}", text_of(&root).chars().take(300).collect::<String>()))
        }
        other => Err(format!("not a WMS, WMTS or WFS capabilities document (<{other}>)")),
    }
}

fn local<'a>(n: &Node<'a, '_>) -> &'a str {
    n.tag_name().name()
}

fn child<'a, 'i>(n: &Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && local(c) == name)
}

fn children<'a, 'i>(n: &Node<'a, 'i>, name: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(move |c| c.is_element() && local(c) == name)
}

fn text_of(n: &Node) -> String {
    n.descendants().filter(|c| c.is_text()).filter_map(|c| c.text()).collect::<String>().trim().to_string()
}

fn child_text(n: &Node, name: &str) -> Option<String> {
    child(n, name).map(|c| text_of(&c)).filter(|t| !t.is_empty())
}

fn href(n: &Node) -> Option<String> {
    n.attributes().find(|a| a.name() == "href").map(|a| a.value().to_string())
}

/// A finite longitude/latitude box.
fn lonlat_box(w: f64, s: f64, e: f64, n: f64) -> Option<[f64; 4]> {
    ([w, s, e, n].iter().all(|v| v.is_finite()) && w < e && s < n && w >= -180.5 && e <= 180.5 && s >= -90.5 && n <= 90.5)
        .then_some([w, s, e, n])
}

/// Codes a server uses for Web Mercator.
fn is_web_mercator(code: &str) -> bool {
    let c = code.to_ascii_uppercase();
    ["EPSG:3857", "EPSG:900913", "EPSG:102100", "EPSG:102113", "EPSG::3857", "OSGEO:41001"].iter().any(|k| c.contains(k))
        || c.ends_with(":3857")
}

// ---------------------------------------------------------------------------------------------
// WMS

fn wms(url: &str, root: &Node) -> Service {
    let v13 = root.attribute("version").is_none_or(|v| v.starts_with("1.3"));
    let title = child(root, "Service").and_then(|s| child_text(&s, "Title")).unwrap_or_default();
    let get_map = root
        .descendants()
        .find(|n| n.is_element() && local(n) == "GetMap")
        .and_then(|g| g.descendants().find(|n| n.is_element() && local(n) == "Get"))
        .and_then(|g| child(&g, "OnlineResource"))
        .and_then(|o| href(&o))
        .filter(|h| is_web(h))
        .unwrap_or_else(|| base(url).to_string());
    let formats: Vec<String> = root
        .descendants()
        .find(|n| n.is_element() && local(n) == "GetMap")
        .map(|g| children(&g, "Format").map(|f| text_of(&f)).collect())
        .unwrap_or_default();
    let format = ["image/png", "image/png8", "image/jpeg"]
        .iter()
        .find(|f| formats.iter().any(|g| g.eq_ignore_ascii_case(f)))
        .copied()
        .unwrap_or("image/png");
    let mut service = Service { kind: "WMS".into(), title, ..Service::default() };
    let mut skipped = 0;
    // Layers inherit their parents' coordinate systems.
    let mut stack: Vec<(Node, Vec<String>)> = root
        .descendants()
        .filter(|n| n.is_element() && local(n) == "Capability")
        .flat_map(|c| children(&c, "Layer").collect::<Vec<_>>())
        .map(|l| (l, Vec::new()))
        .collect();
    while let Some((layer, inherited)) = stack.pop() {
        if service.layers.len() >= MAX_LAYERS {
            break;
        }
        let mut crs = inherited.clone();
        crs.extend(layer.children().filter(|c| c.is_element() && matches!(local(c), "CRS" | "SRS")).map(|c| text_of(&c)));
        for sub in children(&layer, "Layer") {
            stack.push((sub, crs.clone()));
        }
        let Some(name) = child_text(&layer, "Name") else { continue };
        if !crs.iter().any(|c| c.split_whitespace().any(is_web_mercator)) {
            skipped += 1;
            continue;
        }
        let bbox = child(&layer, "EX_GeographicBoundingBox")
            .and_then(|b| {
                let f = |k: &str| child_text(&b, k).and_then(|t| t.parse::<f64>().ok());
                lonlat_box(f("westBoundLongitude")?, f("southBoundLatitude")?, f("eastBoundLongitude")?, f("northBoundLatitude")?)
            })
            .or_else(|| {
                let b = child(&layer, "LatLonBoundingBox")?;
                let f = |k: &str| b.attribute(k).and_then(|t| t.parse::<f64>().ok());
                lonlat_box(f("minx")?, f("miny")?, f("maxx")?, f("maxy")?)
            });
        let crs_key = if v13 { "CRS" } else { "SRS" };
        let version = if v13 { "1.3.0" } else { "1.1.1" };
        let template = with_query(
            &get_map,
            &format!(
                "SERVICE=WMS&VERSION={version}&REQUEST=GetMap&LAYERS={}&STYLES=&{crs_key}=EPSG:3857&BBOX={{bbox-epsg-3857}}&WIDTH=256&HEIGHT=256&FORMAT={}&TRANSPARENT=TRUE",
                encode(&name),
                encode(format)
            ),
        );
        let title = child_text(&layer, "Title").unwrap_or_else(|| name.clone());
        service.layers.push(ServiceLayer { name, title, offer: Offer::Tiles { template }, bbox });
    }
    service.layers.reverse();
    if skipped > 0 {
        service.notes.push(format!("{skipped} layers aren't offered in Web Mercator (EPSG:3857) and are left out"));
    }
    service
}

/// Percent-encodes a query value.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' | b',' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// WMTS

/// The scale denominator of Web Mercator zoom 0 for 256-pixel tiles (OGC's 0.28 mm pixels).
const Z0_SCALE: f64 = 559_082_264.028_717_8;

/// A tile matrix set's zoom levels, when it is Web Mercator's own pyramid: each matrix's
/// identifier and its zoom.
fn mercator_matrices(tms: &Node) -> Option<Vec<(String, u8)>> {
    let crs = child_text(tms, "SupportedCRS")?;
    if !is_web_mercator(&crs) {
        return None;
    }
    let mut out = Vec::new();
    for m in children(tms, "TileMatrix") {
        let id = child_text(&m, "Identifier")?;
        let sd = child_text(&m, "ScaleDenominator")?.parse::<f64>().ok()?;
        let w = child_text(&m, "TileWidth").and_then(|t| t.parse::<u32>().ok()).unwrap_or(256);
        // Zoom from the scale (tiles of 512 px are a zoom finer for the same scale).
        let z = (Z0_SCALE / sd * f64::from(w) / 256.0).log2();
        let zr = z.round();
        if (z - zr).abs() > 0.02 || !(0.0..=30.0).contains(&zr) {
            return None;
        }
        // The pyramid's corner must be Web Mercator's.
        let corner: Vec<f64> = child_text(&m, "TopLeftCorner")?.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        let ok = |a: f64, b: f64| (a.abs() - b).abs() < 1.0;
        if corner.len() != 2 || !corner.iter().all(|c| ok(*c, 20_037_508.342_789_244)) {
            return None;
        }
        out.push((id, zr as u8));
    }
    (!out.is_empty()).then_some(out)
}

fn wmts(url: &str, root: &Node) -> Service {
    let title = child(root, "ServiceIdentification").and_then(|s| child_text(&s, "Title")).unwrap_or_default();
    let mut service = Service { kind: "WMTS".into(), title, ..Service::default() };
    let Some(contents) = child(root, "Contents") else { return service };
    // Tile matrix sets that are Web Mercator pyramids, with a zoom per matrix identifier.
    let sets: Vec<(String, Vec<(String, u8)>)> =
        children(&contents, "TileMatrixSet").filter_map(|t| Some((child_text(&t, "Identifier")?, mercator_matrices(&t)?))).collect();
    let kvp = root
        .descendants()
        .find(|n| n.is_element() && local(n) == "GetTile")
        .and_then(|g| g.descendants().find(|n| n.is_element() && local(n) == "Get"))
        .and_then(|g| href(&g))
        .filter(|h| is_web(h))
        .unwrap_or_else(|| base(url).to_string());
    let mut skipped = 0;
    for layer in children(&contents, "Layer").take(MAX_LAYERS) {
        let Some(name) = child_text(&layer, "Identifier") else { continue };
        let title = child_text(&layer, "Title").unwrap_or_else(|| name.clone());
        let style = children(&layer, "Style")
            .find(|s| s.attribute("isDefault") == Some("true"))
            .or_else(|| child(&layer, "Style"))
            .and_then(|s| child_text(&s, "Identifier"))
            .unwrap_or_else(|| "default".into());
        let linked: Vec<String> = children(&layer, "TileMatrixSetLink").filter_map(|l| child_text(&l, "TileMatrixSet")).collect();
        let Some((set, matrices)) = sets.iter().find(|(id, _)| linked.contains(id)) else {
            skipped += 1;
            continue;
        };
        // A template needs the zoom in the matrix identifier: a fixed prefix then the zoom.
        let prefix = matrices.iter().find_map(|(id, z)| id.strip_suffix(&z.to_string()).map(str::to_string));
        let consistent = prefix.as_ref().is_some_and(|p| matrices.iter().all(|(id, z)| *id == format!("{p}{z}")));
        let Some(prefix) = prefix.filter(|_| consistent) else {
            skipped += 1;
            continue;
        };
        let matrix = format!("{prefix}{{z}}");
        let resource = children(&layer, "ResourceURL").find(|r| r.attribute("resourceType") == Some("tile")).and_then(|r| {
            let f = r.attribute("format").unwrap_or("");
            (!f.contains("pbf") && !f.contains("mvt")).then(|| r.attribute("template").filter(|t| is_web(t)).map(str::to_string)).flatten()
        });
        let template = match resource {
            Some(t) => t
                .replace("{TileMatrixSet}", set)
                .replace("{Style}", &style)
                .replace("{TileMatrix}", &matrix)
                .replace("{TileRow}", "{y}")
                .replace("{TileCol}", "{x}"),
            None => {
                let format =
                    children(&layer, "Format").map(|f| text_of(&f)).find(|f| f.starts_with("image/")).unwrap_or_else(|| "image/png".into());
                with_query(
                    &kvp,
                    &format!(
                        "SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER={}&STYLE={}&TILEMATRIXSET={}&TILEMATRIX={matrix}&TILEROW={{y}}&TILECOL={{x}}&FORMAT={}",
                        encode(&name),
                        encode(&style),
                        encode(set),
                        encode(&format)
                    ),
                )
            }
        };
        let bbox = child(&layer, "WGS84BoundingBox").and_then(|b| {
            let lo: Vec<f64> = child_text(&b, "LowerCorner")?.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            let hi: Vec<f64> = child_text(&b, "UpperCorner")?.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            lonlat_box(*lo.first()?, *lo.get(1)?, *hi.first()?, *hi.get(1)?)
        });
        service.layers.push(ServiceLayer { name, title, offer: Offer::Tiles { template }, bbox });
    }
    if skipped > 0 {
        service.notes.push(format!("{skipped} layers have no Web Mercator tile matrix set RustGIS can draw and are left out"));
    }
    service
}

// ---------------------------------------------------------------------------------------------
// WFS

/// The output formats an `outputFormat` parameter lists among `scope`'s nodes.
fn output_formats<'a, 'i: 'a>(scope: impl Iterator<Item = Node<'a, 'i>>) -> Vec<String> {
    scope
        .filter(|n| {
            n.is_element() && local(n) == "Parameter" && n.attribute("name").is_some_and(|a| a.eq_ignore_ascii_case("outputFormat"))
        })
        .flat_map(|p| p.descendants().filter(|v| v.is_element() && local(v) == "Value").map(|v| text_of(&v)).collect::<Vec<_>>())
        .collect()
}

/// Whether a coordinate system code is OGC's CRS84 (longitude/latitude by definition).
fn is_crs84(code: &str) -> bool {
    let c = code.to_ascii_uppercase();
    c.contains("CRS84") || c.contains("CRS:84")
}

fn wfs(url: &str, root: &Node) -> Service {
    let title =
        child(root, "ServiceIdentification").or_else(|| child(root, "Service")).and_then(|s| child_text(&s, "Title")).unwrap_or_default();
    let mut service = Service { kind: "WFS".into(), title, ..Service::default() };
    let version = root.attribute("version").unwrap_or("1.1.0");
    // GetFeature's address: an `xlink:href` (WFS 1.1, 2.0) or an `onlineResource` attribute
    // (WFS 1.0); the capabilities address when the document gives no web address.
    let get = root
        .descendants()
        .find(|n| {
            n.is_element()
                && matches!(local(n), "Operation" | "GetFeature")
                && (local(n) == "GetFeature" || n.attribute("name") == Some("GetFeature"))
        })
        .and_then(|g| g.descendants().find(|n| n.is_element() && local(n) == "Get"))
        .and_then(|g| href(&g).or_else(|| g.attribute("onlineResource").map(str::to_string)))
        .filter(|h| is_web(h))
        .unwrap_or_else(|| base(url).to_string());
    // The GeoJSON output format the server names (GetFeature's, else the service-wide list);
    // `application/json` when it lists none.
    let formats: Vec<String> = {
        let op = root.descendants().find(|n| n.is_element() && local(n) == "Operation" && n.attribute("name") == Some("GetFeature"));
        let in_op = output_formats(op.iter().flat_map(|o| o.descendants()));
        if in_op.is_empty() { output_formats(root.descendants()) } else { in_op }
    };
    let json = if formats.is_empty() {
        Some("application/json".to_string())
    } else {
        formats.iter().find(|f| f.to_ascii_lowercase().contains("json")).cloned()
    };
    let Some(json) = json else {
        service.notes.push("the service offers no GeoJSON output, so its feature types can't be added".into());
        return service;
    };
    let Some(list) = child(root, "FeatureTypeList") else { return service };
    for ft in children(&list, "FeatureType").take(MAX_LAYERS) {
        let Some(name) = child_text(&ft, "Name") else { continue };
        let title = child_text(&ft, "Title").unwrap_or_else(|| name.clone());
        let bbox = child(&ft, "WGS84BoundingBox")
            .and_then(|b| {
                let lo: Vec<f64> = child_text(&b, "LowerCorner")?.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                let hi: Vec<f64> = child_text(&b, "UpperCorner")?.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                lonlat_box(*lo.first()?, *lo.get(1)?, *hi.first()?, *hi.get(1)?)
            })
            .or_else(|| {
                let b = child(&ft, "LatLongBoundingBox")?;
                let f = |k: &str| b.attribute(k).and_then(|t| t.parse::<f64>().ok());
                lonlat_box(f("minx")?, f("miny")?, f("maxx")?, f("maxy")?)
            });
        // CRS84 when the type offers it: longitude first by definition. Otherwise EPSG:4326, which
        // WFS 1.0 answers longitude first; a 1.1 or 2.0 server answering latitude first says so
        // with a `urn:…EPSG::4326` crs in its GeoJSON, which is turned round (`fix_axes`).
        let crs84 = ft.children().any(|c| {
            c.is_element() && matches!(local(&c), "DefaultCRS" | "OtherCRS" | "DefaultSRS" | "OtherSRS" | "SRS") && is_crs84(&text_of(&c))
        });
        let srs = if crs84 { "urn:ogc:def:crs:OGC:1.3:CRS84" } else { "EPSG:4326" };
        // WFS 2.0 names the type and the count differently.
        let query = if version.starts_with('2') {
            format!(
                "SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES={}&OUTPUTFORMAT={}&SRSNAME={srs}&COUNT={FEATURE_LIMIT}",
                encode(&name),
                encode(&json)
            )
        } else {
            let v = if version.starts_with("1.0") { "1.0.0" } else { "1.1.0" };
            format!(
                "SERVICE=WFS&VERSION={v}&REQUEST=GetFeature&TYPENAME={}&OUTPUTFORMAT={}&SRSNAME={srs}&MAXFEATURES={FEATURE_LIMIT}",
                encode(&name),
                encode(&json)
            )
        };
        let url = with_query(&get, &query);
        service.layers.push(ServiceLayer { name, title, offer: Offer::Features { url }, bbox });
    }
    service
}

// ---------------------------------------------------------------------------------------------
// ArcGIS REST

/// Web Mercator metres to longitude/latitude.
fn merc_to_lonlat(x: f64, y: f64) -> (f64, f64) {
    const R: f64 = 6_378_137.0;
    ((x / R).to_degrees(), (2.0 * (y / R).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees())
}

/// An ArcGIS extent (`{xmin, ymin, xmax, ymax, spatialReference}`) in longitude/latitude, for
/// WGS 84 and Web Mercator extents.
fn arcgis_extent(e: &serde_json::Value) -> Option<[f64; 4]> {
    let f = |k: &str| e.get(k).and_then(|v| v.as_f64());
    let (x0, y0, x1, y1) = (f("xmin")?, f("ymin")?, f("xmax")?, f("ymax")?);
    let wkid = e.pointer("/spatialReference/latestWkid").or_else(|| e.pointer("/spatialReference/wkid")).and_then(|w| w.as_i64())?;
    match wkid {
        4326 => lonlat_box(x0, y0, x1, y1),
        3857 | 102100 | 102113 | 900913 => {
            let (w, s) = merc_to_lonlat(x0, y0);
            let (e, n) = merc_to_lonlat(x1, y1);
            lonlat_box(w.max(-180.0), s.max(-85.06), e.min(180.0), n.min(85.06))
        }
        _ => None,
    }
}

/// Whether a cached map service's tiles are Web Mercator's own pyramid (zoom 0 at 156543 m/px).
fn arcgis_mercator_cache(v: &serde_json::Value) -> bool {
    let Some(ti) = v.get("tileInfo") else { return false };
    let wkid = ti.pointer("/spatialReference/latestWkid").or_else(|| ti.pointer("/spatialReference/wkid")).and_then(|w| w.as_i64());
    let first = ti.pointer("/lods/0/resolution").and_then(|r| r.as_f64());
    let level0 = ti.pointer("/lods/0/level").and_then(|l| l.as_i64());
    let rows = ti.get("rows").and_then(|r| r.as_i64()).unwrap_or(256);
    matches!(wkid, Some(3857 | 102100 | 102113))
        && rows == 256
        && level0 == Some(0)
        && first.is_some_and(|r| (r - 156_543.033_928_041).abs() < 0.01)
}

fn arcgis(url: &str, v: &serde_json::Value) -> Result<Service, String> {
    let b = base(url).trim_end_matches('/').to_string();
    let lower = b.to_ascii_lowercase();
    let title = ["mapName", "name", "serviceDescription"]
        .iter()
        // "Layers" is ArcGIS's default map name, which says nothing.
        .find_map(|k| v.get(*k).and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty() && *t != "Layers"))
        .or_else(|| v.pointer("/documentInfo/Title").and_then(|t| t.as_str()).filter(|t| !t.is_empty()))
        .map(|t| t.chars().take(200).collect())
        .unwrap_or_else(|| b.rsplit('/').nth(1).unwrap_or("ArcGIS").to_string());
    let full = v.get("fullExtent").or_else(|| v.get("extent")).and_then(arcgis_extent);
    let mut service = Service { kind: "ArcGIS".into(), title: title.clone(), ..Service::default() };
    // A directory of services: say which to open.
    if v.get("services").is_some() && !lower.ends_with("server") {
        let names: Vec<String> = v
            .get("services")
            .and_then(|s| s.as_array())
            .map(|a| a.iter().filter_map(|s| Some(format!("{}/{}", s.get("name")?.as_str()?, s.get("type")?.as_str()?))).take(30).collect())
            .unwrap_or_default();
        return Err(format!("an ArcGIS services directory: open one of its services ({})", names.join(", ")));
    }
    // Feature layers (FeatureServer, and a MapServer's layers): queried as GeoJSON.
    let features = |service: &mut Service| {
        for l in v.get("layers").and_then(|l| l.as_array()).into_iter().flatten().take(MAX_LAYERS) {
            let (Some(id), Some(name)) = (l.get("id").and_then(|i| i.as_i64()), l.get("name").and_then(|n| n.as_str())) else { continue };
            // Group layers have sub-layers and no features of their own.
            if l.get("subLayerIds").is_some_and(|s| s.as_array().is_some_and(|a| !a.is_empty())) {
                continue;
            }
            let url = format!("{b}/{id}/query?where=1%3D1&outFields=*&returnGeometry=true&outSR=4326&f=geojson");
            service.layers.push(ServiceLayer { name: id.to_string(), title: name.to_string(), offer: Offer::Features { url }, bbox: full });
        }
    };
    if lower.ends_with("/featureserver") {
        service.kind = "ArcGIS FeatureServer".into();
        features(&mut service);
    } else if lower.ends_with("/mapserver") {
        service.kind = "ArcGIS MapServer".into();
        // The whole map: its cache when it is Web Mercator's pyramid, else exported images.
        let template = if v.get("singleFusedMapCache").and_then(|c| c.as_bool()) == Some(true) && arcgis_mercator_cache(v) {
            format!("{b}/tile/{{z}}/{{y}}/{{x}}")
        } else {
            format!("{b}/export?bbox={{bbox-epsg-3857}}&bboxSR=3857&imageSR=3857&size=256,256&format=png32&transparent=true&f=image")
        };
        service.layers.push(ServiceLayer {
            name: "map".into(),
            title: format!("{title} (map)"),
            offer: Offer::Tiles { template },
            bbox: full,
        });
        features(&mut service);
    } else if lower.ends_with("/imageserver") {
        service.kind = "ArcGIS ImageServer".into();
        let template = format!("{b}/exportImage?bbox={{bbox-epsg-3857}}&bboxSR=3857&imageSR=3857&size=256,256&format=png&f=image");
        service.layers.push(ServiceLayer { name: "image".into(), title: title.clone(), offer: Offer::Tiles { template }, bbox: full });
    } else if lower.ends_with("/vectortileserver") {
        service.kind = "ArcGIS VectorTileServer".into();
        let style = v
            .get("defaultStyles")
            .and_then(|s| s.as_str())
            .map(|s| format!("{b}/{}/root.json", s.trim_matches('/')))
            .unwrap_or_else(|| format!("{b}/resources/styles/root.json"));
        service.layers.push(ServiceLayer { name: "style".into(), title: title.clone(), offer: Offer::Style { url: style }, bbox: full });
    } else {
        return Err("an ArcGIS address, but not a FeatureServer, MapServer, ImageServer or VectorTileServer".into());
    }
    // A feature layer is one request: the server's record limit bounds it (a cut-off answer is
    // pointed out when it lands).
    if let Some(max) = v.get("maxRecordCount").and_then(|m| m.as_i64())
        && service.layers.iter().any(|l| matches!(l.offer, Offer::Features { .. }))
    {
        service.notes.push(format!("a feature layer is read up to the server's {max} features per request"));
    }
    Ok(service)
}

// ---------------------------------------------------------------------------------------------
// OGC API Features

fn ogc_api(url: &str, v: &serde_json::Value) -> Result<Service, String> {
    let collections = v.get("collections").and_then(|c| c.as_array());
    let Some(collections) = collections else {
        // A landing page: say where the collections are.
        let has = v
            .get("links")
            .and_then(|l| l.as_array())
            .is_some_and(|l| l.iter().any(|l| l.get("rel").and_then(|r| r.as_str()) == Some("data")));
        return Err(if has {
            format!("{url} is an OGC API landing page: add its /collections address")
        } else {
            "not an OGC API collections document (no \"collections\")".into()
        });
    };
    let title = v.get("title").and_then(|t| t.as_str()).unwrap_or_default().to_string();
    let mut service = Service { kind: "OGC API Features".into(), title, ..Service::default() };
    let root = base(url).trim_end_matches('/').to_string();
    let mut unreadable = 0;
    for c in collections.iter().take(MAX_LAYERS) {
        let Some(id) = c.get("id").and_then(|i| i.as_str()) else { continue };
        let links = c.get("links").and_then(|l| l.as_array()).cloned().unwrap_or_default();
        let link = links
            .iter()
            .filter(|l| l.get("rel").and_then(|r| r.as_str()) == Some("items"))
            .find(|l| l.get("type").and_then(|t| t.as_str()).is_none_or(|t| t.contains("json")));
        let declared_geojson = link.and_then(|l| l.get("type").and_then(|t| t.as_str())).is_some_and(|t| t.contains("geo+json"));
        // Without an items link: the standard path under the collections address. A relative
        // link is resolved against the document's address.
        let items = match link.and_then(|l| l.get("href").and_then(|h| h.as_str())) {
            Some(h) => match resolve(url, h) {
                Some(u) => u,
                None => {
                    unreadable += 1;
                    continue;
                }
            },
            None => format!("{root}/{}/items", encode(id)),
        };
        let items = if query_value(&items, "limit").is_some() { items } else { with_query(&items, &format!("limit={OGC_API_LIMIT}")) };
        // GeoJSON asked for unless the link says it is already.
        let items = if declared_geojson || query_value(&items, "f").is_some() { items } else { with_query(&items, "f=json") };
        let title = c.get("title").and_then(|t| t.as_str()).unwrap_or(id).to_string();
        let bbox = c.pointer("/extent/spatial/bbox/0").and_then(|b| b.as_array()).and_then(|b| {
            let f = |i: usize| b.get(i).and_then(|v| v.as_f64());
            lonlat_box(f(0)?, f(1)?, f(2)?, f(3)?)
        });
        service.layers.push(ServiceLayer { name: id.to_string(), title, offer: Offer::Features { url: items }, bbox });
    }
    if unreadable > 0 {
        service.notes.push(format!("{unreadable} collections link to their items with an address RustGIS doesn't fetch and are left out"));
    }
    if !service.layers.is_empty() {
        service.notes.push(format!("a collection is read up to {OGC_API_LIMIT} features (one page of the server's)"));
    }
    Ok(service)
}

// ---------------------------------------------------------------------------------------------
// Feature answers

/// How much of an answer's head and tail is searched for its paging members (they sit at the
/// start or the end of a feature collection).
const EDGE: usize = 64 * 1024;

/// The head and tail of an answer, as text.
fn edges(bytes: &[u8]) -> (String, String) {
    let head = bytes.get(..bytes.len().min(EDGE)).unwrap_or(bytes);
    let tail = bytes.get(bytes.len().saturating_sub(EDGE)..).unwrap_or(bytes);
    (String::from_utf8_lossy(head).into_owned(), String::from_utf8_lossy(tail).into_owned())
}

/// The number after a JSON key (`"numberMatched": 1234`), in `text`.
fn number_after(text: &str, key: &str) -> Option<u64> {
    let at = text.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = text.get(at..)?.trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Whether `text` holds `"key": true`.
fn is_true(text: &str, key: &str) -> bool {
    text.match_indices(&format!("\"{key}\"")).any(|(i, m)| {
        text.get(i + m.len()..).and_then(|r| r.trim_start().strip_prefix(':')).is_some_and(|r| r.trim_start().starts_with("true"))
    })
}

/// A note when a service's GeoJSON answer is only part of what it holds: an ArcGIS layer past its
/// record limit (`exceededTransferLimit`), or an OGC API or WFS 2.0 page (`numberMatched` above
/// `numberReturned`, or a `next` link).
pub fn partial_note(bytes: &[u8]) -> Option<String> {
    let (head, tail) = edges(bytes);
    if is_true(&head, "exceededTransferLimit") || is_true(&tail, "exceededTransferLimit") {
        return Some("the server sent its first features only (its record limit); the rest aren't read".into());
    }
    let n = |k: &str| number_after(&head, k).or_else(|| number_after(&tail, k));
    if let (Some(matched), Some(returned)) = (n("numberMatched"), n("numberReturned"))
        && matched > returned
    {
        return Some(format!("the server sent {returned} of its {matched} features (one page); the rest aren't read"));
    }
    let next = |t: &str| t.contains("\"rel\":\"next\"") || t.contains("\"rel\": \"next\"");
    (next(&head) || next(&tail)).then(|| "the server sent one page of its features; the rest aren't read".into())
}

/// Whether a GeoJSON answer names, as its `crs`, EPSG:4326 in the URN form that puts latitude
/// first (as GeoServer does for WFS 1.1 and 2.0).
fn lat_first(crs: &str) -> bool {
    let c = crs.to_ascii_lowercase();
    c.starts_with("urn:") && (c.ends_with("epsg::4326") || c.ends_with("epsg:6.6:4326") || c.ends_with("epsg:4326"))
}

fn swap_positions(v: &mut serde_json::Value, depth: usize) {
    if depth > 8 {
        return;
    }
    let Some(a) = v.as_array_mut() else { return };
    if a.len() >= 2 && a.iter().take(2).all(serde_json::Value::is_number) {
        a.swap(0, 1);
        return;
    }
    for c in a {
        swap_positions(c, depth + 1);
    }
}

fn swap_geometry(g: &mut serde_json::Value, depth: usize) {
    if depth > 8 {
        return;
    }
    if let Some(c) = g.get_mut("coordinates") {
        swap_positions(c, 0);
    }
    if let Some(gs) = g.get_mut("geometries").and_then(|g| g.as_array_mut()) {
        for g in gs {
            swap_geometry(g, depth + 1);
        }
    }
}

/// A service's GeoJSON answer with latitude-first coordinates (see [`lat_first`]) turned round to
/// longitude first, and its `crs` dropped; `None` when it is longitude first already.
pub fn fix_axes(bytes: &[u8]) -> Option<Vec<u8>> {
    let (head, tail) = edges(bytes);
    let mentions =
        |t: &str| t.to_ascii_lowercase().contains("urn:ogc:def:crs:epsg:") || t.to_ascii_lowercase().contains("urn:x-ogc:def:crs:epsg:");
    if !(mentions(&head) || mentions(&tail)) {
        return None;
    }
    let mut v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let crs = v.pointer("/crs/properties/name").and_then(|n| n.as_str())?;
    if !lat_first(crs) {
        return None;
    }
    if let Some(o) = v.as_object_mut() {
        o.remove("crs");
    }
    match v.get("type").and_then(|t| t.as_str()) {
        Some("FeatureCollection") => {
            for f in v.get_mut("features").and_then(|f| f.as_array_mut()).into_iter().flatten() {
                if let Some(g) = f.get_mut("geometry") {
                    swap_geometry(g, 0);
                }
            }
        }
        Some("Feature") => {
            if let Some(g) = v.get_mut("geometry") {
                swap_geometry(g, 0);
            }
        }
        _ => swap_geometry(&mut v, 0),
    }
    serde_json::to_vec(&v).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arcgis_services() {
        assert_eq!(
            capabilities_url("https://x.org/arcgis/rest/services/A/FeatureServer", None),
            "https://x.org/arcgis/rest/services/A/FeatureServer?f=json"
        );
        let fs = r#"{"currentVersion": 11.1, "serviceDescription": "Parcels", "maxRecordCount": 2000,
            "fullExtent": {"xmin": -1000000, "ymin": 0, "xmax": 1000000, "ymax": 1000000, "spatialReference": {"wkid": 102100, "latestWkid": 3857}},
            "layers": [{"id": 0, "name": "Parcels"}, {"id": 1, "name": "Group", "subLayerIds": [2]}, {"id": 2, "name": "Roads"}]}"#;
        let s = parse("https://x.org/arcgis/rest/services/A/FeatureServer", fs.as_bytes()).unwrap();
        assert_eq!(s.kind, "ArcGIS FeatureServer");
        assert_eq!(s.layers.iter().map(|l| l.title.as_str()).collect::<Vec<_>>(), ["Parcels", "Roads"]);
        assert!(
            matches!(&s.layers[0].offer, Offer::Features { url } if url == "https://x.org/arcgis/rest/services/A/FeatureServer/0/query?where=1%3D1&outFields=*&returnGeometry=true&outSR=4326&f=geojson")
        );
        let b = s.layers[0].bbox.unwrap();
        assert!((b[0] + 8.983).abs() < 0.01 && b[1].abs() < 1e-9, "{b:?}");
        let tiled = r#"{"currentVersion": 10.9, "mapName": "World", "singleFusedMapCache": true,
            "tileInfo": {"rows": 256, "spatialReference": {"wkid": 102100}, "lods": [{"level": 0, "resolution": 156543.03392800014}]}, "layers": []}"#;
        let s = parse("https://x.org/arcgis/rest/services/World/MapServer?f=json", tiled.as_bytes()).unwrap();
        assert!(
            matches!(&s.layers[0].offer, Offer::Tiles { template } if template == "https://x.org/arcgis/rest/services/World/MapServer/tile/{z}/{y}/{x}")
        );
        let dynamic = r#"{"currentVersion": 10.9, "mapName": "Dyn", "layers": [{"id": 3, "name": "Wells"}]}"#;
        let s = parse("https://x.org/arcgis/rest/services/Dyn/MapServer", dynamic.as_bytes()).unwrap();
        assert!(matches!(&s.layers[0].offer, Offer::Tiles { template } if template.contains("/export?bbox={bbox-epsg-3857}")));
        assert!(matches!(&s.layers[1].offer, Offer::Features { url } if url.contains("/MapServer/3/query?")));
        let vts = r#"{"currentVersion": 11, "name": "Basemap", "defaultStyles": "resources/styles"}"#;
        let s = parse("https://x.org/arcgis/rest/services/B/VectorTileServer", vts.as_bytes()).unwrap();
        assert!(
            matches!(&s.layers[0].offer, Offer::Style { url } if url == "https://x.org/arcgis/rest/services/B/VectorTileServer/resources/styles/root.json")
        );
        let img = r#"{"currentVersion": 11, "name": "Elev"}"#;
        let s = parse("https://x.org/arcgis/rest/services/E/ImageServer", img.as_bytes()).unwrap();
        assert!(matches!(&s.layers[0].offer, Offer::Tiles { template } if template.contains("/exportImage?")));
        let dir = r#"{"currentVersion": 11, "folders": [], "services": [{"name": "A", "type": "FeatureServer"}]}"#;
        assert!(parse("https://x.org/arcgis/rest/services", dir.as_bytes()).unwrap_err().contains("A/FeatureServer"));
        assert!(
            parse("https://x.org/arcgis/rest/services/A/FeatureServer", br#"{"error": {"code": 499, "message": "Token Required"}}"#)
                .unwrap_err()
                .contains("Token Required")
        );
    }

    #[test]
    fn capabilities_addresses() {
        assert_eq!(capabilities_url("https://x.org/wms", None), "https://x.org/wms?SERVICE=WMS&REQUEST=GetCapabilities");
        assert_eq!(
            capabilities_url("https://x.org/geoserver/ows?", Some("wfs")),
            "https://x.org/geoserver/ows?SERVICE=WFS&REQUEST=GetCapabilities"
        );
        assert_eq!(
            capabilities_url("https://x.org/a?map=b&request=GetCapabilities&service=WMS", None),
            "https://x.org/a?map=b&request=GetCapabilities&service=WMS"
        );
        assert_eq!(capabilities_url("https://x.org/ogc/features/collections", None), "https://x.org/ogc/features/collections?f=json");
        assert_eq!(
            capabilities_url("https://ows.example.org/osm/service", None),
            "https://ows.example.org/osm/service?SERVICE=WMS&REQUEST=GetCapabilities"
        );
        // The host doesn't decide the type: only the path and query do.
        assert_eq!(
            capabilities_url("https://rapid.example.org/geoserver/ows", None),
            "https://rapid.example.org/geoserver/ows?SERVICE=WMS&REQUEST=GetCapabilities"
        );
        assert_eq!(
            capabilities_url("https://wfs.example.org/maps", None),
            "https://wfs.example.org/maps?SERVICE=WMS&REQUEST=GetCapabilities"
        );
        assert_eq!(capabilities_url("https://x.org/geo/api/v1", None), "https://x.org/geo/api/v1?f=json");
        assert_eq!(capabilities_url("https://x.org/rapids/v1", None), "https://x.org/rapids/v1?SERVICE=WMS&REQUEST=GetCapabilities");
        // Whole query keys: `prof=` isn't `f=`.
        assert_eq!(
            capabilities_url("https://x.org/arcgis/rest/services/A/MapServer?prof=json", None),
            "https://x.org/arcgis/rest/services/A/MapServer?prof=json&f=json"
        );
        // ArcGIS's own OGC endpoints are read as WMS and WMTS.
        assert_eq!(
            capabilities_url("https://x.org/arcgis/rest/services/A/MapServer/WMSServer", None),
            "https://x.org/arcgis/rest/services/A/MapServer/WMSServer?SERVICE=WMS&REQUEST=GetCapabilities"
        );
        assert_eq!(query_value("https://x.org/a?F=json&limit=5#x", "f"), Some("json"));
        assert_eq!(query_value("https://x.org/a?shelf=1", "f"), None);
    }

    #[test]
    fn links_resolved_and_schemes_checked() {
        assert_eq!(
            resolve("https://x.org/api/collections?f=json", "collections/a/items").as_deref(),
            Some("https://x.org/api/collections/a/items")
        );
        assert_eq!(resolve("https://x.org/api/collections", "/ogc/a/items").as_deref(), Some("https://x.org/ogc/a/items"));
        assert_eq!(resolve("https://x.org", "a/items").as_deref(), Some("https://x.org/a/items"));
        assert_eq!(resolve("http://x.org/c", "//y.org/a").as_deref(), Some("http://y.org/a"));
        assert_eq!(resolve("https://x.org/c", "https://y.org/a").as_deref(), Some("https://y.org/a"));
        for bad in ["file:///etc/passwd", "httpfoo://x/a", "javascript:alert(1)", ""] {
            assert_eq!(resolve("https://x.org/c", bad), None, "{bad}");
        }
        assert!(is_web("HTTPS://x.org") && !is_web("httpfoo:x") && !is_web("http:/x"));
        // A capabilities document's endpoints in other schemes are not used.
        let xml = r#"<WMS_Capabilities version="1.3.0" xmlns:xlink="http://www.w3.org/1999/xlink"><Capability><Request><GetMap><DCPType><HTTP><Get><OnlineResource xlink:href="httpfoo://evil/wms"/></Get></HTTP></DCPType></GetMap></Request>
<Layer><Name>a</Name><CRS>EPSG:3857</CRS></Layer></Capability></WMS_Capabilities>"#;
        let s = parse("https://maps.example.org/wms", xml.as_bytes()).unwrap();
        assert!(matches!(&s.layers[0].offer, Offer::Tiles { template } if template.starts_with("https://maps.example.org/wms?")));
        // OGC API items links that are relative, or not web addresses.
        let json = r#"{"collections": [{"id": "a", "links": [{"rel": "items", "href": "collections/a/items"}]}, {"id": "b", "links": [{"rel": "items", "href": "file:///x"}]}]}"#;
        let s = parse("https://api.example.org/collections", json.as_bytes()).unwrap();
        assert_eq!(s.layers.len(), 1);
        assert!(
            matches!(&s.layers[0].offer, Offer::Features { url } if url == "https://api.example.org/collections/a/items?limit=10000&f=json"),
            "{:?}",
            s.layers[0].offer
        );
        assert!(s.notes.iter().any(|n| n.contains("1 collections")));
    }

    #[test]
    fn wfs_versions_and_axes() {
        // WFS 1.0: onlineResource, TYPENAME and MAXFEATURES.
        let v10 = r#"<WFS_Capabilities version="1.0.0"><Capability><Request><GetFeature><DCPType><HTTP><Get onlineResource="https://old.example.org/wfs?"/></HTTP></DCPType></GetFeature></Request></Capability>
<FeatureTypeList><FeatureType><Name>roads</Name><SRS>EPSG:4326</SRS></FeatureType></FeatureTypeList></WFS_Capabilities>"#;
        let s = parse("https://old.example.org/wfs", v10.as_bytes()).unwrap();
        assert!(
            matches!(&s.layers[0].offer, Offer::Features { url } if url.starts_with("https://old.example.org/wfs?SERVICE=WFS&VERSION=1.0.0&REQUEST=GetFeature&TYPENAME=roads&") && url.contains("SRSNAME=EPSG:4326&MAXFEATURES=")),
            "{:?}",
            s.layers[0].offer
        );
        // WFS 2.0 offering CRS84: asked for it, longitude first by definition.
        let v20 = r#"<WFS_Capabilities version="2.0.0"><FeatureTypeList><FeatureType><Name>a</Name><DefaultCRS>urn:ogc:def:crs:EPSG::25832</DefaultCRS><OtherCRS>urn:ogc:def:crs:OGC:1.3:CRS84</OtherCRS></FeatureType></FeatureTypeList></WFS_Capabilities>"#;
        let s = parse("https://x.org/wfs", v20.as_bytes()).unwrap();
        assert!(
            matches!(&s.layers[0].offer, Offer::Features { url } if url.contains("VERSION=2.0.0") && url.contains("TYPENAMES=a") && url.contains("SRSNAME=urn:ogc:def:crs:OGC:1.3:CRS84&COUNT=")),
            "{:?}",
            s.layers[0].offer
        );
        // An answer that says latitude comes first is turned round; others are left alone.
        let lat_lon = br#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[46.5,7.1],[47.0,8.2]]}},{"type":"Feature","properties":{},"geometry":{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[45,6]}]}}],"crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::4326"}}}"#;
        let fixed: serde_json::Value = serde_json::from_slice(&fix_axes(lat_lon).unwrap()).unwrap();
        assert_eq!(fixed.pointer("/features/0/geometry/coordinates/0").unwrap(), &serde_json::json!([7.1, 46.5]));
        assert_eq!(fixed.pointer("/features/1/geometry/geometries/0/coordinates").unwrap(), &serde_json::json!([6, 45]));
        assert!(fixed.get("crs").is_none());
        assert!(
            fix_axes(br#"{"type":"FeatureCollection","features":[],"crs":{"type":"name","properties":{"name":"EPSG:4326"}}}"#).is_none()
        );
        assert!(fix_axes(br#"{"type":"FeatureCollection","features":[]}"#).is_none());
        assert!(fix_axes(b"urn:ogc:def:crs:EPSG::4326 not json").is_none());
    }

    #[test]
    fn partial_answers_pointed_out() {
        assert!(
            partial_note(br#"{"type":"FeatureCollection","features":[],"properties":{"exceededTransferLimit":true}}"#)
                .is_some_and(|n| n.contains("record limit"))
        );
        assert!(
            partial_note(br#"{"type":"FeatureCollection","numberMatched": 25000, "numberReturned":10000,"features":[]}"#)
                .is_some_and(|n| n.contains("10000 of its 25000"))
        );
        assert!(partial_note(br#"{"type":"FeatureCollection","features":[],"links":[{"rel":"next","href":"x"}]}"#).is_some());
        assert!(
            partial_note(
                br#"{"type":"FeatureCollection","numberMatched":3,"numberReturned":3,"features":[],"exceededTransferLimit":false}"#
            )
            .is_none()
        );
        assert!(partial_note(b"").is_none());
    }

    #[test]
    fn wms_layers_in_web_mercator() {
        let xml = r#"<?xml version="1.0"?><WMS_Capabilities version="1.3.0" xmlns="http://www.opengis.net/wms" xmlns:xlink="http://www.w3.org/1999/xlink">
<Service><Title>Demo WMS</Title></Service>
<Capability><Request><GetMap><Format>image/jpeg</Format><Format>image/png</Format><DCPType><HTTP><Get><OnlineResource xlink:href="https://maps.example.org/wms?map=demo"/></Get></HTTP></DCPType></GetMap></Request>
<Layer><Title>Root</Title><CRS>EPSG:4326</CRS><CRS>EPSG:3857</CRS>
  <Layer><Name>roads</Name><Title>Roads</Title><EX_GeographicBoundingBox><westBoundLongitude>-10</westBoundLongitude><eastBoundLongitude>20</eastBoundLongitude><southBoundLatitude>35</southBoundLatitude><northBoundLatitude>60</northBoundLatitude></EX_GeographicBoundingBox></Layer>
  <Layer><Name>water bodies</Name></Layer>
</Layer>
<Layer><Name>utm only</Name><CRS>EPSG:32633</CRS></Layer></Capability></WMS_Capabilities>"#;
        let s = parse("https://maps.example.org/wms", xml.as_bytes()).unwrap();
        assert_eq!((s.kind.as_str(), s.title.as_str()), ("WMS", "Demo WMS"));
        let names: Vec<&str> = s.layers.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"roads") && names.contains(&"water bodies") && !names.contains(&"utm only"), "{names:?}");
        let roads = s.layers.iter().find(|l| l.name == "roads").unwrap();
        assert_eq!(roads.bbox, Some([-10.0, 35.0, 20.0, 60.0]));
        let Offer::Tiles { template } = &roads.offer else { panic!() };
        assert_eq!(
            template,
            "https://maps.example.org/wms?map=demo&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=roads&STYLES=&CRS=EPSG:3857&BBOX={bbox-epsg-3857}&WIDTH=256&HEIGHT=256&FORMAT=image%2Fpng&TRANSPARENT=TRUE"
        );
        let water = s.layers.iter().find(|l| l.name == "water bodies").unwrap();
        assert!(matches!(&water.offer, Offer::Tiles { template } if template.contains("LAYERS=water%20bodies")));
        assert!(s.notes.iter().any(|n| n.contains("1 layers")));
    }

    #[test]
    fn wmts_web_mercator_pyramids() {
        let mut matrices = String::new();
        for z in 0..=3 {
            let sd = Z0_SCALE / f64::from(1u32 << z);
            matrices.push_str(&format!("<TileMatrix><ows:Identifier>EPSG:3857:{z}</ows:Identifier><ScaleDenominator>{sd}</ScaleDenominator><TopLeftCorner>-20037508.3427892 20037508.3427892</TopLeftCorner><TileWidth>256</TileWidth><TileHeight>256</TileHeight></TileMatrix>"));
        }
        let xml = format!(
            r#"<Capabilities xmlns="http://www.opengis.net/wmts/1.0" xmlns:ows="http://www.opengis.net/ows/1.1" version="1.0.0">
<ows:ServiceIdentification><ows:Title>Tiles</ows:Title></ows:ServiceIdentification>
<Contents>
<Layer><ows:Title>Ortho</ows:Title><ows:Identifier>ortho</ows:Identifier><ows:WGS84BoundingBox><ows:LowerCorner>5 45</ows:LowerCorner><ows:UpperCorner>11 48</ows:UpperCorner></ows:WGS84BoundingBox>
 <Style isDefault="true"><ows:Identifier>normal</ows:Identifier></Style><Format>image/jpeg</Format><TileMatrixSetLink><TileMatrixSet>gm</TileMatrixSet></TileMatrixSetLink>
 <ResourceURL format="image/jpeg" resourceType="tile" template="https://tiles.example.org/{{Style}}/{{TileMatrixSet}}/{{TileMatrix}}/{{TileRow}}/{{TileCol}}.jpg"/></Layer>
<Layer><ows:Title>Swiss grid</ows:Title><ows:Identifier>lv95</ows:Identifier><TileMatrixSetLink><TileMatrixSet>2056</TileMatrixSet></TileMatrixSetLink></Layer>
<TileMatrixSet><ows:Identifier>gm</ows:Identifier><ows:SupportedCRS>urn:ogc:def:crs:EPSG::3857</ows:SupportedCRS>{matrices}</TileMatrixSet>
<TileMatrixSet><ows:Identifier>2056</ows:Identifier><ows:SupportedCRS>urn:ogc:def:crs:EPSG::2056</ows:SupportedCRS></TileMatrixSet>
</Contents></Capabilities>"#
        );
        let s = parse("https://tiles.example.org/WMTSCapabilities.xml", xml.as_bytes()).unwrap();
        assert_eq!(s.kind, "WMTS");
        assert_eq!(s.layers.len(), 1);
        let Offer::Tiles { template } = &s.layers[0].offer else { panic!() };
        assert_eq!(template, "https://tiles.example.org/normal/gm/EPSG:3857:{z}/{y}/{x}.jpg");
        assert_eq!(s.layers[0].bbox, Some([5.0, 45.0, 11.0, 48.0]));
        assert!(s.notes.iter().any(|n| n.contains("1 layers")));
    }

    #[test]
    fn wfs_and_ogc_api_features() {
        let xml = r#"<wfs:WFS_Capabilities version="2.0.0" xmlns:wfs="http://www.opengis.net/wfs/2.0" xmlns:ows="http://www.opengis.net/ows/1.1" xmlns:xlink="http://www.w3.org/1999/xlink">
<ows:ServiceIdentification><ows:Title>Parcels</ows:Title></ows:ServiceIdentification>
<ows:OperationsMetadata><ows:Operation name="GetFeature"><ows:DCP><ows:HTTP><ows:Get xlink:href="https://gis.example.org/wfs?"/></ows:HTTP></ows:DCP></ows:Operation></ows:OperationsMetadata>
<wfs:FeatureTypeList><wfs:FeatureType><wfs:Name>ns:parcels</wfs:Name><wfs:Title>Parcels</wfs:Title><ows:WGS84BoundingBox><ows:LowerCorner>-1 2</ows:LowerCorner><ows:UpperCorner>3 4</ows:UpperCorner></ows:WGS84BoundingBox></wfs:FeatureType></wfs:FeatureTypeList></wfs:WFS_Capabilities>"#;
        let s = parse("https://gis.example.org/wfs", xml.as_bytes()).unwrap();
        assert_eq!(s.kind, "WFS");
        let Offer::Features { url } = &s.layers[0].offer else { panic!() };
        assert!(
            url.starts_with("https://gis.example.org/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES=ns:parcels&OUTPUTFORMAT=application%2Fjson&SRSNAME=EPSG:4326&COUNT="),
            "{url}"
        );
        assert_eq!(s.layers[0].bbox, Some([-1.0, 2.0, 3.0, 4.0]));
        // The JSON format the server names, or a note when it has none.
        let named = xml.replace("<ows:DCP>", r#"<ows:Parameter name="outputFormat"><ows:Value>text/xml; subtype=gml/3.1.1</ows:Value><ows:Value>application/json; subtype=geojson</ows:Value></ows:Parameter><ows:DCP>"#);
        let s = parse("https://gis.example.org/wfs", named.as_bytes()).unwrap();
        assert!(
            matches!(&s.layers[0].offer, Offer::Features { url } if url.contains("OUTPUTFORMAT=application%2Fjson%3B%20subtype%3Dgeojson"))
        );
        let gml_only = xml.replace(
            "<ows:DCP>",
            r#"<ows:Parameter name="outputFormat"><ows:Value>text/xml; subtype=gml/3.1.1</ows:Value></ows:Parameter><ows:DCP>"#,
        );
        let s = parse("https://gis.example.org/wfs", gml_only.as_bytes()).unwrap();
        assert!(s.layers.is_empty() && s.notes.iter().any(|n| n.contains("no GeoJSON")));

        let json = r#"{"title": "API", "collections": [
            {"id": "buildings", "title": "Buildings", "extent": {"spatial": {"bbox": [[7, 46, 8, 47]]}},
             "links": [{"rel": "items", "type": "application/geo+json", "href": "https://api.example.org/collections/buildings/items"}]},
            {"id": "roads"}]}"#;
        let s = parse("https://api.example.org/collections?f=json", json.as_bytes()).unwrap();
        assert_eq!(s.kind, "OGC API Features");
        assert_eq!(s.layers.len(), 2);
        assert!(
            matches!(&s.layers[0].offer, Offer::Features { url } if url == "https://api.example.org/collections/buildings/items?limit=10000")
        );
        assert!(
            matches!(&s.layers[1].offer, Offer::Features { url } if url == "https://api.example.org/collections/roads/items?limit=10000&f=json")
        );
        assert_eq!(s.layers[0].bbox, Some([7.0, 46.0, 8.0, 47.0]));
        assert!(parse("x", br#"{"links": [{"rel": "data", "href": "c"}]}"#).unwrap_err().contains("landing page"));
        assert!(parse("x", b"<ServiceExceptionReport>boom</ServiceExceptionReport>").unwrap_err().contains("boom"));
        assert!(parse("x", b"<html/>").is_err());
        assert!(parse("x", &[0xff]).is_err());
    }
}
