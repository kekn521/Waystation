//! Preserve Unix path bytes in persisted JSON while keeping ordinary paths readable.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    ffi::OsString,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Wire {
    Text(String),
    Raw { unix_bytes: Vec<u8> },
}
impl From<&Path> for Wire {
    fn from(p: &Path) -> Self {
        match p.to_str() {
            Some(s) => Self::Text(s.into()),
            None => Self::Raw {
                unix_bytes: p.as_os_str().as_bytes().to_vec(),
            },
        }
    }
}
impl From<Wire> for PathBuf {
    fn from(w: Wire) -> Self {
        match w {
            Wire::Text(s) => s.into(),
            Wire::Raw { unix_bytes } => OsString::from_vec(unix_bytes).into(),
        }
    }
}
pub fn serialize<S: Serializer>(p: &Path, s: S) -> Result<S::Ok, S::Error> {
    Wire::from(p).serialize(s)
}
pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PathBuf, D::Error> {
    Wire::deserialize(d).map(Into::into)
}
pub mod option {
    use super::*;
    pub fn serialize<S: Serializer>(p: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
        p.as_deref().map(Wire::from).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PathBuf>, D::Error> {
        Option::<Wire>::deserialize(d).map(|w| w.map(Into::into))
    }
}
pub mod vec {
    use super::*;
    pub fn serialize<S: Serializer>(p: &[PathBuf], s: S) -> Result<S::Ok, S::Error> {
        p.iter()
            .map(|p| Wire::from(p.as_path()))
            .collect::<Vec<_>>()
            .serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PathBuf>, D::Error> {
        Vec::<Wire>::deserialize(d).map(|w| w.into_iter().map(Into::into).collect())
    }
}
