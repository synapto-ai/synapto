# Synapto Plugin Skills

Agent Skills standard (`SKILL.md`) integration plugin for the Synapto framework.

## Overview

This plugin enables the agent to discover and use skills based on the Agent Skills specification. It implements progressive disclosure to maintain a minimal context window size:

1. **Context Provider (`skills`):** Injects a lightweight candidate index (`available_skills`) or pre-loads an auto-activated skill (`active_skill`) into `TemporalScope::Current`. If a `DecisionProvider` is configured, candidate skills and auto-activations are dynamically evaluated against conversational history.
2. **State-Locked Meta-Tool (`load_skill`):** Enables the LLM to load full instructions and discover bundled scripts and references on demand. Following the State-Locked Availability pattern, `load_skill` dynamically hides itself when an active skill is already pre-loaded or when no candidate skills match the current context.

---

## State Matrix: All Execution Cases

The table below documents all runtime outcomes based on the decision probability distribution, threshold configuration, and catalog state.

| Case | Condition | `active_skill` | `available_skills` | `load_skill` Tool | Behavioral Outcome |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **1. Dominant Auto-Activation** | Top skill $P \ge 0.85$, all others $< 0.50$ | **`Some(Skill)`** (full instructions + files) | **`[]`** (empty) | **Hidden (`false`)** | **Zero-Hop Execution:** LLM receives full instructions on Turn 1; `load_skill` is completely omitted as redundant. |
| **2. Multi-Candidate Auto-Activation** | Top skill $P \ge 0.85$, secondary skill $P \ge 0.50$ | **`Some(TopSkill)`** (pre-loaded) | **`[SecondarySkill]`** (candidate only) | **Hidden (`false`)** | Top skill executes immediately on Turn 1. Secondary candidates remain documented in context. |
| **3. Medium-Confidence Disambiguation** | $0.50 \le P_{\text{top}} < 0.85$ | **`None`** | **`[SkillA, SkillB, ...]`** ($P \ge 0.50$) | **Exposed (`true`)** | LLM inspects filtered candidate index and autonomously invokes `load_skill` if desired. |
| **4. High-Confidence Rejection** | `none` choice $P_{\text{none}} \ge 0.60$ | **`None`** | **`[]`** (empty) | **Hidden (`false`)** | User request is unrelated to specialized skills. Zero token overhead and no tool clutter. |
| **5. Low Confidence / Ambiguous Spread** | All $P < 0.50$, $P_{\text{none}} < 0.60$ | **`None`** | **All eligible skills** | **Exposed (`true`)** | Fallback to full catalog progressive disclosure index; LLM determines match. |
| **6. Turn 1 / No Prior History** | `recent_interactions` is empty | **`None`** | **All eligible skills** | **Exposed (`true`)** | Fallback to full catalog progressive disclosure index; LLM determines match. |
| **7. Decision Backend Unavailable** | Decision provider unconfigured / error | **`None`** | **All eligible skills** | **Exposed (`true`)** | Graceful degradation; system functions as standard progressive disclosure without decision gating. |
| **8. Empty Skills Catalog** | 0 skills discovered in filesystem | **`None`** | **`[]`** (empty) | **Hidden (`false`)** | Plugin remains inert; no tools or context keys populated. |

---

## State-Locked Progressive Disclosure Architecture

```text
                                [ User Turn Context ]
                                          │
                                          ▼
                      ┌───────────────────────────────────────┐
                      │    Decision Provider (ChoiceQuestion) │
                      └───────────────────┬───────────────────┘
                                          │
                   ┌──────────────────────┴──────────────────────┐
                   ▼                                             ▼
          Top Skill P >= 0.85                            0.50 <= P < 0.85
                   │                                             │
                   ▼                                             ▼
            [ active_skill ]                           [ available_skills ]
        Pre-loads full SKILL.md                     Injects candidate index
        and bundled file paths                      (name + description)
                   │                                             │
                   ▼                                             ▼
         load_skill: HIDDEN                            load_skill: EXPOSED
        (Zero-Hop Turn 1 Execution)                   (On-Demand Tool Invocation)
```

---

## Discovery Precedence

Skills are discovered across standard directories in the following order of precedence:

1. `<project_root>/.claude/skills/*/SKILL.md`
2. `<project_root>/.agents/skills/*/SKILL.md`
3. `~/.claude/skills/*/SKILL.md` (or `$CLAUDE_CONFIG_DIR/skills/*/SKILL.md`)
4. `~/.agents/skills/*/SKILL.md`

Higher-precedence locations override duplicate skill names.

---

## Configuration

Configuration is optional. By default, `<project_root>` resolves to the current working directory.

```json
{
  "synapto_plugin_skills": {
    "project_root": "/path/to/project",
    "candidate_threshold": 0.50,
    "activation_threshold": 0.85
  }
}
```

* `candidate_threshold`: Minimum probability for a skill to appear in `available_skills` (default: `0.50`).
* `activation_threshold`: Minimum probability for a skill to auto-activate into `active_skill` (default: `0.85`).
