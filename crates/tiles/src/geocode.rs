//! Geocoding: places and addresses found by name through a Nominatim or Photon server (OSM's
//! public ones, or the user's own). Requests are built and answers read here; the fetching is the
//! caller's (blocking on the command line, off the frame in the app).

use serde_json::Value;

/// The kind of server: its request and its answer's form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Nominatim's `/search` (`format=jsonv2`).
    Nominatim,
    /// Photon's `/api` (GeoJSON).
    Photon,
}

/// A geocoding server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Geocoder {
    pub kind: Kind,
    /// Its address, without the endpoint's path (`https://nominatim.openstreetmap.org`).
    pub base: String,
}

/// OSM's public Nominatim, the default.
pub const NOMINATIM: &str = "https://nominatim.openstreetmap.org";
/// Komoot's public Photon.
pub const PHOTON: &str = "https://photon.komoot.io";

/// How many addresses a run sends to a public server: their usage policies allow light use only
/// (a server of one's own takes [`MAX_BATCH`]).
pub const PUBLIC_BATCH: usize = 100;
/// How many addresses a run geocodes at most.
pub const MAX_BATCH: usize = 10_000;
/// How many places a search returns at most.
pub const MAX_RESULTS: usize = 20;
/// The longest query sent.
const MAX_QUERY: usize = 500;

/// A place found.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// What the server calls it (the full address for Nominatim).
    pub name: String,
    pub lon: f64,
    pub lat: f64,
    /// Its extent, west, south, east, north, when the server gives one.
    pub bbox: Option<[f64; 4]>,
}

impl Geocoder {
    /// A server by `provider` (`nominatim`, the default, or `photon`) and an optional address of
    /// one's own.
    pub fn new(provider: Option<&str>, url: Option<&str>) -> Result<Geocoder, String> {
        let kind = match provider.map(|p| p.trim().to_ascii_lowercase()).as_deref() {
            None | Some("") | Some("nominatim") => Kind::Nominatim,
            Some("photon") => Kind::Photon,
            Some(other) => return Err(format!("unknown geocoder '{other}' (nominatim or photon)")),
        };
        let base = match url.map(str::trim).filter(|u| !u.is_empty()) {
            Some(u) => {
                crate::address::check(u, None)?;
                // The endpoint's path is the request's.
                let u = u.trim_end_matches('/');
                u.strip_suffix("/search").or_else(|| u.strip_suffix("/api")).unwrap_or(u).to_string()
            }
            None if kind == Kind::Photon => PHOTON.to_string(),
            None => NOMINATIM.to_string(),
        };
        Ok(Geocoder { kind, base })
    }

    /// Whether it is a public server (OSM's Nominatim or komoot's Photon), whose usage policy asks
    /// for at most a request a second and light use.
    pub fn is_public(&self) -> bool {
        let b = self.base.to_ascii_lowercase();
        b.contains("nominatim.openstreetmap.org") || b.contains("photon.komoot.io")
    }

    /// How long to wait between requests, in milliseconds.
    pub fn spacing_ms(&self) -> u64 {
        if self.is_public() { 1100 } else { 0 }
    }

    /// How many addresses a run may send.
    pub fn batch_limit(&self) -> usize {
        if self.is_public() { PUBLIC_BATCH } else { MAX_BATCH }
    }

    /// The request for `query`, returning up to `limit` places.
    pub fn url(&self, query: &str, limit: usize) -> String {
        let q: String = query.trim().chars().take(MAX_QUERY).collect();
        let limit = limit.clamp(1, MAX_RESULTS);
        match self.kind {
            Kind::Nominatim => format!("{}/search?format=jsonv2&limit={limit}&q={}", self.base, encode(&q)),
            Kind::Photon => format!("{}/api/?limit={limit}&q={}", self.base, encode(&q)),
        }
    }

    /// The places in an answer.
    pub fn parse(&self, bytes: &[u8]) -> Result<Vec<Place>, String> {
        let v: Value = serde_json::from_slice(bytes).map_err(|_| "the geocoder's answer isn't JSON".to_string())?;
        let places = match self.kind {
            Kind::Nominatim => {
                v.as_array().ok_or("the geocoder's answer isn't a list of places")?.iter().filter_map(nominatim_place).collect()
            }
            Kind::Photon => v
                .get("features")
                .and_then(Value::as_array)
                .ok_or("the geocoder's answer isn't GeoJSON")?
                .iter()
                .filter_map(photon_place)
                .collect::<Vec<_>>(),
        };
        Ok(places.into_iter().take(MAX_RESULTS).collect())
    }
}

/// A number given as a number or as text (Nominatim's).
fn num(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    v.as_f64().or_else(|| v.as_str()?.trim().parse().ok()).filter(|x: &f64| x.is_finite())
}

fn valid(lon: f64, lat: f64) -> bool {
    (-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat)
}

fn nominatim_place(p: &Value) -> Option<Place> {
    let (lon, lat) = (num(p.get("lon"))?, num(p.get("lat"))?);
    if !valid(lon, lat) {
        return None;
    }
    // boundingbox: south, north, west, east.
    let bbox = p.get("boundingbox").and_then(Value::as_array).and_then(|b| {
        let n: Vec<f64> = b.iter().take(4).filter_map(|x| num(Some(x))).collect();
        match n.as_slice() {
            [s, nn, w, e] if valid(*w, *s) && valid(*e, *nn) => Some([*w, *s, *e, *nn]),
            _ => None,
        }
    });
    let name = p.get("display_name").and_then(Value::as_str).unwrap_or("").chars().take(500).collect();
    Some(Place { name, lon, lat, bbox })
}

fn photon_place(f: &Value) -> Option<Place> {
    let c = f.get("geometry")?.get("coordinates")?.as_array()?;
    let (lon, lat) = (num(c.first())?, num(c.get(1))?);
    if !valid(lon, lat) {
        return None;
    }
    let props = f.get("properties");
    let text = |k: &str| props.and_then(|p| p.get(k)).and_then(Value::as_str).filter(|s| !s.is_empty());
    // A name from its parts, most specific first.
    let street = match (text("street"), text("housenumber")) {
        (Some(s), Some(n)) => Some(format!("{s} {n}")),
        (s, _) => s.map(str::to_string),
    };
    let parts: Vec<String> = [text("name").map(str::to_string), street]
        .into_iter()
        .chain(["city", "state", "country"].iter().map(|k| text(k).map(str::to_string)))
        .flatten()
        .collect();
    let mut name: Vec<String> = Vec::new();
    for p in parts {
        if !name.contains(&p) {
            name.push(p);
        }
    }
    // extent: west, north, east, south.
    let bbox = props.and_then(|p| p.get("extent")).and_then(Value::as_array).and_then(|b| {
        let n: Vec<f64> = b.iter().take(4).filter_map(|x| num(Some(x))).collect();
        match n.as_slice() {
            [w, nn, e, s] if valid(*w, *s) && valid(*e, *nn) => Some([*w, s.min(*nn), *e, nn.max(*s)]),
            _ => None,
        }
    });
    Some(Place { name: name.join(", ").chars().take(500).collect(), lon, lat, bbox })
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

/// Finds places, waiting for the answer (the command line; the app searches off the frame).
#[cfg(not(target_arch = "wasm32"))]
pub fn search_blocking(g: &Geocoder, query: &str, limit: usize) -> Result<Vec<Place>, String> {
    if query.trim().is_empty() {
        return Err("give a place or an address to find".into());
    }
    g.parse(&crate::cache::download_blocking(&g.url(query, limit))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_and_requests() {
        let g = Geocoder::new(None, None).unwrap();
        assert_eq!(g.kind, Kind::Nominatim);
        assert!(g.is_public() && g.spacing_ms() >= 1000 && g.batch_limit() == PUBLIC_BATCH);
        assert_eq!(
            g.url(" 10 Downing St, London ", 3),
            "https://nominatim.openstreetmap.org/search?format=jsonv2&limit=3&q=10%20Downing%20St%2C%20London"
        );
        let own = Geocoder::new(Some("Photon"), Some("http://localhost:2322/api/")).unwrap();
        assert_eq!((own.kind, own.base.as_str()), (Kind::Photon, "http://localhost:2322"));
        assert!(!own.is_public() && own.spacing_ms() == 0 && own.batch_limit() == MAX_BATCH);
        assert_eq!(own.url("Zürich", 99), "http://localhost:2322/api/?limit=20&q=Z%C3%BCrich");
        assert!(Geocoder::new(Some("google"), None).is_err());
        assert!(Geocoder::new(None, Some("file:///etc/passwd")).is_err());
        assert!(Geocoder::new(None, Some("http://169.254.169.254/")).is_err());
    }

    #[test]
    fn answers() {
        let g = Geocoder::new(None, None).unwrap();
        let a = br#"[{"lat":"51.5033","lon":"-0.1276","display_name":"10 Downing Street, London","boundingbox":["51.50","51.51","-0.13","-0.12"]},{"lat":"x","lon":"1"},{"lat":"95","lon":"1"}]"#;
        let p = g.parse(a).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(
            p[0],
            Place { name: "10 Downing Street, London".into(), lon: -0.1276, lat: 51.5033, bbox: Some([-0.13, 51.50, -0.12, 51.51]) }
        );
        assert!(g.parse(b"{}").is_err() && g.parse(b"<html>").is_err());
        let ph = Geocoder::new(Some("photon"), None).unwrap();
        let b = br#"{"type":"FeatureCollection","features":[{"geometry":{"type":"Point","coordinates":[8.54,47.37]},"properties":{"name":"Zurich","city":"Zurich","country":"Switzerland","extent":[8.44,47.43,8.62,47.32]}},{"geometry":null}]}"#;
        let p = ph.parse(b).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "Zurich, Switzerland");
        assert_eq!(p[0].bbox, Some([8.44, 47.32, 8.62, 47.43]));
        assert!(ph.parse(b"[]").is_err());
    }
}
