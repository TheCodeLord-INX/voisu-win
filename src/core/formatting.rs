//! Deterministic spoken punctuation parser, document structure commands, and text markup engine.
//!
//! Replaces spoken verbal commands (e.g. "exclamation", "question mark", "create a paragraph",
//! "bullet point", "numbered list", "bold that", "scratch that") with standardized symbols,
//! structured Markdown/typography, and clean sentence capitalization in < 2ms without LLMs.

use regex::{Captures, Regex};
use std::sync::{LazyLock, Mutex};

// ---------------------------------------------------------------------------
// Document Structure Regexes
// ---------------------------------------------------------------------------

static RE_NEW_PARAGRAPH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*(new paragraph|create a paragraph|create paragraph|start paragraph|make a paragraph)\s*").unwrap()
});
static RE_NEW_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(new line|newline|next line)\s*").unwrap());

static RE_BULLET_LIST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*(create a list|create an unnumbered list|create unnumbered list|start a list|bullet list|bullet point|add bullet|next bullet|next item)\s*").unwrap()
});

static RE_NUMBERED_LIST_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*(create a numbered list|create numbered list|start a numbered list|start numbered list|numbered list)\s*").unwrap()
});

static RE_NEXT_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*next number\s*").unwrap());

static RE_NUMBER_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*number (one|two|three|four|five|six|seven|eight|nine|ten)\b\s*").unwrap()
});

static RE_END_LIST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*end list\s*").unwrap());

static RE_TAB: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(tab key|tab)\s*").unwrap());
static RE_INDENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*\bindent\b\s*").unwrap());

static RE_HEADINGS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*heading (one|two|three|four|five|six)\s*").unwrap()
});

// ---------------------------------------------------------------------------
// Spoken Punctuation & Signs Regexes (Multi-word processed first)
// ---------------------------------------------------------------------------

static RE_QUESTION_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*question mark\b").unwrap());
static RE_EXCLAMATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*(exclamation mark|exclamation point|exclamation)\b").unwrap()
});
static RE_FULL_STOP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*full stop\b").unwrap());

// Dashes
static RE_EM_DASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*em dash\s*").unwrap());
static RE_EN_DASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*en dash\s*").unwrap());

// Brackets & Braces
static RE_OPEN_PAREN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(open parenthesis|open paren)\s*").unwrap());
static RE_CLOSE_PAREN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(close parenthesis|close paren)\b").unwrap());

static RE_OPEN_BRACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(open curly brace|open curly bracket|open brace)\s*").unwrap()
});
static RE_CLOSE_BRACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*(close curly brace|close curly bracket|close brace)\b").unwrap()
});

static RE_OPEN_BRACKET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(open square bracket|open bracket)\s*").unwrap()
});
static RE_CLOSE_BRACKET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*(close square bracket|close bracket)\b").unwrap()
});

static RE_OPEN_ANGLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(open angle bracket|less than sign|less than)\s*").unwrap()
});
static RE_CLOSE_ANGLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*(close angle bracket|greater than sign|greater than)\b").unwrap()
});

// Quotes
static RE_OPEN_QUOTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\b(open quote|start quote)\s*"#).unwrap());
static RE_CLOSE_QUOTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\s*(close quote|end quote|unquote)\b"#).unwrap());

// Special symbols
static RE_AT_SIGN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(at sign|at symbol)\s*").unwrap());
static RE_EMAIL_AT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)\s*@\s*(\w+)").unwrap());
static RE_DOT_COM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*dot (com|org|net|io|edu|gov|co|in|dev)\b").unwrap());

static RE_HASHTAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(hashtag|hash tag|hash sign|pound sign)\s*").unwrap()
});
static RE_DOLLAR_SIGN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(dollar sign|dollars sign)\s*").unwrap());
static RE_PERCENT_SIGN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(percent sign|percentage sign|percent)\b").unwrap());
static RE_AMPERSAND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(ampersand|and sign)\s*").unwrap());
static RE_ASTERISK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(asterisk|star sign)\s*").unwrap());
static RE_FORWARD_SLASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(forward slash|slash)\s*").unwrap());
static RE_BACKSLASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*backslash\s*").unwrap());
static RE_UNDERSCORE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(underscore|under score)\s*").unwrap());
static RE_PIPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*(vertical bar|vertical pipe|pipe sign|pipe symbol|pipe)\s*").unwrap()
});
static RE_TILDE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*tilde\s*").unwrap());
static RE_PLUS_SIGN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(plus sign|plus symbol)\s*").unwrap());
static RE_EQUALS_SIGN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(equals sign|equal sign|equals)\s*").unwrap());
static RE_ELLIPSIS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(ellipsis|dot dot dot)\b").unwrap());

// Apostrophe & Single Quote
static RE_APOSTROPHE_CONTRACTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b([a-z]+)\s*(?:apostrophe|single quote)\s*(s|t|d|ll|m|re|ve)\b").unwrap()
});
static RE_APOSTROPHE_STANDALONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(apostrophe|single quote)\b").unwrap());

static RE_QUOTE_ATTACH_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)'\s+([A-Za-z0-9])").unwrap());
static RE_QUOTE_ATTACH_CLOSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z0-9])\s+'(?:$|\s|[,.:;?!])").unwrap());

// Single-word punctuation commands
static RE_COMMA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*\bcomma\b").unwrap());
static RE_PERIOD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*\bperiod\b").unwrap());
static RE_COLON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*\bcolon\b").unwrap());
static RE_SEMICOLON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*\bsemicolon\b").unwrap());
static RE_DASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*\b(hyphen|dash)\b\s*").unwrap());

// ---------------------------------------------------------------------------
// Formatting Markup Commands (Inline markers, Retroactive "that", Whole text)
// ---------------------------------------------------------------------------

// Inline bounded markup
static RE_INLINE_BOLD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:bold|start bold)\s+([\s\S]+?)\s+end bold\b").unwrap()
});
static RE_INLINE_ITALIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:italic|italicize|start italic)\s+([\s\S]+?)\s+(?:end italic|end italicize)\b").unwrap()
});
static RE_INLINE_UNDERLINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:underline|start underline)\s+([\s\S]+?)\s+end underline\b").unwrap()
});
static RE_INLINE_STRIKE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:strikethrough|start strikethrough)\s+([\s\S]+?)\s+end strikethrough\b").unwrap()
});
static RE_INLINE_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:code|inline code)\s+([\s\S]+?)\s+end code\b").unwrap()
});
static RE_CODE_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bcode block\s+([\s\S]+?)\s+end code block\b").unwrap()
});
static RE_INLINE_QUOTES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:add quotes|quotes|quote)\s+([\s\S]+?)\s+(?:end quotes|end quote)\b"#).unwrap()
});
static RE_BLOCK_QUOTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:block quote|quote block)\s+([\s\S]+?)\s+(?:end block quote|end quote block)\b").unwrap()
});

// Retroactive "that" commands (operates on the word or phrase immediately before the command)
static RE_BOLD_THAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(\w+)\s+bold that\b").unwrap()
});
static RE_ITALIC_THAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(\w+)\s+(?:italicize|italic) that\b").unwrap()
});
static RE_QUOTE_THAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(\w+)\s+(?:quote that|add quotes to that|add quotes that)\b"#).unwrap()
});
static RE_UNDERLINE_THAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(\w+)\s+underline that\b").unwrap()
});
static RE_STRIKE_THAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(\w+)\s+strikethrough that\b").unwrap()
});

// Whole-text commands
static RE_BOLD_WHOLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*\bbold (whole|all|everything)\b\s*").unwrap()
});
static RE_ITALIC_WHOLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*\b(?:italicize|italic) (whole|all|everything)\b\s*").unwrap()
});
static RE_QUOTE_WHOLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\s*\b(?:quote|quotes|add quotes to) (whole|all|everything)\b\s*"#).unwrap()
});

// Intra-utterance "scratch that" / correction (discards the word preceding "scratch that")
static RE_INTRA_SCRATCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b\w+\s+(?:scratch that|undo that|delete that)\s+").unwrap()
});

// Standalone scratch / undo command matcher
static RE_STANDALONE_SCRATCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(scratch that|undo that|delete that|undo|scratch)\s*[.?!]?\s*$").unwrap()
});

// ---------------------------------------------------------------------------
// Typography & Spacing Normalization Regexes
// ---------------------------------------------------------------------------

// Eliminates whitespace before standard punctuation marks and closing delimiters
static RE_SPACE_BEFORE_PUNCT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\s+([,.:;?!…)\]\}])").unwrap()
});

// Ensures exactly one character space after punctuation marks if followed by a character
static RE_MISSING_SPACE_AFTER_PUNCT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([,.:;?!…])([^\s\d,.:;?!…)\]\}'"])"#).unwrap()
});

static RE_CLEAN_NEWLINES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t]*\n[ \t]*").unwrap());
static RE_CONSECUTIVE_SPACES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t]+").unwrap());

// ---------------------------------------------------------------------------
// Engine State & Implementation
// ---------------------------------------------------------------------------

/// Persistent state for multi-turn dictation sessions (e.g. list numbering).
#[derive(Debug, Clone, Default)]
pub struct FormattingState {
    pub list_counter: Option<u32>,
}

static GLOBAL_STATE: LazyLock<Mutex<FormattingState>> =
    LazyLock::new(|| Mutex::new(FormattingState::default()));

pub struct FormattingEngine;

impl FormattingEngine {
    /// Formats raw spoken text using the shared global dictation session state.
    pub fn format(text: &str) -> String {
        let mut state = match GLOBAL_STATE.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        Self::format_with_state(text, &mut state)
    }

    /// Resets the global list and session state (e.g. on new user session or test isolation).
    pub fn reset_state() {
        let mut state = match GLOBAL_STATE.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *state = FormattingState::default();
    }

    /// Checks if an entire transcription candidate is a standalone "scratch that" / "undo" command.
    pub fn is_scratch_command(text: &str) -> bool {
        RE_STANDALONE_SCRATCH.is_match(text)
    }

    /// Formats spoken text with deterministic rules, verbal punctuation, and structured markup.
    pub fn format_with_state(text: &str, state: &mut FormattingState) -> String {
        if text.trim().is_empty() {
            return String::new();
        }

        // 0. Handle Intra-utterance corrections ("scratch that")
        let mut s = RE_INTRA_SCRATCH.replace_all(text, "").into_owned();

        // 1. Whole-text formatting flags
        let wrap_bold_whole = RE_BOLD_WHOLE.is_match(&s);
        let wrap_italic_whole = RE_ITALIC_WHOLE.is_match(&s);
        let wrap_quote_whole = RE_QUOTE_WHOLE.is_match(&s);

        if wrap_bold_whole {
            s = RE_BOLD_WHOLE.replace_all(&s, "").into_owned();
        }
        if wrap_italic_whole {
            s = RE_ITALIC_WHOLE.replace_all(&s, "").into_owned();
        }
        if wrap_quote_whole {
            s = RE_QUOTE_WHOLE.replace_all(&s, "").into_owned();
        }

        // 2. Document Structure Commands (Paragraphs, Headings, Lists, Tabs)
        s = RE_NEW_PARAGRAPH.replace_all(&s, "\n\n").into_owned();
        s = RE_NEW_LINE.replace_all(&s, "\n").into_owned();
        s = RE_TAB.replace_all(&s, "\t").into_owned();
        s = RE_INDENT.replace_all(&s, "    ").into_owned();

        // Headings
        s = RE_HEADINGS
            .replace_all(&s, |caps: &Captures| {
                let level = match caps[1].to_lowercase().as_str() {
                    "one" => "#",
                    "two" => "##",
                    "three" => "###",
                    "four" => "####",
                    "five" => "#####",
                    "six" => "######",
                    _ => "#",
                };
                format!("\n{} ", level)
            })
            .into_owned();

        // Bullet lists
        s = RE_BULLET_LIST.replace_all(&s, "\n• ").into_owned();

        // Numbered lists
        s = RE_NUMBERED_LIST_START
            .replace_all(&s, |_caps: &Captures| {
                state.list_counter = Some(1);
                "\n1. "
            })
            .into_owned();

        s = RE_NEXT_NUMBER
            .replace_all(&s, |_caps: &Captures| {
                let next = state.list_counter.map(|c| c + 1).unwrap_or(1);
                state.list_counter = Some(next);
                format!("\n{}. ", next)
            })
            .into_owned();

        s = RE_NUMBER_WORD
            .replace_all(&s, |caps: &Captures| {
                let num = match caps[1].to_lowercase().as_str() {
                    "one" => 1,
                    "two" => 2,
                    "three" => 3,
                    "four" => 4,
                    "five" => 5,
                    "six" => 6,
                    "seven" => 7,
                    "eight" => 8,
                    "nine" => 9,
                    "ten" => 10,
                    _ => 1,
                };
                state.list_counter = Some(num);
                format!("\n{}. ", num)
            })
            .into_owned();

        if RE_END_LIST.is_match(&s) {
            state.list_counter = None;
            s = RE_END_LIST.replace_all(&s, "\n\n").into_owned();
        }

        // 3. Text Formatting Markup (Inline bounded & Retroactive "that")
        s = RE_INLINE_BOLD.replace_all(&s, "**$1**").into_owned();
        s = RE_INLINE_ITALIC.replace_all(&s, "*$1*").into_owned();
        s = RE_INLINE_UNDERLINE.replace_all(&s, "<u>$1</u>").into_owned();
        s = RE_INLINE_STRIKE.replace_all(&s, "~~$1~~").into_owned();
        s = RE_INLINE_CODE.replace_all(&s, "`$1`").into_owned();
        s = RE_CODE_BLOCK.replace_all(&s, "\n```\n$1\n```\n").into_owned();
        s = RE_INLINE_QUOTES.replace_all(&s, "\"$1\"").into_owned();
        s = RE_BLOCK_QUOTE.replace_all(&s, "\n> $1").into_owned();

        // Retroactive "that" commands
        s = RE_BOLD_THAT.replace_all(&s, " **$1**").into_owned();
        s = RE_ITALIC_THAT.replace_all(&s, " *$1*").into_owned();
        s = RE_QUOTE_THAT.replace_all(&s, " \"$1\"").into_owned();
        s = RE_UNDERLINE_THAT.replace_all(&s, " <u>$1</u>").into_owned();
        s = RE_STRIKE_THAT.replace_all(&s, " ~~$1~~").into_owned();

        // 4. Spoken Punctuation & Special Signs (Multi-word commands first)
        s = RE_QUESTION_MARK.replace_all(&s, "?").into_owned();
        s = RE_EXCLAMATION.replace_all(&s, "!").into_owned();
        s = RE_FULL_STOP.replace_all(&s, ".").into_owned();
        s = RE_EM_DASH.replace_all(&s, " — ").into_owned();
        s = RE_EN_DASH.replace_all(&s, " – ").into_owned();
        s = RE_ELLIPSIS.replace_all(&s, "…").into_owned();

        // Brackets / Quotes
        s = RE_OPEN_PAREN.replace_all(&s, "(").into_owned();
        s = RE_CLOSE_PAREN.replace_all(&s, ")").into_owned();
        s = RE_OPEN_BRACE.replace_all(&s, "{").into_owned();
        s = RE_CLOSE_BRACE.replace_all(&s, "}").into_owned();
        s = RE_OPEN_BRACKET.replace_all(&s, "[").into_owned();
        s = RE_CLOSE_BRACKET.replace_all(&s, "]").into_owned();
        s = RE_OPEN_ANGLE.replace_all(&s, "<").into_owned();
        s = RE_CLOSE_ANGLE.replace_all(&s, ">").into_owned();
        s = RE_OPEN_QUOTE.replace_all(&s, "\"").into_owned();
        s = RE_CLOSE_QUOTE.replace_all(&s, "\"").into_owned();

        // Symbols & Signs
        s = RE_AT_SIGN.replace_all(&s, "@").into_owned();
        s = RE_EMAIL_AT.replace_all(&s, "$1@$2").into_owned();
        s = RE_DOT_COM.replace_all(&s, "__DOT_${1}__").into_owned();
        s = RE_HASHTAG.replace_all(&s, "#").into_owned();
        s = RE_DOLLAR_SIGN.replace_all(&s, "$").into_owned();
        s = RE_PERCENT_SIGN.replace_all(&s, "%").into_owned();
        s = RE_AMPERSAND.replace_all(&s, " & ").into_owned();
        s = RE_ASTERISK.replace_all(&s, "*").into_owned();
        s = RE_FORWARD_SLASH.replace_all(&s, "/").into_owned();
        s = RE_BACKSLASH.replace_all(&s, "\\").into_owned();
        s = RE_UNDERSCORE.replace_all(&s, "_").into_owned();
        s = RE_PIPE.replace_all(&s, "|").into_owned();
        s = RE_TILDE.replace_all(&s, "~").into_owned();
        s = RE_PLUS_SIGN.replace_all(&s, "+").into_owned();
        s = RE_EQUALS_SIGN.replace_all(&s, "=").into_owned();

        // Apostrophe / Contractions
        s = RE_APOSTROPHE_CONTRACTION
            .replace_all(&s, "$1'$2")
            .into_owned();
        s = RE_APOSTROPHE_STANDALONE.replace_all(&s, "'").into_owned();

        // Attach single quotes to words ('quoted')
        s = RE_QUOTE_ATTACH_OPEN.replace_all(&s, " '$1").into_owned();
        s = RE_QUOTE_ATTACH_CLOSE.replace_all(&s, "$1' ").into_owned();

        // Single-word punctuation commands
        s = RE_COMMA.replace_all(&s, ",").into_owned();
        s = RE_PERIOD.replace_all(&s, ".").into_owned();
        s = RE_COLON.replace_all(&s, ":").into_owned();
        s = RE_SEMICOLON.replace_all(&s, ";").into_owned();
        s = RE_DASH.replace_all(&s, "-").into_owned();

        // 5. Whitespace and Typography Normalization
        // Remove spaces before punctuation (, . : ; ? ! … — – ) ] })
        s = RE_SPACE_BEFORE_PUNCT.replace_all(&s, "$1").into_owned();

        // Ensure exactly one space after punctuation (, . : ; ? ! …)
        s = RE_MISSING_SPACE_AFTER_PUNCT
            .replace_all(&s, "$1 $2")
            .into_owned();

        // Clean newline whitespace
        s = RE_CLEAN_NEWLINES.replace_all(&s, "\n").into_owned();

        // Normalize multiple consecutive spaces to a single character space
        s = RE_CONSECUTIVE_SPACES.replace_all(&s, " ").into_owned();

        // Clean double-newlines formatting
        while s.contains("\n\n\n") {
            s = s.replace("\n\n\n", "\n\n");
        }

        // 6. Sentence Capitalization
        let mut result = Self::capitalize_sentences(&s);

        // Restore domain extensions (.com, .org, etc.)
        static RE_RESTORE_DOT: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"__DOT_([a-zA-Z]+)__").unwrap()
        });
        result = RE_RESTORE_DOT
            .replace_all(&result, |caps: &Captures| {
                format!(".{}", caps[1].to_lowercase())
            })
            .into_owned();

        // 7. Apply whole-text wrapping if requested
        if wrap_bold_whole && !result.is_empty() {
            result = format!("**{}**", result);
        } else if wrap_italic_whole && !result.is_empty() {
            result = format!("*{}*", result);
        } else if wrap_quote_whole && !result.is_empty() {
            result = format!("\"{}\"", result);
        }

        result
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
                if c == '.' || c == '?' || c == '!' || c == '\n' || c == '"' || c == '…' || c == '—' {
                    capitalize_next = true;
                } else if !c.is_whitespace() && c != '\'' && c != '(' && c != '*' && c != '#' && c != '•' {
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
    fn test_exclamation_short_and_spacing() {
        let raw = "hello exclamation world exclamation how are you question mark";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "Hello! World! How are you?");
    }

    #[test]
    fn test_missing_symbols() {
        let raw = "email user at sign example dot com hashtag trending dollar sign 50 percent sign";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "Email user@example.com #trending $50%");
    }

    #[test]
    fn test_apostrophe_and_contractions() {
        let raw = "it apostrophe s working and don apostrophe t stop apostrophe quoted apostrophe";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "It's working and don't stop 'quoted'");
    }

    #[test]
    fn test_brackets_braces_and_dashes() {
        let raw = "start open bracket test close bracket open brace obj close brace em dash note";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "Start [test] {obj} — Note");
    }

    #[test]
    fn test_document_structure_paragraphs_and_bullets() {
        let raw = "create a paragraph bullet point first item next bullet second item";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "• First item\n• Second item");
    }

    #[test]
    fn test_numbered_lists() {
        FormattingEngine::reset_state();
        let raw = "create a numbered list buy groceries next number wash car next number cook dinner end list";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "1. Buy groceries\n2. Wash car\n3. Cook dinner");
    }

    #[test]
    fn test_headings() {
        let raw = "heading one introduction heading two details";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "# Introduction\n## Details");
    }

    #[test]
    fn test_inline_formatting_markup() {
        let raw = "this is bold important end bold and italic urgent end italic and code fn test end code";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "This is **important** and *urgent* and `fn test`");
    }

    #[test]
    fn test_retroactive_and_whole_formatting() {
        let raw = "this is urgent bold that";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "This is **urgent**");

        let raw_whole = "critical alert bold whole";
        let formatted_whole = FormattingEngine::format(raw_whole);
        assert_eq!(formatted_whole, "**Critical alert**");
    }

    #[test]
    fn test_intra_utterance_scratch_that() {
        let raw = "we should meet on Friday scratch that Saturday at noon";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "We should meet on Saturday at noon");
    }

    #[test]
    fn test_standalone_scratch_command() {
        assert!(FormattingEngine::is_scratch_command("scratch that"));
        assert!(FormattingEngine::is_scratch_command("undo that"));
        assert!(FormattingEngine::is_scratch_command("delete that"));
        assert!(!FormattingEngine::is_scratch_command("hello scratch that world"));
    }

    #[test]
    fn test_punctuation_spacing_cleanup() {
        let raw = "hello , world ! how are you ? fine ; thanks .";
        let formatted = FormattingEngine::format(raw);
        assert_eq!(formatted, "Hello, world! How are you? Fine; thanks.");
    }
}
