// Bindgen happens here
wit_bindgen_wrpc::generate!({
  with: {
      "wasmcloud:libsql/types@0.1.0-draft": generate,
      "wasmcloud:libsql/execute@0.1.0-draft": generate,
      "wasmcloud:libsql/query@0.1.0-draft": generate,
  },
});

// Start bindgen-generated type imports
pub(crate) use exports::wasmcloud::libsql::types;
pub(crate) use exports::wasmcloud::libsql::{execute, query};

pub(crate) use types::LibsqlValue;

impl From<LibsqlValue> for libsql::Value {
    fn from(value: LibsqlValue) -> Self {
        match value {
            LibsqlValue::Null => libsql::Value::Null,
            LibsqlValue::Integer(i) => libsql::Value::Integer(i),
            LibsqlValue::Real(f) => libsql::Value::Real(f),
            LibsqlValue::Text(s) => libsql::Value::Text(s),
            LibsqlValue::Blob(b) => libsql::Value::Blob(b.to_vec()),
        }
    }
}

impl From<libsql::Value> for LibsqlValue {
    fn from(value: libsql::Value) -> Self {
        match value {
            libsql::Value::Null => LibsqlValue::Null,
            libsql::Value::Integer(i) => LibsqlValue::Integer(i),
            libsql::Value::Real(f) => LibsqlValue::Real(f),
            libsql::Value::Text(s) => LibsqlValue::Text(s),
            libsql::Value::Blob(b) => LibsqlValue::Blob(b.into()),
        }
    }
}
