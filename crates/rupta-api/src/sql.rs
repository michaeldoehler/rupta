//! Helpers for building parameterized SQL.

use rupta_core::query::Param;
use rupta_store::squad::{ToSql, Value};

/// Bound parameters of a statement under construction.
#[derive(Default)]
pub struct Params(pub Vec<Value>);

impl Params {
    pub fn new() -> Params {
        Params::default()
    }

    /// Adds a value and returns its placeholder.
    pub fn bind(&mut self, v: impl ToSql) -> String {
        self.0.push(v.to_value());
        format!("${}", self.0.len())
    }

    /// The number the next placeholder will get.
    pub fn next_index(&self) -> usize {
        self.0.len() + 1
    }

    /// Adds the parameters of a compiled search query.
    pub fn extend_query(&mut self, params: &[Param]) {
        for p in params {
            self.0.push(match p {
                Param::Text(s) => s.to_value(),
                Param::Int(i) => i.to_value(),
                Param::Time(t) => t.to_value(),
            });
        }
    }

    pub fn refs(&self) -> Vec<&dyn ToSql> {
        self.0.iter().map(|v| v as &dyn ToSql).collect()
    }
}

/// `1, 2, 3` for an `IN (...)` list of trusted integers.
pub fn id_list(ids: &[i64]) -> String {
    if ids.is_empty() {
        return "NULL".into();
    }
    ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
}
