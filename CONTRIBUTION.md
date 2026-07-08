# Contributing to Synapto

We welcome contributions through issues and pull requests.

### Issues

- **Bug**: Report code issues (include logs/stacktraces).
- **Proposal/Discussion**: Propose features or discuss ideas before coding.
- **Question**: Ask for help.

Search existing issues before creating a new one.

### Pull Requests

1. Open an issue first to set expectations.
2. Fork the repository and create a branch.
3. Write your changes and include tests.
4. Update relevant documentation.
5. Commit with DCO sign-off (`git commit -s`).
6. Open a PR and ensure CI checks pass. Use `[WIP]` in the title for early feedback.

### ✍️ Developer Certificate of Origin (DCO)

Every commit needs to be signed. By adding a `Signed-off-by` line to your commit message (`git commit -s -m "message"`), you certify the following:

[Developer Certificate of Origin (https://developercertificate.org/)](https://developercertificate.org/)

```text
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.

Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

If you forgot to sign your commit, easily fix it:

```bash
git commit --amend --no-edit --signoff
git push --force-with-lease <remote-name> <branch-name>
```

### 📋 RFC Requirements for Significant Changes

To keep our architecture robust, reliable, and decoupled:

- **Significant Changes / New Features:** Any significant change, major new feature, or architectural shift (e.g., introducing a new plugin type, changing cross-boundary interfaces, modifying the core loop) **MUST have a formal Request for Comments (RFC)** and be driven by the lifecycle detailed in [`AGENTS.md`](AGENTS.md) before any code is merged.
- **Using AI to Write RFCs:** You are highly encouraged to use AI coding assistants (such as the Zed coding agent, Claude, or GPT) to author, review, and refine your RFC document. However
- **How to Start:** Open an issue on GitHub using our **Feature Proposal & RFC Request** template. This template will help gather high-level feedback before drafting the markdown RFC under `docs/rfcs/`.
- **Small Changes:** Localized bug fixes, documentation updates, typo corrections, or adding/refining tests **do not** require an RFC. Simply proceed to opening a Pull Request!

---

## Code Style & Clippy Guidelines

### Allowing `clippy::new_without_default`

You may use `#[allow(clippy::new_without_default)]` locally on a struct's `new` method when implementing `Default` is semantically incorrect or doesn't make sense. Specifically, when `new` doesn't return exactly the same initialized struct, it can be `new(...)` and not `default()`.

### Error Handling: Silently Dropping Results

Silently dropping `Result` values using `let _ = result;` is an anti-pattern as it hides failure modes and makes debugging difficult. While this is already enforced by project lints, you must handle errors properly when replacing them.

Do not ignore errors. Instead, ensure visibility into failures by explicitly logging them.

These rules don't apply in tests.

**Preferred Patterns:**

1.  **For standalone statements (logging and discarding):**

    ```rust
    if let Err(e) = result {
        tracing::error!("Operation failed: {}", e);
    }
    ```

2.  **When chaining method calls (logging and discarding):**

    ```rust
    result.inspect_err(|e| tracing::error!("Operation failed: {}", e)).ok();
    ```

    _Note:_ `result.unwrap_or_else(|e| tracing::error!("{}", e));` is acceptable _only_ if the `Ok` type is `()`, but `if let Err(e)` is generally preferred for clarity.

3.  **For theoretically impossible errors or missing values:**
    Use `unreachable!` when an error variant or `None` variant exists in the type signature but the specific state of your program makes it impossible to reach.

    ```rust
    // For Result types:
    result.unwrap_or_else(|e| unreachable!("This state should be impossible because... {}", e));

    // For Option types:
    option.unwrap_or_else(|| unreachable!("This state should be impossible because..."));
    ```

4.  **For fatal, unrecoverable errors (Result types):**
    Never use `.expect("<message>")` on `Result` types. Instead, always use `.unwrap_or_else` with a lazy `panic!`, appending the error context. This avoids formatting overhead on the happy path and ensures the error is preserved.

    Discarding the error variable (e.g., using `|_| panic!("<message>")`) is **strictly forbidden**. The error must always be part of the panic message.

    ```rust
    // Do not do this:
    // let client = plugin.expect("Failed to create plugin");
    // let client = plugin.unwrap_or_else(|_| panic!("Failed to create plugin"));

    // Instead, do this:
    let client = plugin.unwrap_or_else(|e| panic!("Failed to create plugin: {:?}", e));
    ```

    _Exceptions for non-Debug/non-Display errors (such as `Box<dyn Any>` downcasting):_
    If the `Err` variant cannot be formatted because it does not implement `Debug` or `Display`, you must still capture the error variable and include helpful dynamic type context (using `std::any::type_name_of_val(&*e)` or its TypeId) in the panic message rather than discarding it with `|_|`.

    ```rust
    // For Downcasting any types (Do not use `|_|`):
    let plugin = any_value.downcast::<MyPlugin>().unwrap_or_else(|e| {
        panic!(
            "Downcast failed to target type: {}. Actual dynamic type of error value was: {}",
            std::any::type_name::<MyPlugin>(),
            std::any::type_name_of_val(&*e)
        )
    });
    ```

    _Note:_ This rule also applies to `Option` types when a `None` value represents a fatal, unrecoverable developer error or invariant violation, but only if you are adding dynamic context (e.g., `.unwrap_or_else(|| panic!("State invalid: {}", variable))`). If you are only providing a static string message with no formatting, using `.expect("<message>")` on an `Option` is fully allowed and preferred for conciseness.

5.  **Readability vs. Allocations on Happy Path:**
    Readability is more important than avoiding minor allocations. It is completely acceptable to use simple allocations like `.ok_or("error message".to_string())?` or `.unwrap_or("default".to_string())`. The goal of avoiding "overhead on the happy path" is specifically to prevent calling heavy formatting functions (like `format!`) or executing complex logic directly in method arguments, not to forbid all basic allocations.

---

## Testing & Verification

We maintain strict test-driven development practices across the entire workspace. All code contributions must be accompanied by relevant tests.

For detailed guidelines on running unit tests, setting up live integration tests with `test_config.json`, or creating new tests for core modules and plugins, refer to our [Testing Guidelines in test-harness/README.md](test-harness/README.md).
