//! Provides `language`-related settings.

use crate::{File, Language, LanguageName, LanguageServerName};
use anyhow::Result;
use collections::{HashMap, HashSet};
use core::slice;
use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};
use gpui::AppContext;
use itertools::{Either, Itertools};
use schemars::{
    schema::{InstanceType, ObjectValidation, Schema, SchemaObject, SingleOrVec},
    JsonSchema,
};
use serde::{
    de::{self, IntoDeserializer, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use serde_json::Value;
use settings::{add_references_to_properties, Settings, SettingsLocation, SettingsSources};
use std::{num::NonZeroU32, path::Path, sync::Arc};
use util::serde::default_true;

/// Initializes the language settings.
pub fn init(cx: &mut AppContext) {
    AllLanguageSettings::register(cx);
}

/// Returns the settings for the specified language from the provided file.
pub fn language_settings<'a>(
    language: Option<&Arc<Language>>,
    file: Option<&Arc<dyn File>>,
    cx: &'a AppContext,
) -> &'a LanguageSettings {
    let language_name = language.map(|l| l.name());
    all_language_settings(file, cx).language(language_name.as_ref())
}

/// Returns the settings for all languages from the provided file.
pub fn all_language_settings<'a>(
    file: Option<&Arc<dyn File>>,
    cx: &'a AppContext,
) -> &'a AllLanguageSettings {
    let location = file.map(|f| SettingsLocation {
        worktree_id: f.worktree_id(cx),
        path: f.path().as_ref(),
    });
    AllLanguageSettings::get(location, cx)
}

/// The settings for all languages.
#[derive(Debug, Clone)]
pub struct AllLanguageSettings {
    /// The inline completion settings.
    pub inline_completions: InlineCompletionSettings,
    defaults: LanguageSettings,
    languages: HashMap<LanguageName, LanguageSettings>,
    pub(crate) file_types: HashMap<Arc<str>, GlobSet>,
}

/// The settings for a particular language.
#[derive(Debug, Clone, Deserialize)]
pub struct LanguageSettings {
    /// How many columns a tab should occupy.
    pub tab_size: NonZeroU32,
    /// Whether to indent lines using tab characters, as opposed to multiple
    /// spaces.
    pub hard_tabs: bool,
    /// How to soft-wrap long lines of text.
    pub soft_wrap: SoftWrap,
    /// The column at which to soft-wrap lines, for buffers where soft-wrap
    /// is enabled.
    pub preferred_line_length: u32,
    /// Whether to show wrap guides (vertical rulers) in the editor.
    /// Setting this to true will show a guide at the 'preferred_line_length' value
    /// if softwrap is set to 'preferred_line_length', and will show any
    /// additional guides as specified by the 'wrap_guides' setting.
    pub show_wrap_guides: bool,
    /// Character counts at which to show wrap guides (vertical rulers) in the editor.
    pub wrap_guides: Vec<usize>,
    /// Indent guide related settings.
    pub indent_guides: IndentGuideSettings,
    /// Whether or not to perform a buffer format before saving.
    pub format_on_save: FormatOnSave,
    /// Whether or not to remove any trailing whitespace from lines of a buffer
    /// before saving it.
    pub remove_trailing_whitespace_on_save: bool,
    /// Whether or not to ensure there's a single newline at the end of a buffer
    /// when saving it.
    pub ensure_final_newline_on_save: bool,
    /// How to perform a buffer format.
    pub formatter: SelectedFormatter,
    /// Zed's Prettier integration settings.
    pub prettier: PrettierSettings,
    /// Whether to use language servers to provide code intelligence.
    pub enable_language_server: bool,
    /// The list of language servers to use (or disable) for this language.
    ///
    /// This array should consist of language server IDs, as well as the following
    /// special tokens:
    /// - `"!<language_server_id>"` - A language server ID prefixed with a `!` will be disabled.
    /// - `"..."` - A placeholder to refer to the **rest** of the registered language servers for this language.
    pub language_servers: Vec<Arc<str>>,
    /// Controls whether inline completions are shown immediately (true)
    /// or manually by triggering `editor::ShowInlineCompletion` (false).
    pub show_inline_completions: bool,
    /// Whether to show tabs and spaces in the editor.
    pub show_whitespaces: ShowWhitespaceSetting,
    /// Whether to start a new line with a comment when a previous line is a comment as well.
    pub extend_comment_on_newline: bool,
    /// Inlay hint related settings.
    pub inlay_hints: InlayHintSettings,
    /// Whether to automatically close brackets.
    pub use_autoclose: bool,
    /// Whether to automatically surround text with brackets.
    pub use_auto_surround: bool,
    // Controls how the editor handles the autoclosed characters.
    pub always_treat_brackets_as_autoclosed: bool,
    /// Which code actions to run on save
    pub code_actions_on_format: HashMap<String, bool>,
    /// Whether to perform linked edits
    pub linked_edits: bool,
    /// Task configuration for this language.
    pub tasks: LanguageTaskConfig,

    /// Rainbow brackets config
    pub rainbow_brackets: bool,
}

impl LanguageSettings {
    /// A token representing the rest of the available language servers.
    const REST_OF_LANGUAGE_SERVERS: &'static str = "...";

    /// Returns the customized list of language servers from the list of
    /// available language servers.
    pub fn customized_language_servers(
        &self,
        available_language_servers: &[LanguageServerName],
    ) -> Vec<LanguageServerName> {
        Self::resolve_language_servers(&self.language_servers, available_language_servers)
    }

    pub(crate) fn resolve_language_servers(
        configured_language_servers: &[Arc<str>],
        available_language_servers: &[LanguageServerName],
    ) -> Vec<LanguageServerName> {
        let (disabled_language_servers, enabled_language_servers): (Vec<Arc<str>>, Vec<Arc<str>>) =
            configured_language_servers.iter().partition_map(
                |language_server| match language_server.strip_prefix('!') {
                    Some(disabled) => Either::Left(disabled.into()),
                    None => Either::Right(language_server.clone()),
                },
            );

        let rest = available_language_servers
            .iter()
            .filter(|&available_language_server| {
                !disabled_language_servers.contains(&available_language_server.0)
                    && !enabled_language_servers.contains(&available_language_server.0)
            })
            .cloned()
            .collect::<Vec<_>>();

        enabled_language_servers
            .into_iter()
            .flat_map(|language_server| {
                if language_server.as_ref() == Self::REST_OF_LANGUAGE_SERVERS {
                    rest.clone()
                } else {
                    vec![LanguageServerName(language_server.clone())]
                }
            })
            .collect::<Vec<_>>()
    }
}

/// The provider that supplies inline completions.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InlineCompletionProvider {
    None,
    #[default]
    Copilot,
    Supermaven,
}

/// The settings for inline completions, such as [GitHub Copilot](https://github.com/features/copilot)
/// or [Supermaven](https://supermaven.com).
#[derive(Clone, Debug, Default)]
pub struct InlineCompletionSettings {
    /// The provider that supplies inline completions.
    pub provider: InlineCompletionProvider,
    /// A list of globs representing files that inline completions should be disabled for.
    pub disabled_globs: Vec<GlobMatcher>,
}

/// The settings for all languages.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AllLanguageSettingsContent {
    /// The settings for enabling/disabling features.
    #[serde(default)]
    pub features: Option<FeaturesContent>,
    /// The inline completion settings.
    #[serde(default)]
    pub inline_completions: Option<InlineCompletionSettingsContent>,
    /// The default language settings.
    #[serde(flatten)]
    pub defaults: LanguageSettingsContent,
    /// The settings for individual languages.
    #[serde(default)]
    pub languages: HashMap<LanguageName, LanguageSettingsContent>,
    /// Settings for associating file extensions and filenames
    /// with languages.
    #[serde(default)]
    pub file_types: HashMap<Arc<str>, Vec<String>>,
}

/// The settings for a particular language.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LanguageSettingsContent {
    /// How many columns a tab should occupy.
    ///
    /// Default: 4
    #[serde(default)]
    pub tab_size: Option<NonZeroU32>,
    /// Whether to indent lines using tab characters, as opposed to multiple
    /// spaces.
    ///
    /// Default: false
    #[serde(default)]
    pub hard_tabs: Option<bool>,
    /// How to soft-wrap long lines of text.
    ///
    /// Default: none
    #[serde(default)]
    pub soft_wrap: Option<SoftWrap>,
    /// The column at which to soft-wrap lines, for buffers where soft-wrap
    /// is enabled.
    ///
    /// Default: 80
    #[serde(default)]
    pub preferred_line_length: Option<u32>,
    /// Whether to show wrap guides in the editor. Setting this to true will
    /// show a guide at the 'preferred_line_length' value if softwrap is set to
    /// 'preferred_line_length', and will show any additional guides as specified
    /// by the 'wrap_guides' setting.
    ///
    /// Default: true
    #[serde(default)]
    pub show_wrap_guides: Option<bool>,
    /// Character counts at which to show wrap guides in the editor.
    ///
    /// Default: []
    #[serde(default)]
    pub wrap_guides: Option<Vec<usize>>,
    /// Indent guide related settings.
    #[serde(default)]
    pub indent_guides: Option<IndentGuideSettings>,
    /// Whether or not to perform a buffer format before saving.
    ///
    /// Default: on
    #[serde(default)]
    pub format_on_save: Option<FormatOnSave>,
    /// Whether or not to remove any trailing whitespace from lines of a buffer
    /// before saving it.
    ///
    /// Default: true
    #[serde(default)]
    pub remove_trailing_whitespace_on_save: Option<bool>,
    /// Whether or not to ensure there's a single newline at the end of a buffer
    /// when saving it.
    ///
    /// Default: true
    #[serde(default)]
    pub ensure_final_newline_on_save: Option<bool>,
    /// How to perform a buffer format.
    ///
    /// Default: auto
    #[serde(default)]
    pub formatter: Option<SelectedFormatter>,
    /// Zed's Prettier integration settings.
    /// Allows to enable/disable formatting with Prettier
    /// and configure default Prettier, used when no project-level Prettier installation is found.
    ///
    /// Default: off
    #[serde(default)]
    pub prettier: Option<PrettierSettings>,
    /// Whether to use language servers to provide code intelligence.
    ///
    /// Default: true
    #[serde(default)]
    pub enable_language_server: Option<bool>,
    /// The list of language servers to use (or disable) for this language.
    ///
    /// This array should consist of language server IDs, as well as the following
    /// special tokens:
    /// - `"!<language_server_id>"` - A language server ID prefixed with a `!` will be disabled.
    /// - `"..."` - A placeholder to refer to the **rest** of the registered language servers for this language.
    ///
    /// Default: ["..."]
    #[serde(default)]
    pub language_servers: Option<Vec<Arc<str>>>,
    /// Controls whether inline completions are shown immediately (true)
    /// or manually by triggering `editor::ShowInlineCompletion` (false).
    ///
    /// Default: true
    #[serde(default)]
    pub show_inline_completions: Option<bool>,
    /// Whether to show tabs and spaces in the editor.
    #[serde(default)]
    pub show_whitespaces: Option<ShowWhitespaceSetting>,
    /// Whether to start a new line with a comment when a previous line is a comment as well.
    ///
    /// Default: true
    #[serde(default)]
    pub extend_comment_on_newline: Option<bool>,
    /// Inlay hint related settings.
    #[serde(default)]
    pub inlay_hints: Option<InlayHintSettings>,
    /// Whether to automatically type closing characters for you. For example,
    /// when you type (, Zed will automatically add a closing ) at the correct position.
    ///
    /// Default: true
    pub use_autoclose: Option<bool>,
    /// Whether to automatically surround text with characters for you. For example,
    /// when you select text and type (, Zed will automatically surround text with ().
    ///
    /// Default: true
    pub use_auto_surround: Option<bool>,
    // Controls how the editor handles the autoclosed characters.
    // When set to `false`(default), skipping over and auto-removing of the closing characters
    // happen only for auto-inserted characters.
    // Otherwise(when `true`), the closing characters are always skipped over and auto-removed
    // no matter how they were inserted.
    ///
    /// Default: false
    pub always_treat_brackets_as_autoclosed: Option<bool>,
    /// Which code actions to run on save after the formatter.
    /// These are not run if formatting is off.
    ///
    /// Default: {} (or {"source.organizeImports": true} for Go).
    pub code_actions_on_format: Option<HashMap<String, bool>>,
    /// Whether to perform linked edits of associated ranges, if the language server supports it.
    /// For example, when editing opening <html> tag, the contents of the closing </html> tag will be edited as well.
    ///
    /// Default: true
    pub linked_edits: Option<bool>,
    /// Task configuration for this language.
    ///
    /// Default: {}
    pub tasks: Option<LanguageTaskConfig>,
    /// Task configuration for this language.
    ///
    /// Default: {}
    pub rainbow_brackets: Option<bool>,
}

/// The contents of the inline completion settings.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct InlineCompletionSettingsContent {
    /// A list of globs representing files that inline completions should be disabled for.
    #[serde(default)]
    pub disabled_globs: Option<Vec<String>>,
}

/// The settings for enabling/disabling features.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FeaturesContent {
    /// Whether the GitHub Copilot feature is enabled.
    pub copilot: Option<bool>,
    /// Determines which inline completion provider to use.
    pub inline_completion_provider: Option<InlineCompletionProvider>,
}

/// Controls the soft-wrapping behavior in the editor.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SoftWrap {
    /// Do not soft wrap.
    None,
    /// Prefer a single line generally, unless an overly long line is encountered.
    PreferLine,
    /// Soft wrap lines that exceed the editor width
    EditorWidth,
    /// Soft wrap lines at the preferred line length
    PreferredLineLength,
    /// Soft wrap line at the preferred line length or the editor width (whichever is smaller)
    Bounded,
}

/// Controls the behavior of formatting files when they are saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatOnSave {
    /// Files should be formatted on save.
    On,
    /// Files should not be formatted on save.
    Off,
    List(FormatterList),
}

impl JsonSchema for FormatOnSave {
    fn schema_name() -> String {
        "OnSaveFormatter".into()
    }

    fn json_schema(generator: &mut schemars::r#gen::SchemaGenerator) -> Schema {
        let mut schema = SchemaObject::default();
        let formatter_schema = Formatter::json_schema(generator);
        schema.instance_type = Some(
            vec![
                InstanceType::Object,
                InstanceType::String,
                InstanceType::Array,
            ]
            .into(),
        );

        let valid_raw_values = SchemaObject {
            enum_values: Some(vec![
                Value::String("on".into()),
                Value::String("off".into()),
                Value::String("prettier".into()),
                Value::String("language_server".into()),
            ]),
            ..Default::default()
        };
        let mut nested_values = SchemaObject::default();

        nested_values.array().items = Some(formatter_schema.clone().into());

        schema.subschemas().any_of = Some(vec![
            nested_values.into(),
            valid_raw_values.into(),
            formatter_schema,
        ]);
        schema.into()
    }
}

impl Serialize for FormatOnSave {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::On => serializer.serialize_str("on"),
            Self::Off => serializer.serialize_str("off"),
            Self::List(list) => list.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for FormatOnSave {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FormatDeserializer;

        impl<'d> Visitor<'d> for FormatDeserializer {
            type Value = FormatOnSave;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a valid on-save formatter kind")
            }
            fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v == "on" {
                    Ok(Self::Value::On)
                } else if v == "off" {
                    Ok(Self::Value::Off)
                } else if v == "language_server" {
                    Ok(Self::Value::List(FormatterList(
                        Formatter::LanguageServer { name: None }.into(),
                    )))
                } else {
                    let ret: Result<FormatterList, _> =
                        Deserialize::deserialize(v.into_deserializer());
                    ret.map(Self::Value::List)
                }
            }
            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'d>,
            {
                let ret: Result<FormatterList, _> =
                    Deserialize::deserialize(de::value::MapAccessDeserializer::new(map));
                ret.map(Self::Value::List)
            }
            fn visit_seq<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'d>,
            {
                let ret: Result<FormatterList, _> =
                    Deserialize::deserialize(de::value::SeqAccessDeserializer::new(map));
                ret.map(Self::Value::List)
            }
        }
        deserializer.deserialize_any(FormatDeserializer)
    }
}

/// Controls how whitespace should be displayedin the editor.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShowWhitespaceSetting {
    /// Draw whitespace only for the selected text.
    Selection,
    /// Do not draw any tabs or spaces.
    None,
    /// Draw all invisible symbols.
    All,
    /// Draw whitespaces at boundaries only.
    ///
    /// For a whitespace to be on a boundary, any of the following conditions need to be met:
    /// - It is a tab
    /// - It is adjacent to an edge (start or end)
    /// - It is adjacent to a whitespace (left or right)
    Boundary,
}

/// Controls which formatter should be used when formatting code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SelectedFormatter {
    /// Format files using Zed's Prettier integration (if applicable),
    /// or falling back to formatting via language server.
    #[default]
    Auto,
    List(FormatterList),
}

impl JsonSchema for SelectedFormatter {
    fn schema_name() -> String {
        "Formatter".into()
    }

    fn json_schema(generator: &mut schemars::r#gen::SchemaGenerator) -> Schema {
        let mut schema = SchemaObject::default();
        let formatter_schema = Formatter::json_schema(generator);
        schema.instance_type = Some(
            vec![
                InstanceType::Object,
                InstanceType::String,
                InstanceType::Array,
            ]
            .into(),
        );

        let valid_raw_values = SchemaObject {
            enum_values: Some(vec![
                Value::String("auto".into()),
                Value::String("prettier".into()),
                Value::String("language_server".into()),
            ]),
            ..Default::default()
        };

        let mut nested_values = SchemaObject::default();

        nested_values.array().items = Some(formatter_schema.clone().into());

        schema.subschemas().any_of = Some(vec![
            nested_values.into(),
            valid_raw_values.into(),
            formatter_schema,
        ]);
        schema.into()
    }
}

impl Serialize for SelectedFormatter {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            SelectedFormatter::Auto => serializer.serialize_str("auto"),
            SelectedFormatter::List(list) => list.serialize(serializer),
        }
    }
}
impl<'de> Deserialize<'de> for SelectedFormatter {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FormatDeserializer;

        impl<'d> Visitor<'d> for FormatDeserializer {
            type Value = SelectedFormatter;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a valid formatter kind")
            }
            fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v == "auto" {
                    Ok(Self::Value::Auto)
                } else if v == "language_server" {
                    Ok(Self::Value::List(FormatterList(
                        Formatter::LanguageServer { name: None }.into(),
                    )))
                } else {
                    let ret: Result<FormatterList, _> =
                        Deserialize::deserialize(v.into_deserializer());
                    ret.map(SelectedFormatter::List)
                }
            }
            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'d>,
            {
                let ret: Result<FormatterList, _> =
                    Deserialize::deserialize(de::value::MapAccessDeserializer::new(map));
                ret.map(SelectedFormatter::List)
            }
            fn visit_seq<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'d>,
            {
                let ret: Result<FormatterList, _> =
                    Deserialize::deserialize(de::value::SeqAccessDeserializer::new(map));
                ret.map(SelectedFormatter::List)
            }
        }
        deserializer.deserialize_any(FormatDeserializer)
    }
}
/// Controls which formatter should be used when formatting code.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case", transparent)]
pub struct FormatterList(pub SingleOrVec<Formatter>);

impl AsRef<[Formatter]> for FormatterList {
    fn as_ref(&self) -> &[Formatter] {
        match &self.0 {
            SingleOrVec::Single(single) => slice::from_ref(single),
            SingleOrVec::Vec(v) => v,
        }
    }
}

/// Controls which formatter should be used when formatting code. If there are multiple formatters, they are executed in the order of declaration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Formatter {
    /// Format code using the current language server.
    LanguageServer { name: Option<String> },
    /// Format code using Zed's Prettier integration.
    Prettier,
    /// Format code using an external command.
    External {
        /// The external program to run.
        command: Arc<str>,
        /// The arguments to pass to the program.
        arguments: Arc<[String]>,
    },
    /// Files should be formatted using code actions executed by language servers.
    CodeActions(HashMap<String, bool>),
}

/// The settings for indent guides.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IndentGuideSettings {
    /// Whether to display indent guides in the editor.
    ///
    /// Default: true
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The width of the indent guides in pixels, between 1 and 10.
    ///
    /// Default: 1
    #[serde(default = "line_width")]
    pub line_width: u32,
    /// The width of the active indent guide in pixels, between 1 and 10.
    ///
    /// Default: 1
    #[serde(default = "active_line_width")]
    pub active_line_width: u32,
    /// Determines how indent guides are colored.
    ///
    /// Default: Fixed
    #[serde(default)]
    pub coloring: IndentGuideColoring,
    /// Determines how indent guide backgrounds are colored.
    ///
    /// Default: Disabled
    #[serde(default)]
    pub background_coloring: IndentGuideBackgroundColoring,
}

fn line_width() -> u32 {
    1
}

fn active_line_width() -> u32 {
    line_width()
}

/// Determines how indent guides are colored.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IndentGuideColoring {
    /// Do not render any lines for indent guides.
    Disabled,
    /// Use the same color for all indentation levels.
    #[default]
    Fixed,
    /// Use a different color for each indentation level.
    IndentAware,
}

/// Determines how indent guide backgrounds are colored.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IndentGuideBackgroundColoring {
    /// Do not render any background for indent guides.
    #[default]
    Disabled,
    /// Use a different color for each indentation level.
    IndentAware,
}

/// The settings for inlay hints.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InlayHintSettings {
    /// Global switch to toggle hints on and off.
    ///
    /// Default: false
    #[serde(default)]
    pub enabled: bool,
    /// Whether type hints should be shown.
    ///
    /// Default: true
    #[serde(default = "default_true")]
    pub show_type_hints: bool,
    /// Whether parameter hints should be shown.
    ///
    /// Default: true
    #[serde(default = "default_true")]
    pub show_parameter_hints: bool,
    /// Whether other hints should be shown.
    ///
    /// Default: true
    #[serde(default = "default_true")]
    pub show_other_hints: bool,
    /// Whether or not to debounce inlay hints updates after buffer edits.
    ///
    /// Set to 0 to disable debouncing.
    ///
    /// Default: 700
    #[serde(default = "edit_debounce_ms")]
    pub edit_debounce_ms: u64,
    /// Whether or not to debounce inlay hints updates after buffer scrolls.
    ///
    /// Set to 0 to disable debouncing.
    ///
    /// Default: 50
    #[serde(default = "scroll_debounce_ms")]
    pub scroll_debounce_ms: u64,
}

fn edit_debounce_ms() -> u64 {
    700
}

fn scroll_debounce_ms() -> u64 {
    50
}

/// The task settings for a particular language.
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize, JsonSchema)]
pub struct LanguageTaskConfig {
    /// Extra task variables to set for a particular language.
    pub variables: HashMap<String, String>,
}

impl InlayHintSettings {
    /// Returns the kinds of inlay hints that are enabled based on the settings.
    pub fn enabled_inlay_hint_kinds(&self) -> HashSet<Option<InlayHintKind>> {
        let mut kinds = HashSet::default();
        if self.show_type_hints {
            kinds.insert(Some(InlayHintKind::Type));
        }
        if self.show_parameter_hints {
            kinds.insert(Some(InlayHintKind::Parameter));
        }
        if self.show_other_hints {
            kinds.insert(None);
        }
        kinds
    }
}

impl AllLanguageSettings {
    /// Returns the [`LanguageSettings`] for the language with the specified name.
    pub fn language<'a>(&'a self, language_name: Option<&LanguageName>) -> &'a LanguageSettings {
        if let Some(name) = language_name {
            if let Some(overrides) = self.languages.get(name) {
                return overrides;
            }
        }
        &self.defaults
    }

    /// Returns whether inline completions are enabled for the given path.
    pub fn inline_completions_enabled_for_path(&self, path: &Path) -> bool {
        !self
            .inline_completions
            .disabled_globs
            .iter()
            .any(|glob| glob.is_match(path))
    }

    /// Returns whether inline completions are enabled for the given language and path.
    pub fn inline_completions_enabled(
        &self,
        language: Option<&Arc<Language>>,
        path: Option<&Path>,
    ) -> bool {
        if let Some(path) = path {
            if !self.inline_completions_enabled_for_path(path) {
                return false;
            }
        }

        self.language(language.map(|l| l.name()).as_ref())
            .show_inline_completions
    }
}

/// The kind of an inlay hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InlayHintKind {
    /// An inlay hint for a type.
    Type,
    /// An inlay hint for a parameter.
    Parameter,
}

impl InlayHintKind {
    /// Returns the [`InlayHintKind`] from the given name.
    ///
    /// Returns `None` if `name` does not match any of the expected
    /// string representations.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "type" => Some(InlayHintKind::Type),
            "parameter" => Some(InlayHintKind::Parameter),
            _ => None,
        }
    }

    /// Returns the name of this [`InlayHintKind`].
    pub fn name(&self) -> &'static str {
        match self {
            InlayHintKind::Type => "type",
            InlayHintKind::Parameter => "parameter",
        }
    }
}

impl settings::Settings for AllLanguageSettings {
    const KEY: Option<&'static str> = None;

    type FileContent = AllLanguageSettingsContent;

    fn load(sources: SettingsSources<Self::FileContent>, _: &mut AppContext) -> Result<Self> {
        let default_value = sources.default;

        // A default is provided for all settings.
        let mut defaults: LanguageSettings =
            serde_json::from_value(serde_json::to_value(&default_value.defaults)?)?;

        let mut languages = HashMap::default();
        for (language_name, settings) in &default_value.languages {
            let mut language_settings = defaults.clone();
            merge_settings(&mut language_settings, settings);
            languages.insert(language_name.clone(), language_settings);
        }

        let mut copilot_enabled = default_value.features.as_ref().and_then(|f| f.copilot);
        let mut inline_completion_provider = default_value
            .features
            .as_ref()
            .and_then(|f| f.inline_completion_provider);
        let mut completion_globs = default_value
            .inline_completions
            .as_ref()
            .and_then(|c| c.disabled_globs.as_ref())
            .ok_or_else(Self::missing_default)?;

        let mut file_types: HashMap<Arc<str>, GlobSet> = HashMap::default();

        for (language, suffixes) in &default_value.file_types {
            let mut builder = GlobSetBuilder::new();

            for suffix in suffixes {
                builder.add(Glob::new(suffix)?);
            }

            file_types.insert(language.clone(), builder.build()?);
        }

        for user_settings in sources.customizations() {
            if let Some(copilot) = user_settings.features.as_ref().and_then(|f| f.copilot) {
                copilot_enabled = Some(copilot);
            }
            if let Some(provider) = user_settings
                .features
                .as_ref()
                .and_then(|f| f.inline_completion_provider)
            {
                inline_completion_provider = Some(provider);
            }
            if let Some(globs) = user_settings
                .inline_completions
                .as_ref()
                .and_then(|f| f.disabled_globs.as_ref())
            {
                completion_globs = globs;
            }

            // A user's global settings override the default global settings and
            // all default language-specific settings.
            merge_settings(&mut defaults, &user_settings.defaults);
            for language_settings in languages.values_mut() {
                merge_settings(language_settings, &user_settings.defaults);
            }

            // A user's language-specific settings override default language-specific settings.
            for (language_name, user_language_settings) in &user_settings.languages {
                merge_settings(
                    languages
                        .entry(language_name.clone())
                        .or_insert_with(|| defaults.clone()),
                    user_language_settings,
                );
            }

            for (language, suffixes) in &user_settings.file_types {
                let mut builder = GlobSetBuilder::new();

                let default_value = default_value.file_types.get(&language.clone());

                // Merge the default value with the user's value.
                if let Some(suffixes) = default_value {
                    for suffix in suffixes {
                        builder.add(Glob::new(suffix)?);
                    }
                }

                for suffix in suffixes {
                    builder.add(Glob::new(suffix)?);
                }

                file_types.insert(language.clone(), builder.build()?);
            }
        }

        Ok(Self {
            inline_completions: InlineCompletionSettings {
                provider: if let Some(provider) = inline_completion_provider {
                    provider
                } else if copilot_enabled.unwrap_or(true) {
                    InlineCompletionProvider::Copilot
                } else {
                    InlineCompletionProvider::None
                },
                disabled_globs: completion_globs
                    .iter()
                    .filter_map(|g| Some(globset::Glob::new(g).ok()?.compile_matcher()))
                    .collect(),
            },
            defaults,
            languages,
            file_types,
        })
    }

    fn json_schema(
        generator: &mut schemars::gen::SchemaGenerator,
        params: &settings::SettingsJsonSchemaParams,
        _: &AppContext,
    ) -> schemars::schema::RootSchema {
        let mut root_schema = generator.root_schema_for::<Self::FileContent>();

        // Create a schema for a 'languages overrides' object, associating editor
        // settings with specific languages.
        assert!(root_schema
            .definitions
            .contains_key("LanguageSettingsContent"));

        let languages_object_schema = SchemaObject {
            instance_type: Some(InstanceType::Object.into()),
            object: Some(Box::new(ObjectValidation {
                properties: params
                    .language_names
                    .iter()
                    .map(|name| {
                        (
                            name.clone(),
                            Schema::new_ref("#/definitions/LanguageSettingsContent".into()),
                        )
                    })
                    .collect(),
                ..Default::default()
            })),
            ..Default::default()
        };

        root_schema
            .definitions
            .extend([("Languages".into(), languages_object_schema.into())]);

        add_references_to_properties(
            &mut root_schema,
            &[("languages", "#/definitions/Languages")],
        );

        root_schema
    }
}

fn merge_settings(settings: &mut LanguageSettings, src: &LanguageSettingsContent) {
    fn merge<T>(target: &mut T, value: Option<T>) {
        if let Some(value) = value {
            *target = value;
        }
    }

    merge(&mut settings.tab_size, src.tab_size);
    merge(&mut settings.hard_tabs, src.hard_tabs);
    merge(&mut settings.soft_wrap, src.soft_wrap);
    merge(&mut settings.use_autoclose, src.use_autoclose);
    merge(&mut settings.use_auto_surround, src.use_auto_surround);
    merge(
        &mut settings.always_treat_brackets_as_autoclosed,
        src.always_treat_brackets_as_autoclosed,
    );
    merge(&mut settings.show_wrap_guides, src.show_wrap_guides);
    merge(&mut settings.wrap_guides, src.wrap_guides.clone());
    merge(&mut settings.indent_guides, src.indent_guides);
    merge(
        &mut settings.code_actions_on_format,
        src.code_actions_on_format.clone(),
    );
    merge(&mut settings.linked_edits, src.linked_edits);
    merge(&mut settings.tasks, src.tasks.clone());

    merge(
        &mut settings.preferred_line_length,
        src.preferred_line_length,
    );
    merge(&mut settings.formatter, src.formatter.clone());
    merge(&mut settings.prettier, src.prettier.clone());
    merge(&mut settings.format_on_save, src.format_on_save.clone());
    merge(
        &mut settings.remove_trailing_whitespace_on_save,
        src.remove_trailing_whitespace_on_save,
    );
    merge(
        &mut settings.ensure_final_newline_on_save,
        src.ensure_final_newline_on_save,
    );
    merge(
        &mut settings.enable_language_server,
        src.enable_language_server,
    );
    merge(&mut settings.language_servers, src.language_servers.clone());
    merge(
        &mut settings.show_inline_completions,
        src.show_inline_completions,
    );
    merge(&mut settings.show_whitespaces, src.show_whitespaces);
    merge(
        &mut settings.extend_comment_on_newline,
        src.extend_comment_on_newline,
    );
    merge(&mut settings.inlay_hints, src.inlay_hints);
}

/// Allows to enable/disable formatting with Prettier
/// and configure default Prettier, used when no project-level Prettier installation is found.
/// Prettier formatting is disabled by default.
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PrettierSettings {
    /// Enables or disables formatting with Prettier for a given language.
    #[serde(default)]
    pub allowed: bool,

    /// Forces Prettier integration to use a specific parser name when formatting files with the language.
    #[serde(default)]
    pub parser: Option<String>,

    /// Forces Prettier integration to use specific plugins when formatting files with the language.
    /// The default Prettier will be installed with these plugins.
    #[serde(default)]
    pub plugins: HashSet<String>,

    /// Default Prettier options, in the format as in package.json section for Prettier.
    /// If project installs Prettier via its package.json, these options will be ignored.
    #[serde(flatten)]
    pub options: HashMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_formatter_deserialization() {
        let raw_auto = "{\"formatter\": \"auto\"}";
        let settings: LanguageSettingsContent = serde_json::from_str(raw_auto).unwrap();
        assert_eq!(settings.formatter, Some(SelectedFormatter::Auto));
        let raw = "{\"formatter\": \"language_server\"}";
        let settings: LanguageSettingsContent = serde_json::from_str(raw).unwrap();
        assert_eq!(
            settings.formatter,
            Some(SelectedFormatter::List(FormatterList(
                Formatter::LanguageServer { name: None }.into()
            )))
        );
        let raw = "{\"formatter\": [{\"language_server\": {\"name\": null}}]}";
        let settings: LanguageSettingsContent = serde_json::from_str(raw).unwrap();
        assert_eq!(
            settings.formatter,
            Some(SelectedFormatter::List(FormatterList(
                vec![Formatter::LanguageServer { name: None }].into()
            )))
        );
        let raw = "{\"formatter\": [{\"language_server\": {\"name\": null}}, \"prettier\"]}";
        let settings: LanguageSettingsContent = serde_json::from_str(raw).unwrap();
        assert_eq!(
            settings.formatter,
            Some(SelectedFormatter::List(FormatterList(
                vec![
                    Formatter::LanguageServer { name: None },
                    Formatter::Prettier
                ]
                .into()
            )))
        );
    }

    #[test]
    pub fn test_resolve_language_servers() {
        fn language_server_names(names: &[&str]) -> Vec<LanguageServerName> {
            names
                .iter()
                .copied()
                .map(|name| LanguageServerName(name.into()))
                .collect::<Vec<_>>()
        }

        let available_language_servers = language_server_names(&[
            "typescript-language-server",
            "biome",
            "deno",
            "eslint",
            "tailwind",
        ]);

        // A value of just `["..."]` is the same as taking all of the available language servers.
        assert_eq!(
            LanguageSettings::resolve_language_servers(
                &[LanguageSettings::REST_OF_LANGUAGE_SERVERS.into()],
                &available_language_servers,
            ),
            available_language_servers
        );

        // Referencing one of the available language servers will change its order.
        assert_eq!(
            LanguageSettings::resolve_language_servers(
                &[
                    "biome".into(),
                    LanguageSettings::REST_OF_LANGUAGE_SERVERS.into(),
                    "deno".into()
                ],
                &available_language_servers
            ),
            language_server_names(&[
                "biome",
                "typescript-language-server",
                "eslint",
                "tailwind",
                "deno",
            ])
        );

        // Negating an available language server removes it from the list.
        assert_eq!(
            LanguageSettings::resolve_language_servers(
                &[
                    "deno".into(),
                    "!typescript-language-server".into(),
                    "!biome".into(),
                    LanguageSettings::REST_OF_LANGUAGE_SERVERS.into()
                ],
                &available_language_servers
            ),
            language_server_names(&["deno", "eslint", "tailwind"])
        );

        // Adding a language server not in the list of available language servers adds it to the list.
        assert_eq!(
            LanguageSettings::resolve_language_servers(
                &[
                    "my-cool-language-server".into(),
                    LanguageSettings::REST_OF_LANGUAGE_SERVERS.into()
                ],
                &available_language_servers
            ),
            language_server_names(&[
                "my-cool-language-server",
                "typescript-language-server",
                "biome",
                "deno",
                "eslint",
                "tailwind",
            ])
        );
    }
}
