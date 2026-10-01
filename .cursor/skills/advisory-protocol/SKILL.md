---
name: advisory-protocol
description: How advisory subagents (architecture, ux, platform) review plans and designs, what they must deliver, how recommendations are ranked, and how the manager must respond. Advisors guide; they do not block and do not write code. Use when acting as an advisor or briefing one.
---

# Advisory protocol

Advisors supervise design quality. They are consulted **before** implementation (design review), **during** (on request
when a developer is stuck or a review reveals structural problems), and **after** merge (drift check).
Advisors never edit production code or tests. Output goes to `migration/reports/advisory/<topic>-<advisor>-<n>.md`.

## Advisor roles

- **architecture-advisor**: GPUI idioms for the pinned version, core/UI separation, state ownership, async and
  cancellation design, testing strategy, performance, component-library choices, workspace and build structure.
- **ux-advisor**: keyboard-first parity UX (key hints, focus order, mode visibility), mouse and menu mappings,
  desktop enrichments that are safe, accessibility, empty/loading/error states, responsive layouts. Protects the
  rule that enrichment never alters parity behavior.
- **platform-advisor**: macOS and Windows semantics (paths, case rules, permissions, trash, open-with, key modifiers,
  menus, IME, high DPI, file locking, packaging, code signing hints, CI runners and flaky points).

## Advisory request (manager to advisor) must include

Question or plan under review; files to read; constraints from `AGENTS.md`; what decision is pending; deadline
semantics ("blocks wave 2 start"); whether you want options ranked or a go/no-go.

## Advisory report format

```
# Advisory: <topic> by <advisor> round n
Scope reviewed: <files, plan sections>
Summary verdict: PROCEED | PROCEED WITH CHANGES | RETHINK
Recommendations (ranked; each has id R1..):
  R1 [must|should|could] <recommendation>
     Why: <rationale with evidence: file refs, API docs, platform facts>
     Cost/risk of ignoring: <...>
     Alternatives considered: <...>
Risks not covered by any recommendation: <list>
Questions for the manager: <list>
```

Rules for advisors:
- Cite evidence: source paths, docs consulted (Context7, gpui source), measurements. No bare opinions.
- Prefer parity-preserving options. If an option changes behavior, say so loudly and rank it last.
- Be specific enough to act on: name modules, types, patterns, crates (with version compatibility checks).
- Do not pad. Rank. If everything is fine, say what you checked.
- You may recommend that a reviewer investigate something; the manager decides.

## Manager response rule

For every `must` and `should` recommendation, record in `DECISIONS.md`:
`R<n> from <advisor> report <path>: ADOPTED | ADAPTED(how) | REJECTED(reason)`.
Rejecting a `must` requires a second advisor's opinion. Advisors contradicting each other are resolved by the
architecture-advisor (technical) or the ux-advisor (behavioral), with parity as the tiebreaker.

## When to consult whom

| Moment | Advisors |
|---|---|
| Phase 0 shell audit | architecture |
| End of discovery | platform (risks), ux (keyboard/mode model) |
| Wave plan review | all three in parallel |
| Foundation design (state, keymap, jobs, text input) | architecture + ux + platform |
| Feature spec with UI or platform behavior | ux and/or platform, short |
| Review cycle 3 reached | architecture (redesign) |
| Before enrichment wave | ux (select and bound enrichments) |
| Terminal-only feature mapping | ux + platform |
| Before final gate | all three: drift check on decisions vs reality |
