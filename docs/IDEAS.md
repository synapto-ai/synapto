# Ideas & Future Work

This document tracks high-level ideas, architectural experiments, and potential improvements for the AI project.

## Robustness & Resilience

### Plugin Panic Handling
- **Current State**: Termination on panic is a **deliberate design choice** for maximum visibility. In many asynchronous systems, panics in spawned tasks can go unnoticed in logs. To prevent silent failures, a global panic hook in `synapto` catches any task panic and triggers a fatal program shutdown.
- **Idea**: Introduce a configurable "Fault Tolerance" policy.
    - **Policies**: 
        - `Terminate` (Default): Stop the entire program to ensure the failure is addressed.
        - `Restart`: Attempt to re-initialize and restart the failed plugin.
        - `Ignore`: Log the error and continue without that plugin.
    - **Scope**: This could be configurable globally in the bundle's `Config`, or overridden per-plugin during registration.
- **Trade-off**: Managing "Restart" logic requires plugins to be truly stateless or have robust re-initialization routines to avoid accumulating broken state.

## Cognitive Capabilities
*(Add more ideas here...)*
