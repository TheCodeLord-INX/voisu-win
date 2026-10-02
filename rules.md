# Project Rules & Invariants — Voisu for Windows (`voisu-win`)

## 1. Coding Standards
- **Language & Edition**: Rust 2024 edition, strict `clippy` pedantic conformance. *(Source: PRD §4)*
- **Formatting**: Format all files with `cargo fmt`. Lines hard-wrapped at 100 characters. *(Source: PRD §4)*
- **Async Runtime**: Standardize on `tokio` (multi-threaded runtime). Audio capture tasks must run on dedicated OS threads with high priority. The keyboard hook must run on a **dedicated Win32 message-pump thread** (calling `GetMessage`/`DispatchMessage`) — never on the Tokio runtime — to prevent Windows' `LowLevelHooksTimeout` (~1000ms) from silently removing the hook. *(Source: Architecture §5, Audit Finding #3)*
- **Zero Raw Pointers / Unsafe Discipline**: Wrap all Win32 API calls (`SetWindowsHookExW`, `SendInput`, `GetForegroundWindow`) in safe, strongly typed Rust abstraction wrappers. Zero unmanaged raw memory leaks. *(Source: architecture-essentials.md ADR-002)*

---

## 2. Auth & Secret Management
- **Credential Storage**: API keys (`deepgram_api_key`, `groq_api_key`) must never be hardcoded, logged, or printed in error dumps. Store keys in the Windows Credential Manager (`wincred`) or in a user-restricted local file (`%APPDATA%\voisu\config.json` with DACL restricted to the current user SID). *(Source: PRD §4, schema.md §2)*
- **Transmission Security**: All cloud communication must strictly use TLS 1.3 encrypted connections (`wss://` for Deepgram, `https://` for Groq). *(Source: Architecture §3)*

---

## 3. Error Handling Contract
- **Fail-Closed Principle**: A provider timeout, parsing error, or network drop must NEVER crash the daemon or hang the user's desktop. *(Source: architecture-essentials.md ADR-001)*
- **Seamless Provider Fallback**: If Deepgram stalls or fails, Groq is delivered; if Groq fails or times out (800ms), Deepgram is delivered. If both fail, the session aborts cleanly without altering clipboard or injecting garbage text. *(Source: Architecture §3)*
- **Deterministic Local Formatting**: Local baseline formatting (`"comma"` $\rightarrow$ `","`, number conversions) must be deterministic, run in $< 2\text{ ms}$, and never panic. *(Source: PRD §4, architecture-essentials.md ADR-004)*
- **Groq Rate Limit Awareness**: The Dual-Provider Coordinator must track Groq RPM/RPD consumption locally and gracefully degrade to Deepgram-only mode when approaching the free tier limits (20 RPM, 2,000 RPD). *(Source: Audit Finding #5)*
- **Hook Liveness Watchdog**: The keyboard hook module must include a periodic liveness check (inject synthetic `VK_F24`, verify receipt) to detect and recover from Windows' silent hook removal. *(Source: Audit Finding #3)*

---

## 4. Testing Requirements
- **Mandatory Test Suite**: Run `cargo test` prior to every milestone stage. All tests must pass 100%. *(Source: user_global rule #5)*
- **Unit Tests**:
  - Positional Levenshtein alignment & Slice B4 confidence arbitration unit tests with 100% path coverage.
  - Spoken punctuation parser tests covering all English punctuation commands.
- **Integration Tests**:
  - Mocked WebSocket Deepgram stream + mocked Groq REST response races verifying bounded deadline cutoffs and fallback logic.
  - Safe clipboard backup and restoration round-trip verification.

---

## 5. Branching & Trunk-Based Commit Rules
- **Micro-Commits Only**: Never commit massive multi-feature blocks. Commits must be small, self-contained atomic units with clear conventional commit messages (`feat: ...`, `fix: ...`, `refactor: ...`). *(Source: user_global rule #2)*
- **Zero Tolerance for Conflict Markers**: Before any file edit or stage, the code must be scanned for `<<<<<<<`, `=======`, or `>>>>>>>`. Any conflict markers halt work immediately. *(Source: user_global rule #1)*

---

## 6. Dependency Policy
- **Allowed Core Dependencies**:
  - Audio: `cpal`, `rubato` (real-time sinc resampling 48kHz→16kHz), `hound` (WAV encoding).
  - Async & Networking: `tokio`, `tokio-tungstenite`, `reqwest` (with rustls).
  - Serialization: `serde`, `serde_json`.
  - Windows Native: `windows-sys` (minimal Win32 surface).
  - Regex & Text: `regex`.
- **Banned Patterns**:
  - No Electron or heavy embedded web engines. *(Source: architecture-essentials.md ADR-005)*
  - No unmanaged background worker threads that outlive the session without cancellation handles.
