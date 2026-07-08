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
