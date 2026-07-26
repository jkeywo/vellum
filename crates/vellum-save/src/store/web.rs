//! `localStorage["<namespace>:<slot>"]`.

use core::fmt;

use super::{is_slot, Store};

/// The browser's `localStorage`, namespaced so two games served from one
/// origin — which is exactly what a GitHub Pages account is — cannot read or
/// overwrite each other's saves.
#[derive(Clone, Debug)]
pub struct LocalStorage {
    namespace: String,
}

impl LocalStorage {
    pub fn new(namespace: impl Into<String>) -> LocalStorage {
        LocalStorage {
            namespace: namespace.into(),
        }
    }

    fn key(&self, slot: &str) -> Result<String, WebError> {
        if !is_slot(slot) {
            return Err(WebError::BadSlot(slot.to_owned()));
        }
        Ok(format!("{}:{slot}", self.namespace))
    }

    fn storage() -> Result<web_sys::Storage, WebError> {
        web_sys::window()
            .ok_or(WebError::Unavailable)?
            .local_storage()
            .map_err(|_| WebError::Unavailable)?
            .ok_or(WebError::Unavailable)
    }
}

/// Why the browser would not store something.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebError {
    /// No window, or storage denied. A page in private browsing, or with
    /// site data blocked, has no `localStorage` at all — which is a state to
    /// report to the player, not to panic on.
    Unavailable,
    /// Storage exists and refused the write. In practice: the origin's quota
    /// is full.
    Refused,
    BadSlot(String),
}

impl fmt::Display for WebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WebError::Unavailable => {
                f.write_str("this browser is not allowing the page to store anything")
            }
            WebError::Refused => f.write_str("the browser refused to store more data"),
            WebError::BadSlot(slot) => write!(f, "`{slot}` is not a slot name"),
        }
    }
}

impl std::error::Error for WebError {}

impl Store for LocalStorage {
    type Error = WebError;

    fn read(&self, slot: &str) -> Result<Option<String>, Self::Error> {
        Self::storage()?
            .get_item(&self.key(slot)?)
            .map_err(|_| WebError::Unavailable)
    }

    fn write(&self, slot: &str, contents: &str) -> Result<(), Self::Error> {
        Self::storage()?
            .set_item(&self.key(slot)?, contents)
            .map_err(|_| WebError::Refused)
    }

    fn remove(&self, slot: &str) -> Result<(), Self::Error> {
        Self::storage()?
            .remove_item(&self.key(slot)?)
            .map_err(|_| WebError::Unavailable)
    }

    fn slots(&self) -> Result<Vec<String>, Self::Error> {
        let storage = Self::storage()?;
        let count = storage.length().map_err(|_| WebError::Unavailable)?;
        let prefix = format!("{}:", self.namespace);
        let mut slots = Vec::new();
        for index in 0..count {
            let Ok(Some(key)) = storage.key(index) else {
                continue;
            };
            if let Some(slot) = key.strip_prefix(&prefix) {
                if is_slot(slot) {
                    slots.push(slot.to_owned());
                }
            }
        }
        Ok(slots)
    }
}
