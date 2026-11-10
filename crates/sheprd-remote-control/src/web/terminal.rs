//! Terminal tabs: the page for one pane, its screen as the stream sends it,
//! and what the phone types or asks of Herdr. Everything goes through
//! Herdr's public API, which neither focuses nor resizes anything, so a
//! phone watching or typing leaves the desktop's layout alone. A Herdr 2
//! pane is watched as an observer, and typing into one takes its driver's
//! role at its current size. Only panes of the layout's terminal tabs and
//! chats are reachable.

use super::{ApiError, Device, Shared, render, render::now_ms};
use crate::hub::{ApiMethod, Layout};
use axum::{
    Json,
    extract::{Path, State},
    response::Html,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// The most keys one request sends.
const MAX_KEYS: usize = 64;
/// The longest paste one request sends.
const MAX_TEXT: usize = 64 * 1024;

/// The lines of history one read asks for, the most Herdr gives.
const HISTORY_LINES: u32 = 1000;

/// Key names Herdr's `pane.send_keys` reads besides single characters.
const NAMED_KEYS: &[&str] = &[
    // keep-sorted start
    "backspace",
    "backtick",
    "down",
    "enter",
    "esc",
    "left",
    "minus",
    "plus",
    "right",
    "slash",
    "space",
    "tab",
    "up",
    // keep-sorted end
];
const MODIFIERS: &[&str] = &["alt", "ctrl", "shift"];

/// Whether `key` is one key in Herdr's syntax: modifiers joined by `+`,
/// then a named key, a function key, or one printable character.
pub(crate) fn valid_key(key: &str) -> bool {
    if key.is_empty() || key.len() > 32 {
        return false;
    }
    let mut parts: Vec<&str> = key.split('+').collect();
    let Some(last) = parts.pop() else {
        return false;
    };
    let modifiers_ok = parts.iter().all(|part| MODIFIERS.contains(part));
    let mut chars = last.chars();
    let single = matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_control() && !c.is_whitespace());
    let function = last
        .strip_prefix('f')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=12).contains(&number));
    modifiers_ok && (single || function || NAMED_KEYS.contains(&last))
}

/// The terminal pane `pane` names in `layout`, or not found.
fn known(layout: &Layout, pane: &str) -> Result<(), ApiError> {
    layout.terminal(pane).map(|_| ()).ok_or(ApiError::NotFound)
}

/// The pane's visible screen, as text with SGR colors.
pub(crate) async fn read_screen(state: &Shared, pane: &str) -> Result<String, ApiError> {
    Ok(read_scrolled(state, pane, 0, None).await?.text)
}

/// A Herdr 2 screen as one view scrolled it.
#[derive(Debug, Default)]
pub(crate) struct Scrolled {
    pub(crate) text: String,
    /// History lines above the screen.
    pub(crate) history: usize,
    /// Lines the view is scrolled up now, after output moved its lines.
    pub(crate) up: usize,
    /// Lines pushed into history so far, for the view's next read.
    pub(crate) lines: Option<u64>,
}

/// The pane's screen `up` lines into its history, for a view that last
/// read at `since` lines pushed: only a Herdr 2 pane scrolls its reads
/// and counts its history; Herdr 1's attach scrolls the terminal itself.
pub(crate) async fn read_scrolled(
    state: &Shared,
    pane: &str,
    up: usize,
    since: Option<u64>,
) -> Result<Scrolled, ApiError> {
    let mut params = json!({"pane_id": pane, "source": "visible", "format": "ansi"});
    if up > 0 {
        params["scroll"] = json!(up);
        if let Some(since) = since {
            params["since"] = json!(since);
        }
    }
    let read = state.hub.call(ApiMethod::PaneRead, params).await?;
    let read = &read["read"];
    let count = |key: &str| {
        read[key]
            .as_u64()
            .and_then(|lines| usize::try_from(lines).ok())
            .unwrap_or(0)
    };
    Ok(Scrolled {
        text: read["text"].as_str().unwrap_or_default().to_owned(),
        history: count("history"),
        up: count("scroll"),
        lines: read["lines"].as_u64(),
    })
}

pub(super) async fn fragment(
    State(state): State<Shared>,
    _device: Device,
    Path(pane): Path<String>,
) -> Result<Html<String>, ApiError> {
    // Checked before reading, so a chat's pane is never read.
    known(&state.hub.layout(), &pane)?;
    let text = read_screen(&state, &pane).await?;
    let layout = state.hub.layout();
    state.mark_opened(&layout, &pane);
    let (workspace, terminal) = layout.terminal(&pane).ok_or(ApiError::NotFound)?;
    let row = terminal
        .panes
        .iter()
        .find(|candidate| candidate.pane == pane)
        .ok_or(ApiError::NotFound)?;
    let screen = render::screen(&text);
    Ok(Html(
        render::terminal(workspace, terminal, row, &screen).into_string(),
    ))
}

/// The pane's history down to its last line, for the phone to scroll. Asked
/// for when the phone scrolls back rather than streamed with the screen: it
/// is far larger, and the stream resends what it follows on every change.
pub(super) async fn history(
    State(state): State<Shared>,
    _device: Device,
    Path(pane): Path<String>,
) -> Result<Html<String>, ApiError> {
    // Checked before reading, so a chat's pane is never read.
    known(&state.hub.layout(), &pane)?;
    let read = state
        .hub
        .call(
            ApiMethod::PaneRead,
            json!({
                "pane_id": pane,
                "source": "recent",
                "lines": HISTORY_LINES,
                "format": "ansi",
            }),
        )
        .await?;
    let text = read["read"]["text"].as_str().unwrap_or_default();
    let screen = render::screen(crate::ansi::tail(text));
    Ok(Html(render::terminal_screen(&screen).into_string()))
}

#[derive(Deserialize)]
pub(super) struct Keys {
    keys: Vec<String>,
}

pub(super) async fn keys(
    State(state): State<Shared>,
    device: Device,
    Path(pane): Path<String>,
    Json(request): Json<Keys>,
) -> Result<Json<Value>, ApiError> {
    known(&state.hub.layout(), &pane)?;
    if request.keys.is_empty()
        || request.keys.len() > MAX_KEYS
        || !request.keys.iter().all(|key| valid_key(key))
    {
        return Err(ApiError::Invalid("those keys cannot be sent"));
    }
    state
        .hub
        .call(
            ApiMethod::PaneSendKeys,
            json!({"pane_id": pane, "keys": request.keys}),
        )
        .await?;
    state.typed.typed_by(&pane, &device.0);
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
pub(super) struct Paste {
    text: String,
}

/// Text pasted as a whole: Herdr wraps it in a bracketed paste when the
/// program asked for one.
pub(super) async fn paste(
    State(state): State<Shared>,
    device: Device,
    Path(pane): Path<String>,
    Json(request): Json<Paste>,
) -> Result<Json<Value>, ApiError> {
    known(&state.hub.layout(), &pane)?;
    if request.text.is_empty() || request.text.len() > MAX_TEXT {
        return Err(ApiError::Invalid("that text cannot be pasted"));
    }
    state
        .hub
        .call(
            ApiMethod::PaneSendText,
            json!({"pane_id": pane, "text": request.text}),
        )
        .await?;
    state.typed.typed_by(&pane, &device.0);
    Ok(Json(json!({})))
}

/// A picture pasted into a terminal: stored like a chat's upload, then its
/// path typed into the pane as a terminal file drop would, so a program such
/// as Claude Code attaches it, as the pane's WSL distribution reads the path
/// when it is in one. The phone never names the path; this computer chooses
/// it.
pub(super) async fn upload(
    State(state): State<Shared>,
    device: Device,
    Path(pane): Path<String>,
    bytes: axum::body::Bytes,
) -> Result<Json<Value>, ApiError> {
    known(&state.hub.layout(), &pane)?;
    let id = crate::auth::random_id()?;
    let stored = state.clone();
    let path = tokio::task::spawn_blocking(move || {
        let now = now_ms();
        let id = stored.uploads.store(id, &bytes, now)?;
        stored.uploads.path(&id, now)
    })
    .await
    .map_err(|_| ApiError::Invalid("the upload was interrupted"))??;
    let text = match state.hub.wsl_path(&pane, &path).await? {
        Some(path) => dropped_unix_path(&path),
        None => dropped_path(&path),
    };
    state
        .hub
        .call(
            ApiMethod::PaneSendText,
            json!({"pane_id": pane, "text": text}),
        )
        .await?;
    state.typed.typed_by(&pane, &device.0);
    Ok(Json(json!({})))
}

/// `path` as a terminal types a dropped file: shell characters escaped with
/// a backslash, and a space after it so typing goes on.
pub(crate) fn dropped_path(path: &std::path::Path) -> String {
    let path = path.to_string_lossy();
    if cfg!(windows) {
        return format!("\"{path}\" ");
    }
    dropped_unix_path(&path)
}

/// [`dropped_path`] for a Unix shell, whichever computer this is.
pub(crate) fn dropped_unix_path(path: &str) -> String {
    let mut text = String::with_capacity(path.len() + 8);
    for character in path.chars() {
        if !(character.is_alphanumeric()
            || matches!(character, '/' | '.' | '_' | '-' | '+' | ',' | ':' | '@'))
        {
            text.push('\\');
        }
        text.push(character);
    }
    text.push(' ');
    text
}

/// What the action sheet asks of Herdr.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    // keep-sorted start
    ClosePane,
    CloseTab,
    NewTab,
    SplitDown,
    SplitRight,
    Zoom,
    // keep-sorted end
}

#[derive(Deserialize)]
pub(super) struct ActionRequest {
    action: Action,
}

/// The Herdr method and parameters for `action` on `pane` in `tab`.
pub(crate) fn action_call(
    action: Action,
    pane: &str,
    tab: &str,
    workspace: &str,
) -> (ApiMethod, Value) {
    match action {
        Action::SplitRight | Action::SplitDown => (
            ApiMethod::PaneSplit,
            json!({
                "target_pane_id": pane,
                "direction": if action == Action::SplitRight { "right" } else { "down" },
                "focus": false,
            }),
        ),
        Action::Zoom => (
            ApiMethod::PaneZoom,
            json!({"pane_id": pane, "mode": "toggle"}),
        ),
        Action::ClosePane => (ApiMethod::PaneClose, json!({"pane_id": pane})),
        Action::NewTab => (
            ApiMethod::TabCreate,
            json!({"workspace_id": workspace, "focus": false}),
        ),
        Action::CloseTab => (ApiMethod::TabClose, json!({"tab_id": tab})),
    }
}

/// Runs an action from the sheet; answers where the phone goes next: the
/// new pane or tab, or home when the pane is gone. A chat's pane is a
/// Herdr pane too, so the phone closes an agent the same way.
pub(super) async fn action(
    State(state): State<Shared>,
    _device: Device,
    Path(pane): Path<String>,
    Json(request): Json<ActionRequest>,
) -> Result<Json<Value>, ApiError> {
    let (tab, workspace) = {
        let layout = state.hub.layout();
        layout
            .terminal(&pane)
            .map(|(workspace, terminal)| (terminal.tab.clone(), workspace.id.clone()))
            .or_else(|| {
                layout
                    .chat(&pane)
                    .map(|(workspace, chat)| (chat.tab.clone(), workspace.id.clone()))
            })
            .ok_or(ApiError::NotFound)?
    };
    // herdr-core keeps no layout: Sheprd or this daemon makes the change.
    let op = match request.action {
        Action::SplitRight => Some(crate::control::layout::Op::SplitRight),
        Action::SplitDown => Some(crate::control::layout::Op::SplitDown),
        Action::Zoom => Some(crate::control::layout::Op::Zoom),
        Action::ClosePane | Action::CloseTab | Action::NewTab => None,
    };
    if let Some(op) = op
        && let Some(keeper) = state.hub.core_keeper(&tab, &pane)?
    {
        let created = crate::native::changes::arrange(&state, keeper, op).await?;
        // A split shows its new shell next, as Herdr 1's does.
        let next = match created {
            Some(address) => state.hub.core_pane_listed(&address).await,
            None => None,
        }
        .unwrap_or(pane);
        let location = format!("/term/{}", render::urlencode(&next));
        return Ok(Json(json!({ "location": location, "pane": next })));
    }
    // A Herdr 2 tab is named before its terminals end (they leave the
    // listing as they do), and its keeper drops it after, or its last pane
    // would stay in Sheprd's saved tab, ended.
    let saved_tab = (request.action == Action::CloseTab)
        .then(|| state.hub.core_keeper(&tab, &pane).ok().flatten())
        .flatten();
    let core_tab = if request.action == Action::NewTab {
        state.hub.new_core_tab(&workspace).await?
    } else {
        None
    };
    let result = match core_tab {
        Some(result) => result,
        None => {
            let (method, params) = action_call(request.action, &pane, &tab, &workspace);
            state.hub.call(method, params).await?
        }
    };
    if let Some(keeper) = saved_tab {
        let op = crate::control::layout::Op::Close;
        if let Err(error) = crate::native::changes::arrange(&state, keeper, op).await {
            tracing::debug!(%error, "Sheprd did not drop a Herdr 2 tab a phone closed");
        }
    }
    let created = result["pane"]["pane_id"]
        .as_str()
        .or_else(|| result["root_pane"]["pane_id"].as_str())
        .filter(|id| !id.is_empty());
    let next = next_pane(request.action, &pane, created);
    let location = next.map_or_else(
        || "/".to_owned(),
        |next| format!("/term/{}", render::urlencode(next)),
    );
    // `location` for the web app, `pane` for the native one.
    Ok(Json(json!({ "location": location, "pane": next })))
}

/// The pane the phone shows after `action` on `pane`: the one it created,
/// the same one, or none (home) when it is gone.
pub(crate) fn next_pane<'a>(
    action: Action,
    pane: &'a str,
    created: Option<&'a str>,
) -> Option<&'a str> {
    match (action, created) {
        (Action::SplitRight | Action::SplitDown | Action::NewTab, Some(created)) => Some(created),
        (Action::ClosePane | Action::CloseTab, _) => None,
        _ => Some(pane),
    }
}

/// The longest name the phone gives a chat or a tab, in characters.
pub(crate) const MAX_LABEL: usize = 200;

#[derive(Deserialize)]
pub(super) struct RenameRequest {
    label: String,
}

/// What the phone renames: a chat by its pane, whose label the chat's
/// title reads first, a terminal by its tab, whose label its row shows, or
/// a workspace by its id.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenameTarget<'a> {
    Chat { pane: &'a str },
    Tab { tab: &'a str },
    Workspace { workspace: &'a str },
}

/// The Herdr method and parameters that give `target` the name `label`.
/// An empty name clears a chat's label, so its title is the agent's again;
/// a tab or a workspace always has a name.
pub(crate) fn rename_call(
    target: RenameTarget<'_>,
    label: &str,
) -> Result<(ApiMethod, Value), ApiError> {
    let label = label.trim();
    if label.chars().count() > MAX_LABEL {
        return Err(ApiError::Invalid("that name is too long"));
    }
    match target {
        RenameTarget::Chat { pane } if label.is_empty() => {
            Ok((ApiMethod::PaneRename, json!({ "pane_id": pane })))
        }
        RenameTarget::Chat { pane } => Ok((
            ApiMethod::PaneRename,
            json!({ "pane_id": pane, "label": label }),
        )),
        RenameTarget::Tab { .. } if label.is_empty() => {
            Err(ApiError::Invalid("a tab needs a name"))
        }
        RenameTarget::Tab { tab } => Ok((
            ApiMethod::TabRename,
            json!({ "tab_id": tab, "label": label }),
        )),
        RenameTarget::Workspace { .. } if label.is_empty() => {
            Err(ApiError::Invalid("a workspace needs a name"))
        }
        RenameTarget::Workspace { workspace } => Ok((
            ApiMethod::WorkspaceRename,
            json!({ "workspace_id": workspace, "label": label }),
        )),
    }
}

/// Renames the chat `pane` runs, or the terminal tab it is in.
pub(super) async fn rename(
    State(state): State<Shared>,
    _device: Device,
    Path(pane): Path<String>,
    Json(request): Json<RenameRequest>,
) -> Result<Json<Value>, ApiError> {
    let (chat, tab) = {
        let layout = state.hub.layout();
        match layout.chat(&pane) {
            Some((_, chat)) => (true, chat.tab.clone()),
            None => {
                let (_, terminal) = layout.terminal(&pane).ok_or(ApiError::NotFound)?;
                (false, terminal.tab.clone())
            }
        }
    };
    let label = request.label.trim();
    if label.chars().count() > MAX_LABEL {
        return Err(ApiError::Invalid("that name is too long"));
    }
    // herdr-core keeps no names: Sheprd or this daemon keeps them. A blank
    // name gives a tab its program's title back, as Sheprd's menu does.
    if let Some(keeper) = state.hub.core_keeper(&tab, &pane)? {
        let name = Some(label.to_owned()).filter(|name| !name.is_empty());
        let op = if chat {
            crate::control::layout::Op::RenamePane(name)
        } else {
            crate::control::layout::Op::RenameTab(name)
        };
        crate::native::changes::arrange(&state, keeper, op).await?;
        return Ok(Json(json!({})));
    }
    let target = if chat {
        RenameTarget::Chat { pane: &pane }
    } else {
        RenameTarget::Tab { tab: &tab }
    };
    let (method, params) = rename_call(target, label)?;
    state.hub.call(method, params).await?;
    Ok(Json(json!({})))
}

/// Names a workspace, which every client of the computer then shows.
pub(super) async fn rename_workspace(
    State(state): State<Shared>,
    _device: Device,
    Path(workspace): Path<String>,
    Json(request): Json<RenameRequest>,
) -> Result<Json<Value>, ApiError> {
    if state.hub.layout().workspace(&workspace).is_none() {
        return Err(ApiError::NotFound);
    }
    let target = RenameTarget::Workspace {
        workspace: &workspace,
    };
    let (method, params) = rename_call(target, &request.label)?;
    let name = request.label.trim();
    // A workspace Sheprd keeps for Herdr 2 is Sheprd's to rename; while it
    // is closed, its saved tabs are renamed for it.
    if state.hub.kept_workspace(&workspace) {
        if !state.changes.open() {
            state.hub.rename_kept(workspace, name.to_owned()).await?;
            return Ok(Json(json!({})));
        }
        let op = crate::control::layout::Op::RenameWorkspace {
            workspace,
            name: name.to_owned(),
        };
        crate::native::changes::change_layout(&state, 0, String::new(), op).await?;
        return Ok(Json(json!({})));
    }
    // The groups this daemon lists for Herdr 2 it names itself.
    if state.hub.rename_core_group(&workspace, name).await? {
        return Ok(Json(json!({})));
    }
    state.hub.call(method, params).await?;
    Ok(Json(json!({})))
}
