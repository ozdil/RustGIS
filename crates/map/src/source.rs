//! Streamed layers: a vector source (GeoPackage, GeoParquet, FlatGeobuf) read in place, a view's
//! worth at a time.
//!
//! The layer's `dataset.features` hold a window onto the source: the features in the last view
//! read, padded so small pans don't read again, at most [`VIEW_LIMIT`] of them (an evenly spread
//! sample when more match). Reads run on a background thread on the desktop, so the map stays
//! responsive; in the browser they run in place. The window isn't saved with the project: the
//! source is opened and read again.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rustgis_data::source::{Batch, FeatureSource, Request, SourceInfo};
use rustgis_geom::Extent;
use serde::{Deserialize, Serialize};

/// Sources with more features than this are streamed when added; smaller ones are loaded whole.
pub const STREAM_ABOVE: u64 = 100_000;

/// The most features a view reads.
pub const VIEW_LIMIT: usize = if cfg!(target_arch = "wasm32") { 100_000 } else { 250_000 };

/// A streamed layer's source.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SourceLayer {
    /// The file it reads; reopened when the project loads.
    #[serde(default)]
    pub path: Option<String>,
    /// The file's own name (for its format) when it is embedded.
    #[serde(default)]
    pub file_name: Option<String>,
    /// The GeoPackage table.
    #[serde(default)]
    pub table: Option<String>,
    /// The original file, kept in the project when there is no path (a browser upload).
    #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::raster::b64")]
    pub embedded: Option<Arc<Vec<u8>>>,
    #[serde(skip)]
    handle: Option<Arc<dyn FeatureSource>>,
    #[serde(skip)]
    window: Option<Window>,
    #[serde(skip)]
    pending: Option<Arc<Pending>>,
    /// Why the source couldn't be opened or read.
    #[serde(skip)]
    pub error: Option<String>,
    /// The most features a view reads (0 = [`VIEW_LIMIT`]).
    #[serde(skip)]
    pub limit: usize,
    /// Reads installed so far: the features changing with it came from the source, not an edit.
    #[serde(skip)]
    pub reads: u64,
    /// For a layer read through GDAL: the original file, converted again into the cache when the
    /// converted copy is gone (desktop only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gdal: Option<GdalOrigin>,
    /// For a layer from a DuckDB query or database table: a table is converted again when the
    /// result in the cache is gone (desktop only); a query is not run again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duckdb: Option<DuckdbOrigin>,
}

/// Where a layer from DuckDB came from: a query, or a table of a database.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DuckdbOrigin {
    /// The query (for a table, none).
    #[serde(default)]
    pub sql: Option<String>,
    /// The database opened for the query, or holding the table.
    #[serde(default)]
    pub database: Option<String>,
    /// The table, as `table` or `schema.table`.
    #[serde(default)]
    pub table: Option<String>,
    /// The coordinate system given for a geometry that doesn't say.
    #[serde(default)]
    pub crs: Option<String>,
    /// The geometry family of the result this layer shows.
    pub family: rustgis_data::source::Family,
}

/// Where a layer read through GDAL came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GdalOrigin {
    pub path: String,
    pub layer: String,
    #[serde(default)]
    pub family: Option<rustgis_data::source::Family>,
    /// The coordinate system given for a file that doesn't say.
    #[serde(default)]
    pub crs: Option<String>,
}

impl std::fmt::Debug for SourceLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceLayer")
            .field("path", &self.path)
            .field("file_name", &self.file_name)
            .field("table", &self.table)
            .field("window", &self.window)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// Converts a DuckDB layer's table again: the result's part of the layer's geometry family.
/// A query is never run again here: a project file is hostile input, and a query can read any file
/// and send it to any address. Its user runs it again.
#[cfg(not(target_arch = "wasm32"))]
fn rerun(d: &DuckdbOrigin) -> rustgis_data::Result<std::path::PathBuf> {
    use rustgis_data::source::duckdb;
    let out = match (&d.table, d.database.as_deref().map(duckdb::Database::parse)) {
        (Some(t), Some(db)) => duckdb::convert_table_of(&db, &duckdb::find_table_of(&db, t)?, d.crs.as_deref())?,
        _ => {
            return Err(rustgis_data::DataError::Format(
                "the query's result is no longer in the cache: run the query again (Add Data From SQL)".into(),
            ));
        }
    };
    out.parts
        .into_iter()
        .find(|p| p.family == d.family)
        .map(|p| p.path)
        .ok_or_else(|| rustgis_data::DataError::Format(format!("the query returns no {} any more", d.family.label())))
}

/// What the layer's features hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    /// The extent read (longitude/latitude).
    pub extent: Extent,
    /// Whether every feature in it was read.
    pub complete: bool,
    /// How many features matched it.
    pub matched: u64,
    /// How many were read.
    pub kept: usize,
}

/// A read in flight.
struct Pending {
    extent: Extent,
    result: Mutex<Option<Result<Batch, String>>>,
}

/// What [`SourceLayer::poll`] found.
pub enum Polled {
    /// Nothing new.
    Idle,
    /// A read is still running.
    Waiting,
    /// A read finished: install its features.
    Done(Window, Batch),
    /// A read failed.
    Failed(String),
}

impl SourceLayer {
    /// A source read from a file on disk.
    pub fn from_path(path: impl Into<String>, table: Option<String>, handle: Box<dyn FeatureSource>) -> Self {
        SourceLayer { path: Some(path.into()), table, handle: Some(Arc::from(handle)), ..SourceLayer::default() }
    }

    /// A source read from a file's bytes, kept in the project.
    pub fn from_bytes(file_name: impl Into<String>, bytes: Arc<Vec<u8>>, table: Option<String>, handle: Box<dyn FeatureSource>) -> Self {
        SourceLayer {
            file_name: Some(file_name.into()),
            embedded: Some(bytes),
            table,
            handle: Some(Arc::from(handle)),
            ..SourceLayer::default()
        }
    }

    /// Opens the source again (after a project loads).
    pub fn open(&mut self) -> Result<(), String> {
        let src = if let Some(bytes) = &self.embedded {
            let name = self.file_name.as_deref().unwrap_or("layer.parquet");
            rustgis_data::source::open_shared(name, bytes.clone(), self.table.as_deref())
        } else if let Some(p) = &self.path {
            let opened = rustgis_data::source::open_path(std::path::Path::new(p), self.table.as_deref());
            match (opened, &self.gdal) {
                // The converted copy is gone (a cleared cache, another machine): convert again.
                #[cfg(not(target_arch = "wasm32"))]
                // (Only when it is missing: a copy that's there but unreadable is reported as is.)
                (Err(_), Some(g)) if !std::path::Path::new(p).exists() => {
                    match rustgis_data::source::gdal::convert_one(std::path::Path::new(&g.path), &g.layer, g.family, g.crs.as_deref()) {
                        Ok(Some(fresh)) => {
                            let r = rustgis_data::source::open_path(&fresh, None);
                            if r.is_ok() {
                                self.path = Some(fresh.display().to_string());
                            }
                            r
                        }
                        Ok(None) => Err(rustgis_data::DataError::Format(format!("{}: the layer has no features", g.path))),
                        Err(e) => Err(e),
                    }
                }
                (r, _) => match (r, &self.duckdb) {
                    #[cfg(not(target_arch = "wasm32"))]
                    (Err(_), Some(d)) if !std::path::Path::new(p).exists() => match rerun(d) {
                        Ok(fresh) => {
                            let r = rustgis_data::source::open_path(&fresh, None);
                            if r.is_ok() {
                                self.path = Some(fresh.display().to_string());
                            }
                            r
                        }
                        Err(e) => Err(e),
                    },
                    (r, _) => r,
                },
            }
        } else {
            return Err("the layer has no source".into());
        };
        match src {
            Ok(s) => {
                self.handle = Some(Arc::from(s));
                self.window = None;
                self.error = None;
                Ok(())
            }
            Err(e) => {
                let e = e.to_string();
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    pub fn is_open(&self) -> bool {
        self.handle.is_some()
    }

    pub fn info(&self) -> Option<&SourceInfo> {
        self.handle.as_deref().map(|h| h.info())
    }

    /// Every feature of the source, up to the read limit.
    pub fn read_all(&self) -> Result<(rustgis_data::Dataset, rustgis_data::ReadReport), String> {
        let h = self.handle.as_deref().ok_or("the layer's source isn't open")?;
        rustgis_data::source::materialize(h).map_err(|e| e.to_string())
    }

    pub fn window(&self) -> Option<&Window> {
        self.window.as_ref()
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Forgets the window, so the next view reads again (after the source changes).
    pub fn reset(&mut self) {
        self.window = None;
        self.pending = None;
    }

    /// The extent to read for a view (longitude/latitude), or `None` when the window serves it.
    pub fn wanted(&self, view: &Extent) -> Option<Extent> {
        let v = clamp_world(view);
        if v.is_empty() {
            return None;
        }
        if let Some(w) = &self.window {
            let all = self.info().map(|i| i.extent).filter(|e| !e.is_empty());
            // Everything has been read: nothing more to fetch, wherever the view goes.
            if w.complete && all.is_some_and(|a| contains(&w.extent, &a)) {
                return None;
            }
            // The window covers the view, and is complete or not much larger (a view zoomed well
            // inside a sampled window reads again, for the detail the sample left out).
            if contains(&w.extent, &v) && (w.complete || area(&v) * 6.0 > area(&w.extent)) {
                return None;
            }
        }
        // One read at a time per layer: while one runs, the view waits for it, and the next
        // frame after it lands asks again for wherever the view has gone (so fast panning
        // doesn't pile up reads).
        if self.pending.is_some() {
            return None;
        }
        Some(clamp_world(&v.padded(0.5, 0.0)))
    }

    /// Starts reading an extent: on a background thread unless `block` (or in the browser).
    pub fn start(&mut self, extent: Extent, block: bool) {
        let Some(h) = self.handle.clone() else { return };
        let req = Request { extent: Some(extent), limit: if self.limit == 0 { VIEW_LIMIT } else { self.limit } };
        let pending = Arc::new(Pending { extent, result: Mutex::new(None) });
        let p = pending.clone();
        let job = move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h.read(&req)))
                .unwrap_or_else(|_| Err(rustgis_data::DataError::Format("the read failed unexpectedly".into())))
                .map_err(|e| e.to_string());
            if let Ok(mut slot) = p.result.lock() {
                *slot = Some(r);
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        if !block {
            let spawned = std::thread::Builder::new().name("rustgis-source".into()).spawn(move || {
                job();
                GENERATION.fetch_add(1, Ordering::Relaxed);
                if let Some(w) = waker().lock().ok().and_then(|w| w.clone()) {
                    w();
                }
            });
            match spawned {
                Ok(_) => self.pending = Some(pending),
                // No thread to read on: say so, and don't ask again until the view moves on.
                Err(e) => {
                    self.error = Some(format!("couldn't start reading the layer: {e}"));
                    self.window = Some(Window { extent, complete: true, matched: 0, kept: 0 });
                }
            }
            return;
        }
        let _ = block;
        job();
        self.pending = Some(pending);
    }

    /// Takes a finished read, if any. With `block`, waits for one in flight (up to a minute).
    pub fn poll(&mut self, block: bool) -> Polled {
        let Some(p) = self.pending.clone() else { return Polled::Idle };
        let start = web_time::Instant::now();
        loop {
            let done = p.result.lock().ok().and_then(|mut r| r.take());
            match done {
                Some(Ok(b)) => {
                    self.pending = None;
                    let w = Window { extent: p.extent, complete: b.complete, matched: b.matched, kept: b.features.len() };
                    self.window = Some(w);
                    self.error = None;
                    return Polled::Done(w, b);
                }
                Some(Err(e)) => {
                    self.pending = None;
                    self.error = Some(e.clone());
                    // Not read again until the view moves past it.
                    self.window = Some(Window { extent: p.extent, complete: true, matched: 0, kept: 0 });
                    return Polled::Failed(e);
                }
                None if block && cfg!(not(target_arch = "wasm32")) && start.elapsed() < web_time::Duration::from_secs(60) => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                // An export that waited a minute for the read: say so rather than draw what's
                // there as if it were the view. (The read carries on; the next view takes it.)
                None if block && cfg!(not(target_arch = "wasm32")) => {
                    let e = "reading the layer took more than a minute".to_string();
                    self.error = Some(e.clone());
                    return Polled::Failed(e);
                }
                None => return Polled::Waiting,
            }
        }
    }

    /// (features read, features matching) when the window is a sample of the view.
    pub fn sampled(&self) -> Option<(usize, u64)> {
        self.window.as_ref().filter(|w| !w.complete).map(|w| (w.kept, w.matched))
    }
}

fn area(e: &Extent) -> f64 {
    e.width() * e.height()
}

fn contains(outer: &Extent, inner: &Extent) -> bool {
    !outer.is_empty()
        && !inner.is_empty()
        && outer.min_x <= inner.min_x
        && outer.min_y <= inner.min_y
        && outer.max_x >= inner.max_x
        && outer.max_y >= inner.max_y
}

fn clamp_world(e: &Extent) -> Extent {
    if e.is_empty() {
        return *e;
    }
    let c = Extent::new(e.min_x.max(-180.0), e.min_y.max(-90.0), e.max_x.min(180.0), e.max_y.min(90.0));
    if c.is_empty() { Extent::EMPTY } else { c }
}

static GENERATION: AtomicU64 = AtomicU64::new(0);

fn waker() -> &'static Mutex<Option<Arc<dyn Fn() + Send + Sync>>> {
    static W: OnceLock<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(None))
}

/// Called whenever a background read finishes (the UI asks for a repaint).
pub fn set_waker(f: impl Fn() + Send + Sync + 'static) {
    if let Ok(mut w) = waker().lock() {
        *w = Some(Arc::new(f));
    }
}

/// Bumped whenever a background read finishes, so a cached map image knows to redraw.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}
