# Workspace Rules & Agent Instructions

## Mandatory Project Bootstrap Guardrail
Whenever the user asks to "build", "create", "start", "bootstrap", or plan a new project, application, or system (e.g. phrases containing "let's build", "build this", "create a new"):
1. **Always invoke the `project-foundation` skill** ([`.agents/skills/project-foundation/SKILL.md`](file:///c:/Users/adity/OneDrive/Desktop/TTS_OP/.agents/skills/project-foundation/SKILL.md)).
2. **Execute Phase 0 (Intercept)**: Verify workspace state and whether foundation files already exist.
3. **Execute Phase 1 (Regressive Analysis & Grilling)**: State back the root problem, run background research on prior art/pitfalls, and ask sharp clarifying questions (one at a time, recommending an answer) to pin down target users, constraints, and explicit non-goals.
4. **Execute Phase 2 (Foundation Documentation)**: Generate all foundational documents in strict dependency order before writing ANY production code:
   - `PRD.md` (Product Requirements Document)
   - `schema.md` (Data Model, API Contracts, State Machines)
   - `Architecture.md` (System Architecture, Components, Failure Modes)
   - `architecture-essentials.md` (Key Architectural Decisions & Tradeoffs)
   - `appflow.md` (User & System Workflows)
   - `design.md` (UI/UX Specification, Overlays, Interaction Details)
   - `rules.md` (Codebase Standards & Invariants)
   - `implementation_plan.md` (Phased Milestones & Risk Register)
   - `tracker.md` (Living Sprint Backlog & Status)
5. **Phase 3 (Review & Harden)**: Ensure cross-document consistency and populate sprint tasks.
6. **Strict Guardrail**: Under no circumstance should production code files be created until these foundation documents are written, verified, and approved.
