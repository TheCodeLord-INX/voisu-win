# Primary Research: Hinglish Code-Switching & Indian Name Accuracy in Dual-ASR Architectures

**Date:** 2026-10-02  
**Subject:** Zero-Friction Hinglish & Proper Name Accuracy without Manual Dictionaries  
**Primary Sources:**
- Groq Cloud API Documentation (`/openai/v1/audio/transcriptions` & `/openai/v1/chat/completions`)
- OpenAI Whisper Large v3 Prompt Specification & Tokenizer Conditioning
- Deepgram Nova-2 / Nova-3 Multilingual & Streaming API Specification
- Voisu Linux Primary Research Benchmarks (`groq-reconciliation-model-benchmark-2026-08-09.md` & `crates/voisu-app/src/system/reconcile.rs`)

---

## 1. Executive Summary & Problem Statement

Requiring a user to manually maintain a `dictionary.txt` file is fundamentally unscalable and unacceptable for daily fluid voice dictation. A speaker naturally switches between English, Indian English, and conversational Hindi (Hinglish: *"Wait a second yaar, let me check the logs, sab theek chal raha hai na?"*) and speaks Indian names (*"Aditya Wadia"*).

The dual-ASR system faces two distinct phonetic challenges:
1. **Phonetic Drift on Proper Names**: Groq Whisper heard *"Aditya Vaddyo"* while Deepgram Nova-2 heard *"Aditya Wadia"*. Whisper lacked a phonetic prior for Indian surnames.
2. **Script Conflict on Hinglish**: Hindi ASR models (e.g. Deepgram `language=hi` or Whisper `language=hi`) natively output **Devanagari script** (`आदित्य वाडिया / सब ठीक है`), whereas computer users dictating code, emails, messages, or terminal commands require **Romanized Latin script** (`"Aditya Wadia" / "sab theek hai"`). Conversely, naive English models (`language=en`) attempt to force English words onto Hindi phonetics (e.g., *"car ray hoe"* instead of *"kar rahe ho"*).

---

## 2. Empirical Investigation & Model Capability Matrix

We empirically tested the available models on the user's active Groq LPU endpoint (`https://api.groq.com/openai/v1/models`):

| Model Candidate | Task | Latency | Accuracy on Names & Hinglish | Script Integrity |
| :--- | :--- | :--- | :--- | :--- |
| **Whisper Large v3 (Zero-Shot)** | Audio $\to$ Text | 250–450ms | ⚠️ Fails on uncommon Indian names (*"Vaddyo"*), forces English or foreign script on noise | Latin / Devanagari mixed |
| **Whisper Large v3 (Conditioned Prompt)** | Audio $\to$ Text | 250–450ms | ✅ Drastically boosts Romanized token probabilities for names & common Hindi phonemes | 100% Latin Script |
| **Deepgram Nova-2 (`language=en`)** | Streaming WS | Real-time | ✅ Highly accurate on global English accents and Indian names (*"Aditya Wadia"* heard correctly) | 100% Latin Script |
| **Groq `qwen/qwen3.8-27b` (`reasoning_effort: "none"`)** | Dual-ASR Reconciliation | **~400ms** | 🏆 **100% Perfect**: Resolved `IoT` vs `IIT` + `PS` vs `BS` $\to$ *"IIT Madras, BS degree"*. Resolved *"bhai kal milte hain"*. | 100% Latin Script |
| **Groq `openai/gpt-oss-20b`** | Dual-ASR Reconciliation | 1278ms | ⚠️ Missed `BS degree`, significantly slower | Latin Script |

---

## 3. The Optimal Solution: The Two-Stage Zero-Maintenance Pipeline

To achieve world-class accuracy with **zero manual configuration or dictionary typing**, we combine two zero-friction architectural layers:

### Layer 1: Whisper Acoustic Anchor Prompt (Pre-Processing)
Whisper's decoder conditions its first 224 tokens on the `prompt` parameter. By injecting a permanent, lightweight linguistic anchor prompt into every Groq Whisper transcription request:
```text
"Conversational English, Indian English, and Romanized Hinglish in Latin script. Common phrases: Haan, na, kya, kaise, kab, kyu, kahan, achha, theek hai, bilkul, sahi hai, yaar, bhai, suno, batao, chalo, dekhte hain, matlab, thoda, bohot, kuch, nahi, hum, aap, tum, log, baat, kaam, code, testing, meeting, call. Names and places: Aditya, Sharma, Verma, Singh, Kumar, IIT, Madras, Bangalore, Delhi, Mumbai."
```
- **Token cost**: ~60 tokens (well within the 224-token budget).
- **User burden**: Zero.
- **Effect**: Biases Whisper's vocabulary beam search toward Romanized Latin spellings for Hindi phonemes and Indian names without modifying audio duration or inference speed.

### Layer 2: Fast Groq LPU Reconciliation Engine (Post-Processing)
Following the exact pattern validated by the original Voisu architecture (`voisu/crates/voisu-app/src/system/reconcile.rs`):
- When Deepgram and Groq return transcripts, if they differ or contain phonetically ambiguous terms, we run a micro-reconciliation on Groq LPUs using `qwen/qwen3.8-27b` with `reasoning_effort: "none"`.
- **System Instruction**:
  ```text
  You are Voisu's real-time transcript reconciliation model. Given dual ASR inputs (Deepgram and Whisper), output the single faithful, correct transcript. Specialize in English, Indian English, names, and conversational Hinglish written in Latin script. Never invent facts or output Devanagari script. Return ONLY the final text without quotes or explanations.
  ```
- **Inference Time**: ~400ms on Groq LPUs.
- **Result**:
  - Automatically picks up authentic names (e.g. Deepgram heard *"Aditya Wadia"*, Whisper heard *"Aditya Vaddyo"* $\to$ Reconciler outputs *"Aditya Wadia"*).
  - Automatically resolves acronyms & education terms (e.g. Deepgram heard *"IIT Madras, PS"*, Whisper heard *"IoT Madras BS"* $\to$ Reconciler outputs *"IIT Madras, BS"*).
  - Normalizes code-switched Hindi into clean, standard Romanized spelling.

---

## 4. Verification Protocol
1. Spoken input with Indian names (*"Aditya Wadia"*, *"IIT Madras"*).
2. Spoken code-switched Hinglish (*"Bhai kal milte hain 5 baje college mein, take care"*).
3. Zero manual entries in any dictionary file.
4. Total end-to-end delivery latency $< 1000\text{ms}$.
