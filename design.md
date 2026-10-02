# UI/UX Design System — Voisu for Windows (`voisu-win`)

## 1. Design Philosophy
Voisu for Windows is designed to feel completely weightless, surgical, and telepathic. It never demands visual attention when you are focused on thinking and coding, yet provides instant, elegant confirmation the millisecond voice input is engaged. Using native Windows acrylic blur, subtle rounded geometry, and fluid micro-animations, Voisu integrates into the Windows desktop as if it were a built-in OS capability—zero latency, zero clutter, and zero distraction.

---

## 2. Color Palette & Theming

Voisu renders with high-contrast, translucent dark acrylic styling by default, matching Windows 11 Fluent Design principles.

| Token | Dark Mode (Default) | Light Mode | Purpose |
|---|---|---|---|
| `--bg-pill` | `rgba(18, 20, 24, 0.82)` | `rgba(255, 255, 255, 0.88)` | Background of floating acrylic pill |
| `--border-pill` | `rgba(255, 255, 255, 0.12)` | `rgba(0, 0, 0, 0.08)` | 1px subtle glowing border |
| `--accent-listening` | `#FF3B30` (Vibrant Coral Red) | `#E0241A` | Pulsing recording indicator |
| `--accent-arbitrating` | `#F5A623` (Warm Amber) | `#D97706` | Processing & dual-cloud arbitration state |
| `--accent-delivered` | `#34C759` (Emerald Green) | `#16A34A` | Success checkmark & delivery confirmation |
| `--fg-primary` | `#F8FAFC` | `#0F172A` | Primary status label & transcript preview |
| `--fg-muted` | `#94A3B8` | `#64748B` | Secondary hints, hotkey reminder |
| `--meter-bar` | `#38BDF8` (Cyan Neon) | `#0284C7` | Live audio waveform meter bars |

---

## 3. Typography
- **Primary Font Family**: `Segoe UI Variable Text`, `Segoe UI`, system-ui, sans-serif.
- **Monospace Family** (for hotkeys & technical stats): `Cascadia Code`, `Consolas`, monospace.
- **Scale**:
  - Pill Status Label: `12px` / Regular (`400`) & Semibold (`600`)
  - Subtext / Hint: `10px` / Medium (`500`)
  - Audio Level Meter: 4 dynamic vertical bars ($12\text{px} \times 3\text{px}$) with rounded caps.

---

## 4. Component Inventory

### 4.1 The Floating Pill (`ui::overlay`)
- **Dimensions**: $160\text{px} \text{ width} \times 38\text{px} \text{ height}$, `border-radius: 19px` (capsule).
- **Positioning**: Screen bottom-center ($80\text{px}$ from bottom edge) or anchored $24\text{px}$ below active text cursor.
- **Attributes**: `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOPMOST`. Never steals keyboard or mouse focus; clicks pass straight through to the underlying app.
- **Acrylic Blur**: Applied via `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_TRANSIENTWINDOW)` on Windows 11. On Windows 10, falls back to a simple semi-transparent dark window without blur (graceful degradation).
- **Internal Elements**:
  1. *State Indicator Dot*: $8\text{px}$ circle with radial glow animation.
  2. *Live Audio Waveform*: 4 micro-bars whose heights scale smoothly with audio RMS input ($2\text{px}$ to $14\text{px}$).
  3. *Status Text*: `"Listening..."` $\rightarrow$ `"Arbitrating..."` $\rightarrow$ `"Delivered"`.

### 4.2 System Tray Component (`ui::tray`)
- High-DPI Windows notification area icon (microphone glyph with state colors).
- Context Menu:
  - `Voisu for Windows (v1.0)` [Header]
  - `Status: Ready (Deepgram + Groq)`
  - `---`
  - `Preferences & Hotkey...`
  - `View Transcript History`
  - `Check for Updates`
  - `---`
  - `Exit Voisu`

### 4.3 Setup & Configuration Wizard (`cli::wizard` / Dialog)
- Clean, accessible terminal/GUI wizard with field validation:
  - Deepgram API Token input with real-time ping check.
  - Groq API Token input with model availability verification.
  - Radio button hotkey selector (`Caps Lock` [Recommended], `Right Alt`, `F8`).
  - Toggle switch for Sound Cues (on/off).

---

## 5. Interaction Patterns & Micro-Animations

```
State: IDLE (Pill Hidden)
   │
   ▼ Hotkey Down (0ms)
[Fade In + Scale 0.95 -> 1.0 (120ms ease-out)]
   ├── State: RECORDING
   │   ├── Red Dot: Subtle breathe pulse (1.2s loop)
   │   └── Waveform: 60fps RMS height interpolation
   │
   ▼ Hotkey Up / Toggle Stop
[State Transition (60ms)]
   ├── State: ARBITRATING
   │   ├── Amber Dot: Rapid spinner / pulse
   │   └── Text: "Arbitrating..."
   │
   ▼ Delivery Completed (<250ms)
[State Transition (80ms)]
   ├── State: DELIVERED
   │   ├── Green Checkmark Glyph
   │   └── Text: "Delivered"
   │
   ▼ Hold for 600ms
[Fade Out + Scale 1.0 -> 0.95 (150ms ease-in)]
   └── Pill Hidden -> IDLE
```

---

## 6. Accessibility Baseline
- **Target Standard**: WCAG 2.2 Level AA compliance.
- **Contrast Ratios**: Status labels (`#F8FAFC` on `rgba(18, 20, 24, 0.82)`) exceed $12:1$ contrast (well above the $4.5:1$ requirement).
- **Non-Visual Feedback**: Optional auditory earcons (high-frequency soft click on start, satisfying double-tone on delivery) ensure complete usability for screen-reader users or users looking away from the screen.
