//! Explanation language.
//!
//! The user picks this in the UI, so it travels with the request. Adding a
//! language means adding a variant here plus its prompt block in `prompt.rs` —
//! nothing else in the pipeline is language-aware.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Ja,
}

/// Everything the UI needs to render a language picker.
pub const SUPPORTED_LANGUAGES: &[(Language, &str, &str)] = &[
    (Language::En, "en", "English"),
    (Language::Ja, "ja", "日本語"),
];

impl Default for Language {
    fn default() -> Self {
        Language::En
    }
}

impl Language {
    /// BCP-47 style short code, as it appears in the API.
    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Ja => "ja",
        }
    }

    /// Endonym, for the picker.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::En => "English",
            Language::Ja => "日本語",
        }
    }

    /// Whether a chess-term glossary needs to be injected into the prompt.
    /// English needs none — the source terminology is already English.
    pub fn needs_glossary(self) -> bool {
        !matches!(self, Language::En)
    }

    pub fn parse(code: &str) -> Option<Language> {
        // Accept regional tags too: "ja-JP" resolves to Ja.
        let base = code
            .split(['-', '_'])
            .next()
            .unwrap_or(code)
            .to_ascii_lowercase();
        match base.as_str() {
            "en" => Some(Language::En),
            "ja" => Some(Language::Ja),
            _ => None,
        }
    }
}

impl std::str::FromStr for Language {
    type Err = crate::LlmError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Language::parse(s).ok_or_else(|| crate::LlmError::UnsupportedLanguage(s.to_string()))
    }
}
