# Voisu for Windows

Fast, dual-engine speech-to-text dictation client designed specifically for Windows.

Voisu runs locally in the background, races two cloud speech engines in real time, arbitrates the best transcription, and types or copies the text right where your cursor is.

---

## About Voisu

Most Windows dictation tools fall into two camps: either they're clunky cloud apps running inside a browser tab, or built-in OS tools that feel slow and struggle with technical vocabulary, slang, and bilingual speech like Hinglish. Dictating your thoughts shouldn't break your train of thought.

We built Voisu to solve that frustration. It sits quietly in your Windows system tray and gives you instant speech-to-text with a single keypress. Under the hood, it streams your voice to two independent engines at the same time—Deepgram Nova-2 and Groq Whisper Large v3 on LPUs. Whichever gives the cleanest, fastest transcription wins. If both engines stumble over a niche acronym or proper noun, a tiny background LPU reconciliation step fixes the disagreement before anything touches your screen.

Everything happens in under 400 milliseconds. Punctuation signs, bullet points, numbered lists, and Markdown styling work naturally through voice commands. The app never steals window focus, pastes directly where you type, and respects your workflow.

---

## Highlights

- **Dual-Engine Race**: Streams audio simultaneously to Deepgram Nova-2 and Groq Whisper Large v3 LPUs, keeping latency around ~400ms.
- **AI Name & Jargon Reconciler**: When engines disagree on tricky proper nouns or Hinglish, a background LPU prompt reconciles them instantly without manual dictionaries.
- **Voice Commands & Formatting**: Speak punctuation names, Markdown headings, bullet lists, numbered lists, and bold/italic markup on the fly.
- **Neo-Brutalist Floating Pill**: High-contrast, snappy visual feedback that pops up at the bottom of your screen when you speak and gets out of your way when done.
- **Dynamic Waveform Visualizer**: Equalizer bars respond to your voice volume across low and high frequencies.
- **Focus-Aware Delivery**: Pastes directly into active text inputs. If you're on your desktop or a non-editable surface, it silently copies the transcription to your clipboard instead.
- **Background Tray & Boot Support**: Runs quietly in the Windows system tray. Can automatically start with Windows with zero console flicker.
- **Smart Mic Detection**: Automatically detects and switches between built-in laptop mic arrays and external headsets.

---

## Voice Commands & Formatting

Speak naturally — Voisu formats punctuation, lists, and markup on the fly with zero cloud roundtrips (< 1ms).

### Punctuation & Symbols
- **Exclamation & Questions**: Say `"exclamation"` $\to$ `!`, `"question mark"` $\to$ `?` (with clean spacing and capitalization).
- **Basics**: Say `"comma"`, `"period"`, `"colon"`, `"semicolon"` $\to$ `,`, `.`, `:`, `;`
- **Contractions & Quotes**: Say `"it apostrophe s"` $\to$ `it's`, `"apostrophe test apostrophe"` $\to$ `'test'`.
- **Web & Social**: Say `"user at sign domain dot com"` $\to$ `user@domain.com`, `"hashtag trending"` $\to$ `#trending`.
- **Numbers & Math**: Say `"dollar sign 50"` $\to$ `$50`, `"50 percent sign"` $\to$ `50%`, `"plus sign"`, `"equals sign"`.
- **Code & Syntax**: Brackets (`[]`), braces (`{}`), angle brackets (`<>`), forward/backslash (`/`, `\`), pipes (`|`), tildes (`~`).

### Lists & Headings
- **Paragraphs**: Say `"create a paragraph"` or `"new paragraph"` for double line breaks.
- **Bullet Lists**: Say `"create a list"` or `"bullet point"` $\to$ inserts `• ` items.
- **Numbered Lists**: Say `"create a numbered list"` $\to$ begins with `1. `, then say `"next number"` to auto-increment (`2. `, `3. `...). Say `"end list"` when finished.
- **Headings**: Say `"heading one"` through `"heading six"` for Markdown headings (`# ` through `###### `).

### Text Markup
- **Inline Styling**: Say `"bold urgent end bold"` $\to$ `**urgent**`. Supports `italic`, `code`, `underline`, and `strikethrough`.
- **Retroactive Styling**: Say `"critical bold that"` $\to$ `**critical**`.
- **Entire Sentence**: Say `"meeting postponed bold whole"` $\to$ `**Meeting postponed**`.

### Corrections & Voice Undo
- **In-sentence fix**: Say `"meet on Friday scratch that Saturday"` $\to$ drops the mistaken word and writes `"meet on Saturday"`.
- **Instant undo**: Say `"scratch that"` or `"undo that"` right after dictating to delete the last pasted phrase.

---

## Requirements

- Windows 10 or Windows 11 (x64)
- Rust 1.80+ (to build from source)
- API keys:
  - [Deepgram](https://deepgram.com) API Key
  - [Groq](https://groq.com) API Key

---

## Quick Setup

### 1. Build from Source

```powershell
git clone https://github.com/TheCodeLord-INX/voisu-win.git
cd voisu-win
cargo build --release
```

The compiled binary will be located at `target\release\voisu-win.exe`.

### 2. Configure API Credentials

Run the interactive setup wizard:

```powershell
.\target\release\voisu-win.exe setup
```

You'll be prompted to enter your Deepgram and Groq API keys, choose a default hotkey (defaults to `F8`), and pick your delivery mode.

You can verify your setup anytime with:

```powershell
.\target\release\voisu-win.exe doctor
```

### 3. Run the App

#### Normal Mode (with console logs)
```powershell
.\target\release\voisu-win.exe run
```

#### Silent Tray Mode
```powershell
.\target\release\voisu-win.exe run --tray
```

Once running, press your trigger key (`F8` by default) and start speaking.

---

## Windows Startup & System Tray

- **From the Tray**: Right-click the Voisu tray icon (next to your system clock) and click **Start with Windows** to toggle auto-start on boot.
- **From the Terminal**:
  ```powershell
  # Check status
  .\target\release\voisu-win.exe autostart status

  # Enable autostart on boot
  .\target\release\voisu-win.exe autostart enable

  # Disable autostart
  .\target\release\voisu-win.exe autostart disable
  ```

---

## Configuration

Config files live in `%APPDATA%\voisu\config.json`. You can inspect or tweak settings like hotkey modes, timeout windows, and audio device preferences directly.

---

## License

MIT
