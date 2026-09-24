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
    pub(crate) smooth_cursor: bool,
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
            smooth_cursor: false,
            font_family: "TX-02".to_string(),
            font_size: 13,
            match_brackets: MatchBracketsSetting::Always,
            bracket_pair_colorization: true,
            bracket_pair_guides: GuideMode::Off,
            bracket_pair_horizontal_guides: GuideMode::Off,
            indent_guides: false,
            highlight_active_indent_guide: false,
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
pub(crate) struct VoiceSettings {
    pub(crate) language: String,
    pub(crate) directory: Option<PathBuf>,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            language: "en".into(),
            directory: None,
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
    pub(crate) voice: VoiceSettings,
    pub(crate) keybindings: BTreeMap<String, Vec<String>>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            editor: EditorSettings::default(),
            appearance: AppearanceSettings::default(),
            files: FileSettings::default(),
            voice: VoiceSettings::default(),
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
        Self::load_from(config_path())
    }

    fn load_from(path: Option<PathBuf>) -> Self {
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
        (source != self.source).then(|| Self::load_from(Some(path.clone())))
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
    document["editor"]["input_mode"] = enum_value(settings.editor.input_mode);
    document["editor"]["word_wrap"] = value(settings.editor.word_wrap);
    document["editor"]["line_numbers"] = enum_value(settings.editor.line_numbers);
    document["editor"]["cursor_blink"] = value(settings.editor.cursor_blink);
    document["editor"]["smooth_cursor"] = value(settings.editor.smooth_cursor);
    document["editor"]["font_family"] = value(&settings.editor.font_family);
    document["editor"]["font_size"] = value(i64::from(settings.editor.font_size));
    document["editor"]["match_brackets"] = enum_value(settings.editor.match_brackets);
    document["editor"]["bracket_pair_colorization"] = value(settings.editor.bracket_pair_colorization);
    document["editor"]["bracket_pair_guides"] = enum_value(settings.editor.bracket_pair_guides);
    document["editor"]["bracket_pair_horizontal_guides"] = enum_value(settings.editor.bracket_pair_horizontal_guides);
    document["editor"]["indent_guides"] = value(settings.editor.indent_guides);
    document["editor"]["highlight_active_indent_guide"] = value(settings.editor.highlight_active_indent_guide);
    document["editor"]["render_whitespace"] = enum_value(settings.editor.render_whitespace);
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
    document["appearance"]["theme"] = enum_value(settings.appearance.theme);
    document["appearance"]["zoom_level"] = value(i64::from(settings.appearance.zoom_level));

    ensure_table(document, "files");
    document["files"]["autosave"] = enum_value(settings.files.autosave);
    document["files"]["trim_trailing_whitespace"] = value(settings.files.trim_trailing_whitespace);
    document["files"]["ensure_final_newline"] = value(settings.files.ensure_final_newline);
    if let Some(path) = &settings.files.scratchpad_directory {
        document["files"]["scratchpad_directory"] = value(path.to_string_lossy().as_ref());
    } else if let Some(files) = document.get_mut("files").and_then(Item::as_table_mut) {
        files.remove("scratchpad_directory");
    }

    ensure_table(document, "voice");
    document["voice"]["language"] = value(&settings.voice.language);
    if let Some(path) = &settings.voice.directory {
        document["voice"]["directory"] = value(path.to_string_lossy().as_ref());
    } else if let Some(table) = document["voice"].as_table_mut() {
        table.remove("directory");
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

/// Writes a setting enum under the same name serde reads it by.
fn enum_value(setting: impl Serialize) -> Item {
    Item::Value(
        setting
            .serialize(toml_edit::ser::ValueSerializer::new())
            .expect("setting enums serialize to strings"),
    )
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

    fn parse(source: &str) -> Result<AppSettings, toml_edit::de::Error> {
        toml_edit::de::from_str(source)
    }

    fn written(settings: &AppSettings) -> String {
        let mut document = DocumentMut::new();
        write_settings_to_document(&mut document, settings);
        document.to_string()
    }

    #[test]
    fn every_setting_round_trips_through_the_config_file() {
        let mut settings = AppSettings {
            version: CONFIG_VERSION,
            editor: EditorSettings {
                input_mode: InputModeSetting::Vim,
                word_wrap: false,
                line_numbers: LineNumbersSetting::Hybrid,
                cursor_blink: false,
                smooth_cursor: true,
                font_family: "Iosevka".to_string(),
                font_size: 17,
                match_brackets: MatchBracketsSetting::Near,
                bracket_pair_colorization: false,
                bracket_pair_guides: GuideMode::All,
                bracket_pair_horizontal_guides: GuideMode::Active,
                indent_guides: true,
                highlight_active_indent_guide: true,
                render_whitespace: RenderWhitespaceSetting::Trailing,
                render_control_characters: false,
                rulers: RulerColumns::new(vec![80, 120]).unwrap(),
                smart_select_subwords: false,
                smart_select_include_whitespace: false,
                multi_cursor_limit: 42,
            },
            appearance: AppearanceSettings {
                theme: ThemePreference::Dark,
                zoom_level: -2,
            },
            files: FileSettings {
                autosave: AutosaveMode::All,
                trim_trailing_whitespace: true,
                ensure_final_newline: true,
                scratchpad_directory: Some(PathBuf::from("/tmp/lst-scratch")),
            },
            voice: VoiceSettings {
                language: "auto".to_string(),
                directory: Some(PathBuf::from("/tmp/lst-voice")),
            },
            keybindings: BTreeMap::from([("edit.duplicate_line".to_string(), vec!["ctrl-shift-d".to_string()])]),
        };
        assert_eq!(parse(&written(&settings)).unwrap(), settings);

        // Clearing an optional path removes its key instead of leaving the
        // previous value behind.
        let mut document: DocumentMut = written(&settings).parse().unwrap();
        settings.files.scratchpad_directory = None;
        settings.voice.directory = None;
        write_settings_to_document(&mut document, &settings);
        assert_eq!(parse(&document.to_string()).unwrap(), settings);
    }

    #[test]
    fn out_of_range_values_are_clamped_on_load() {
        let cases = [
            ("[editor]\nfont_size = 0", "font size floor"),
            ("[editor]\nfont_size = 200", "font size ceiling"),
            ("[appearance]\nzoom_level = -99", "zoom floor"),
            ("[appearance]\nzoom_level = 99", "zoom ceiling"),
            ("[editor]\nmulti_cursor_limit = 0", "cursor limit floor"),
            ("[editor]\nmulti_cursor_limit = 99999", "cursor limit ceiling"),
            ("[editor]\nfont_family = '  '", "blank font family"),
        ];
        let clamped = |settings: AppSettings| {
            let settings = settings.normalized();
            (
                settings.editor.font_size,
                settings.appearance.zoom_level,
                settings.editor.multi_cursor_limit,
                settings.editor.font_family,
            )
        };
        let expected = [
            (8, 0, 10_000, "TX-02"),
            (40, 0, 10_000, "TX-02"),
            (13, -4, 10_000, "TX-02"),
            (13, 8, 10_000, "TX-02"),
            (13, 0, 1, "TX-02"),
            (13, 0, 10_000, "TX-02"),
            (13, 0, 10_000, "TX-02"),
        ];
        for ((body, case), (font_size, zoom, limit, family)) in cases.into_iter().zip(expected) {
            let settings = parse(&format!("version = 1\n{body}\n")).unwrap();
            assert_eq!(
                clamped(settings),
                (font_size, zoom, limit, family.to_string()),
                "{case}"
            );
        }
    }

    #[test]
    fn rulers_are_sorted_deduplicated_and_strictly_validated() {
        let rulers = |list: &str| parse(&format!("version = 1\n[editor]\nrulers = [{list}]\n"));
        assert_eq!(rulers("120, 80, 80").unwrap().editor.rulers.as_slice(), &[80, 120]);
        assert!(rulers("0").is_err());
        assert!(rulers("1001").is_err());
        let seventeen = (1..=17).map(|column| column.to_string()).collect::<Vec<_>>().join(", ");
        assert!(rulers(&seventeen).is_err());
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

    #[test]
    fn unreadable_config_files_are_reported_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for (source, reason) in [
            ("version = 1\n[editor\n", "Invalid settings"),
            ("version = 1\n[editor]\nfont_size = 'large'\n", "Invalid settings"),
            ("version = 2\n", "Unsupported settings version"),
        ] {
            fs::write(&path, source).unwrap();
            let mut store = SettingsStore::load_from(Some(path.clone()));
            assert!(
                store.error().is_some_and(|error| error.contains(reason)),
                "{source:?}: {:?}",
                store.error()
            );
            assert_eq!(store.settings, AppSettings::default(), "{source:?}");

            store.settings.editor.font_size = 20;
            assert!(store.save().is_err(), "{source:?}");
            assert_eq!(fs::read_to_string(&path).unwrap(), source);
        }
    }

    #[test]
    fn reload_reports_only_external_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut store = SettingsStore::load_from(Some(path.clone()));
        store.settings.editor.font_size = 20;
        store.save().unwrap();
        assert!(
            store.reloaded_if_changed().is_none(),
            "our own save is not an external change"
        );

        fs::write(&path, "version = 1\n[editor]\nfont_size = 22\n").unwrap();
        let reloaded = store.reloaded_if_changed().expect("external edit is detected");
        assert_eq!(reloaded.settings.editor.font_size, 22);

        fs::write(&path, "not toml [").unwrap();
        let broken = store.reloaded_if_changed().expect("broken edit is detected");
        assert!(broken.error().is_some());
        store.mark_source_seen(broken);
        assert!(
            store.reloaded_if_changed().is_none(),
            "a rejected edit is not reported again"
        );
    }
}
