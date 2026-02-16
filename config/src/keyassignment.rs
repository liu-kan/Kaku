use crate::default_true;
use crate::keys::KeyNoAction;
use crate::window::WindowLevel;
use luahelper::impl_lua_conversion_dynamic;
use ordered_float::NotNan;
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use wezterm_dynamic::{FromDynamic, FromDynamicOptions, ToDynamic, Value};
use wezterm_input_types::{KeyCode, Modifiers};
use wezterm_term::input::MouseButton;
use wezterm_term::SemanticType;

#[derive(Default, Debug, Clone, FromDynamic, ToDynamic, PartialEq, Eq)]
pub struct LauncherActionArgs {
    pub flags: LauncherFlags,
    pub title: Option<String>,
    pub help_text: Option<String>,
    pub fuzzy_help_text: Option<String>,
    pub alphabet: Option<String>,
}

bitflags::bitflags! {
    #[derive(Default,  FromDynamic, ToDynamic)]
    #[dynamic(try_from="String", into="String")]
    pub struct LauncherFlags :u32 {
        const ZERO = 0;
        const FUZZY = 1;
        const TABS = 2;
        const LAUNCH_MENU_ITEMS = 4;
        const DOMAINS = 8;
        const KEY_ASSIGNMENTS = 16;
        const WORKSPACES = 32;
        const COMMANDS = 64;
        const PANE_ENCODINGS = 128;
    }
}

impl From<LauncherFlags> for String {
    fn from(val: LauncherFlags) -> Self {
        val.to_string()
    }
}

impl From<&LauncherFlags> for String {
    fn from(val: &LauncherFlags) -> Self {
        val.to_string()
    }
}

impl ToString for LauncherFlags {
    fn to_string(&self) -> String {
        let mut s = vec![];
        if self.contains(Self::FUZZY) {
            s.push("FUZZY");
        }
        if self.contains(Self::TABS) {
            s.push("TABS");
        }
        if self.contains(Self::LAUNCH_MENU_ITEMS) {
            s.push("LAUNCH_MENU_ITEMS");
        }
        if self.contains(Self::DOMAINS) {
            s.push("DOMAINS");
        }
        if self.contains(Self::KEY_ASSIGNMENTS) {
            s.push("KEY_ASSIGNMENTS");
        }
        if self.contains(Self::WORKSPACES) {
            s.push("WORKSPACES");
        }
        if self.contains(Self::COMMANDS) {
            s.push("COMMANDS");
        }
        if self.contains(Self::PANE_ENCODINGS) {
            s.push("PANE_ENCODINGS");
        }
        s.join("|")
    }
}

impl TryFrom<String> for LauncherFlags {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        let mut flags = LauncherFlags::default();

        for ele in s.split('|') {
            let ele = ele.trim();
            match ele {
                "FUZZY" => flags |= Self::FUZZY,
                "TABS" => flags |= Self::TABS,
                "LAUNCH_MENU_ITEMS" => flags |= Self::LAUNCH_MENU_ITEMS,
                "DOMAINS" => flags |= Self::DOMAINS,
                "KEY_ASSIGNMENTS" => flags |= Self::KEY_ASSIGNMENTS,
                "WORKSPACES" => flags |= Self::WORKSPACES,
                "COMMANDS" => flags |= Self::COMMANDS,
                "PANE_ENCODINGS" => flags |= Self::PANE_ENCODINGS,
                _ => {
                    return Err(format!("invalid LauncherFlags `{}` in `{}`", ele, s));
                }
            }
        }

        Ok(flags)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, FromDynamic, ToDynamic)]
pub enum SelectionMode {
    Cell,
    Word,
    Line,
    SemanticZone,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum Pattern {
    CaseSensitiveString(String),
    CaseInSensitiveString(String),
    Regex(String),
    CurrentSelectionOrEmptyString,
}

impl Pattern {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::CaseSensitiveString(s) | Self::CaseInSensitiveString(s) | Self::Regex(s) => {
                s.is_empty()
            }
            Self::CurrentSelectionOrEmptyString => true,
        }
    }
}

impl Default for Pattern {
    fn default() -> Self {
        Self::CurrentSelectionOrEmptyString
    }
}

/// A mouse event that can trigger an action
#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Hash, FromDynamic, ToDynamic)]
pub enum MouseEventTrigger {
    /// Mouse button is pressed. streak is how many times in a row
    /// it was pressed.
    Down { streak: usize, button: MouseButton },
    /// Mouse button is held down while the cursor is moving. streak is how many times in a row
    /// it was pressed, with the last of those being held to form the drag.
    Drag { streak: usize, button: MouseButton },
    /// Mouse button is being released. streak is how many times
    /// in a row it was pressed and released.
    Up { streak: usize, button: MouseButton },
}

/// When spawning a tab, specify which domain should be used to
/// host/spawn that tab.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum SpawnTabDomain {
    /// Use the default domain
    DefaultDomain,
    /// Use the domain from the current tab in the associated window
    CurrentPaneDomain,
    /// Use a specific domain by name
    DomainName(String),
    /// Use a specific domain by id
    DomainId(usize),
}

impl Default for SpawnTabDomain {
    fn default() -> Self {
        Self::CurrentPaneDomain
    }
}

#[derive(
    Debug, Copy, Clone, Deserialize, Serialize, PartialEq, Eq, FromDynamic, ToDynamic, Default,
)]
pub enum PaneEncoding {
    #[default]
    Utf8 = 0,
    Gbk = 1,
    Gb18030 = 2,
    Big5 = 3,
    ShiftJis = 4,
    EucKr = 5,
}

impl std::fmt::Display for PaneEncoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Utf8 => write!(f, "UTF-8"),
            Self::Gbk => write!(f, "GBK"),
            Self::Gb18030 => write!(f, "GB18030"),
            Self::Big5 => write!(f, "Big5"),
            Self::ShiftJis => write!(f, "Shift-JIS"),
            Self::EucKr => write!(f, "EUC-KR"),
        }
    }
}

/// Tracks the most recently user-selected pane encoding for dynamic menu reordering.
static LAST_SELECTED_ENCODING: AtomicU8 = AtomicU8::new(0);

impl PaneEncoding {
    /// Default encoding order: UTF-8, GBK, GB18030, Big5, EUC-KR, Shift-JIS.
    pub const DEFAULT_ORDER: [PaneEncoding; 6] = [
        PaneEncoding::Utf8,
        PaneEncoding::Gbk,
        PaneEncoding::Gb18030,
        PaneEncoding::Big5,
        PaneEncoding::EucKr,
        PaneEncoding::ShiftJis,
    ];

    /// Convert a numeric value to a `PaneEncoding` variant.
    /// Values correspond to the discriminants of the enum
    /// (0 = Utf8, 1 = Gbk, 2 = Gb18030, 3 = Big5, 4 = ShiftJis,
    /// 5 = EucKr).  Unknown values default to `Utf8`.
    pub fn from_u8(val: u8) -> PaneEncoding {
        match val {
            1 => PaneEncoding::Gbk,
            2 => PaneEncoding::Gb18030,
            3 => PaneEncoding::Big5,
            4 => PaneEncoding::ShiftJis,
            5 => PaneEncoding::EucKr,
            _ => PaneEncoding::Utf8,
        }
    }

    /// Record the most recently selected encoding for menu reordering.
    pub fn set_last_selected(encoding: PaneEncoding) {
        LAST_SELECTED_ENCODING.store(encoding as u8, Ordering::Relaxed);
    }

    /// Return encodings in display order: UTF-8 always first, the most
    /// recently selected encoding second, remaining encodings in default
    /// order.  If the last selection was UTF-8 (or none), no reordering
    /// is performed.
    pub fn ordered_list() -> Vec<PaneEncoding> {
        let last = LAST_SELECTED_ENCODING.load(Ordering::Relaxed);
        let last_encoding = PaneEncoding::from_u8(last);

        let mut result = Vec::with_capacity(6);
        result.push(PaneEncoding::Utf8);

        if last_encoding != PaneEncoding::Utf8 {
            result.push(last_encoding);
            for &enc in &Self::DEFAULT_ORDER {
                if enc != PaneEncoding::Utf8 && enc != last_encoding {
                    result.push(enc);
                }
            }
        } else {
            for &enc in &Self::DEFAULT_ORDER[1..] {
                result.push(enc);
            }
        }

        result
    }
}

#[derive(Default, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct SpawnCommand {
    /// Optional descriptive label
    pub label: Option<String>,

    /// The command line to use.
    /// If omitted, the default command associated with the
    /// domain will be used instead, which is typically the
    /// shell for the user.
    pub args: Option<Vec<String>>,

    /// Specifies the current working directory for the command.
    /// If omitted, a default will be used; typically that will
    /// be the home directory of the user, but may also be the
    /// current working directory of the wezterm process when
    /// it was launched, or for some domains it may be some
    /// other location appropriate to the domain.
    pub cwd: Option<PathBuf>,

    /// Specifies a map of environment variables that should be set.
    /// Whether this is used depends on the domain.
    #[dynamic(default)]
    pub set_environment_variables: HashMap<String, String>,

    #[dynamic(default)]
    pub domain: SpawnTabDomain,

    #[dynamic(default)]
    pub encoding: Option<PaneEncoding>,

    pub position: Option<crate::GuiPosition>,
}
impl_lua_conversion_dynamic!(SpawnCommand);

impl std::fmt::Debug for SpawnCommand {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self)
    }
}

impl std::fmt::Display for SpawnCommand {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "SpawnCommand")?;
        if let Some(label) = &self.label {
            write!(fmt, " label='{}'", label)?;
        }
        write!(fmt, " domain={:?}", self.domain)?;
        if let Some(args) = &self.args {
            write!(fmt, " args={:?}", args)?;
        }
        if let Some(cwd) = &self.cwd {
            write!(fmt, " cwd={}", cwd.display())?;
        }
        for (k, v) in &self.set_environment_variables {
            write!(fmt, " {}={}", k, v)?;
        }
        if let Some(encoding) = &self.encoding {
            write!(fmt, " encoding={encoding}")?;
        }
        Ok(())
    }
}

impl SpawnCommand {
    pub fn label_for_palette(&self) -> Option<String> {
        if let Some(label) = &self.label {
            Some(label.to_string())
        } else if let Some(args) = &self.args {
            Some(shlex::try_join(args.iter().map(|s| s.as_str())).ok()?)
        } else {
            None
        }
    }

    pub fn from_command_builder(cmd: &CommandBuilder) -> anyhow::Result<Self> {
        let mut args = vec![];
        let mut set_environment_variables = HashMap::new();
        for arg in cmd.get_argv() {
            // Use lossy conversion instead of hard-failing so that
            // non-UTF-8 paths (e.g. GBK filenames on Linux) don't
            // prevent spawning.
            args.push(arg.to_string_lossy().into_owned());
        }
        for (k, v) in cmd.iter_full_env_as_str() {
            set_environment_variables.insert(k.to_string(), v.to_string());
        }
        let cwd = match cmd.get_cwd() {
            Some(cwd) => Some(PathBuf::from(cwd)),
            None => None,
        };
        Ok(Self {
            label: None,
            domain: SpawnTabDomain::DefaultDomain,
            args: if args.is_empty() { None } else { Some(args) },
            set_environment_variables,
            cwd,
            encoding: None,
            position: None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, FromDynamic, ToDynamic)]
pub enum PaneDirection {
    Up,
    Down,
    Left,
    Right,
    Next,
    Prev,
}

impl PaneDirection {
    pub fn direction_from_str(arg: &str) -> Result<PaneDirection, String> {
        for candidate in PaneDirection::variants() {
            if candidate.to_lowercase() == arg.to_lowercase() {
                if let Ok(direction) = PaneDirection::from_dynamic(
                    &Value::String(candidate.to_string()),
                    FromDynamicOptions::default(),
                ) {
                    return Ok(direction);
                }
            }
        }
        Err(format!(
            "invalid direction {arg}, possible values are {:?}",
            PaneDirection::variants()
        ))
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, FromDynamic, ToDynamic, Serialize, Deserialize)]
pub enum ScrollbackEraseMode {
    ScrollbackOnly,
    ScrollbackAndViewport,
}

impl Default for ScrollbackEraseMode {
    fn default() -> Self {
        Self::ScrollbackOnly
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum ClipboardCopyDestination {
    Clipboard,
    PrimarySelection,
    ClipboardAndPrimarySelection,
}
impl_lua_conversion_dynamic!(ClipboardCopyDestination);

impl Default for ClipboardCopyDestination {
    fn default() -> Self {
        Self::ClipboardAndPrimarySelection
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum ClipboardPasteSource {
    Clipboard,
    PrimarySelection,
}

impl Default for ClipboardPasteSource {
    fn default() -> Self {
        Self::Clipboard
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum PaneSelectMode {
    Activate,
    SwapWithActive,
    SwapWithActiveKeepFocus,
    MoveToNewTab,
    MoveToNewWindow,
}

impl Default for PaneSelectMode {
    fn default() -> Self {
        Self::Activate
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
pub struct PaneSelectArguments {
    /// Overrides the main quick_select_alphabet config
    #[dynamic(default)]
    pub alphabet: String,

    #[dynamic(default)]
    pub mode: PaneSelectMode,

    #[dynamic(default)]
    pub show_pane_ids: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum CharSelectGroup {
    RecentlyUsed,
    SmileysAndEmotion,
    PeopleAndBody,
    AnimalsAndNature,
    FoodAndDrink,
    TravelAndPlaces,
    Activities,
    Objects,
    Symbols,
    Flags,
    NerdFonts,
    UnicodeNames,
    ShortCodes,
}

// next is default, previous is the reverse
macro_rules! char_select_group_impl_next_prev {
    ($($x:ident => $y:ident),+ $(,)?) => {
        impl CharSelectGroup {
            pub const fn next(self) -> Self {
                match self {
                    $(CharSelectGroup::$x => CharSelectGroup::$y),+
                }
            }

            pub const fn previous(self) -> Self {
                match self {
                    $(CharSelectGroup::$y => CharSelectGroup::$x),+
                }
            }
        }
    };
}

char_select_group_impl_next_prev! (
    RecentlyUsed => SmileysAndEmotion,
    SmileysAndEmotion => PeopleAndBody,
    PeopleAndBody => AnimalsAndNature,
    AnimalsAndNature => FoodAndDrink,
    FoodAndDrink => TravelAndPlaces,
    TravelAndPlaces => Activities,
    Activities => Objects,
    Objects => Symbols,
    Symbols => Flags,
    Flags => NerdFonts,
    NerdFonts => UnicodeNames,
    UnicodeNames => ShortCodes,
    ShortCodes => RecentlyUsed,
);

impl Default for CharSelectGroup {
    fn default() -> Self {
        Self::SmileysAndEmotion
    }
}

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
pub struct CharSelectArguments {
    #[dynamic(default)]
    pub group: Option<CharSelectGroup>,
    #[dynamic(default = "default_true")]
    pub copy_on_select: bool,
    #[dynamic(default)]
    pub copy_to: ClipboardCopyDestination,
}

impl Default for CharSelectArguments {
    fn default() -> Self {
        Self {
            group: None,
            copy_on_select: true,
            copy_to: ClipboardCopyDestination::default(),
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct QuickSelectArguments {
    /// Overrides the main quick_select_alphabet config
    #[dynamic(default)]
    pub alphabet: String,
    /// Overrides the main quick_select_patterns config
    #[dynamic(default)]
    pub patterns: Vec<String>,
    #[dynamic(default)]
    pub action: Option<Box<KeyAssignment>>,
    /// Skip triggering `action` after paste is performed (capital selection)
    #[dynamic(default)]
    pub skip_action_on_paste: bool,
    /// Label to use in place of "copy" when `action` is set
    #[dynamic(default)]
    pub label: String,
    /// How many lines before and how many lines after the viewport to
    /// search to produce the quickselect results
    pub scope_lines: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct PromptInputLine {
    pub action: Box<KeyAssignment>,
    /// Optional label to pre-fill the input line with
    #[dynamic(default)]
    pub initial_value: Option<String>,
    /// Descriptive text to show ahead of prompt
    #[dynamic(default)]
    pub description: String,
    /// Text to show for prompt
    #[dynamic(default = "default_prompt")]
    pub prompt: String,
}

fn default_prompt() -> String {
    "> ".to_string()
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct InputSelectorEntry {
    pub label: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct InputSelector {
    pub action: Box<KeyAssignment>,
    #[dynamic(default)]
    pub title: String,

    pub choices: Vec<InputSelectorEntry>,

    #[dynamic(default)]
    pub fuzzy: bool,

    #[dynamic(default = "default_num_alphabet")]
    pub alphabet: String,

    #[dynamic(default = "default_description")]
    pub description: String,

    #[dynamic(default = "default_fuzzy_description")]
    pub fuzzy_description: String,
}

fn default_num_alphabet() -> String {
    "1234567890abcdefghilmnopqrstuvwxyz".to_string()
}

fn default_description() -> String {
    "Select an item and press Enter = accept,  Esc = cancel,  / = filter".to_string()
}

fn default_fuzzy_description() -> String {
    "Fuzzy matching: ".to_string()
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct Confirmation {
    pub action: Box<KeyAssignment>,
    #[dynamic(default)]
    pub cancel: Option<Box<KeyAssignment>>,
    /// Text to show for confirmation
    #[dynamic(default = "default_message")]
    pub message: String,
}

fn default_message() -> String {
    "🛑 Really continue?".to_string()
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub enum KeyAssignment {
    SpawnTab(SpawnTabDomain),
    SpawnWindow,
    ToggleFullScreen,
    ToggleAlwaysOnTop,
    ToggleAlwaysOnBottom,
    SetWindowLevel(WindowLevel),
    CopyTo(ClipboardCopyDestination),
    CopyTextTo {
        text: String,
        destination: ClipboardCopyDestination,
    },
    PasteFrom(ClipboardPasteSource),
    ActivateTabRelative(isize),
    ActivateTabRelativeNoWrap(isize),
    IncreaseFontSize,
    DecreaseFontSize,
    ResetFontSize,
    ResetFontAndWindowSize,
    ActivateTab(isize),
    ActivateLastTab,
    SendString(String),
    SendKey(KeyNoAction),
    Nop,
    DisableDefaultAssignment,
    Hide,
    Show,
    CloseCurrentTab {
        confirm: bool,
    },
    ReloadConfiguration,
    MoveTabRelative(isize),
    MoveTab(usize),
    ScrollByPage(NotNan<f64>),
    ScrollByLine(isize),
    ScrollByCurrentEventWheelDelta,
    ScrollToPrompt(isize),
    ScrollToTop,
    ScrollToBottom,
    ShowTabNavigator,
    ShowDebugOverlay,
    HideApplication,
    QuitApplication,
    SpawnCommandInNewTab(SpawnCommand),
    SpawnCommandInNewWindow(SpawnCommand),
    SplitHorizontal(SpawnCommand),
    SplitVertical(SpawnCommand),
    ShowLauncher,
    ShowLauncherArgs(LauncherActionArgs),
    ClearScrollback(ScrollbackEraseMode),
    Search(Pattern),
    ActivateCopyMode,

    SelectTextAtMouseCursor(SelectionMode),
    ExtendSelectionToMouseCursor(SelectionMode),
    OpenLinkAtMouseCursor,
    ClearSelection,
    CompleteSelection(ClipboardCopyDestination),
    CompleteSelectionOrOpenLinkAtMouseCursor(ClipboardCopyDestination),
    StartWindowDrag,

    AdjustPaneSize(PaneDirection, usize),
    ActivatePaneDirection(PaneDirection),
    ActivatePaneByIndex(usize),
    TogglePaneZoomState,
    SetPaneZoomState(bool),
    SetPaneEncoding(PaneEncoding),
    CloseCurrentPane {
        confirm: bool,
    },
    EmitEvent(String),
    QuickSelect,
    QuickSelectArgs(QuickSelectArguments),

    Multiple(Vec<KeyAssignment>),

    SwitchToWorkspace {
        name: Option<String>,
        spawn: Option<SpawnCommand>,
    },
    SwitchWorkspaceRelative(isize),

    ActivateKeyTable {
        name: String,
        #[dynamic(default)]
        timeout_milliseconds: Option<u64>,
        #[dynamic(default)]
        replace_current: bool,
        #[dynamic(default = "crate::default_true")]
        one_shot: bool,
        #[dynamic(default)]
        until_unknown: bool,
        #[dynamic(default)]
        prevent_fallback: bool,
    },
    PopKeyTable,
    ClearKeyTableStack,
    DetachDomain(SpawnTabDomain),
    AttachDomain(String),

    CopyMode(CopyModeAssignment),
    RotatePanes(RotationDirection),
    SplitPane(SplitPane),
    PaneSelect(PaneSelectArguments),
    CharSelect(CharSelectArguments),

    ResetTerminal,
    OpenUri(String),
    ActivateCommandPalette,
    ActivateWindow(usize),
    ActivateWindowRelative(isize),
    ActivateWindowRelativeNoWrap(isize),
    PromptInputLine(PromptInputLine),
    InputSelector(InputSelector),
    Confirmation(Confirmation),
}
impl_lua_conversion_dynamic!(KeyAssignment);

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct SplitPane {
    pub direction: PaneDirection,
    #[dynamic(default)]
    pub size: SplitSize,
    #[dynamic(default)]
    pub command: SpawnCommand,
    #[dynamic(default)]
    pub top_level: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum SplitSize {
    Cells(usize),
    Percent(u8),
}

impl Default for SplitSize {
    fn default() -> Self {
        Self::Percent(50)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum RotationDirection {
    Clockwise,
    CounterClockwise,
}

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum CopyModeAssignment {
    MoveToViewportBottom,
    MoveToViewportTop,
    MoveToViewportMiddle,
    MoveToScrollbackTop,
    MoveToScrollbackBottom,
    SetSelectionMode(Option<SelectionMode>),
    ClearSelectionMode,
    MoveToStartOfLineContent,
    MoveToEndOfLineContent,
    MoveToStartOfLine,
    MoveToStartOfNextLine,
    MoveToSelectionOtherEnd,
    MoveToSelectionOtherEndHoriz,
    MoveBackwardWord,
    MoveForwardWord,
    MoveForwardWordEnd,
    MoveRight,
    MoveLeft,
    MoveUp,
    MoveDown,
    MoveByPage(NotNan<f64>),
    PageUp,
    PageDown,
    Close,
    PriorMatch,
    NextMatch,
    PriorMatchPage,
    NextMatchPage,
    CycleMatchType,
    ClearPattern,
    EditPattern,
    AcceptPattern,
    MoveBackwardSemanticZone,
    MoveForwardSemanticZone,
    MoveBackwardZoneOfType(SemanticType),
    MoveForwardZoneOfType(SemanticType),
    JumpForward { prev_char: bool },
    JumpBackward { prev_char: bool },
    JumpAgain,
    JumpReverse,
}

pub type KeyTable = HashMap<(KeyCode, Modifiers), KeyTableEntry>;

#[derive(Debug, Clone, Default)]
pub struct KeyTables {
    pub default: KeyTable,
    pub by_name: HashMap<String, KeyTable>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyTableEntry {
    pub action: KeyAssignment,
}

#[cfg(test)]
mod tests {
    use super::{PaneEncoding, SpawnCommand, LAST_SELECTED_ENCODING};
    use std::sync::atomic::Ordering;

    /// Reset global state before each ordering test.
    fn reset_last_selected() {
        LAST_SELECTED_ENCODING.store(0, Ordering::Relaxed);
    }

    #[test]
    fn pane_encoding_default_is_utf8() {
        assert_eq!(PaneEncoding::default(), PaneEncoding::Utf8);
    }

    #[test]
    fn spawn_command_default_has_no_explicit_encoding() {
        assert_eq!(SpawnCommand::default().encoding, None);
    }

    #[test]
    fn from_u8_round_trip() {
        for &enc in &PaneEncoding::DEFAULT_ORDER {
            assert_eq!(PaneEncoding::from_u8(enc as u8), enc);
        }
        // Unknown values map to Utf8
        assert_eq!(PaneEncoding::from_u8(99), PaneEncoding::Utf8);
    }

    #[test]
    fn default_order_is_correct() {
        assert_eq!(
            PaneEncoding::DEFAULT_ORDER,
            [
                PaneEncoding::Utf8,
                PaneEncoding::Gbk,
                PaneEncoding::Gb18030,
                PaneEncoding::Big5,
                PaneEncoding::EucKr,
                PaneEncoding::ShiftJis,
            ]
        );
    }

    #[test]
    fn ordered_list_default_no_selection() {
        reset_last_selected();
        let list = PaneEncoding::ordered_list();
        assert_eq!(list.len(), 6);
        assert_eq!(list[0], PaneEncoding::Utf8);
        assert_eq!(list, PaneEncoding::DEFAULT_ORDER);
    }

    #[test]
    fn ordered_list_utf8_selected_no_reorder() {
        reset_last_selected();
        PaneEncoding::set_last_selected(PaneEncoding::Utf8);
        let list = PaneEncoding::ordered_list();
        assert_eq!(list[0], PaneEncoding::Utf8);
        assert_eq!(list, PaneEncoding::DEFAULT_ORDER);
    }

    #[test]
    fn ordered_list_gbk_selected() {
        reset_last_selected();
        PaneEncoding::set_last_selected(PaneEncoding::Gbk);
        let list = PaneEncoding::ordered_list();
        assert_eq!(list[0], PaneEncoding::Utf8);
        assert_eq!(list[1], PaneEncoding::Gbk);
        assert_eq!(list.len(), 6);
        // Gbk should not appear again after position 1
        assert!(!list[2..].contains(&PaneEncoding::Gbk));
    }

    #[test]
    fn ordered_list_big5_selected() {
        reset_last_selected();
        PaneEncoding::set_last_selected(PaneEncoding::Big5);
        let list = PaneEncoding::ordered_list();
        assert_eq!(list[0], PaneEncoding::Utf8);
        assert_eq!(list[1], PaneEncoding::Big5);
        assert_eq!(list.len(), 6);
        assert!(!list[2..].contains(&PaneEncoding::Big5));
    }

    #[test]
    fn ordered_list_shift_jis_selected() {
        reset_last_selected();
        PaneEncoding::set_last_selected(PaneEncoding::ShiftJis);
        let list = PaneEncoding::ordered_list();
        assert_eq!(list[0], PaneEncoding::Utf8);
        assert_eq!(list[1], PaneEncoding::ShiftJis);
        assert_eq!(list.len(), 6);
        assert!(!list[2..].contains(&PaneEncoding::ShiftJis));
    }

    #[test]
    fn ordered_list_euc_kr_selected() {
        reset_last_selected();
        PaneEncoding::set_last_selected(PaneEncoding::EucKr);
        let list = PaneEncoding::ordered_list();
        assert_eq!(list[0], PaneEncoding::Utf8);
        assert_eq!(list[1], PaneEncoding::EucKr);
        assert_eq!(list.len(), 6);
        assert!(!list[2..].contains(&PaneEncoding::EucKr));
    }

    #[test]
    fn ordered_list_gb18030_selected() {
        reset_last_selected();
        PaneEncoding::set_last_selected(PaneEncoding::Gb18030);
        let list = PaneEncoding::ordered_list();
        assert_eq!(list[0], PaneEncoding::Utf8);
        assert_eq!(list[1], PaneEncoding::Gb18030);
        assert_eq!(list.len(), 6);
        assert!(!list[2..].contains(&PaneEncoding::Gb18030));
    }

    #[test]
    fn ordered_list_all_encodings_present() {
        for &enc in &PaneEncoding::DEFAULT_ORDER {
            reset_last_selected();
            PaneEncoding::set_last_selected(enc);
            let list = PaneEncoding::ordered_list();
            assert_eq!(list.len(), 6);
            // Every encoding from DEFAULT_ORDER must appear exactly once
            for &expected in &PaneEncoding::DEFAULT_ORDER {
                assert!(
                    list.contains(&expected),
                    "ordered_list after selecting {:?} is missing {:?}",
                    enc,
                    expected
                );
            }
        }
    }

    #[test]
    fn ordered_list_no_duplicates() {
        use std::collections::HashSet;
        for &enc in &PaneEncoding::DEFAULT_ORDER {
            reset_last_selected();
            PaneEncoding::set_last_selected(enc);
            let list = PaneEncoding::ordered_list();
            let mut seen = HashSet::new();
            for item in &list {
                assert!(
                    seen.insert(*item as u8),
                    "duplicate {:?} after selecting {:?}",
                    item,
                    enc
                );
            }
        }
    }

    #[test]
    fn set_last_selected_overwrites_previous() {
        reset_last_selected();
        PaneEncoding::set_last_selected(PaneEncoding::Gbk);
        PaneEncoding::set_last_selected(PaneEncoding::Big5);
        let list = PaneEncoding::ordered_list();
        assert_eq!(list[0], PaneEncoding::Utf8);
        assert_eq!(list[1], PaneEncoding::Big5);
    }
}
