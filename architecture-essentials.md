# Critical Architectural Decisions — Voisu for Windows (`voisu-win`)

## ADR-001: Adopt Dual-Cloud Concurrent Streaming Over Local Speech Models

- **Status**: Accepted
- **Context**: High-accuracy speech recognition (Whisper Large v3) requires substantial computational resources. On Windows workstations, users frequently run IDEs, local development servers, Docker, compilers, or games that heavily compete for CPU and GPU VRAM.
- **Alternatives Considered**:
  1. *Local Whisper via `whisper.cpp` / `whisper-rs`*: Rejected because running Large v3 locally takes $1.5\text{–}3.5\text{ s}$ per utterance on average GPUs, consumes $>3\text{ GB}$ VRAM, drains laptop batteries, and triggers thermal throttling. Smaller local models (Tiny/Base) yield unacceptable error rates on programming terminology.
  2. *Single Cloud Provider (Groq-only or Deepgram-only)*: Rejected because relying solely on one provider creates a single point of failure (rate limits or regional network blips) and misses the speed benefit of streaming (Deepgram) coupled with the sheer accuracy of Groq Whisper Large v3.
- **Decision**: Stream audio in real-time to Deepgram Nova-2 over WebSocket during speech, and post the buffered audio simultaneously to Groq LPU Whisper Large v3 upon key release. Fall back automatically if only one provider is configured.
- **Consequences**:
  - *Trade-offs*: Requires network connectivity and cloud API keys (though Groq has a generous free tier and Deepgram provides starter credits).
  - *Risks*: Network latency variations, handled cleanly by a hard bounded deadline (<800ms) with single-provider fallback.

---

## ADR-002: Use Low-Level Windows Keyboard Hook (`WH_KEYBOARD_LL`) with Hybrid Trigger

- **Status**: Accepted
- **Context**: Voisu needs to intercept hotkeys globally across all Windows applications without interfering with normal typing, while supporting both Push-to-Talk (Hold) and Toggle (Tap).
- **Alternatives Considered**:
  1. *Win32 `RegisterHotKey` API*: Rejected because `RegisterHotKey` only fires a single event on key down; it does not reliably notify on key release (crucial for Hold-to-Talk), and it fails to suppress the native Caps Lock LED/state toggle.
  2. *Polling `GetAsyncKeyState` in a Background Loop*: Rejected because polling introduces either latency (if sleeping 50ms) or high CPU usage (if sleeping 1ms), and can drop quick key taps during heavy system load.
- **Decision**: Implement a low-level keyboard hook using `SetWindowsHookExW(WH_KEYBOARD_LL)`. Track press vs. release duration to distinguish between Hold-to-Talk (>300ms hold) and Tap-to-Toggle (<300ms tap). For `Caps Lock`, consume the event to prevent toggling the system caps state.
- **Consequences**:
  - *Trade-offs*: Low-level hooks must process messages quickly to avoid Windows hook timeouts (~1000ms threshold).
  - *Mitigation*: The hook procedure only dispatches non-blocking channel messages to Tokio worker threads and returns immediately (`CallNextHookEx`).

---

## ADR-003: Deliver Text via Smart Clipboard Paste with Atomic Restoration

- **Status**: Accepted
- **Context**: Once transcription completes, text must be injected into the target application exactly where the cursor is positioned, preserving code formatting, indentation, and unicode symbols.
- **Alternatives Considered**:
  1. *Direct Keystroke Emulation (`SendInput` Unicode)*: Rejected as the primary mechanism because typing long sentences character-by-character is noticeably sluggish (takes 300–800ms for a paragraph) and frequently triggers aggressive IDE auto-pairing (e.g. VS Code automatically inserting matching brackets or quotes, mangling the transcript).
  2. *Windows UI Automation (UIA) SetValue*: Rejected because many applications (terminals, web browsers with canvas, game chats, Electron apps) do not implement or expose UIA text pattern endpoints.
- **Decision**: Use Smart Clipboard Paste: backup existing clipboard content (`CF_UNICODETEXT`), set the transcript text, synthesize `Ctrl+V` key events, and immediately restore the previous clipboard content after 50ms. Provide an automatic fallback to direct `SendInput` if the clipboard cannot be opened.
- **Consequences**:
  - *Trade-offs*: Momentarily touches the clipboard.
  - *Mitigation*: Pre-allocates memory and restores original clipboard data immediately so clipboard managers and user copy-paste buffers remain undisturbed.

---

## ADR-004: Asymmetric Divergence-Point Confidence Arbitration (Slice B4) Over LLM Reconciliation

- **Status**: Accepted (Amended 2026-10-02 — Groq confidence limitation discovered)
- **Context**: When Deepgram and Groq transcripts disagree on words, we must reconcile them without introducing latency or AI hallucinations. **Critical constraint discovered during audit**: Groq's OpenAI-compatible Whisper API does **not** return per-word confidence scores. Only **segment-level** metrics (`avg_logprob`, `no_speech_prob`) are available. Deepgram provides full per-word confidence.
- **Alternatives Considered**:
  1. *LLM-Based Reconciliation (e.g. Llama-3 prompt "merge these two transcripts")*: Rejected because LLM calls add 200–500ms of latency, cost tokens, and occasionally hallucinate or delete intended words.
  2. *Simple Rule-Based Preference (Always pick Groq or always pick Deepgram)*: Rejected because Deepgram excels at technical continuous flow, whereas Whisper Large v3 excels at sentence boundaries and quiet speech; discarding one wastes 50% of our accuracy signal.
  3. *Symmetric Confidence Comparison (original design)*: **Rejected** because Groq does not expose per-word confidence, making symmetric word-level comparison impossible.
  4. *Self-host Whisper for log-probability extraction*: Rejected because it kills the "zero local model" design goal and adds massive infrastructure complexity.
- **Decision**: Implement **Hybrid Asymmetric + Segment Proxy** positional Levenshtein word alignment with confidence arbitration:
  - Deepgram is the **incumbent backbone** (it provides real per-word confidence scores).
  - Groq words inherit their parent segment's `avg_logprob` converted to a `[0.0, 1.0]` confidence proxy via: `proxy_confidence = min(1.0, exp(avg_logprob))`.
  - If words diverge, inspect Deepgram's per-word confidence vs Groq's segment-level proxy confidence.
  - Only flip from Deepgram (incumbent) to Groq (challenger) if Deepgram word confidence $< 0.50$ AND Groq segment proxy confidence $\ge 0.75$.
  - Enforce fail-closed meaning guards: never flip negations (`not`, `never`, `no`), numbers, or question words.
  - Enforce mathematical source derivation: every delivered token must have been heard by at least one provider.
  - Track `ArbitrationMode` (DualProvider / SingleDeepgram / SingleGroq) in the output.
- **Consequences**:
  - *Trade-offs*: Groq's confidence signal is coarser (segment-level, not word-level), so the arbitration is inherently more conservative—it will flip fewer words from Deepgram to Groq compared to the hypothetical symmetric case. This is acceptable because it biases toward safety.
  - *Benefits*: Executes in $< 2\text{ ms}$, costs zero extra tokens, has a 0.0% hallucination rate, and honestly reflects the available data.

---

## ADR-005: Lightweight Native Win32 Layered Window for Floating Pill

- **Status**: Accepted
- **Context**: The user requires instant visual feedback indicating when Voisu is recording (with live audio meter), processing, or done, without stealing keyboard focus or lagging the system.
- **Alternatives Considered**:
  1. *Electron / Web-Based Overlay*: Rejected because Electron requires $>150\text{ MB}$ RAM, has a 1-second cold start, and frequently steals focus during window initialization.
  2. *DirectX Fullscreen Overlay*: Rejected because it is complex, prone to GPU driver conflicts, and unnecessary for a simple status indicator.
- **Decision**: Use a native Win32 Layered Window with `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TOPMOST`.
- **Consequences**:
  - *Trade-offs*: Requires Win32 GDI/Direct2D code.
  - *Benefits*: Consumes $< 2\text{ MB}$ RAM, never steals input focus from active IDEs or terminals, and renders at 60fps with zero latency.
