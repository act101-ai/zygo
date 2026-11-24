use serde::{Deserialize, Serialize};

use crate::session_dir::{write_atomic, SessionDir};
use crate::Agent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    pub session: String,
    pub agent: Agent,
    pub main_role: String,
    pub effective_role: Option<String>,
    pub role_prompt: Option<String>,
    pub user_prompt: Option<String>,
    pub profile: Option<String>,
    pub profile_prompt: Option<String>,
    pub work_item: Option<WorkItemView>,
    pub legend: Vec<LegendEntry>,
    pub disabled_roles: Vec<String>,
    pub work_units: bool,
    pub results: bool,
    #[serde(default = "default_true")]
    pub compact_tool: bool,
    pub navigation: bool,
    #[serde(default = "default_trim_threshold")]
    pub trim_threshold_bytes: u64,
    #[serde(default = "default_head_lines")]
    pub head_lines: u32,
    #[serde(default = "default_tail_lines")]
    pub tail_lines: u32,
    /// The status line style the launcher took from the user config (`statusline.style`):
    /// `compact` or `verbose`. A `view.json` without the field reads as `compact`.
    #[serde(default = "default_statusline_style")]
    pub statusline_style: String,
    /// Set by the launcher for a workspace member (AU58, AU72); `None` for every other launch
    /// and for a `view.json` without the field.
    #[serde(default)]
    pub workspace: Option<ViewWorkspace>,
    /// The top-level fields of `view.json` that this zygo does not know; a save puts them back, so a read, change and
    /// save of the view (the member hooks, H1) keeps the fields of a newer zygo (the AU78 pattern of `workspace.json`).
    /// `workspace`, its `member`, its `resume_note` and each `interrupted` entry keep theirs the same way (R5).
    #[serde(flatten)]
    pub extra: UnknownFields,
}

/// Each field that has a serde default holds that default; `agent` is Claude Code; every other field is empty or `false`.
impl Default for SessionView {
    fn default() -> Self {
        SessionView {
            session: String::new(),
            agent: Agent::ClaudeCode,
            main_role: String::new(),
            effective_role: None,
            role_prompt: None,
            user_prompt: None,
            profile: None,
            profile_prompt: None,
            work_item: None,
            legend: Vec::new(),
            disabled_roles: Vec::new(),
            work_units: false,
            results: false,
            compact_tool: default_true(),
            navigation: false,
            trim_threshold_bytes: default_trim_threshold(),
            head_lines: default_head_lines(),
            tail_lines: default_tail_lines(),
            statusline_style: default_statusline_style(),
            workspace: None,
            extra: UnknownFields::default(),
        }
    }
}

/// The fields of a `view.json` object that this zygo does not know (the AU78 pattern of `workspace.json`). Only a load
/// fills it and a save writes it back. Its map is private and the type has no setter, and no code adds a key. The
/// type still derives a public `Deserialize`, so code could build a value by deserializing a map whose key is a known
/// field name; `save` would then write that key twice and `load` would refuse the file (duplicate field). A literal of
/// another crate takes the empty value with `..Default::default()` (H1 review round 2 R6, round 3 S5).
///
/// Another crate cannot read the map field (E0616, private field):
///
/// ```compile_fail,E0616
/// let _ = zygo_core::session_view::SessionView::default().extra.0.is_empty();
/// ```
///
/// ```
/// let v = zygo_core::session_view::SessionView { session: "s".into(), ..Default::default() };
/// assert_eq!(v.extra, Default::default());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnknownFields(serde_json::Map<String, serde_json::Value>);

fn default_true() -> bool {
    true
}

fn default_trim_threshold() -> u64 {
    16384
}

fn default_head_lines() -> u32 {
    60
}

fn default_tail_lines() -> u32 {
    20
}

fn default_statusline_style() -> String {
    "compact".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkItemView {
    pub unit: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegendEntry {
    pub role: String,
    pub description: String,
    pub custom: bool,
}

/// The membership marker that the launcher records (workspaces design AU58).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub ws_id: String,
    pub pane: String,
    #[serde(flatten)]
    pub extra: UnknownFields,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubagentRef {
    pub agent_id: String,
    pub agent_type: String,
    #[serde(flatten)]
    pub extra: UnknownFields,
}

/// The inputs of the resume note (W§9.4, AU72).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResumeNote {
    pub clean_stop: bool,
    pub interrupted: Vec<SubagentRef>,
    pub closed_units: Vec<String>,
    /// The agent session that these inputs belong to: the id that `ws run` resumes, or none for a fresh start (W§9.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<String>,
    #[serde(flatten)]
    pub extra: UnknownFields,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewWorkspace {
    pub member: Member,
    pub resume_note: Option<ResumeNote>,
    #[serde(flatten)]
    pub extra: UnknownFields,
}

impl SessionView {
    /// Reads `view.json`. A FIFO does not make the read wait: on unix the open is `O_NONBLOCK`, and a FIFO is the
    /// error `<path>: is a FIFO`; every other entry gives the result of a plain read (`O_NONBLOCK` is cleared before
    /// it). A link is followed. The whole file is read; there is no size cap (FIX-22). Because of the `O_NONBLOCK`
    /// open, for a device that waits at open, such as a tty with no carrier, the wait moves from the open to the
    /// read; the read result is unchanged.
    pub fn load(dir: &SessionDir) -> anyhow::Result<Self> {
        use std::io::Read;
        let path = dir.view_json();
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
        }
        let mut f = options.open(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            use std::os::unix::io::AsRawFd;
            if f.metadata()?.file_type().is_fifo() {
                anyhow::bail!("{}: is a FIFO", path.display());
            }
            let fd = f.as_raw_fd();
            // SAFETY: `fd` is the open descriptor of `f`, which lives for the call.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags == -1 {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: `fd` is the open descriptor of `f`, which lives for the call.
            if unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) } == -1 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Unique temp file, then rename.
    pub fn save(&self, dir: &SessionDir) -> anyhow::Result<()> {
        write_atomic(&dir.view_json(), &serde_json::to_vec(self)?)
    }
}
