//! Deterministic spoken punctuation parser and baseline text formatter.
//!
//! Replaces spoken verbal commands (e.g. "comma", "period", "new line", "question mark")
//! with standardized punctuation symbols, adjusts whitespace typography, and ensures
//! clean sentence capitalization in < 2ms without LLMs or non-deterministic behaviors.

use regex::Regex;
use std::sync::LazyLock;

static RE_NEW_PARAGRAPH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*new paragraph\s*").unwrap());
static RE_NEW_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(new line|newline)\s*").unwrap());
static RE_QUESTION_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*question mark\b").unwrap());
static RE_EXCLAMATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(exclamation mark|exclamation point)\b").unwrap());
static RE_FULL_STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*full stop\b").unwrap());
static RE_OPEN_PAREN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(open parenthesis|open paren)\s*").unwrap());
static RE_CLOSE_PAREN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(close parenthesis|close paren)\b").unwrap());
static RE_OPEN_QUOTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\bopen quote\s*"#).unwrap());
static RE_CLOSE_QUOTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\s*close quote\b"#).unwrap());

static RE_COMMA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*\bcomma\b").unwrap());
static RE_PERIOD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*\bperiod\b").unwrap());
static RE_COLON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*\bcolon\b").unwrap());
static RE_SEMICOLON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*\bsemicolon\b").unwrap());
static RE_DASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*\b(hyphen|dash)\b\s*").unwrap());

// Typography / Spacing cleanups
static RE_SPACE_BEFORE_PUNCT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+([,.:;?!)\-])").unwrap());
static RE_MISSING_SPACE_AFTER_PUNCT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([,.:;?!])([A-Za-z0-9])").unwrap());
static RE_CLEAN_NEWLINES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t]*\n[ \t]*").unwrap());
static RE_CONSECUTIVE_SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+").unwrap());

pub struct FormattingEngine;

impl FormattingEngine {
    /// Formats raw spoken text with deterministic verbal punctuation and capitalization rules.
    pub fn format(text: &str) -> String {
        if text.trim().is_empty() {
            return String::new();
        }

        // 1. Verbal Punctuation Commands (multi-word first, eating adjacent whitespace)
        let mut s = RE_NEW_PARAGRAPH.replace_all(text, "\n\n").into_owned();
        s = RE_NEW_LINE.replace_all(&s, "\n").into_owned();
        s = RE_QUESTION_MARK.replace_all(&s, "?").into_owned();
        s = RE_EXCLAMATION.replace_all(&s, "!").into_owned();
        s = RE_FULL_STOP.replace_all(&s, ".").into_owned();
        s = RE_OPEN_PAREN.replace_all(&s, "(").into_owned();
        s = RE_CLOSE_PAREN.replace_all(&s, ")").into_owned();
        s = RE_OPEN_QUOTE.replace_all(&s, "\"").into_owned();
        s = RE_CLOSE_QUOTE.replace_all(&s, "\"").into_owned();

        // Single-word punctuation commands
        s = RE_COMMA.replace_all(&s, ",").into_owned();
        s = RE_PERIOD.replace_all(&s, ".").into_owned();
        s = RE_COLON.replace_all(&s, ":").into_owned();
        s = RE_SEMICOLON.replace_all(&s, ";").into_owned();
        s = RE_DASH.replace_all(&s, "-").into_owned();

        // 2. Whitespace and Typography Normalization
        s = RE_SPACE_BEFORE_PUNCT.replace_all(&s, "$1").into_owned();
        s = RE_MISSING_SPACE_AFTER_PUNCT
            .replace_all(&s, "$1 $2")
            .into_owned();
        s = RE_CLEAN_NEWLINES.replace_all(&s, "\n").into_owned();
        // Restore double newlines for paragraphs
        s = s.replace("\n\n\n", "\n\n");
        s = RE_CONSECUTIVE_SPACES.replace_all(&s, " ").into_owned();

        // 3. Sentence Capitalization
        Self::capitalize_sentences(&s)
    }

    /// Capitalizes the first letter of sentences (following start of string, or '.', '?', '!', '\n', or quote start).
    fn capitalize_sentences(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let mut capitalize_next = true;

        for c in input.chars() {
            if capitalize_next && c.is_alphabetic() {
                result.extend(c.to_uppercase());
                capitalize_next = false;
            } else {
                result.push(c);
                if c == '.' || c == '?' || c == '!' || c == '\n' || c == '"' {
                    capitalize_next = true;
                } else if !c.is_whitespace() && c != '\'' && c != '(' {
                    capitalize_next = false;
                }
            }
        }

        result.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spoken_punctuation_replacement() {
        let raw = "hello comma world period this is a test question mark";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "Hello, world. This is a test?");
    }

    #[test]
    fn test_newlines_and_paragraphs() {
        let raw = "first paragraph period new paragraph second paragraph with a newline and more text period";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(
            formatted,
            "First paragraph.\n\nSecond paragraph with a\nAnd more text."
        );
    }

    #[test]
    fn test_parentheses_and_quotes() {
        let raw = "note colon open paren important close paren open quote hello close quote";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "Note: (important) \"Hello\"");
    }

    #[test]
    fn test_punctuation_spacing_cleanup() {
        let raw = "hello , world ! how are you ? fine ; thanks .";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "Hello, world! How are you? Fine; thanks.");
    }
}
