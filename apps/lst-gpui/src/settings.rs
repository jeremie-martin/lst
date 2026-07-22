use serde::{de, Deserialize, Deserializer, Serialize};
use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
};
use toml_edit::{value, Array, DocumentMut, Item, Table};

pub(crate) const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum InputModeSetting {
    #[default]
    Standard,
    Vim,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ThemePreference {
    System,
    Dark,
    #[default]
    Light,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AutosaveMode {
    #[default]
    Scratchpads,
    All,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LineNumbersSetting {
    #[default]
    Absolute,
    Relative,
    Hybrid,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum MatchBracketsSetting {
    Never,
    Near,
    #[default]
    Always,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum GuideMode {
    Off,
    #[default]
    Active,
    All,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RenderWhitespaceSetting {
    None,
    Boundary,
    #[default]
    Selection,
    Trailing,
    All,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub(crate) struct RulerColumns(Vec<u16>);

impl RulerColumns {
    pub(crate) const MAX_COUNT: usize = 16;
    pub(crate) const MAX_COLUMN: u16 = 1_000;

    pub(crate) fn new(mut columns: Vec<u16>) -> Result<Self, String> {
        if columns.len() > Self::MAX_COUNT {
            return Err(format!("rulers accepts at most {} columns", Self::MAX_COUNT));
        }
        if let Some(column) = columns
            .iter()
            .copied()
            .find(|column| !(1..=Self::MAX_COLUMN).contains(column))
        {
            return Err(format!(
                "ruler column {column} is outside the supported range 1..={}",
                Self::MAX_COLUMN
            ));
        }
        columns.sort_unstable();
        columns.dedup();
        Ok(Self(columns))
    }

    pub(crate) fn as_slice(&self) -> &[u16] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for RulerColumns {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<u16>::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct EditorSettings {
    pub(crate) input_mode: InputModeSetting,
    pub(crate) word_wrap: bool,
    pub(crate) line_numbers: LineNumbersSetting,
    pub(crate) cursor_blink: bool,
    pub(crate) font_family: String,
    pub(crate) font_size: u16,
    pub(crate) match_brackets: MatchBracketsSetting,
    pub(crate) bracket_pair_colorization: bool,
    pub(crate) bracket_pair_guides: GuideMode,
    pub(crate) bracket_pair_horizontal_guides: GuideMode,
    pub(crate) indent_guides: bool,
    pub(crate) highlight_active_indent_guide: bool,
    pub(crate) render_whitespace: RenderWhitespaceSetting,
    pub(crate) render_control_characters: bool,
    pub(crate) rulers: RulerColumns,
    pub(crate) smart_select_subwords: bool,
    pub(crate) smart_select_include_whitespace: bool,
    pub(crate) multi_cursor_limit: usize,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            input_mode: InputModeSetting::Standard,
            word_wrap: true,
            line_numbers: LineNumbersSetting::Absolute,
            cursor_blink: true,
            font_family: "TX-02".to_string(),
            font_size: 13,
            match_brackets: MatchBracketsSetting::Always,
            bracket_pair_colorization: true,
            bracket_pair_guides: GuideMode::Active,
            bracket_pair_horizontal_guides: GuideMode::Active,
            indent_guides: true,
            highlight_active_indent_guide: true,
            render_whitespace: RenderWhitespaceSetting::Selection,
            render_control_characters: true,
            rulers: RulerColumns::default(),
            smart_select_subwords: true,
            smart_select_include_whitespace: true,
            multi_cursor_limit: 10_000,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct AppearanceSettings {
    pub(crate) theme: ThemePreference,
    pub(crate) zoom_level: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct FileSettings {
    pub(crate) autosave: AutosaveMode,
    pub(crate) trim_trailing_whitespace: bool,
    pub(crate) ensure_final_newline: bool,
    pub(crate) scratchpad_directory: Option<PathBuf>,
}

impl Default for FileSettings {
    fn default() -> Self {
        Self {
            autosave: AutosaveMode::Scratchpads,
            trim_trailing_whitespace: false,
            ensure_final_newline: false,
            scratchpad_directory: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct AppSettings {
    pub(crate) version: u32,
    pub(crate) editor: EditorSettings,
    pub(crate) appearance: AppearanceSettings,
    pub(crate) files: FileSettings,
    pub(crate) keybindings: BTreeMap<String, Vec<String>>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            editor: EditorSettings::default(),
            appearance: AppearanceSettings::default(),
            files: FileSettings::default(),
            keybindings: BTreeMap::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct SettingsStore {
    path: Option<PathBuf>,
    document: DocumentMut,
    parse_error: Option<String>,
    /// The config file content as last read from or written to disk. Reload
    /// detection compares against this — not `document.to_string()`, which
    /// never matches for content that does not round-trip (parse errors,
    /// whitespace-only files) and would retrigger a reload every poll.
    source: String,
    pub(crate) settings: AppSettings,
}

impl SettingsStore {
    pub(crate) fn load() -> Self {
        let path = config_path();
        let Some(path_ref) = path.clone() else {
            return Self {
                path,
                document: DocumentMut::new(),
                parse_error: Some(
                    "HOME and XDG_CONFIG_HOME are unavailable; settings cannot be persisted.".to_string(),
                ),
                source: String::new(),
                settings: AppSettings::default(),
            };
        };
        let source = match fs::read_to_string(&path_ref) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Self {
                    path,
                    document: DocumentMut::new(),
                    parse_error: Some(format!("Failed to read {}: {error}", path_ref.display())),
                    source: String::new(),
                    settings: AppSettings::default(),
                };
            }
        };
        if source.trim().is_empty() {
            return Self {
                path,
                document: DocumentMut::new(),
                parse_error: None,
                source,
                settings: AppSettings::default(),
            };
        }
        match (
            source.parse::<DocumentMut>(),
            toml_edit::de::from_str::<AppSettings>(&source),
        ) {
            (Ok(document), Ok(settings)) if settings.version == CONFIG_VERSION => Self {
                path,
                document,
                parse_error: None,
                source,
                settings: settings.normalized(),
            },
            (Ok(document), Ok(settings)) => Self {
                path,
                document,
                parse_error: Some(format!(
                    "Unsupported settings version {}; expected {}.",
                    settings.version, CONFIG_VERSION
                )),
                source,
                settings: AppSettings::default(),
            },
            (Ok(document), Err(error)) => Self {
                path,
                document,
                parse_error: Some(format!("Invalid settings in {}: {error}", path_ref.display())),
                source,
                settings: AppSettings::default(),
            },
            (Err(error), _) => Self {
                path,
                document: DocumentMut::new(),
                parse_error: Some(format!("Invalid settings in {}: {error}", path_ref.display())),
                source,
                settings: AppSettings::default(),
            },
        }
    }

    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.parse_error.as_deref()
    }

    pub(crate) fn save(&mut self) -> io::Result<()> {
        if let Some(error) = &self.parse_error {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("refusing to overwrite invalid settings: {error}"),
            ));
        }
        self.settings = self.settings.clone().normalized();
        write_settings_to_document(&mut self.document, &self.settings);
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "settings path unavailable"))?;
        let serialized = self.document.to_string();
        atomic_write(path, serialized.as_bytes())?;
        self.source = serialized;
        Ok(())
    }

    pub(crate) fn reset(&mut self) -> io::Result<()> {
        self.settings = AppSettings::default();
        self.document = DocumentMut::new();
        self.parse_error = None;
        self.save()
    }

    pub(crate) fn reloaded_if_changed(&self) -> Option<Self> {
        let path = self.path.as_ref()?;
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(_) => return None,
        };
        (source != self.source).then(Self::load)
    }

    /// Records a reloaded store's on-disk content as seen without adopting
    /// its values. Used when a reload fails to parse: the old settings stay
    /// in effect, but the poll must not rediscover the same content forever.
    pub(crate) fn mark_source_seen(&mut self, reloaded: Self) {
        self.source = reloaded.source;
    }
}

impl AppSettings {
    fn normalized(mut self) -> Self {
        self.version = CONFIG_VERSION;
        self.editor.font_size = self.editor.font_size.clamp(8, 40);
        self.editor.multi_cursor_limit = self.editor.multi_cursor_limit.clamp(1, 10_000);
        self.appearance.zoom_level = self.appearance.zoom_level.clamp(-4, 8);
        if self.editor.font_family.trim().is_empty() {
            self.editor.font_family = EditorSettings::default().font_family;
        }
        self
    }
}

fn config_path() -> Option<PathBuf> {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(config_home).join("lst").join("config.toml"));
    }
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".config").join("lst").join("config.toml"))
}

fn write_settings_to_document(document: &mut DocumentMut, settings: &AppSettings) {
    document["version"] = value(i64::from(CONFIG_VERSION));
    ensure_table(document, "editor");
    document["editor"]["input_mode"] = value(match settings.editor.input_mode {
        InputModeSetting::Standard => "standard",
        InputModeSetting::Vim => "vim",
    });
    document["editor"]["word_wrap"] = value(settings.editor.word_wrap);
    document["editor"]["line_numbers"] = value(match settings.editor.line_numbers {
        LineNumbersSetting::Absolute => "absolute",
        LineNumbersSetting::Relative => "relative",
        LineNumbersSetting::Hybrid => "hybrid",
    });
    document["editor"]["cursor_blink"] = value(settings.editor.cursor_blink);
    document["editor"]["font_family"] = value(&settings.editor.font_family);
    document["editor"]["font_size"] = value(i64::from(settings.editor.font_size));
    document["editor"]["match_brackets"] = value(match settings.editor.match_brackets {
        MatchBracketsSetting::Never => "never",
        MatchBracketsSetting::Near => "near",
        MatchBracketsSetting::Always => "always",
    });
    document["editor"]["bracket_pair_colorization"] = value(settings.editor.bracket_pair_colorization);
    document["editor"]["bracket_pair_guides"] = value(match settings.editor.bracket_pair_guides {
        GuideMode::Off => "off",
        GuideMode::Active => "active",
        GuideMode::All => "all",
    });
    document["editor"]["bracket_pair_horizontal_guides"] =
        value(match settings.editor.bracket_pair_horizontal_guides {
            GuideMode::Off => "off",
            GuideMode::Active => "active",
            GuideMode::All => "all",
        });
    document["editor"]["indent_guides"] = value(settings.editor.indent_guides);
    document["editor"]["highlight_active_indent_guide"] = value(settings.editor.highlight_active_indent_guide);
    document["editor"]["render_whitespace"] = value(match settings.editor.render_whitespace {
        RenderWhitespaceSetting::None => "none",
        RenderWhitespaceSetting::Boundary => "boundary",
        RenderWhitespaceSetting::Selection => "selection",
        RenderWhitespaceSetting::Trailing => "trailing",
        RenderWhitespaceSetting::All => "all",
    });
    document["editor"]["render_control_characters"] = value(settings.editor.render_control_characters);
    let mut rulers = Array::new();
    for &column in settings.editor.rulers.as_slice() {
        rulers.push(i64::from(column));
    }
    document["editor"]["rulers"] = Item::Value(rulers.into());
    document["editor"]["smart_select_subwords"] = value(settings.editor.smart_select_subwords);
    document["editor"]["smart_select_include_whitespace"] = value(settings.editor.smart_select_include_whitespace);
    document["editor"]["multi_cursor_limit"] =
        value(i64::try_from(settings.editor.multi_cursor_limit).unwrap_or(10_000));

    ensure_table(document, "appearance");
    document["appearance"]["theme"] = value(match settings.appearance.theme {
        ThemePreference::System => "system",
        ThemePreference::Dark => "dark",
        ThemePreference::Light => "light",
    });
    document["appearance"]["zoom_level"] = value(i64::from(settings.appearance.zoom_level));

    ensure_table(document, "files");
    document["files"]["autosave"] = value(match settings.files.autosave {
        AutosaveMode::Scratchpads => "scratchpads",
        AutosaveMode::All => "all",
    });
    document["files"]["trim_trailing_whitespace"] = value(settings.files.trim_trailing_whitespace);
    document["files"]["ensure_final_newline"] = value(settings.files.ensure_final_newline);
    if let Some(path) = &settings.files.scratchpad_directory {
        document["files"]["scratchpad_directory"] = value(path.to_string_lossy().as_ref());
    } else if let Some(files) = document.get_mut("files").and_then(Item::as_table_mut) {
        files.remove("scratchpad_directory");
    }

    ensure_table(document, "keybindings");
    let table = document["keybindings"].as_table_mut().expect("keybindings is a table");
    table.clear();
    for (command, bindings) in &settings.keybindings {
        let mut array = Array::new();
        for binding in bindings {
            array.push(binding.as_str());
        }
        table.insert(command, Item::Value(array.into()));
    }
}

fn ensure_table(document: &mut DocumentMut, key: &str) {
    if !document.get(key).is_some_and(Item::is_table) {
        document[key] = Item::Table(Table::new());
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "settings path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".config.toml.tmp-{}", std::process::id()));
    fs::write(&temp, bytes)?;
    fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_standard_scratchpad_safe_and_light() {
        let settings = AppSettings::default();
        assert_eq!(settings.editor.input_mode, InputModeSetting::Standard);
        assert_eq!(settings.files.autosave, AutosaveMode::Scratchpads);
        assert_eq!(settings.appearance.theme, ThemePreference::Light);
    }

    #[test]
    fn parses_partial_config_and_clamps_numeric_values() {
        let settings: AppSettings = toml_edit::de::from_str(
            "version = 1\n[editor]\ninput_mode = 'vim'\nfont_size = 200\n[appearance]\nzoom_level = -99\n",
        )
        .unwrap();
        let settings = settings.normalized();
        assert_eq!(settings.editor.input_mode, InputModeSetting::Vim);
        assert_eq!(settings.editor.font_size, 40);
        assert_eq!(settings.appearance.zoom_level, -4);
        assert_eq!(settings.editor.multi_cursor_limit, 10_000);
    }

    #[test]
    fn polish_defaults_are_quiet_and_structurally_aware() {
        let editor = EditorSettings::default();
        assert_eq!(editor.match_brackets, MatchBracketsSetting::Always);
        assert!(editor.bracket_pair_colorization);
        assert_eq!(editor.bracket_pair_guides, GuideMode::Active);
        assert_eq!(editor.bracket_pair_horizontal_guides, GuideMode::Active);
        assert!(editor.indent_guides);
        assert!(editor.highlight_active_indent_guide);
        assert_eq!(editor.render_whitespace, RenderWhitespaceSetting::Selection);
        assert!(editor.render_control_characters);
        assert!(editor.rulers.as_slice().is_empty());
        assert!(editor.smart_select_subwords);
        assert!(editor.smart_select_include_whitespace);
        assert_eq!(editor.multi_cursor_limit, 10_000);
    }

    #[test]
    fn rulers_are_sorted_deduplicated_and_strictly_validated() {
        let settings: AppSettings = toml_edit::de::from_str("version = 1\n[editor]\nrulers = [120, 80, 80]\n").unwrap();
        assert_eq!(settings.editor.rulers.as_slice(), &[80, 120]);

        assert!(toml_edit::de::from_str::<AppSettings>("version = 1\n[editor]\nrulers = [0]\n").is_err());
        assert!(toml_edit::de::from_str::<AppSettings>("version = 1\n[editor]\nrulers = [1001]\n").is_err());
    }

    #[test]
    fn known_updates_preserve_unrelated_comments_and_keys() {
        let mut document: DocumentMut = "# keep me\ncustom = 'value'\n[editor]\n# font comment\nfont_size = 12\n"
            .parse()
            .unwrap();
        write_settings_to_document(&mut document, &AppSettings::default());
        let output = document.to_string();
        assert!(output.contains("# keep me"));
        assert!(output.contains("custom = 'value'"));
        assert!(output.contains("# font comment"));
    }
}
