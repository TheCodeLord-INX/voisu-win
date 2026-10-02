# Implementation Plan — Voisu for Windows (`voisu-win`)

## 1. Phases Overview

```
Phase 1: Foundation & Infrastructure ──► Phase 2: Core Speech & Dual Cloud ──► Phase 3: Arbitration & Native Delivery ──► Phase 4: Polish & UI Overlay
```

---

## 2. Phase Breakdown

### Phase 1: Foundation & Project Harness
- **Goal Statement**: Establish the Rust workspace, configure Windows dependencies, configuration storage, and test harnesses.
- **Deliverables**:
  - `Cargo.toml` with pinned dependencies (`cpal`, `tokio`, `windows-sys`, `reqwest`, `serde`).
  - `AppConfig` loader & serializer (`%APPDATA%\voisu\config.json`).
  - CLI commands: `voisu-win setup`, `voisu-win run`, `voisu-win doctor`.
- **Success Criterion**: `cargo check` and `cargo test` pass cleanly; config round-trips from disk.

#### Milestones & Tasks:
- **[M1.1] Project Scaffolding** (Effort: S | Owner: Core Systems Lead | Deps: None)
  - Create Cargo package structure with `src/bin/main.rs` and core modules.
- **[M1.2] Configuration & Credential Store** (Effort: M | Owner: Core Systems Lead | Deps: M1.1)
  - Implement `AppConfig` entity (per `schema.md §1.5`) and secure key validation logic.
- **[M1.3] System Doctor Diagnostics** (Effort: S | Owner: QA Lead | Deps: M1.2)
  - Implement `voisu-win doctor` verifying microphone input, network reachability to Deepgram/Groq, and keyboard hook readiness.

---

### Phase 2: Audio Capture & Dual-Cloud Providers
- **Goal Statement**: Implement low-latency WASAPI 16kHz audio capture and parallel Deepgram / Groq cloud client connections.
- **Deliverables**:
  - WASAPI capture thread emitting `AudioFrame` chunks with RMS audio level calculation.
  - Deepgram WebSocket streaming client (`providers::deepgram`).
  - Groq Whisper LPU REST client (`providers::groq`).
  - In-memory WAV encoder (`hound`).
- **Success Criterion**: 5 seconds of spoken audio captured and transcribed concurrently by both providers in $< 350\text{ ms}$ total latency.

#### Milestones & Tasks:
- **[M2.1] WASAPI Audio Engine with Resampling** (Effort: M | Owner: Audio Lead | Deps: M1.1)
  - Configure `cpal` default input stream at **native hardware sample rate** (typically 48kHz).
  - Implement real-time sinc resampling to 16kHz via `rubato` crate on a dedicated thread.
  - Implement ring buffer and real-time RMS amplitude calculation on resampled output.
- **[M2.2] Deepgram WebSocket Client** (Effort: M | Owner: Provider Lead | Deps: M2.1)
  - Implement persistent connection with binary audio frame streaming.
  - Parse `SourceTranscript` and `WordToken` confidence array.
- **[M2.3] Groq LPU Whisper Client** (Effort: M | Owner: Provider Lead | Deps: M2.1)
  - Package in-memory WAV buffer and post to Groq endpoint with `response_format: verbose_json`.
  - Parse word confidence and timestamps.
- **[M2.4] Dual-Provider Coordinator** (Effort: M | Owner: Provider Lead | Deps: M2.2, M2.3)
  - Orchestrate simultaneous race with 800ms bounded deadline and single-provider fallback.
  - Track Groq RPM/RPD rate limit consumption locally; degrade to Deepgram-only when approaching limits.
- **[M2.5] Windows Low-Level Keyboard Hook** (Effort: M | Owner: Windows Native Lead | Deps: M1.1)
  - Implement `WH_KEYBOARD_LL` hook on dedicated Win32 message-pump thread with Caps Lock suppression and hybrid duration tracking.
  - Implement hook liveness watchdog (synthetic `VK_F24` injection + receipt verification).
  - **Rationale for Phase 2**: The hook is the user's primary trigger for recording start/stop; audio capture cannot be tested end-to-end without it.

---

### Phase 3: Arbitration, Local Formatting & Windows Delivery
- **Goal Statement**: Build the Slice B4 Levenshtein alignment and confidence arbitration engine, deterministic punctuation formatter, and smart clipboard injector.
- **Deliverables**:
  - `core::arbitration`: Positional Levenshtein alignment with word confidence arbitration and fail-closed meaning guards.
  - `core::formatting`: Punctuation command substitution and number normalization.
  - `delivery::clipboard`: Instant clipboard paste and auto-restoration with fallback to `SendInput`.
- **Success Criterion**: Complex technical sentences transcribed, arbitrated, formatted, and injected into VS Code or Notepad in $< 50\text{ ms}$ post-transcription without losing clipboard history.

#### Milestones & Tasks:
- **[M3.1] Asymmetric Confidence Arbitration Engine** (Effort: L | Owner: Arbitration Lead | Deps: M2.4)
  - Implement word-level Levenshtein alignment between Deepgram (per-word confidence) and Groq (segment-level `avg_logprob` proxy).
  - Implement asymmetric confidence gap evaluation: flip from Deepgram only when Deepgram word confidence $< 0.50$ AND Groq segment proxy $\ge 0.75$.
  - Implement fail-closed guards for negations, numbers, and polarity tokens.
  - Report `ArbitrationMode` (DualProvider / SingleDeepgram / SingleGroq).
- **[M3.2] Deterministic Local Formatter** (Effort: M | Owner: Core Systems Lead | Deps: M3.1)
  - Implement fast regex substitution for spoken punctuation (`"comma"`, `"period"`, `"question mark"`, `"new line"`).
- **[M3.3] Windows Smart Clipboard Injector** (Effort: M | Owner: Windows Native Lead | Deps: M3.2)
  - Implement safe Win32 clipboard backup, `Ctrl+V` synthesis, and clipboard consumption monitoring via `AddClipboardFormatListener` with configurable restore timeout (default 200ms).
  - Implement `ExcludeClipboardContentFromMonitorProcessing` to suppress Win+V history pollution.

---

### Phase 4: UI Overlay, System Tray & Polish
- **Goal Statement**: Create the lightweight floating acrylic pill overlay, system tray icon, graceful shutdown, and final test suite.
- **Deliverables**:
  - Transparent Win32 layered pill overlay with live audio waveform and DWM acrylic blur.
  - System tray icon and menu.
  - Graceful shutdown handler (`SetConsoleCtrlHandler`).
- **Success Criterion**: End-to-end user workflow: hold `Caps Lock` $\rightarrow$ speak $\rightarrow$ release $\rightarrow$ text immediately appears in focused app with smooth visual overlay feedback.

#### Milestones & Tasks:
- **[M4.1] Native Floating Pill Overlay** (Effort: L | Owner: UI Lead | Deps: M2.1)
  - Create `WS_EX_LAYERED` transparent Win32 window with `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_TRANSIENTWINDOW)` for acrylic blur.
  - Implement GDI/Direct2D rendering for live RMS waveform and state badges.
  - Windows 10 fallback: simple semi-transparent window without blur.
- **[M4.2] System Tray Integration** (Effort: S | Owner: UI Lead | Deps: M4.1)
  - Implement Windows notification area tray icon and context menu.
- **[M4.3] Graceful Shutdown Handler** (Effort: S | Owner: Core Systems Lead | Deps: M2.5, M4.1)
  - Implement `SetConsoleCtrlHandler` + tray Exit orderly teardown sequence.
  - Ensure Caps Lock state is restored on shutdown.
- **[M4.4] End-to-End Hardening & Latency Benchmarks** (Effort: M | Owner: QA Lead | Deps: All)
  - Benchmark p90 latency, verify memory stability under 1,000 repeated dictation cycles.

---

## 3. Risk Register

| Risk ID | Risk Description | Likelihood | Impact | Mitigation Strategy |
|---|---|---|---|---|
| **R1** | Target window clipboard lock or focus loss during injection | Low | High | Pre-test clipboard open with exponential retry (5 attempts); automatic fallback to direct `SendInput` Unicode character synthesis. |
| **R2** | Cloud network jitter or temporary provider outage | Medium | Medium | Dual-provider race ensures single-provider continuity; hard 800ms timeout prevents hanging; fail-closed local fallback. |
| **R3** | Low-level keyboard hook delay / OS hook removal | Low | High | Hook callback does zero heavy processing; immediately posts event to Tokio channel and returns via `CallNextHookEx`. Liveness watchdog detects silent unhooking and re-installs. |
| **R4** | Elevated target app (UIPI privilege boundary) | Medium | Medium | Detect injection failure; notify user via tray tooltip to run Voisu as Administrator if elevated dictation is required. |
| **R5** | Audio buffer underruns during high CPU load | Low | High | WASAPI capture runs in dedicated high-priority native OS thread with lockless ring-buffer handoff to Tokio runtime. |
| **R6** | Windows silently removes keyboard hook (LowLevelHooksTimeout) | Medium | Critical | Hook runs on dedicated message-pump thread (never Tokio). Liveness watchdog injects synthetic `VK_F24` and verifies receipt. Auto-reinstall on detection. |
| **R7** | Groq free tier rate limits throttle power users (20 RPM, 2K RPD) | High | Medium | Track RPM/RPD consumption locally; gracefully degrade to Deepgram-only with tray notification when approaching limits. Consider utterance batching for short (<3s) dictations. |
| **R8** | 16kHz audio capture fails on Windows hardware (WASAPI defaults to 48kHz) | High | Critical | Capture at native device rate; resample to 16kHz via `rubato` sinc resampler. Never hard-code 16kHz in `cpal` stream config. |

---

## 4. Open Decisions
- **D-01**: Default hotkey set to `Caps Lock` with lock-toggle suppression, with `Right Alt` and `F8` as immediate configuration wizard alternatives.
