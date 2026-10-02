# Product Requirements Document (PRD) — Voisu for Windows (`voisu-win`)

## 1. Problem Statement
Windows desktop users—especially developers, writers, and technical professionals—frequently experience severe friction when attempting voice dictation. Existing solutions either force users to run heavy local models that drain battery, max out GPU VRAM, and induce high latency (1–4 seconds), or rely on simplistic single-cloud APIs prone to hallucination, sluggish response times, and jarring text replacement. Furthermore, standard operating system speech tools lack developer-first vocabulary handling, fail to preserve multi-line code indentation, and fail closed when speech is ambiguous. Voisu for Windows solves this by delivering an ultra-fast (<350ms release-to-text), dual-provider cloud transcription pipeline with token-level confidence arbitration, zero-leak local baseline formatting, and seamless direct clipboard injection with instant restoration into any focused Windows application.

## 2. Target Users
- **The Flow-State Software Engineer ("Dev Raja")**: Writes code, terminal commands, PR comments, and Slack messages continuously. Needs instant, accurate dictation of programming symbols, variable names (`camelCase`, `snake_case`), and multi-line markdown without touching the mouse or waiting for a model to think.
- **The Technical Writer / Researcher ("Doc Elena")**: Dictates long-form technical specs, emails, and documentation. Needs spoken punctuation commands ("comma", "period", "new line"), quote pair preservation, and zero hallucinated rephrasing.
- **The Power Multitasker ("Operator Leo")**: Jumps across multiple windows (VS Code, Discord, Browser, Terminal). Requires an unobtrusive global hotkey (Push-to-Talk or Toggle) and instant delivery into whichever window has active focus.

## 3. Success Metrics
| Metric | Target | Measurement Method | Owner |
|---|---|---|---|
| **Release-to-Text Latency** | $\le 350\text{ ms}$ (p90) | Monotonic clock timestamp from hotkey release to final text injection | Audio/Delivery Lead |
| **Transcription Accuracy** | $\ge 98.5\%$ Word Accuracy on tech vocabulary | Levenshtein distance on test corpus containing dev jargon | Arbitration Lead |
| **Delivery Reliability** | $99.9\%$ successful injection | Successful window focus detection & clipboard auto-restore verification | Windows Native Lead |
| **Idle System Footprint** | $< 25\text{ MB}$ RAM, $0\%$ CPU | Windows Task Manager / Process memory working set during Idle state | Core Systems Lead |
| **Hallucination Rate** | $0.0\%$ (Strict Zero) | Verification that 100% of delivered tokens originate from provider source transcripts | Safety & QA Lead |

## 4. Core Features

### Must-Have (v1)
- **[P0] Dual-Cloud Parallel Race**:
  - Live 16kHz mono audio streaming to Deepgram Nova-2/3 over WebSocket while recording.
  - Concurrent batch dispatch to Groq Whisper Large v3 (via LPUs) on utterance completion.
  - Seamless fallback to whichever provider is configured if only one API key is present.
- **[P0] Asymmetric Divergence-Point Confidence Arbitration (Slice B4)**:
  - Levenshtein positional word alignment between Deepgram and Groq transcripts.
  - **Asymmetric** word-level confidence analysis: Deepgram provides per-word confidence scores; Groq words inherit segment-level `avg_logprob` converted to a `[0.0, 1.0]` proxy. Flips incumbent Deepgram token only when Deepgram word confidence $< 0.50$ and Groq segment proxy $\ge 0.75$.
  - Fail-closed meaning guards (strictly forbids flipping negation words, numbers, or question words).
  - Source-derived mathematical guarantee (no invented words).
  - Reports `ArbitrationMode` (DualProvider / SingleDeepgram / SingleGroq) for diagnostics.
- **[P0] Low-Latency WASAPI Audio Capture**:
  - Native Windows WASAPI loopback/microphone capture using Rust `cpal`.
  - In-memory 16-bit 16kHz PCM buffer management with dynamic RMS audio level calculation.
- **[P0] Global Hotkey & Interaction Engine**:
  - Low-level Windows keyboard hook (`WH_KEYBOARD_LL`) supporting Hybrid Mode: Hold-to-Talk (Push-to-Talk) and Tap-to-Toggle.
  - Default keys: `Caps Lock` (with lock-toggle suppression) or `Right Alt`.
- **[P0] Smart Clipboard Injection with Auto-Restore**:
  - Instant clipboard payload injection into focused window via synthetic `Ctrl+V`.
  - Immediate atomic restoration of prior clipboard content and format to prevent user disruption.
- **[P0] Deterministic Local Baseline Formatter**:
  - Sub-millisecond formatting of spoken punctuation (`"period"` $\rightarrow$ `"."`, `"comma"` $\rightarrow$ `","`, `"new line"` $\rightarrow$ `"\n"`).
  - Number words to digits, quote pair nesting, and contraction normalization.
- **[P0] Windows Tray & Floating Pill Overlay**:
  - Minimal system tray icon with context menu (Status, Setup, Exit).
  - Floating translucent acrylic/Mica pill near cursor or bottom center indicating states: `Listening` (with live audio meter), `Arbitrating`, and `Delivered`.

### Should-Have (v1.1)
- **[P1] Developer Prompt Rendering (DPR) / Smart Writing Gate**:
  - Fast Groq LLM formatting call bounded by a hard 1.0s timeout with guaranteed local baseline fallback.
- **[P1] Custom User Vocabulary / Dictionary**:
  - Local JSON dictionary for domain-specific acronyms, project names, and developer terminology.
- **[P1] Sound Chimes**:
  - Subtle, low-latency audio feedback tones on recording start, stop, and delivery.

### Won't-Have (v1)
- **[P2] Local Whisper On-Device Model**: No local GGML/whisper.cpp engine in v1 to preserve workstation battery and avoid VRAM thrashing.
- **[P2] Screen / Context Scraping**: No reading of DOM, screen OCR, or active window contents.
- **[P2] Automated Submission**: Voisu will never automatically press `Enter` or submit forms.

## 5. Non-Goals
1. **No Auto-Sending**: Voisu strictly delivers text into the active field and yields control; it never presses Enter or Send.
2. **No Free-Form LLM Re-interpretation**: Voisu does not summarize, rewrite, or chat with the user's speech in v1; every delivered word is strictly source-derived.
3. **No Heavy Background Service**: Voisu is a standalone lightweight user-session application, not a multi-tenant Windows Service.

## 6. Open Questions
1. **UAC / Elevated Window Access**: When the user focuses an Administrator command prompt, standard user-level `SendInput` is blocked by Windows UIPI (User Interface Privilege Isolation). Should Voisu ship with an optional `uiAccess` manifest or request Administrator launch when elevated dictation is required?
2. **Custom Hotkey Picker**: Should the v1 configuration wizard allow arbitrary multi-key shortcuts (e.g. `Ctrl+Alt+Space`) via a graphical shortcut recorder, or stick to robust single modifier triggers in v1?
