# Foundation File Specifications

Each file below lists its **purpose**, required **sections**, and its **done condition** — the checkable criterion that declares the file complete.

---

## 1. PRD.md — Product Requirements Document

**Purpose**: Single source of truth for *what* is being built and *why*. No implementation details.

**Required sections**:
- Problem Statement — one paragraph, the root pain being solved
- Target Users — named personas with specific, observable needs
- Success Metrics — measurable outcomes (not vanity metrics)
- Core Features — prioritised list (must-have / should-have / won't-have this version)
- Non-Goals — explicit list of what is out of scope and why
- Open Questions — unresolved decisions that would change scope

**Done when**: every Core Feature has a priority label; Non-Goals are explicit; Success Metrics are measurable and have an owner.

---

## 2. schema.md — Data Model & API Contract

**Purpose**: Canonical definition of every entity, relationship, and API surface.

**Required sections**:
- Entity Definitions — each entity: fields, types, constraints, relationships
- Database Schema — tables/collections in code-fenced SQL, BSON, or equivalent
- API Contracts — endpoint list with method, path, request shape, response shape, auth requirement
- State Machines — any entity with lifecycle states, drawn as a transition table or diagram
- Data Flow Diagram — how data moves between system boundaries

**Done when**: every entity referenced in PRD.md Core Features exists in the schema; every API endpoint maps to at least one core feature; no orphaned fields.

---

## 3. Architecture.md — System Architecture

**Purpose**: How the system is structured: components, boundaries, integrations.

**Required sections**:
- System Context — one diagram (Mermaid) showing the system and its external actors
- Component Map — internal components and their responsibilities
- Integration Points — third-party services, APIs, queues with protocol and ownership
- Deployment Topology — where code runs (cloud provider, containers, edge, etc.)
- Scalability & Reliability Notes — expected load, SLOs, failure modes

**Done when**: the Mermaid context diagram renders; every component in the Component Map has a stated responsibility; every integration names a fallback or failure behaviour.

---

## 4. architecture-essentials.md — Critical Architectural Decisions

**Purpose**: A tight log of decisions that are hard to reverse, surprising without context, or the result of real trade-offs. Not a summary of Architecture.md.

**Required sections per decision**:
- Decision title (imperative verb phrase)
- Status: Proposed | Accepted | Deprecated
- Context — why this was a decision point
- Alternatives Considered — at least two, with why each was rejected
- Decision — what was chosen
- Consequences — trade-offs accepted, risks introduced

**Done when**: every decision marked Accepted has at least two alternatives listed with rejection reasons; no decision lacks a Consequences section.

---

## 5. appflow.md — Application Flow

**Purpose**: End-to-end narrative of how a user moves through the system, from entry to value.

**Required sections**:
- Happy Path Flows — step-by-step for each core feature (user action → system response)
- Error Paths — what happens when each step fails
- Auth Flow — login, token refresh, logout, role escalation
- Notification & Event Flow — what triggers what, in order
- Flow Diagram — Mermaid sequence or flowchart for the most complex flow

**Done when**: every Core Feature in PRD.md has a corresponding Happy Path Flow; every flow names who initiates it (user, system, scheduler); Error Paths exist for every step that can fail.

---

## 6. design.md — UI/UX Design System

**Purpose**: Visual language and interaction patterns. Ground truth before any frontend is written.

**Required sections**:
- Design Philosophy — one paragraph: the emotion and values the product must convey
- Color Palette — primary, secondary, accent, semantic colours (hex or HSL), dark-mode equivalents
- Typography — font families, scale (px or rem), weight usage
- Component Inventory — list of UI components needed (button, modal, card, table, etc.)
- Layout Principles — grid, spacing unit, responsive breakpoints
- Interaction Patterns — hover, focus, loading, empty, error states
- Accessibility Baseline — WCAG level targeted, colour contrast ratios checked

**Done when**: Color Palette has dark-mode values; every Core Feature has at least one component from the Component Inventory; Accessibility Baseline is stated (not assumed).

---

## 7. rules.md — Project Rules & Constraints

**Purpose**: Explicit behavioural rules for the codebase and the team. Every rule must be traceable to a decision in PRD.md or architecture-essentials.md.

**Required sections**:
- Coding Standards — language, linting, formatting, naming conventions
- Auth & Security Rules — token schema, session handling, secret management
- Error Handling Contract — how errors are caught, logged, and surfaced
- Testing Requirements — minimum coverage, test types required per layer
- Branching & Commit Strategy — branch model, commit format, review gates
- Dependency Policy — allowed libraries, banned patterns, upgrade cadence
- Source — for each rule, a citation: (PRD §...) or (architecture-essentials.md §...)

**Done when**: every rule has a Source citation; no rule contradicts another; Security Rules explicitly name the token schema used.

---

## 8. implementation_plan.md — Phased Build Plan

**Purpose**: Ordered sequence of milestones and tasks that builds the system from schema to shipped feature.

**Required sections**:
- Phases — at minimum: Foundation, Core Features, Polish & Hardening, Launch
- Per Phase: goal statement, deliverables, success criterion
- Per Milestone: tasks with owner placeholder, estimated effort (S/M/L), dependencies
- Risk Register — top five risks with likelihood, impact, and mitigation
- Open Decisions — anything still unresolved that will block a task

**Done when**: every Core Feature in PRD.md maps to at least one task; Risk Register has five entries; no task references a schema entity that does not exist in schema.md.

---

## 9. tracker.md — Project Tracker

**Purpose**: Living sprint board. Updated at the end of every bootstrap and at the start of every coding session.

**Required sections**:
- Current Sprint — sprint number, goal, start/end dates
- Task Table — columns: ID | Title | Status (Todo/In Progress/Done/Blocked) | Owner | Phase | Priority
- Milestone Map — milestones from implementation_plan.md with target dates and % complete
- Blockers — items preventing progress, with who is responsible for unblocking
- Changelog — one-line entry per session: date, what changed

**Done when**: at least one milestone exists with a target date; at least three tasks from implementation_plan.md Phase 1 are in the Task Table with Status=Todo; Changelog has the bootstrap entry.
