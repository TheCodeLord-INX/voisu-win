# Project Tracker — Voisu for Windows (`voisu-win`)

## 1. Current Sprint: Sprint 1 — Foundation & Core Audio Pipeline
- **Sprint Goal**: Scaffold Cargo project, establish WASAPI low-latency audio capture, configure API credentials, and connect initial Groq / Deepgram dual streaming clients.
- **Sprint Duration**: Week 1 (Days 1–7)
- **Sprint Status**: In Progress

---

## 2. Task Table

| ID | Title | Status | Owner | Phase | Priority |
|---|---|---|---|---|---|
| **T-001** | Initialize Rust workspace, `Cargo.toml`, and module hierarchy | Done | Core Systems Lead | Phase 1 | P0 |
| **T-002** | Implement `AppConfig` storage, encryption, and CLI setup wizard | Done | Core Systems Lead | Phase 1 | P0 |
| **T-003** | Build WASAPI audio capture engine with `rubato` 48kHz→16kHz resampling + RMS meter | Done | Audio Lead | Phase 2 | P0 |
| **T-004** | Implement Deepgram Nova-2 streaming WebSocket client | Done | Provider Lead | Phase 2 | P0 |
| **T-005** | Implement Groq Whisper Large v3 REST LPU client (word timestamps + segment confidence) | Done | Provider Lead | Phase 2 | P0 |
| **T-006** | Build Dual-Provider Race Coordinator with bounded deadlines + Groq RPM/RPD tracking | Done | Provider Lead | Phase 2 | P0 |
| **T-010** | Implement `WH_KEYBOARD_LL` keyboard hook on dedicated message-pump thread + liveness watchdog | Done | Windows Native Lead | **Phase 2** | P0 |
| **T-007** | Implement Asymmetric Slice B4 Levenshtein Arbitration (Deepgram word + Groq segment proxy) | Todo | Arbitration Lead | Phase 3 | P0 |
| **T-008** | Implement Deterministic Spoken Punctuation & Formatting Engine | Todo | Core Systems Lead | Phase 3 | P0 |
| **T-009** | Build Smart Clipboard Injector with `AddClipboardFormatListener` restore + Win+V suppression | Todo | Windows Native Lead | Phase 3 | P0 |
| **T-011** | Build Win32 Floating Pill Overlay with DWM Acrylic (`DWMSBT_TRANSIENTWINDOW`) + Win10 fallback | Todo | UI Lead | Phase 4 | P0 |
| **T-012** | Implement Windows System Tray Icon and Context Menu | Todo | UI Lead | Phase 4 | P1 |
| **T-014** | Implement Graceful Shutdown Handler (`SetConsoleCtrlHandler` + Caps Lock restore) | Todo | Core Systems Lead | Phase 4 | P0 |
| **T-013** | End-to-End Latency & Accuracy Benchmark Suite | Todo | QA Lead | Phase 4 | P0 |

---

## 3. Milestone Map

| Milestone | Target Date | Description | Status | Complete |
|---|---|---|---|---|
| **M1: Foundation & Scaffold** | Day 2 | Workspace setup, config storage, doctor diagnostics | Done | 100% |
| **M2: Audio, Hook & Dual Cloud** | Day 5 | WASAPI capture + resampling + keyboard hook + Deepgram & Groq parallel race | Done | 100% |
| **M3: Arbitration & Injection** | Day 7 | Asymmetric confidence arbitration, formatting & clipboard paste with listener-based restore | In Progress | 0% |
| **M4: Overlay, Tray & Polish** | Day 9 | DWM acrylic floating pill, system tray, graceful shutdown, benchmarks | Todo | 0% |

---

## 4. Blockers & Dependencies
- **Groq API Confidence Limitation** (Resolved in docs): Groq Whisper API does not provide per-word confidence scores. Arbitration design amended to use asymmetric segment-level proxy. No code blocker.
- **WASAPI 16kHz** (Resolved in docs): Most Windows hardware does not support 16kHz natively. Hardware probe in `voisu-win doctor` confirmed default mic is 48,000 Hz. Resampling via `rubato` configured. No code blocker.
- Both Groq and Deepgram developer APIs are fully accessible from this workstation.

---

## 5. Changelog
- **2026-10-02**: Phase 2 (Audio Capture, Keyboard Hook & Dual Cloud Race) completed. Implemented: (1) `core::audio` WASAPI capture stream with `rubato` sinc resampling (48kHz$\rightarrow$16kHz), multi-channel downmixing, RMS level computation, and in-memory WAV encoding; (2) `core::hotkey` dedicated Win32 message-pump hook thread with CapsLock suppression, Hybrid tap/hold detection, and `VK_F24` synthetic watchdog; (3) `providers::deepgram` live WebSocket streaming client with word-level confidence; (4) `providers::groq` Whisper Large v3 REST LPU client with word timestamps, segment-level confidence proxy, and rate limit tracking; (5) `providers::coordinator` DualProviderCoordinator orchestrating 800ms bounded race with single-provider fallback; (6) `src/main.rs` live event loop wiring hotkey, audio, and provider race. 18 unit tests passing 100%, `cargo clippy` and `cargo fmt` clean. Milestone M2 100% complete.
- **2026-10-02**: Phase 1 (Foundation & Project Harness) completed. Scaffolding complete: `Cargo.toml`, full module tree (`core`, `providers`, `delivery`, `ui`), `AppConfig` loader & serializer with validation, `voisu-win doctor` diagnostics verifying WASAPI hardware + network probes, `voisu-win setup` interactive wizard, and `voisu-win run` entry point. 8 unit tests passing 100%, `cargo clippy` and `cargo fmt` clean. Milestone M1 100% complete.
- **2026-10-02**: Plan Audit v1.1 complete. Applied 10 amendments from batch-grill-me + research-ops audit: (1) Groq asymmetric arbitration, (2) `rubato` resampling, (3) hook liveness watchdog, (4) `AddClipboardFormatListener` restore, (5) Groq rate limit tracking, (6) DWM acrylic API, (7) keyboard hook moved to Phase 2, (8) `ArbitrationMode` enum, (9) graceful shutdown flow, (10) config fields added. All 9 foundation docs updated.
- **2026-10-01**: Project Foundation bootstrap complete. Generated all 9 core foundational documents (`PRD.md`, `schema.md`, `Architecture.md`, `architecture-essentials.md`, `appflow.md`, `design.md`, `rules.md`, `implementation_plan.md`, `tracker.md`). Sprint 1 initialized.
