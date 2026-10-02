# Voice Commands & Smart Features — Research & Proposal

## Context

Voisu currently handles 14 spoken punctuation tokens (`comma`, `period`, `question mark`, `exclamation mark`, `full stop`, `colon`, `semicolon`, `dash`/`hyphen`, `new line`, `new paragraph`, `open paren`, `close paren`, `open quote`, `close quote`) in [`src/core/formatting.rs`](file:///c:/Users/adity/OneDrive/Desktop/TTS_OP/src/core/formatting.rs). This proposal lays out what to add next.

Research sources: Dragon NaturallySpeaking command set (Nuance docs), Apple Dictation symbol list (apple.com), Windows 11 Voice Access (microsoft.com), Google Docs voice typing (support.google.com), Talon community command set (talonvoice.com).

---

## 1. Missing Symbols & Punctuation

These are symbols every major dictation engine supports that Voisu doesn't handle yet.

| Say this | Output | Notes |
|---|---|---|
| "exclamation" (short form) | `!` | Currently requires "exclamation mark" |
| "apostrophe" | `'` | Very common — "don't", "it's" |
| "at sign" | `@` | Email addresses |
| "hashtag" / "hash" / "pound sign" | `#` | Social media, Markdown headings |
| "dollar sign" | `$` | Currency |
| "percent" / "percent sign" | `%` | |
| "ampersand" / "and sign" | `&` | |
| "asterisk" / "star" | `*` | Markdown bold, bullets |
| "forward slash" / "slash" | `/` | URLs, paths |
| "backslash" | `\` | Windows paths |
| "underscore" | `_` | Identifiers |
| "pipe" / "vertical bar" | `\|` | |
| "tilde" | `~` | Markdown strikethrough |
| "plus sign" | `+` | |
| "equals" / "equals sign" | `=` | |
| "open bracket" / "open square bracket" | `[` | |
| "close bracket" / "close square bracket" | `]` | |
| "open brace" / "open curly bracket" | `{` | |
| "close brace" / "close curly bracket" | `}` | |
| "open angle bracket" / "less than" | `<` | |
| "close angle bracket" / "greater than" | `>` | |
| "ellipsis" / "dot dot dot" | `…` | |
| "em dash" | `—` | |
| "en dash" | `–` | |

> **Implementation**: Straightforward regex additions to `FormattingEngine::format()`. Each is one `LazyLock<Regex>` + one `replace_all` call. The existing pattern handles this cleanly.

---

## 2. Document Structure Commands

Voice commands for building structured text on the fly.

| Say this | Output | How it works |
|---|---|---|
| "bullet point" / "add bullet" | `\n• ` | Insert newline + bullet |
| "next bullet" / "next item" | `\n• ` | Continue bullet list |
| "number one" through "number ten" | `\n1. `, `\n2. `, ... | Start/continue numbered list |
| "numbered list" / "start numbered list" | `\n1. ` | Begin numbered list |
| "next number" | `\n{n+1}. ` | Auto-increment counter |
| "end list" | (reset counter, double newline) | Exit list mode |
| "tab" / "tab key" | `\t` | Indent |
| "indent" | `    ` (4 spaces) | Alternative indent |
| "heading one" through "heading six" | `\n# ` through `\n###### ` | Markdown headings |

> **Implementation**: Needs a small piece of state — a `list_counter: Option<u32>` that tracks the current numbered list position. "next number" bumps it, "end list" resets it. The rest are stateless regex replacements.

---

## 3. Text Formatting / Markup Commands

For when you want bold, italic, etc. in the dictated output. Two approaches:

### Approach A: Inline Markers (recommended for Voisu)

Since Voisu outputs plain text / Markdown, wrap content with Markdown syntax.

| Say this | Output | Example |
|---|---|---|
| "bold" ... "end bold" | `**...**` | "bold important end bold" → `**important**` |
| "italic" / "italicize" ... "end italic" | `*...*` | |
| "underline" ... "end underline" | `<u>...</u>` | HTML tag |
| "strikethrough" ... "end strikethrough" | `~~...~~` | Markdown |
| "code" / "inline code" ... "end code" | `` `...` `` | Code fence |
| "code block" ... "end code block" | ` ```\n...\n``` ` | Fenced code block |
| "block quote" ... "end block quote" | `> ...` | Markdown blockquote |

### Approach B: "That" Retroactive Commands (like Dragon)

These act on the last phrase spoken:

| Say this | Effect |
|---|---|
| "bold that" | Wrap the last phrase in `**...**` |
| "italicize that" | Wrap the last phrase in `*...*` |
| "capitalize that" | Title Case the last phrase |
| "all caps that" | UPPERCASE the last phrase |
| "lowercase that" | lowercase the last phrase |

> **Implementation for Approach A**: Track open/close state for each marker. When "bold" is spoken, set `bold_open = true` and insert `**`. When "end bold" is spoken, insert `**` and reset.
>
> **Implementation for Approach B**: Buffer the last N words spoken. When "bold that" fires, retroactively wrap the buffered phrase. More complex but very powerful.

---

## 4. Capitalization & Casing Commands

Pulled directly from Apple Dictation's proven command set.

| Say this | Effect |
|---|---|
| "caps on" ... "caps off" | Title Case everything between |
| "all caps" / "all caps on" ... "all caps off" | UPPERCASE everything between |
| "no caps" / "no caps on" ... "no caps off" | lowercase everything between |
| "no space on" ... "no space off" | Remove spaces between words (for passwords, URLs, identifiers) |
| "numeral [number]" | Force numeric output — "numeral five" → `5` |

> **Implementation**: State machine toggles. `caps_mode: enum { Normal, TitleCase, Upper, Lower }` and `no_space: bool`. The formatter checks these flags per-word before emitting.

---

## 5. Editing & Correction Commands

These are the highest-impact quality-of-life features. Every major engine has them.

| Say this | Effect |
|---|---|
| "scratch that" / "undo that" | Delete the last dictated phrase |
| "delete that" | Delete the last word or phrase |
| "delete last [N] words" | Remove the last N words |
| "select all" | Select all text (Ctrl+A) |
| "undo" | Send Ctrl+Z |
| "redo" | Send Ctrl+Y |

> **Implementation**: These are NOT formatting engine changes — they're delivery-layer commands. Voisu would need to:
> 1. Keep a rolling buffer of the last 3–5 delivered text chunks.
> 2. On "scratch that", send the appropriate number of Backspace keystrokes to erase the last chunk.
> 3. On "undo" / "redo", synthesize Ctrl+Z / Ctrl+Y via `SendInput`.

---

## 6. Smart Number & Date Formatting

When someone dictates numbers or dates, format them like a human would write them.

| Say this | Output |
|---|---|
| "twenty five percent" | `25%` |
| "one hundred dollars" | `$100` |
| "three point one four" | `3.14` |
| "October second twenty twenty six" | `October 2, 2026` |
| "ten AM" / "three thirty PM" | `10:00 AM` / `3:30 PM` |
| "phone number nine eight seven six five four three two one zero" | `(987) 654-3210` |

> **Implementation**: Regex-based number word → digit conversion is doable but fragile. Better approach: let the ASR engines handle this natively (both Deepgram and Groq already output "25%" for "twenty five percent" when configured correctly). Voisu should avoid double-converting.

---

## 7. Text Expansion / Custom Snippets

Let users define personal shortcuts.

| Say this | Expands to |
|---|---|
| "insert my email" | `user@example.com` |
| "insert my phone" | `+91 98765 43210` |
| "insert signature" | `Best regards,\nAditya Wadia` |
| "insert date" | Today's date in user's preferred format |
| "insert time" | Current time |

> **Implementation**: A `[snippets]` section in `config.json` mapping trigger phrases to expansion text. The formatting engine checks for snippet triggers before doing punctuation replacements. `insert date` and `insert time` are hardcoded special cases using `chrono`.

---

## 8. Brainstormed Bonus Features

Things no other dictation tool does well, but Voisu could.

### a) Markdown-Native Mode
Since developers and writers often work in Markdown, add a config toggle `output_format: "markdown" | "plain"`. In markdown mode, heading/bold/list commands emit actual Markdown syntax. In plain mode, they emit Unicode equivalents or skip formatting.

### b) Dictation Clipboard History
Keep a local log of the last 20 dictation chunks with timestamps. Accessible via the system tray context menu ("Show Dictation History"). Useful for recovering something you dictated 10 minutes ago but accidentally overwrote.

### c) Multi-Language Mid-Sentence Switching
The user already speaks Hinglish. A "switch to Hindi" / "switch to English" command could tell the ASR layer to change language mid-session without restarting the recording. Deepgram supports this with its `language` parameter.

### d) Voice-Triggered App Actions
Beyond text — let users say "open browser", "switch window", "take screenshot" to trigger OS-level actions via Voisu. More of a stretch goal, but the hotkey infrastructure is already there.

### e) Whisper Mode / Quiet Dictation
A sensitivity toggle ("whisper mode on") that boosts the audio gain and adjusts the VAD threshold so the user can dictate quietly in meetings or shared spaces without being overheard.

---

## Priority Ranking (What to Build First)

| Priority | Feature | Why |
|---|---|---|
| **P0** | Missing symbols (#1) | Direct user request, trivial to implement, high daily use |
| **P0** | Apostrophe handling | Extremely common in English contractions |
| **P1** | Bullet and numbered list commands (#2) | Direct user request, moderate complexity |
| **P1** | Bold/italic inline markers (#3 Approach A) | Direct user request, moderate complexity |
| **P1** | "Scratch that" / undo (#5) | Highest QoL impact, needs delivery-layer buffer |
| **P2** | Capitalization toggles (#4) | Nice to have, used less often |
| **P2** | Text expansion snippets (#7) | Powerful but needs config UI |
| **P3** | Smart number formatting (#6) | ASR engines mostly handle this already |
| **P3** | Dictation history (#8b) | Polish feature |
| **P3** | Markdown mode toggle (#8a) | Nice differentiator |

---

## Sources

- Nuance Dragon NaturallySpeaking Command Reference — [nuance.com](https://www.nuance.com/dragon/dragon-for-pc/commands.html)
- Apple Dictation Commands — [support.apple.com](https://support.apple.com/guide/mac-help/use-dictation-mh40584/mac)
- Windows 11 Voice Access — [support.microsoft.com](https://support.microsoft.com/en-us/topic/use-voice-access-to-control-your-pc-open-apps-browse-the-web-with-your-voice)
- Google Docs Voice Typing — [support.google.com](https://support.google.com/docs/answer/4492226)
- Talon Community Commands — [talonvoice.com](https://talonvoice.com/docs/)
