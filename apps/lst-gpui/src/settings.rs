use serde::{de, Deserialize, Deserializer, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use toml_edit::{value, DocumentMut, Item, TableLike};

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
    #[serde(deserialize_with = "deserialize_directory")]
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
    #[serde(deserialize_with = "deserialize_directory")]
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
    write_error: Option<String>,
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
                write_error: None,
                document: DocumentMut::new(),
                parse_error: Some(
                    "Neither HOME nor XDG_CONFIG_HOME provides an absolute settings directory.".to_string(),
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
                    write_error: None,
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
                write_error: None,
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
                write_error: None,
                document,
                parse_error: None,
                source,
                settings: settings.normalized(),
            },
            (Ok(document), Ok(settings)) => Self {
                path,
                write_error: None,
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
                write_error: None,
                document,
                parse_error: Some(format!("Invalid settings in {}: {error}", path_ref.display())),
                source,
                settings: AppSettings::default(),
            },
            (Err(error), _) => Self {
                path,
                write_error: None,
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
        self.write_error.as_deref().or(self.parse_error.as_deref())
    }

    pub(crate) fn save(&mut self) -> io::Result<()> {
        let result = self.save_current();
        self.write_error = result
            .as_ref()
            .err()
            .map(|error| format!("Changes are active but not saved: {error}"));
        result
    }

    fn save_current(&mut self) -> io::Result<()> {
        if let Some(error) = &self.parse_error {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("refusing to overwrite invalid settings: {error}"),
            ));
        }
        // An edit made outside lst since the last poll is reloaded on the next
        // poll, valid or not, rather than overwritten by this save.
        if let Some(path) = self.path.as_deref() {
            let on_disk = match fs::read_to_string(path) {
                Ok(source) => source,
                Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
                Err(error) => return Err(error),
            };
            if on_disk != self.source {
                return Err(io::Error::other(format!(
                    "{} changed on disk; reloading it instead of overwriting it",
                    path.display()
                )));
            }
        }
        let settings = self.settings.clone().normalized();
        let mut document = self.document.clone();
        write_settings_to_document(&mut document, &settings)?;
        self.commit(settings, document)
    }

    /// Only a successful disk replacement commits the persisted state.
    fn commit(&mut self, settings: AppSettings, document: DocumentMut) -> io::Result<()> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "settings path unavailable"))?;
        let serialized = document.to_string();
        atomic_write(path, serialized.as_bytes())?;
        self.settings = settings;
        self.document = document;
        self.source = serialized;
        self.parse_error = None;
        Ok(())
    }

    pub(crate) fn reset(&mut self) -> io::Result<()> {
        // Reset explicitly replaces the file, but failure leaves active values
        // and the previous parse/reload state intact.
        let result = (|| {
            let settings = AppSettings::default();
            let mut document = DocumentMut::new();
            write_settings_to_document(&mut document, &settings)?;
            self.commit(settings, document)
        })();
        self.write_error = result
            .as_ref()
            .err()
            .map(|error| format!("Settings were not reset: {error}"));
        result
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
    /// in effect, the poll must not rediscover the same content forever, and
    /// saving must refuse to overwrite the file until it parses again.
    pub(crate) fn mark_source_seen(&mut self, reloaded: Self) {
        self.source = reloaded.source;
        self.parse_error = reloaded.parse_error;
        self.write_error = None;
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
    config_path_from(
        env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
    )
}

fn config_path_from(config_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    config_home
        .filter(|path| path.is_absolute())
        .or_else(|| home.filter(|path| path.is_absolute()).map(|path| path.join(".config")))
        .map(|path| path.join("lst/config.toml"))
}

fn deserialize_directory<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<PathBuf>, D::Error> {
    Option::<PathBuf>::deserialize(deserializer)?
        .map(|path| {
            resolve_directory(path, env::var_os("HOME").map(PathBuf::from).as_deref()).map_err(de::Error::custom)
        })
        .transpose()
}

fn resolve_directory(path: PathBuf, home: Option<&Path>) -> Result<PathBuf, String> {
    let path = if path.as_os_str().as_encoded_bytes().starts_with(b"~/") {
        home.filter(|home| home.is_absolute())
            .ok_or("Cannot expand ~/ without an absolute HOME directory")?
            .join(path.strip_prefix("~").expect("leading ~/"))
    } else {
        path
    };
    if path.is_absolute() {
        Ok(path)
    } else {
        Err("Note directories must be absolute paths or start with ~/".into())
    }
}

fn write_settings_to_document(document: &mut DocumentMut, settings: &AppSettings) -> io::Result<()> {
    // Compare typed values, not their spelling in TOML. This retains quotes,
    // numeric formats, comments, and ~/ paths for every unchanged setting.
    let previous = toml_edit::de::from_document::<AppSettings>(document.clone())
        .unwrap_or_default()
        .normalized();
    let before = toml_edit::ser::to_document(&previous).map_err(io::Error::other)?;
    let after = toml_edit::ser::to_document(settings).map_err(io::Error::other)?;
    patch_settings_table(document.as_table_mut(), before.as_table(), after.as_table());
    if !document.contains_key("version") {
        document["version"] = value(i64::from(CONFIG_VERSION));
    }
    Ok(())
}

fn patch_settings_table(target: &mut dyn TableLike, before: &dyn TableLike, after: &dyn TableLike) {
    // Only remove fields belonging to the typed settings schema. Unknown user
    // keys and comments remain untouched; reset starts with a fresh document.
    for (key, _) in before.iter() {
        if !after.contains_key(key) {
            target.remove(key);
        }
    }
    for (key, next) in after.iter() {
        let previous = before.get(key);
        if previous.is_some_and(|previous| previous.to_string() == next.to_string()) {
            continue;
        }
        if let Some(next_table) = next.as_table_like() {
            if !target.get(key).is_some_and(|item| item.is_table_like()) {
                target.insert(key, Item::Table(toml_edit::Table::new()));
            }
            let empty = toml_edit::Table::new();
            patch_settings_table(
                target.get_mut(key).unwrap().as_table_like_mut().unwrap(),
                previous.and_then(Item::as_table_like).unwrap_or(&empty),
                next_table,
            );
        } else {
            let mut next = next.clone();
            if let (Some(old), Some(new)) = (target.get(key).and_then(Item::as_value), next.as_value_mut()) {
                *new.decor_mut() = old.decor().clone();
            }
            if let Some(slot) = target.get_mut(key) {
                *slot = next;
            } else {
                target.insert(key, next);
            }
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // Existing symlinks keep their identity; a dangling link is an error rather
    // than permission to replace the link or guess a missing chain's target.
    let target = match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(path)?,
        Ok(_) => path.to_path_buf(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(error),
    };
    let permissions = match fs::metadata(&target) {
        Ok(meta) if !meta.is_file() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "settings target is not a regular file",
            ))
        }
        Ok(meta) if meta.permissions().readonly() => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "settings file is read-only",
            ))
        }
        Ok(meta) => Some(meta.permissions()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "settings path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("Cannot write in {}: {}", parent.display(), error.kind()),
        )
    })?;
    temp.write_all(bytes)?;
    if let Some(permissions) = permissions {
        temp.as_file().set_permissions(permissions)?;
    }
    temp.as_file().sync_all()?;
    temp.persist(&target).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<AppSettings, toml_edit::de::Error> {
        toml_edit::de::from_str(source)
    }

    fn written(settings: &AppSettings) -> String {
        let mut document = DocumentMut::new();
        write_settings_to_document(&mut document, settings).unwrap();
        document.to_string()
    }

    #[test]
    fn configuration_roots_and_note_paths_cannot_depend_on_the_working_directory() {
        for root in [None, Some(PathBuf::new()), Some("relative".into())] {
            assert_eq!(
                config_path_from(root, Some("/home/person".into())),
                Some("/home/person/.config/lst/config.toml".into())
            );
        }
        assert_eq!(
            config_path_from(Some("/config".into()), Some("relative".into())),
            Some("/config/lst/config.toml".into())
        );
        assert!(config_path_from(None, Some("relative".into())).is_none());
        let home = Path::new("/home/person");
        assert_eq!(
            resolve_directory("~/notes".into(), Some(home)).unwrap(),
            home.join("notes")
        );
        assert_eq!(
            resolve_directory("/notes".into(), None).unwrap(),
            PathBuf::from("/notes")
        );
        for invalid in ["", "notes", "$HOME/notes", "~someone/notes"] {
            assert!(resolve_directory(invalid.into(), Some(home)).is_err(), "{invalid}");
        }
        assert!(resolve_directory("~/notes".into(), None).is_err());
        for section in ["[files]\nscratchpad_directory", "[voice]\ndirectory"] {
            assert!(parse(&format!("{section} = 'relative'\n")).is_err());
        }
    }

    #[test]
    fn settings_replacement_preserves_symlinks_modes_and_leaves_no_temporary_files() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("actual.toml");
        let link = dir.path().join("config.toml");
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        symlink("actual.toml", &link).unwrap();
        atomic_write(&link, b"new").unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("actual.toml"));
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o600);
        fs::set_permissions(&target, fs::Permissions::from_mode(0o400)).unwrap();
        assert!(atomic_write(&link, b"read-only").is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        fs::remove_file(&target).unwrap();
        assert!(atomic_write(&link, b"missing").is_err());
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        fs::create_dir(&target).unwrap();
        assert!(atomic_write(&target, b"directory").is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn changes_preserve_inline_tables_key_spelling_and_value_comments() {
        let original = "version = 1\neditor = { word_wrap = true, font_size = 0x11 } # editor\n[keybindings]\n# disabled example\n# 'file.save' = ['ctrl-s']\n'edit.duplicate_line' = [ 'ctrl-shift-d' ] # binding\n";
        let mut document: DocumentMut = original.parse().unwrap();
        let mut settings = parse(original).unwrap();
        settings.editor.word_wrap = false;
        write_settings_to_document(&mut document, &settings).unwrap();
        let output = document.to_string();
        assert_eq!(output, original.replace("word_wrap = true", "word_wrap = false"));
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
        write_settings_to_document(&mut document, &settings).unwrap();
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
        write_settings_to_document(&mut document, &AppSettings::default()).unwrap();
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
        assert_eq!(
            store.settings.editor.font_size, 20,
            "the previous settings stay in effect"
        );
        assert!(store.save().is_err(), "the broken file is not overwritten");
        assert_eq!(fs::read_to_string(&path).unwrap(), "not toml [");
    }

    #[test]
    fn saving_never_overwrites_an_edit_the_reload_poll_has_not_seen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for external in ["version = 1\n[editor]\nfont_size = 22\n", "not toml [", ""] {
            fs::write(&path, "version = 1\n").unwrap();
            let mut store = SettingsStore::load_from(Some(path.clone()));
            if external.is_empty() {
                fs::remove_file(&path).unwrap();
            } else {
                fs::write(&path, external).unwrap();
            }
            store.settings.editor.font_size = 20;
            assert!(store.save().is_err(), "{external:?}");
            assert_eq!(fs::read_to_string(&path).unwrap_or_default(), external, "{external:?}");

            let reloaded = store.reloaded_if_changed().expect("the edit is reloaded instead");
            store = reloaded;
            store.settings.editor.font_size = 14;
            if store.error().is_none() {
                store.save().expect("a seen edit can be saved over");
            }
        }
    }
}
