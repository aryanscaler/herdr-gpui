//! Changes to the app's saved tabs made while the app is closed, as the
//! remote control daemon makes a phone's split, zoom, or rename then. Only
//! the fields a change names are touched: every other field, tab, and
//! workspace is written back as read, so what a newer app saves survives.
//! The app reads the file when it starts; while it runs, it alone writes it.

mod error;

pub use error::Error;

use super::MAX_SAVED_BYTES;
use crate::panes::{Axis, Panes};
use serde_json::{Map, Value};
use sheprd_backend::TerminalId;

/// One change to one saved tab of herdr-core terminals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change<'a> {
    /// `new` beside `at`: right of it on [`Axis::Row`], below on
    /// [`Axis::Column`]. The split shows again, as the app's does.
    Split {
        at: &'a TerminalId,
        axis: Axis,
        new: TerminalId,
    },
    /// A zoomed tab shows its split again; otherwise the pane `at` fills it
    /// and takes the keyboard, as the app's zoom does.
    Zoom(&'a TerminalId),
    /// The tab's name; `None` gives back its program's title.
    Rename(Option<&'a str>),
    /// A pane's name; `None` gives back its program's title.
    RenamePane {
        at: &'a TerminalId,
        name: Option<&'a str>,
    },
    /// The tab goes.
    Close,
}

/// `bytes` with `change` made to `scope`'s tab `tab`.
pub fn edit_tab(bytes: &[u8], scope: &str, tab: u64, change: Change<'_>) -> Result<Vec<u8>, Error> {
    let mut root = read(bytes)?;
    let tabs = root
        .get_mut("tabs")
        .and_then(Value::as_array_mut)
        .ok_or(Error::Tab)?;
    let index = tabs
        .iter()
        .position(|saved| {
            saved["scope"].as_str() == Some(scope) && saved["id"].as_u64() == Some(tab)
        })
        .ok_or(Error::Tab)?;
    if change == Change::Close {
        tabs.remove(index);
        return write(&root);
    }
    let location = tabs[index]
        .get_mut("location")
        .and_then(Value::as_object_mut)
        .filter(|location| location.get("kind").and_then(Value::as_str) == Some("core_terminal"))
        .ok_or(Error::Tab)?;
    if location.get("observe").and_then(Value::as_bool) == Some(true) {
        return Err(Error::Watching);
    }
    // Tabs saved before splits named their one terminal `terminal`.
    let saved = location
        .remove("panes")
        .or_else(|| location.remove("terminal"))
        .ok_or(Error::Tab)?;
    let mut panes: Panes = serde_json::from_value(saved)?;
    let present = |at: &TerminalId| {
        panes
            .contains(at)
            .then_some(())
            .ok_or_else(|| crate::panes::Error::Missing(at.clone()))
    };
    match change {
        Change::Split { at, axis, new } => {
            panes.split(at, axis, new)?;
            location.remove("zoomed");
        }
        Change::Zoom(at) => {
            present(at)?;
            if location.remove("zoomed").is_none() {
                location.insert("zoomed".into(), at.as_str().into());
                location.insert("active".into(), at.as_str().into());
            }
        }
        Change::Rename(name) => set(location, "name", name),
        Change::RenamePane { at, name } => {
            present(at)?;
            let names = location
                .entry("names")
                .or_insert_with(|| Value::Object(Map::new()));
            if !names.is_object() {
                *names = Value::Object(Map::new());
            }
            if let Some(names) = names.as_object_mut() {
                set(names, at.as_str(), name);
            }
            if location
                .get("names")
                .and_then(Value::as_object)
                .is_some_and(Map::is_empty)
            {
                location.remove("names");
            }
        }
        Change::Close => {}
    }
    location.insert("panes".into(), serde_json::to_value(&panes)?);
    write(&root)
}

/// `bytes` with `scope`'s workspace `id` named `name` by the user.
pub fn rename_workspace(bytes: &[u8], scope: &str, id: &str, name: &str) -> Result<Vec<u8>, Error> {
    let mut root = read(bytes)?;
    let workspace = root
        .get_mut("workspaces")
        .and_then(Value::as_array_mut)
        .and_then(|workspaces| {
            workspaces.iter_mut().find(|workspace| {
                workspace["scope"].as_str() == Some(scope) && workspace["id"].as_str() == Some(id)
            })
        })
        .and_then(Value::as_object_mut)
        .ok_or(Error::Workspace)?;
    workspace.insert("label".into(), name.into());
    workspace.insert("custom".into(), true.into());
    write(&root)
}

/// A blank or absent name removes `key`, so the program's title shows.
fn set(object: &mut Map<String, Value>, key: &str, name: Option<&str>) {
    match name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => object.insert(key.to_owned(), name.into()),
        None => object.remove(key),
    };
}

fn read(bytes: &[u8]) -> Result<Value, Error> {
    if bytes.len() > MAX_SAVED_BYTES {
        return Err(Error::Large);
    }
    Ok(serde_json::from_slice(bytes)?)
}

fn write(root: &Value) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec_pretty(root)?;
    if bytes.len() > MAX_SAVED_BYTES {
        return Err(Error::Large);
    }
    Ok(bytes)
}
