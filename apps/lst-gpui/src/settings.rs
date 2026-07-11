use serde::{Deserialize, Serialize};
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
    #[default]
    System,
    Dark,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct EditorSettings {
    pub(crate) input_mode: InputModeSetting,
    pub(crate) word_wrap: bool,
    pub(crate) line_numbers: LineNumbersSetting,
    pub(crate) cursor_blink: bool,
    pub(crate) font_family: String,
    pub(crate) font_size: u16,
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

pub(crate) struct SettingsStore {
    path: Option<PathBuf>,
    document: DocumentMut,
    parse_error: Option<String>,
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
                    settings: AppSettings::default(),
                };
            }
        };
        if source.trim().is_empty() {
            return Self {
                path,
                document: DocumentMut::new(),
                parse_error: None,
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
                settings: settings.normalized(),
            },
            (Ok(document), Ok(settings)) => Self {
                path,
                document,
                parse_error: Some(format!(
                    "Unsupported settings version {}; expected {}.",
                    settings.version, CONFIG_VERSION
                )),
                settings: AppSettings::default(),
            },
            (Ok(document), Err(error)) => Self {
                path,
                document,
                parse_error: Some(format!("Invalid settings in {}: {error}", path_ref.display())),
                settings: AppSettings::default(),
            },
            (Err(error), _) => Self {
                path,
                document: DocumentMut::new(),
                parse_error: Some(format!("Invalid settings in {}: {error}", path_ref.display())),
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
        atomic_write(path, self.document.to_string().as_bytes())
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
        (source != self.document.to_string()).then(Self::load)
    }
}

impl AppSettings {
    fn normalized(mut self) -> Self {
        self.version = CONFIG_VERSION;
        self.editor.font_size = self.editor.font_size.clamp(8, 40);
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
    fn defaults_are_standard_and_scratchpad_safe() {
        let settings = AppSettings::default();
        assert_eq!(settings.editor.input_mode, InputModeSetting::Standard);
        assert_eq!(settings.files.autosave, AutosaveMode::Scratchpads);
        assert_eq!(settings.appearance.theme, ThemePreference::System);
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
