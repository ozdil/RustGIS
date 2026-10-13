//! L1: the feature model (geometry + typed attributes) and vector I/O.
//!
//! Coordinates are stored as WGS 84 longitude/latitude. Readers never panic on malformed input:
//! bad features are skipped and counted in [`ReadReport`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod geometry;
pub mod georss;
pub mod gpx;
pub mod interop;
pub mod io;
pub mod kml;
pub mod landxml;
pub mod mbtiles;
pub mod ogc;
pub mod query;
pub mod raster;
pub mod sheets;
pub mod source;
pub mod sqlite;
pub mod value;
pub mod wkb;
pub mod zip;

pub use geometry::{Geometry, GeometryType};
pub use rustgis_geom::{Coord, Extent};
pub use value::{Field, FieldType, Value};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Format(String),
    #[error("unsupported file type: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, DataError>;

/// One row: an optional geometry and one value per field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: u64,
    pub geometry: Option<Geometry>,
    pub attrs: Vec<Value>,
}

/// A feature class: a named table of features sharing a schema and geometry type.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Dataset {
    pub name: String,
    pub geometry_type: GeometryType,
    pub fields: Vec<Field>,
    pub features: Vec<Feature>,
    /// Where it was read from, if anywhere.
    #[serde(default)]
    pub source: Option<String>,
    /// The features are a window onto a source read in place (the part of a large file in view),
    /// so they aren't saved: the source is read again instead.
    #[serde(skip)]
    pub windowed: bool,
}

impl Serialize for Dataset {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Dataset", 5)?;
        st.serialize_field("name", &self.name)?;
        st.serialize_field("geometry_type", &self.geometry_type)?;
        st.serialize_field("fields", &self.fields)?;
        let none: &[Feature] = &[];
        st.serialize_field("features", if self.windowed { none } else { &self.features })?;
        st.serialize_field("source", &self.source)?;
        st.end()
    }
}

/// What a reader skipped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadReport {
    pub skipped: usize,
    pub messages: Vec<String>,
}

impl Dataset {
    pub fn new(name: impl Into<String>, geometry_type: GeometryType, fields: Vec<Field>) -> Self {
        Dataset { name: name.into(), geometry_type, fields, features: Vec::new(), source: None, windowed: false }
    }

    /// Index of a field by case-insensitive name.
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name.eq_ignore_ascii_case(name))
    }

    /// Value of `field` for feature `row` (Null when out of range).
    pub fn value(&self, row: usize, field: usize) -> &Value {
        self.features.get(row).and_then(|f| f.attrs.get(field)).unwrap_or(&Value::Null)
    }

    pub fn extent(&self) -> Extent {
        self.features.iter().filter_map(|f| f.geometry.as_ref()).fold(Extent::EMPTY, |e, g| e.union(&g.extent()))
    }

    /// Pushes a feature with the next id, padding/truncating attributes to the schema.
    pub fn push(&mut self, geometry: Option<Geometry>, mut attrs: Vec<Value>) {
        attrs.resize(self.fields.len(), Value::Null);
        let id = self.features.iter().map(|f| f.id).max().map(|m| m.saturating_add(1)).unwrap_or(1);
        self.features.push(Feature { id, geometry, attrs });
    }

    /// Pushes a feature with the id one past the last feature's, padding/truncating attributes:
    /// for readers filling a new dataset, where [`Dataset::push`]'s search for the largest id
    /// would make reading quadratic.
    pub fn push_next(&mut self, geometry: Option<Geometry>, mut attrs: Vec<Value>) {
        attrs.resize(self.fields.len(), Value::Null);
        let id = self.features.last().map(|f| f.id.saturating_add(1)).unwrap_or(1);
        self.features.push(Feature { id, geometry, attrs });
    }

    /// Adds a field (if missing) and returns its index.
    pub fn ensure_field(&mut self, name: &str, ty: FieldType) -> usize {
        if let Some(i) = self.field_index(name) {
            return i;
        }
        self.fields.push(Field::new(name, ty));
        for f in &mut self.features {
            f.attrs.push(Value::Null);
        }
        self.fields.len() - 1
    }

    /// Numeric (min, max) of a field over all features, ignoring non-numbers.
    pub fn numeric_range(&self, field: usize) -> Option<(f64, f64)> {
        let mut r: Option<(f64, f64)> = None;
        for f in &self.features {
            if let Some(v) = f.attrs.get(field).and_then(Value::as_f64) {
                r = Some(match r {
                    Some((a, b)) => (a.min(v), b.max(v)),
                    None => (v, v),
                });
            }
        }
        r
    }

    /// Distinct display values of a field, sorted, capped at `limit`.
    pub fn unique_values(&self, field: usize, limit: usize) -> Vec<String> {
        let mut v: Vec<String> = self.features.iter().map(|f| f.attrs.get(field).map(|x| x.to_string()).unwrap_or_default()).collect();
        v.sort();
        v.dedup();
        v.truncate(limit);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windowed_features_are_not_saved() {
        let mut d = Dataset::new("t", GeometryType::Point, vec![Field::new("a", FieldType::Integer)]);
        d.push(Some(Geometry::Point(Coord::new(1.0, 2.0))), vec![Value::Int(5)]);
        let back: Dataset = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
        d.windowed = true;
        let back: Dataset = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert!(back.features.is_empty() && !back.windowed);
        assert_eq!(back.fields, d.fields);
    }

    #[test]
    fn push_pads_attrs_and_assigns_ids() {
        let mut d = Dataset::new("t", GeometryType::Point, vec![Field::new("a", FieldType::Integer), Field::new("b", FieldType::Text)]);
        d.push(Some(Geometry::Point(Coord::new(1.0, 2.0))), vec![Value::Int(5)]);
        d.push(None, vec![Value::Int(1), Value::Null, Value::Int(9)]);
        assert_eq!(d.features[0].attrs.len(), 2);
        assert_eq!(d.features[1].attrs.len(), 2);
        assert_eq!(d.features[1].id, 2);
        assert_eq!(d.numeric_range(0), Some((1.0, 5.0)));
        assert_eq!(d.value(99, 99), &Value::Null);
    }
}
