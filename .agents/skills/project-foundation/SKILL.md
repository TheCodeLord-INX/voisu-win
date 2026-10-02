---
name: project-foundation
description: >
  Bootstrap a new coding project with a research-backed foundation.
  Use when the user wants to start a new project, app, system, or feature from scratch,
  says "let's build X", "I want to create X", "new project", "plan a new system",
  or asks for a PRD, architecture plan, implementation plan, or project structure.
  Also fires when another skill needs a project foundation before proceeding.
---

# Project Foundation

**Bootstrap** is the leading word. Every phase of this skill serves one goal: produce a fully grounded, decision-complete bootstrap before a single line of production code is written. A bootstrap is done only when all nine foundation files exist, each satisfying its completion criterion in [FILE-SPECS.md](./FILE-SPECS.md).

Read [FILE-SPECS.md](./FILE-SPECS.md) before producing any foundation file.

---

## Phase 0 — Intercept

Confirm this is a new project, not a feature on an existing one. Scan the workspace:

```
list_dir / grep_search for existing PRD.md, Architecture.md, schema.md
```

If foundation files already exist, read them and surface gaps instead of overwriting. **Done when**: you know whether you are bootstrapping from scratch or gap-filling.

---

## Phase 1 — Regressive Analysis & Grilling

Run a **regressive analysis**: trace every stated requirement back to its root motivation, then challenge each assumption.

1. State back what you heard in one sentence.
2. Invoke the **grilling** discipline (one question at a time, sharpest first, recommend an answer, wait for reply). Walk the full decision tree: user problem → target users → core flows → success metrics → constraints → non-goals.
3. Simultaneously, spin up a background **research** agent to investigate:
   - Problem domain: competitors, existing solutions, market context.
   - Prior art on Reddit, Hacker News, Quora, research papers — via web search.
   - Industry-standard architectural patterns for this class of system.
   - Known failure modes and cautionary tales in this space.
4. Merge research findings into the conversation once they arrive.

**Done when**: you can state the user's core problem in one precise sentence, name the top three risks, and list the non-goals. The user has confirmed this summary.

---

## Phase 2 — Produce Foundation Files

Write the nine files in dependency order. Each file has a done condition in [FILE-SPECS.md](./FILE-SPECS.md); do not mark a file done until its criterion is met.

**Order** (later files depend on earlier ones):

1. `PRD.md`
2. `schema.md`
3. `Architecture.md`
4. `architecture-essentials.md`
5. `appflow.md`
6. `design.md`
7. `rules.md`
8. `implementation_plan.md`
9. `tracker.md`

Write each file completely before moving to the next. Cross-link: every file referencing a decision must cite which file owns it.

**Placement**: write all nine files to the project root, or to `docs/` if that convention already exists.

**Done when**: all nine files exist on disk and each passes its completion criterion.

---

## Phase 3 — Review & Harden

1. Re-read `architecture-essentials.md` and `rules.md` together. Every rule must be traceable to a decision in `architecture-essentials.md` or `PRD.md`. Delete rules with no traceable origin.
2. Re-read `implementation_plan.md` against `schema.md`. Every entity or table referenced in the plan must exist in the schema. Surface gaps.
3. Update `tracker.md` with the first sprint of tasks derived from `implementation_plan.md`.
4. Deliver a one-paragraph summary: what was decided, the three biggest open risks, and the recommended first action.

**Done when**: cross-references are coherent, tracker has tasks, and summary is delivered.

---

## Guardrails

- No production code during bootstrap. Bootstrap ends when foundation files are complete and the user says "let us build."
- Never skip grilling because the project seems simple. Regressive analysis always runs.
- Research must cite primary or near-primary sources (official docs, academic papers, first-hand posts).
- Every architectural decision in `architecture-essentials.md` must record the alternatives considered and the reason for rejection.
- `tracker.md` must have at least one milestone and three tasks before bootstrap is declared done.
