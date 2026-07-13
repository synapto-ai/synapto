name: "💡 Feature Proposal & RFC Request"
description: Propose a new feature, major change, or architectural shift.
title: "[Proposal]: <Short, clear description of the feature/shift>"
labels: ["proposal", "rfc-needed"]
body:
  - type: markdown
    attributes:
      value: |
        Thank you for wanting to improve Synapto! 🧠
        
        To maintain architectural integrity, safety, and performance, **every significant change, major feature, or architectural shift must go through our formal Request for Comments (RFC) protocol** before implementation starts. This protocol is driven by the process defined in the [`synapto-ai/rfcs`](https://github.com/synapto-ai/rfcs) repository.
        
        *Tip: You are highly encouraged to use an AI coding assistant (like Zed's AI agent or Claude/GPT) to help you author and refine your draft!*

  - type: textarea
    id: problem-statement
    attributes:
      label: 1. Problem Statement
      description: What problem or pain point does this proposal solve? Please describe the current limitations.
      placeholder: "e.g., Currently, the STT plugin is hardcoded to a single provider, making offline deployments impossible..."
    validations:
      required: true

  - type: textarea
    id: proposed-solution
    attributes:
      label: 2. Proposed Solution
      description: High-level overview of the proposed changes.
      placeholder: "e.g., Introduce a pluggable speech-to-text interface trait..."
    validations:
      required: true

  - type: textarea
    id: architectural-impact
    attributes:
      label: 3. Architectural Impact
      description: Does this change modify existing boundaries, channels, or database structures? How does it relate to the core design principles in ARCHITECTURE.md?
      placeholder: "e.g., This adds a new binary-named stream but doesn't affect existing chat plugins..."
    validations:
      required: false

  - type: dropdown
    id: rfc-readiness
    attributes:
      label: 4. RFC Readiness
      description: Are you ready to drive this proposal through the formal RFC phase as detailed in the `synapto-ai/rfcs` repository?
      options:
        - "Yes, I will draft the RFC (or use an AI agent to draft it)."
        - "I would like help/collaboration from core maintainers to draft the RFC."
        - "No, this is just an initial idea / request for feedback."
    validations:
      required: true
