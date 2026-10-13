//! Rasters: bands of numbers on a georeferenced grid (elevation, imagery, classified land cover).
//!
//! Pixels are held as `f32` per band. Readers never panic on malformed input and refuse rasters
//! larger than [`MAX_SAMPLES`]; cloud-optimised GeoTIFFs fall back to their largest overview that
//! fits.

pub mod ascii;
pub mod crs;
pub mod geotiff;
pub mod wkt;

use std::path::Path;

pub use crs::{Crs, Transformer};
use rustgis_geom::{Coord, Extent};
use serde::{Deserialize, Serialize};

use crate::{DataError, Result};

/// Most samples (pixels × bands) held in memory: 120 million (480 MB), or 32 million in the
/// browser, whose memory is much tighter. Reading briefly needs about twice this.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_SAMPLES: usize = 120_000_000;
#[cfg(target_arch = "wasm32")]
pub const MAX_SAMPLES: usize = 32_000_000;
/// Largest ASCII grid file read (1 GiB).
pub const MAX_ASCII_BYTES: u64 = 1 << 30;
/// File extensions read as rasters.
pub const READ_EXTENSIONS: &[&str] = &["tif", "tiff", "asc"];

/// Whether a file name is a raster this module reads.
pub fn is_raster_name(name: &str) -> bool {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    READ_EXTENSIONS.contains(&ext.as_str())
}

/// Maps pixel (column, row) corners to coordinates: north-up grids only.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeoTransform {
    /// Top-left corner of the top-left pixel.
    pub origin_x: f64,
    pub origin_y: f64,
    /// Pixel size; `pixel_height` is negative for north-up rasters.
    pub pixel_width: f64,
    pub pixel_height: f64,
}

impl GeoTransform {
    /// Coordinates of a pixel position (corners at whole numbers, centres at +0.5).
    pub fn to_world(&self, col: f64, row: f64) -> Coord {
        Coord::new(self.origin_x + col * self.pixel_width, self.origin_y + row * self.pixel_height)
    }
    /// Pixel position of coordinates.
    pub fn to_pixel(&self, c: Coord) -> (f64, f64) {
        ((c.x - self.origin_x) / self.pixel_width, (c.y - self.origin_y) / self.pixel_height)
    }
    pub fn is_valid(&self) -> bool {
        [self.origin_x, self.origin_y, self.pixel_width, self.pixel_height].iter().all(|v| v.is_finite())
            && self.pixel_width != 0.0
            && self.pixel_height != 0.0
    }
}

/// Bins in [`BandStats::histogram`].
pub const HISTOGRAM_BINS: usize = 256;

/// Summary statistics of one band's valid pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BandStats {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub std_dev: f64,
    /// Pixel counts in [`HISTOGRAM_BINS`] equal bins from `min` to `max`.
    pub histogram: Vec<u64>,
    /// The distinct values, when the band holds whole numbers and has at most 256 of them.
    pub categories: Option<Vec<f64>>,
}

impl BandStats {
    /// Statistics of the finite values in `data`.
    pub fn from_values(data: &[f32]) -> BandStats {
        BandStats::compute(data, f32::is_finite)
    }

    fn compute(data: &[f32], valid: impl Fn(f32) -> bool) -> BandStats {
        let (mut n, mut min, mut max, mut sum, mut sum2) = (0usize, f64::INFINITY, f64::NEG_INFINITY, 0.0f64, 0.0f64);
        let mut whole = true;
        for &v in data {
            if !valid(v) {
                continue;
            }
            let x = v as f64;
            n += 1;
            min = min.min(x);
            max = max.max(x);
            sum += x;
            sum2 += x * x;
            whole &= x.fract() == 0.0;
        }
        if n == 0 {
            return BandStats::default();
        }
        let mean = sum / n as f64;
        let std_dev = (sum2 / n as f64 - mean * mean).max(0.0).sqrt();
        let mut histogram = vec![0u64; HISTOGRAM_BINS];
        let span = max - min;
        // Whole numbers within 16 bits are tracked in a bit set (fast for byte imagery); others in
        // a small set that gives up past 256 values.
        let small_ints = whole && min >= 0.0 && max <= 65_535.0;
        let mut bits = if small_ints { vec![0u64; 1024] } else { Vec::new() };
        let mut distinct = std::collections::BTreeSet::new();
        let mut track = whole;
        let k = (HISTOGRAM_BINS as f64 - 1.0) / if span > 0.0 { span } else { 1.0 };
        for &v in data {
            if !valid(v) {
                continue;
            }
            let x = v as f64;
            let bin = ((x - min) * k).round() as usize;
            if let Some(h) = histogram.get_mut(bin.min(HISTOGRAM_BINS - 1)) {
                *h += 1;
            }
            if small_ints {
                let i = x as usize;
                if let Some(w) = bits.get_mut(i / 64) {
                    *w |= 1 << (i % 64);
                }
            } else if track {
                distinct.insert(x as i64);
                track = distinct.len() <= 256;
            }
        }
        let categories = if small_ints {
            let n: u32 = bits.iter().map(|w| w.count_ones()).sum();
            (n <= 256).then(|| {
                bits.iter()
                    .enumerate()
                    .flat_map(|(wi, w)| (0..64).filter(move |b| w & (1 << b) != 0).map(move |b| (wi * 64 + b) as f64))
                    .collect()
            })
        } else {
            track.then(|| distinct.into_iter().map(|v| v as f64).collect())
        };
        BandStats { count: n, min, max, mean, std_dev, histogram, categories }
    }

    /// The value below which `p` (0–100) percent of pixels fall, from the histogram.
    pub fn percentile(&self, p: f64) -> f64 {
        let total: u64 = self.histogram.iter().sum();
        if total == 0 || !p.is_finite() {
            return self.min;
        }
        let target = (p.clamp(0.0, 100.0) / 100.0 * total as f64).max(0.0);
        let mut acc = 0u64;
        let step = (self.max - self.min) / (HISTOGRAM_BINS as f64 - 1.0);
        for (i, c) in self.histogram.iter().enumerate() {
            acc += c;
            if acc as f64 >= target {
                return self.min + i as f64 * step;
            }
        }
        self.max
    }
}

/// One band of pixels, row by row.
#[derive(Clone)]
pub struct Band {
    pub name: String,
    pub data: Vec<f32>,
    pub stats: BandStats,
}

/// How a raster's bands were meant to be shown, from the file.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ColorHint {
    #[default]
    Gray,
    /// Band 1 is grey and band 2 its transparency.
    GrayAlpha,
    /// Bands 1–3 are red, green, blue (and 4 is alpha, if present).
    Rgb { alpha: bool },
    /// Band 1 indexes this colour table (RGBA).
    Palette(Vec<[u8; 4]>),
}

/// A georeferenced raster.
#[derive(Clone)]
pub struct Raster {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub bands: Vec<Band>,
    pub transform: GeoTransform,
    pub crs: Crs,
    pub nodata: Option<f64>,
    pub color: ColorHint,
    /// Pixel type in the file (`UInt8`, `Float32`…).
    pub data_type: String,
    /// Where it was read from, if anywhere.
    pub source: Option<String>,
    /// Whether the pixels come from a reduced-resolution overview.
    pub overview: Option<(u32, u32)>,
    transformer: Transformer,
    extent: Extent,
}

impl std::fmt::Debug for Raster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Raster")
            .field("name", &self.name)
            .field("size", &(self.width, self.height, self.bands.len()))
            .field("crs", &self.crs)
            .field("transform", &self.transform)
            .finish_non_exhaustive()
    }
}

impl Raster {
    /// Builds a raster from bands of `width × height` pixels and computes statistics.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        width: u32,
        height: u32,
        bands: Vec<Vec<f32>>,
        transform: GeoTransform,
        crs: Crs,
        nodata: Option<f64>,
        color: ColorHint,
        data_type: impl Into<String>,
    ) -> Result<Raster> {
        let n = (width as usize).checked_mul(height as usize).ok_or_else(|| DataError::Format("raster too large".into()))?;
        if n == 0 || bands.is_empty() {
            return Err(DataError::Format("the raster has no pixels".into()));
        }
        if bands.iter().any(|b| b.len() != n) {
            return Err(DataError::Format("band sizes don't match the raster size".into()));
        }
        if !transform.is_valid() {
            return Err(DataError::Format("the raster has no usable georeferencing".into()));
        }
        let transformer = crs.transformer().map_err(DataError::Format)?;
        let nd = nodata.filter(|v| v.is_finite()).map(|v| v as f32);
        let valid = move |v: f32| v.is_finite() && nd.is_none_or(|d| v != d);
        let bands = bands
            .into_iter()
            .enumerate()
            .map(|(i, data)| Band { name: format!("Band {}", i + 1), stats: BandStats::compute(&data, valid), data })
            .collect();
        let mut r = Raster {
            name: name.into(),
            width,
            height,
            bands,
            transform,
            crs,
            nodata: nodata.filter(|v| v.is_finite()),
            color,
            data_type: data_type.into(),
            source: None,
            overview: None,
            transformer,
            extent: Extent::EMPTY,
        };
        r.extent = r.compute_extent();
        if r.extent.is_empty() {
            return Err(DataError::Format("the raster's location couldn't be worked out".into()));
        }
        Ok(r)
    }

    /// Longitude/latitude bounds, from points along the grid's edges.
    fn compute_extent(&self) -> Extent {
        let mut e = Extent::EMPTY;
        let (w, h) = (self.width as f64, self.height as f64);
        let steps = 32;
        let mut add = |col: f64, row: f64| {
            if let Some(c) = self.transformer.to_lonlat(self.transform.to_world(col, row)) {
                e.expand(Coord::new(c.x.clamp(-180.0, 180.0), c.y.clamp(-90.0, 90.0)));
            }
        };
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            for (col, row) in [(t * w, 0.0), (t * w, h), (0.0, t * h), (w, t * h)] {
                add(col, row);
            }
        }
        // Interior points too: in polar and other projections the grid can reach further than its
        // edges do. (A raster crossing the antimeridian still gets a world-wide extent.)
        let n = 16;
        for i in 1..n {
            for j in 1..n {
                add(w * i as f64 / n as f64, h * j as f64 / n as f64);
            }
        }
        e
    }

    /// Longitude/latitude bounds.
    pub fn extent(&self) -> Extent {
        self.extent
    }

    pub fn transformer(&self) -> &Transformer {
        &self.transformer
    }

    pub fn band_count(&self) -> usize {
        self.bands.len()
    }

    /// Whether a value is a real measurement (not no-data or NaN).
    pub fn is_valid(&self, v: f32) -> bool {
        // Compared in f32, as the pixels (and the statistics) are: a no-data value such as
        // -3.4e38 or -9999.1 isn't exactly representable in f32.
        v.is_finite() && self.nodata.is_none_or(|d| v != d as f32)
    }

    /// The value of `band` at pixel (`col`, `row`), unless it is no-data or outside the grid.
    pub fn value(&self, band: usize, col: i64, row: i64) -> Option<f32> {
        if col < 0 || row < 0 || col >= self.width as i64 || row >= self.height as i64 {
            return None;
        }
        let v = *self.bands.get(band)?.data.get(row as usize * self.width as usize + col as usize)?;
        self.is_valid(v).then_some(v)
    }

    /// The pixel under a longitude/latitude point.
    pub fn pixel_at(&self, lonlat: Coord) -> Option<(i64, i64)> {
        let (c, r) = self.transform.to_pixel(self.transformer.from_lonlat(lonlat)?);
        let (c, r) = (c.floor(), r.floor());
        (c >= 0.0 && r >= 0.0 && c < self.width as f64 && r < self.height as f64).then_some((c as i64, r as i64))
    }

    /// Cell size in the raster's units (x, y).
    pub fn cell_size(&self) -> (f64, f64) {
        (self.transform.pixel_width.abs(), self.transform.pixel_height.abs())
    }

    /// Ground size of a pixel in metres (approximate for geographic rasters).
    pub fn cell_size_m(&self) -> (f64, f64) {
        let (x, y) = self.cell_size();
        if self.crs.is_geographic() {
            let lat = self.extent.center().y.to_radians().cos().abs().max(0.01);
            (x * 111_320.0 * lat, y * 110_574.0)
        } else {
            (x, y)
        }
    }
}

/// Reads a raster file by path.
pub fn read_path(path: &Path) -> Result<Raster> {
    // Only regular files (not FIFOs or devices), and ASCII grids within a size limit, as they
    // are read whole.
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(DataError::Format(format!("{} is not a regular file", path.display())));
    }
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("raster").to_string();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let mut r = match ext.as_str() {
        "tif" | "tiff" => {
            let f = std::fs::File::open(path)?;
            geotiff::read(std::io::BufReader::new(f), &name)?
        }
        "asc" => {
            if meta.len() > MAX_ASCII_BYTES {
                return Err(DataError::Format("the ASCII grid is too large to open".into()));
            }
            let text = std::fs::read_to_string(path)?;
            ascii::read(&text, &name, sidecar_crs(path))?
        }
        other => return Err(DataError::Unsupported(format!(".{other}"))),
    };
    r.source = Some(path.display().to_string());
    Ok(r)
}

/// A picture (PNG or JPEG) laid over a longitude/latitude box (west, south, east, north) and
/// turned `rotation` degrees counter-clockwise about its centre: an RGBA raster, as a KML ground
/// overlay is. Pictures are at most 8192 pixels a side.
pub fn from_image(name: &str, bytes: &[u8], bbox: [f64; 4], rotation: f64) -> Result<Raster> {
    let r = from_image_unrotated(name, bytes, bbox)?;
    if !rotation.is_finite() || rotation.abs() < 1e-9 {
        return Ok(r);
    }
    rotate(&r, rotation)
}

/// A raster turned about its centre (in its own coordinates): the turned rectangle's bounding box,
/// at the same pixel size, its corners transparent (nearest pixel).
fn rotate(r: &Raster, degrees: f64) -> Result<Raster> {
    let (w, h) = (f64::from(r.width), f64::from(r.height));
    let (sin, cos) = degrees.to_radians().sin_cos();
    // The pixel grid is square in the picture; a turn in coordinates is a turn of the pixels
    // scaled by the cell's aspect.
    let (px, py) = (r.transform.pixel_width, -r.transform.pixel_height);
    let (cx, cy) = (r.transform.origin_x + w * px / 2.0, r.transform.origin_y - h * py / 2.0);
    let corners =
        [(-w / 2.0 * px, -h / 2.0 * py), (w / 2.0 * px, -h / 2.0 * py), (w / 2.0 * px, h / 2.0 * py), (-w / 2.0 * px, h / 2.0 * py)];
    let turned: Vec<(f64, f64)> = corners.iter().map(|(x, y)| (x * cos - y * sin, x * sin + y * cos)).collect();
    let (min_x, max_x) = turned.iter().fold((f64::MAX, f64::MIN), |(a, b), (x, _)| (a.min(*x), b.max(*x)));
    let (min_y, max_y) = turned.iter().fold((f64::MAX, f64::MIN), |(a, b), (_, y)| (a.min(*y), b.max(*y)));
    let ow = ((max_x - min_x) / px).ceil().clamp(1.0, 16384.0) as u32;
    let oh = ((max_y - min_y) / py).ceil().clamp(1.0, 16384.0) as u32;
    let n = ow as usize * oh as usize;
    let mut bands: Vec<Vec<f32>> = vec![vec![0.0; n]; r.bands.len()];
    for row in 0..oh {
        for col in 0..ow {
            // The output pixel's centre, turned back into the picture.
            let x = min_x + (f64::from(col) + 0.5) * px;
            let y = max_y - (f64::from(row) + 0.5) * py;
            let (sx, sy) = (x * cos + y * sin, -x * sin + y * cos);
            let (c, rr) = ((sx / px + w / 2.0).floor(), (h / 2.0 - sy / py).floor());
            if c < 0.0 || rr < 0.0 || c >= w || rr >= h {
                continue;
            }
            let src = rr as usize * r.width as usize + c as usize;
            let dst = row as usize * ow as usize + col as usize;
            for (out, b) in bands.iter_mut().zip(&r.bands) {
                if let (Some(o), Some(v)) = (out.get_mut(dst), b.data.get(src)) {
                    *o = *v;
                }
            }
        }
    }
    let transform = GeoTransform { origin_x: cx + min_x, origin_y: cy + max_y, pixel_width: px, pixel_height: -py };
    Raster::new(r.name.clone(), ow, oh, bands, transform, r.crs, None, r.color.clone(), r.data_type.clone())
}

fn from_image_unrotated(name: &str, bytes: &[u8], bbox: [f64; 4]) -> Result<Raster> {
    let [w, s, e, n] = bbox;
    if !(w < e && s < n) {
        return Err(DataError::Format(format!("{name}: an empty box")));
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| DataError::Format(format!("{name}: {e}")))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(512 << 20);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| DataError::Format(format!("{name}: {e}")))?.into_rgba8();
    let (width, height) = img.dimensions();
    let mut bands: Vec<Vec<f32>> = (0..4).map(|_| Vec::with_capacity(width as usize * height as usize)).collect();
    for px in img.pixels() {
        for (b, v) in bands.iter_mut().zip(px.0) {
            b.push(f32::from(v));
        }
    }
    let transform =
        GeoTransform { origin_x: w, origin_y: n, pixel_width: (e - w) / f64::from(width), pixel_height: -(n - s) / f64::from(height) };
    Raster::new(name, width, height, bands, transform, Crs::LonLat, None, ColorHint::Rgb { alpha: true }, "UInt8")
}

/// Reads a raster from a file's name and bytes (a browser upload).
pub fn read_bytes(file_name: &str, bytes: &[u8]) -> Result<Raster> {
    let p = Path::new(file_name);
    let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("raster").to_string();
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "tif" | "tiff" => geotiff::read(std::io::Cursor::new(bytes), &name),
        "asc" => {
            let text = std::str::from_utf8(bytes).map_err(|_| DataError::Format("not a text file".into()))?;
            ascii::read(text, &name, None)
        }
        other => Err(DataError::Unsupported(format!(".{other}"))),
    }
}

/// The coordinate system in a `.prj` next to an ASCII grid, when it's one RustGIS understands.
fn sidecar_crs(path: &Path) -> Option<Crs> {
    wkt::crs_from_wkt(&read_prj(path)?).ok()
}

/// The text of the `.prj` next to a file (`None` when there's none, or it's too large to be one).
pub fn read_prj(path: &Path) -> Option<String> {
    let prj = path.with_extension("prj");
    let meta = std::fs::metadata(&prj).ok()?;
    if !meta.is_file() || meta.len() > 1 << 20 {
        return None;
    }
    std::fs::read_to_string(prj).ok()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A PNG of `w`×`h` pixels, red in its top half and blue below.
    pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
        let img =
            image::RgbaImage::from_fn(w, h, |_, y| if y < h / 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn pictures_over_boxes() {
        let r = from_image("o", &png(20, 10), [10.0, 40.0, 12.0, 41.0], 0.0).unwrap();
        assert_eq!((r.width, r.height, r.bands.len()), (20, 10, 4));
        let e = r.extent();
        assert_eq!((e.min_x, e.min_y, e.max_x, e.max_y), (10.0, 40.0, 12.0, 41.0));
        assert_eq!((r.bands[0].data[0], r.bands[2].data[199]), (255.0, 255.0), "red on top, blue below");
        // A quarter turn: the box's centre stays, it is as tall as it was wide, and its corners
        // (outside the picture) are see-through.
        let t = from_image("o", &png(20, 10), [10.0, 40.0, 12.0, 41.0], 90.0).unwrap();
        let e = t.extent();
        assert!(((e.min_x + e.max_x) / 2.0 - 11.0).abs() < 0.11 && ((e.min_y + e.max_y) / 2.0 - 40.5).abs() < 0.11, "{e:?}");
        assert!(((e.max_y - e.min_y) - 2.0).abs() < 0.11 && ((e.max_x - e.min_x) - 1.0).abs() < 0.11, "{e:?}");
        let small = from_image("o", &png(20, 10), [10.0, 40.0, 12.0, 41.0], 30.0).unwrap();
        assert_eq!(small.bands[3].data[0], 0.0, "a turned picture's corner is transparent");
        assert!(from_image("o", b"not a picture", [0.0, 0.0, 1.0, 1.0], 0.0).is_err());
        assert!(from_image("o", &png(2, 2), [1.0, 0.0, 0.0, 1.0], 0.0).is_err(), "an empty box");
    }

    /// A 4×3 north-up lon/lat raster with one no-data pixel.
    pub(crate) fn small() -> Raster {
        let gt = GeoTransform { origin_x: 10.0, origin_y: 50.0, pixel_width: 0.5, pixel_height: -0.5 };
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, -9999.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0];
        Raster::new("small", 4, 3, vec![data], gt, Crs::LonLat, Some(-9999.0), ColorHint::Gray, "Float32").unwrap()
    }

    #[test]
    fn stats_extent_and_lookup() {
        let r = small();
        let s = &r.bands[0].stats;
        assert_eq!(s.count, 11);
        assert_eq!((s.min, s.max), (1.0, 12.0));
        assert_eq!(s.categories.as_ref().map(Vec::len), Some(11));
        assert!(s.percentile(50.0) >= 5.0 && s.percentile(50.0) <= 7.0);
        let e = r.extent();
        assert!((e.min_x - 10.0).abs() < 1e-9 && (e.max_x - 12.0).abs() < 1e-9);
        assert!((e.min_y - 48.5).abs() < 1e-9 && (e.max_y - 50.0).abs() < 1e-9);
        assert_eq!(r.value(0, 1, 1), None, "no-data");
        assert_eq!(r.value(0, 2, 1), Some(7.0));
        assert_eq!(r.value(0, -1, 0), None);
        assert_eq!(r.value(9, 0, 0), None);
        assert_eq!(r.pixel_at(Coord::new(10.6, 49.9)), Some((1, 0)));
        assert_eq!(r.pixel_at(Coord::new(0.0, 0.0)), None);
    }

    #[test]
    fn rejects_bad_shapes() {
        let gt = GeoTransform { origin_x: 0.0, origin_y: 0.0, pixel_width: 1.0, pixel_height: -1.0 };
        assert!(Raster::new("x", 2, 2, vec![vec![0.0; 3]], gt, Crs::LonLat, None, ColorHint::Gray, "").is_err());
        assert!(Raster::new("x", 0, 2, vec![vec![]], gt, Crs::LonLat, None, ColorHint::Gray, "").is_err());
        let bad = GeoTransform { pixel_width: 0.0, ..gt };
        assert!(Raster::new("x", 1, 1, vec![vec![0.0]], bad, Crs::LonLat, None, ColorHint::Gray, "").is_err());
        assert!(read_bytes("x.tif", b"not a tiff").is_err());
        assert!(read_bytes("x.asc", &[0xff, 0xfe]).is_err());
        assert!(read_bytes("x.png", b"").is_err());
    }
}

/// Reads every raster in `$RUSTGIS_SAMPLE_RASTERS` (sample files from
/// <https://source.coop/giswqs/opengeos>): `cargo test -p rustgis-data -- --ignored samples`.
#[cfg(test)]
#[test]
#[ignore]
fn samples() {
    let Ok(dir) = std::env::var("RUSTGIS_SAMPLE_RASTERS") else { return };
    let mut paths: Vec<_> =
        std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| is_raster_name(&p.to_string_lossy())).collect();
    paths.sort();
    assert!(!paths.is_empty());
    for p in paths {
        let t = std::time::Instant::now();
        let r = match read_path(&p) {
            Ok(r) => r,
            Err(e) => {
                println!("FAILED {}: {e}", p.display());
                continue;
            }
        };
        let s = &r.bands[0].stats;
        println!(
            "{}: {}×{}×{} {} {} {:?} nodata={:?} band1 [{}, {}] categories={:?} extent={:?} in {:?}",
            r.name,
            r.width,
            r.height,
            r.band_count(),
            r.data_type,
            r.crs.label(),
            r.color,
            r.nodata,
            s.min,
            s.max,
            s.categories.as_ref().map(Vec::len),
            r.extent(),
            t.elapsed()
        );
    }
}
