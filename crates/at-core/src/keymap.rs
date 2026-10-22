//! Keyboard shortcuts (§4.10): actions, chord parsing and platform defaults.

use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    NewTab,
    NewNestedTab,
    NewTabFromProfile,
    NewWindow,
    TabToNewWindow,
    NextWindow,
    CloseWindow,
    Quit,
    ReopenClosedTab,
    ClosePane,
    CloseTab,
    RenameTab,
    DuplicateTab,
    SelectTab(u8),
    NextTab,
    PrevTab,
    MoveTabUp,
    MoveTabDown,
    IndentTab,
    OutdentTab,
    ToggleCollapse,
    ToggleGroupView,
    ToggleGroupTiles,
    ToggleSidebar,
    NewPane,
    ToggleLayout,
    FocusUp,
    FocusDown,
    FocusLeft,
    FocusRight,
    FocusPane(u8),
    ResizeUp,
    ResizeDown,
    ResizeLeft,
    ResizeRight,
    EqualizePanes,
    ZoomPane,
    MovePaneUp,
    MovePaneDown,
    RestartShell,
    OpenFolder,
    RevealFolder,
    OpenInEditor,
    CopyCwd,
    ShowHistory,
    Find,
    Copy,
    Paste,
    SelectAll,
    ClearScreen,
    FontBigger,
    FontSmaller,
    FontReset,
    ScrollPageUp,
    ScrollPageDown,
    ScrollTop,
    ScrollBottom,
    CommandPalette,
    Settings,
    DetachedTabs,
    OpenConfigFolder,
    ShowLogs,
}

impl Action {
    pub fn all() -> Vec<Action> {
        use Action::*;
        let mut v = vec![
            NewTab,
            NewNestedTab,
            NewTabFromProfile,
            NewWindow,
            TabToNewWindow,
            NextWindow,
            CloseWindow,
            Quit,
            ReopenClosedTab,
            ClosePane,
            CloseTab,
            RenameTab,
            DuplicateTab,
        ];
        v.extend((1..=9).map(SelectTab));
        v.extend([
            NextTab,
            PrevTab,
            MoveTabUp,
            MoveTabDown,
            IndentTab,
            OutdentTab,
            ToggleCollapse,
            ToggleGroupView,
            ToggleGroupTiles,
            ToggleSidebar,
            NewPane,
            ToggleLayout,
            FocusUp,
            FocusDown,
            FocusLeft,
            FocusRight,
        ]);
        v.extend((1..=4).map(FocusPane));
        v.extend([
            ResizeUp,
            ResizeDown,
            ResizeLeft,
            ResizeRight,
            EqualizePanes,
            ZoomPane,
            MovePaneUp,
            MovePaneDown,
            RestartShell,
            OpenFolder,
            RevealFolder,
            OpenInEditor,
            CopyCwd,
            ShowHistory,
            Find,
            Copy,
            Paste,
            SelectAll,
            ClearScreen,
            FontBigger,
            FontSmaller,
            FontReset,
            ScrollPageUp,
            ScrollPageDown,
            ScrollTop,
            ScrollBottom,
            CommandPalette,
            Settings,
            DetachedTabs,
            OpenConfigFolder,
            ShowLogs,
        ]);
        v
    }

    pub fn name(&self) -> String {
        use Action::*;
        match self {
            SelectTab(n) => return format!("select_tab_{n}"),
            FocusPane(n) => return format!("focus_pane_{n}"),
            _ => {}
        }
        match self {
            NewTab => "new_tab",
            NewNestedTab => "new_nested_tab",
            NewTabFromProfile => "new_tab_from_profile",
            NewWindow => "new_window",
            TabToNewWindow => "tab_to_new_window",
            NextWindow => "next_window",
            CloseWindow => "close_window",
            Quit => "quit",
            ReopenClosedTab => "reopen_closed_tab",
            ClosePane => "close_pane",
            CloseTab => "close_tab",
            RenameTab => "rename_tab",
            DuplicateTab => "duplicate_tab",
            NextTab => "next_tab",
            PrevTab => "prev_tab",
            MoveTabUp => "move_tab_up",
            MoveTabDown => "move_tab_down",
            IndentTab => "indent_tab",
            OutdentTab => "outdent_tab",
            ToggleCollapse => "toggle_collapse",
            ToggleGroupView => "toggle_group_view",
            ToggleGroupTiles => "toggle_group_tiles",
            ToggleSidebar => "toggle_sidebar",
            NewPane => "new_pane",
            ToggleLayout => "toggle_layout",
            FocusUp => "focus_up",
            FocusDown => "focus_down",
            FocusLeft => "focus_left",
            FocusRight => "focus_right",
            ResizeUp => "resize_up",
            ResizeDown => "resize_down",
            ResizeLeft => "resize_left",
            ResizeRight => "resize_right",
            EqualizePanes => "equalize_panes",
            ZoomPane => "zoom_pane",
            MovePaneUp => "move_pane_up",
            MovePaneDown => "move_pane_down",
            RestartShell => "restart_shell",
            OpenFolder => "open_folder",
            RevealFolder => "reveal_folder",
            OpenInEditor => "open_in_editor",
            CopyCwd => "copy_cwd",
            ShowHistory => "show_history",
            Find => "find",
            Copy => "copy",
            Paste => "paste",
            SelectAll => "select_all",
            ClearScreen => "clear_screen",
            FontBigger => "font_bigger",
            FontSmaller => "font_smaller",
            FontReset => "font_reset",
            ScrollPageUp => "scroll_page_up",
            ScrollPageDown => "scroll_page_down",
            ScrollTop => "scroll_top",
            ScrollBottom => "scroll_bottom",
            CommandPalette => "command_palette",
            Settings => "settings",
            DetachedTabs => "detached_tabs",
            OpenConfigFolder => "open_config_folder",
            ShowLogs => "show_logs",
            SelectTab(_) | FocusPane(_) => unreachable!(),
        }
        .to_string()
    }

    pub fn from_name(s: &str) -> Option<Action> {
        Action::all().into_iter().find(|a| a.name() == s)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    /// ⌘ on macOS, Super on Linux.
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Normalized key name: a lowercase character or a named key (`up`, `enter`, `f5`, …).
    pub key: String,
}

pub fn normalize_key(k: &str) -> String {
    let lower = k.to_lowercase();
    match lower.as_str() {
        "return" => "enter".into(),
        "escape" => "esc".into(),
        "arrowup" => "up".into(),
        "arrowdown" => "down".into(),
        "arrowleft" => "left".into(),
        "arrowright" => "right".into(),
        "page_up" | "pgup" => "pageup".into(),
        "page_down" | "pgdn" => "pagedown".into(),
        "plus" | "+" | "equal" | "equals" => "=".into(),
        "minus" => "-".into(),
        "comma" => ",".into(),
        "period" | "dot" => ".".into(),
        "backtick" | "grave" => "`".into(),
        "backslash" => "\\".into(),
        "slash" => "/".into(),
        "bracketleft" => "[".into(),
        "bracketright" => "]".into(),
        " " | "spacebar" => "space".into(),
        "del" => "delete".into(),
        _ => lower,
    }
}

impl Chord {
    pub fn parse(s: &str) -> Result<Chord, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty chord".into());
        }
        // Allow "cmd++" style for the plus key.
        let mut parts: Vec<&str> = Vec::new();
        let mut rest = s;
        while let Some(i) = rest.find('+') {
            if i == 0 {
                break;
            }
            parts.push(&rest[..i]);
            rest = &rest[i + 1..];
        }
        parts.push(rest);
        let mut c = Chord {
            cmd: false,
            ctrl: false,
            alt: false,
            shift: false,
            key: String::new(),
        };
        let n = parts.len();
        for (i, p) in parts.iter().enumerate() {
            let pl = p.to_lowercase();
            if i + 1 == n {
                if pl.is_empty() {
                    return Err(format!("missing key in `{s}`"));
                }
                c.key = normalize_key(&pl);
            } else {
                match pl.as_str() {
                    "cmd" | "command" | "super" | "logo" | "meta" | "win" => c.cmd = true,
                    "ctrl" | "control" => c.ctrl = true,
                    "alt" | "opt" | "option" => c.alt = true,
                    "shift" => c.shift = true,
                    other => return Err(format!("unknown modifier `{other}` in `{s}`")),
                }
            }
        }
        Ok(c)
    }

    /// Human form: `⌘⇧T` on macOS, `Ctrl+Shift+T` elsewhere.
    pub fn display(&self) -> String {
        let key = match self.key.as_str() {
            "up" => "↑".to_string(),
            "down" => "↓".to_string(),
            "left" => "←".to_string(),
            "right" => "→".to_string(),
            "enter" => {
                if cfg!(target_os = "macos") {
                    "↩".into()
                } else {
                    "Enter".into()
                }
            }
            "pageup" => "PgUp".into(),
            "pagedown" => "PgDn".into(),
            "home" => "Home".into(),
            "end" => "End".into(),
            "tab" => "Tab".into(),
            "esc" => "Esc".into(),
            "space" => "Space".into(),
            k if k.chars().count() == 1 => k.to_uppercase(),
            k => {
                let mut c = k.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            }
        };
        if cfg!(target_os = "macos") {
            let mut s = String::new();
            if self.ctrl {
                s.push('⌃');
            }
            if self.alt {
                s.push('⌥');
            }
            if self.shift {
                s.push('⇧');
            }
            if self.cmd {
                s.push('⌘');
            }
            s + &key
        } else {
            let mut v = Vec::new();
            if self.cmd {
                v.push("Super");
            }
            if self.ctrl {
                v.push("Ctrl");
            }
            if self.alt {
                v.push("Alt");
            }
            if self.shift {
                v.push("Shift");
            }
            v.push(&key);
            v.join("+")
        }
    }
}

fn mac_defaults() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("new_tab", &["cmd+t"]),
        ("new_nested_tab", &["cmd+shift+t"]),
        ("new_tab_from_profile", &["cmd+alt+t"]),
        ("new_window", &["cmd+n"]),
        ("tab_to_new_window", &["cmd+shift+n"]),
        ("next_window", &["cmd+`"]),
        ("close_window", &["cmd+alt+w"]),
        ("quit", &["cmd+q"]),
        ("reopen_closed_tab", &["cmd+shift+z"]),
        ("close_pane", &["cmd+w"]),
        ("close_tab", &["cmd+shift+w"]),
        ("rename_tab", &["cmd+shift+e"]),
        ("select_tab_1", &["cmd+1"]),
        ("select_tab_2", &["cmd+2"]),
        ("select_tab_3", &["cmd+3"]),
        ("select_tab_4", &["cmd+4"]),
        ("select_tab_5", &["cmd+5"]),
        ("select_tab_6", &["cmd+6"]),
        ("select_tab_7", &["cmd+7"]),
        ("select_tab_8", &["cmd+8"]),
        ("select_tab_9", &["cmd+9"]),
        ("next_tab", &["cmd+shift+]", "ctrl+tab"]),
        ("prev_tab", &["cmd+shift+[", "ctrl+shift+tab"]),
        ("move_tab_up", &["cmd+shift+up"]),
        ("move_tab_down", &["cmd+shift+down"]),
        ("indent_tab", &["cmd+shift+right"]),
        ("outdent_tab", &["cmd+shift+left"]),
        ("toggle_collapse", &["cmd+shift+."]),
        ("toggle_group_view", &["cmd+shift+g"]),
        ("toggle_group_tiles", &["cmd+alt+g"]),
        ("toggle_sidebar", &["cmd+b"]),
        ("new_pane", &["cmd+d"]),
        ("toggle_layout", &["cmd+shift+m"]),
        ("focus_up", &["cmd+alt+up"]),
        ("focus_down", &["cmd+alt+down"]),
        ("focus_left", &["cmd+alt+left"]),
        ("focus_right", &["cmd+alt+right"]),
        ("focus_pane_1", &["cmd+alt+1"]),
        ("focus_pane_2", &["cmd+alt+2"]),
        ("focus_pane_3", &["cmd+alt+3"]),
        ("focus_pane_4", &["cmd+alt+4"]),
        ("resize_up", &["cmd+ctrl+up"]),
        ("resize_down", &["cmd+ctrl+down"]),
        ("resize_left", &["cmd+ctrl+left"]),
        ("resize_right", &["cmd+ctrl+right"]),
        ("equalize_panes", &["cmd+alt+="]),
        ("zoom_pane", &["cmd+shift+enter"]),
        ("move_pane_up", &["cmd+alt+shift+up"]),
        ("move_pane_down", &["cmd+alt+shift+down"]),
        ("restart_shell", &["cmd+shift+r"]),
        ("open_folder", &["cmd+shift+o"]),
        ("open_in_editor", &["cmd+shift+i"]),
        ("copy_cwd", &["cmd+alt+c"]),
        ("show_history", &["cmd+shift+h"]),
        ("find", &["cmd+f"]),
        ("copy", &["cmd+c"]),
        ("paste", &["cmd+v"]),
        ("select_all", &["cmd+a"]),
        ("clear_screen", &["cmd+k"]),
        ("font_bigger", &["cmd+=", "cmd+shift+="]),
        ("font_smaller", &["cmd+-"]),
        ("font_reset", &["cmd+0"]),
        ("scroll_page_up", &["shift+pageup"]),
        ("scroll_page_down", &["shift+pagedown"]),
        ("scroll_top", &["cmd+home"]),
        ("scroll_bottom", &["cmd+end"]),
        ("command_palette", &["cmd+shift+p"]),
        ("settings", &["cmd+,"]),
    ]
}

fn linux_defaults() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("new_tab", &["ctrl+shift+t"]),
        ("new_nested_tab", &["ctrl+shift+alt+t"]),
        ("new_tab_from_profile", &["ctrl+shift+alt+p"]),
        ("new_window", &["ctrl+shift+n"]),
        ("tab_to_new_window", &["ctrl+shift+alt+n"]),
        ("next_window", &["ctrl+shift+`"]),
        ("close_window", &["ctrl+alt+w"]),
        ("quit", &["ctrl+shift+q"]),
        ("reopen_closed_tab", &["ctrl+shift+z"]),
        ("close_pane", &["ctrl+shift+w"]),
        ("close_tab", &["ctrl+shift+alt+w"]),
        ("rename_tab", &["ctrl+shift+e"]),
        ("select_tab_1", &["alt+1"]),
        ("select_tab_2", &["alt+2"]),
        ("select_tab_3", &["alt+3"]),
        ("select_tab_4", &["alt+4"]),
        ("select_tab_5", &["alt+5"]),
        ("select_tab_6", &["alt+6"]),
        ("select_tab_7", &["alt+7"]),
        ("select_tab_8", &["alt+8"]),
        ("select_tab_9", &["alt+9"]),
        ("next_tab", &["ctrl+pagedown", "ctrl+tab"]),
        ("prev_tab", &["ctrl+pageup", "ctrl+shift+tab"]),
        ("move_tab_up", &["ctrl+shift+up"]),
        ("move_tab_down", &["ctrl+shift+down"]),
        ("indent_tab", &["ctrl+shift+right"]),
        ("outdent_tab", &["ctrl+shift+left"]),
        ("toggle_collapse", &["ctrl+shift+."]),
        ("toggle_group_view", &["ctrl+shift+g"]),
        ("toggle_group_tiles", &["ctrl+shift+alt+g"]),
        ("toggle_sidebar", &["ctrl+shift+b"]),
        ("new_pane", &["ctrl+shift+d"]),
        ("toggle_layout", &["ctrl+shift+m"]),
        ("focus_up", &["ctrl+alt+up"]),
        ("focus_down", &["ctrl+alt+down"]),
        ("focus_left", &["ctrl+alt+left"]),
        ("focus_right", &["ctrl+alt+right"]),
        ("focus_pane_1", &["ctrl+alt+1"]),
        ("focus_pane_2", &["ctrl+alt+2"]),
        ("focus_pane_3", &["ctrl+alt+3"]),
        ("focus_pane_4", &["ctrl+alt+4"]),
        ("resize_up", &["ctrl+shift+alt+up"]),
        ("resize_down", &["ctrl+shift+alt+down"]),
        ("resize_left", &["ctrl+shift+alt+left"]),
        ("resize_right", &["ctrl+shift+alt+right"]),
        ("equalize_panes", &["ctrl+alt+="]),
        ("zoom_pane", &["ctrl+shift+enter"]),
        ("move_pane_up", &["ctrl+alt+shift+pageup"]),
        ("move_pane_down", &["ctrl+alt+shift+pagedown"]),
        ("restart_shell", &["ctrl+shift+r"]),
        ("open_folder", &["ctrl+shift+o"]),
        ("open_in_editor", &["ctrl+shift+i"]),
        ("copy_cwd", &["ctrl+alt+c"]),
        ("show_history", &["ctrl+shift+h"]),
        ("find", &["ctrl+shift+f"]),
        ("copy", &["ctrl+shift+c"]),
        ("paste", &["ctrl+shift+v"]),
        ("select_all", &["ctrl+shift+a"]),
        ("clear_screen", &["ctrl+shift+k"]),
        ("font_bigger", &["ctrl+=", "ctrl+shift+="]),
        ("font_smaller", &["ctrl+-"]),
        ("font_reset", &["ctrl+0"]),
        ("scroll_page_up", &["shift+pageup"]),
        ("scroll_page_down", &["shift+pagedown"]),
        ("scroll_top", &["shift+home"]),
        ("scroll_bottom", &["shift+end"]),
        ("command_palette", &["ctrl+shift+p"]),
        ("settings", &["ctrl+,"]),
    ]
}

pub fn default_bindings() -> BTreeMap<String, Vec<String>> {
    let table = if cfg!(target_os = "macos") {
        mac_defaults()
    } else {
        linux_defaults()
    };
    table
        .into_iter()
        .map(|(a, cs)| (a.to_string(), cs.iter().map(|s| s.to_string()).collect()))
        .collect()
}

#[derive(Clone, Debug, Default)]
pub struct Keymap {
    by_chord: HashMap<Chord, Action>,
    by_action: BTreeMap<Action, Vec<Chord>>,
}

impl Keymap {
    /// Defaults overlaid with the `[keys]` section. Returns warnings for bad entries.
    pub fn build(overrides: &BTreeMap<String, crate::config::KeyBinding>) -> (Keymap, Vec<String>) {
        let mut warnings = Vec::new();
        let mut table = default_bindings();
        for (action, binding) in overrides {
            if Action::from_name(action).is_none() {
                warnings.push(format!("keys: unknown action `{action}`"));
                continue;
            }
            let chords: Vec<String> = binding
                .chords()
                .into_iter()
                .filter(|c| !c.eq_ignore_ascii_case("none") && !c.is_empty())
                .collect();
            table.insert(action.clone(), chords);
        }
        let mut km = Keymap::default();
        // User overrides win on conflicts: process them last.
        let mut ordered: Vec<(&String, &Vec<String>)> = table
            .iter()
            .filter(|(a, _)| !overrides.contains_key(*a))
            .collect();
        ordered.extend(table.iter().filter(|(a, _)| overrides.contains_key(*a)));
        for (name, chords) in ordered {
            let action = Action::from_name(name).unwrap();
            for c in chords {
                match Chord::parse(c) {
                    Ok(ch) => {
                        if let Some(prev) = km.by_chord.insert(ch.clone(), action)
                            && prev != action
                        {
                            warnings.push(format!(
                                "keys: `{c}` reassigned from {} to {}",
                                prev.name(),
                                action.name()
                            ));
                            if let Some(v) = km.by_action.get_mut(&prev) {
                                v.retain(|x| *x != ch);
                            }
                        }
                        km.by_action.entry(action).or_default().push(ch);
                    }
                    Err(e) => warnings.push(format!("keys.{name}: {e}")),
                }
            }
        }
        (km, warnings)
    }

    pub fn lookup(&self, chord: &Chord) -> Option<Action> {
        self.by_chord.get(chord).copied()
    }

    pub fn chords(&self, action: Action) -> &[Chord] {
        self.by_action
            .get(&action)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn display(&self, action: Action) -> String {
        self.chords(action)
            .first()
            .map(|c| c.display())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::KeyBinding;

    #[test]
    fn parse_chords() {
        let c = Chord::parse("Cmd+Shift+T").unwrap();
        assert!(c.cmd && c.shift && !c.ctrl && !c.alt);
        assert_eq!(c.key, "t");
        assert_eq!(Chord::parse("cmd++").unwrap().key, "=");
        assert_eq!(Chord::parse("ctrl+PageUp").unwrap().key, "pageup");
        assert!(Chord::parse("hyper+t").is_err());
        assert!(Chord::parse("").is_err());
    }

    #[test]
    fn names_roundtrip() {
        for a in Action::all() {
            assert_eq!(Action::from_name(&a.name()), Some(a), "{}", a.name());
        }
    }

    #[test]
    fn defaults_have_no_conflicts() {
        let (km, warnings) = Keymap::build(&BTreeMap::new());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            km.lookup(
                &Chord::parse(if cfg!(target_os = "macos") {
                    "cmd+t"
                } else {
                    "ctrl+shift+t"
                })
                .unwrap()
            ),
            Some(Action::NewTab)
        );
    }

    #[test]
    fn overrides() {
        let mut o = BTreeMap::new();
        o.insert("new_tab".to_string(), KeyBinding::One("ctrl+alt+x".into()));
        o.insert("find".to_string(), KeyBinding::One("none".into()));
        o.insert("nope".to_string(), KeyBinding::One("ctrl+y".into()));
        let (km, warnings) = Keymap::build(&o);
        assert_eq!(
            km.lookup(&Chord::parse("ctrl+alt+x").unwrap()),
            Some(Action::NewTab)
        );
        assert!(km.chords(Action::Find).is_empty());
        assert_eq!(warnings.len(), 1);
    }
}
