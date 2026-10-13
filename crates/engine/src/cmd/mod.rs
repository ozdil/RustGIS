//! Every command, grouped by ribbon tab.

pub mod clipboard;
pub mod deeplearning;
pub mod design;
pub mod edit;
pub mod effects;
pub mod gp;
pub mod graphics;
pub mod imagery;
pub mod items;
pub mod labels;
pub mod layer;
pub mod layout;
pub mod navigate;
pub mod notebook;
pub mod plugins;
pub mod presentation;
pub mod project;
pub mod raster;
pub mod report;
pub mod select;
pub mod share;
pub mod sharing;
pub mod stac;
pub mod tiles;
pub mod view;
pub mod workflows;
pub mod workspace;

use serde_json::Value;

use crate::{CommandSpec, Session};

pub fn all() -> Vec<CommandSpec> {
    let mut v = Vec::new();
    v.extend(project::commands());
    v.extend(navigate::commands());
    v.extend(tiles::commands());
    v.extend(layer::commands());
    v.extend(raster::commands());
    v.extend(select::commands());
    v.extend(edit::commands());
    v.extend(clipboard::commands());
    v.extend(labels::commands());
    v.extend(graphics::commands());
    v.extend(layout::commands());
    v.extend(report::commands());
    v.extend(presentation::commands());
    v.extend(notebook::commands());
    v.extend(items::commands());
    v.extend(design::commands());
    v.extend(workflows::commands());
    v.extend(effects::commands());
    v.extend(imagery::commands());
    v.extend(deeplearning::commands());
    v.extend(view::commands());
    v.extend(workspace::commands());
    v.extend(gp::commands());
    v.extend(share::commands());
    v.extend(plugins::commands());
    v.extend(sharing::commands());
    v.extend(stac::commands());
    v
}

pub(crate) fn ps<'a>(p: &'a Value, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

pub(crate) fn f(p: &Value, k: &str) -> Option<f64> {
    p.get(k).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).filter(|x: &f64| x.is_finite())
}

/// A whole number from an integer, a float with no fraction, or a numeric string (forms send
/// numbers as floats).
pub(crate) fn uint(p: &Value, k: &str) -> Option<u64> {
    f(p, k).filter(|x| *x >= 0.0 && x.fract() == 0.0 && *x <= u64::MAX as f64).map(|x| x as u64)
}

pub(crate) fn b(p: &Value, k: &str) -> Option<bool> {
    p.get(k).and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| matches!(s, "true" | "yes" | "1" | "on"))))
}

pub(crate) fn need_layer(s: &Session) -> Option<&'static str> {
    match s.active_layer.and_then(|id| s.map().layer(id)) {
        Some(_) => None,
        None => Some("select a layer in the Layers pane first"),
    }
}

/// A feature (vector) layer is active.
pub(crate) fn need_feature_layer(s: &Session) -> Option<&'static str> {
    match s.active_layer.and_then(|id| s.map().layer(id)) {
        Some(l) if !l.is_raster() => None,
        Some(_) => Some("this works on feature layers, not rasters"),
        None => Some("select a layer in the Layers pane first"),
    }
}

pub(crate) fn need_layers(s: &Session) -> Option<&'static str> {
    if s.map().layers.is_empty() { Some("the map has no layers") } else { None }
}

pub(crate) fn need_selection(s: &Session) -> Option<&'static str> {
    if s.map().selection_count() == 0 { Some("nothing is selected") } else { None }
}
