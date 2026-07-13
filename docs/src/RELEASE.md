# Release Process

This document outlines the standard release process for the `synapto` workspace using [`cargo-release`](https://github.com/crate-ci/cargo-release). 

## Prerequisites

Before initiating a release, ensure you have the required tools installed and your environment is correctly configured.

1. **Install `cargo-release`:**
   ```bash
   cargo install cargo-release
   ```

2. **Crates.io Authentication:**
   If you are publishing crates to crates.io, make sure you are authenticated.
   ```bash
   cargo login
   ```
   *You will need a crates.io API token.*

3. **Clean Working Tree:**
   Ensure your git working directory is clean. Commit or stash any uncommitted changes.
   ```bash
   git status
   ```
   You should be on the `main` branch and fully synced with the remote repository.

## Release Steps

`cargo-release` automates the process of bumping versions across the workspace, creating a release commit, tagging the repository, and publishing to crates.io.

### 1. Determine the Release Level

Decide on the appropriate version bump based on Semantic Versioning (SemVer). Common levels include:
- `patch`: Bug fixes and minor tweaks.
- `minor`: New backwards-compatible features.
- `major`: Breaking changes.
- `release`: Strips pre-release identifiers (e.g., changing `0.1.0-dev.1` to `0.1.0`).
- `rc` / `beta` / `alpha`: For pre-release builds.

### 2. Perform a Dry Run (Recommended)

Always run a dry run first to see what changes `cargo-release` will make. This step does not mutate the repository or publish crates.

```bash
cargo release <level>
```
*Example: `cargo release minor`*

Review the output carefully. Ensure that the versions of the workspace and all member crates are being bumped correctly and that no unexpected changes are listed.

### 3. Execute the Release

Once you have verified the dry run, execute the release by appending the `--execute` flag.

```bash
cargo release <level> --execute
```

This command will automatically:
1. Update `Cargo.toml` versions.
2. Run `cargo check` and `cargo test`.
3. Commit the changes.
4. Publish the crates to crates.io.
5. Tag the commit with the new version.
6. Push the commit and the tag to the remote repository.

### 4. Post-Release Verification

Verify the repository tags and crates.io pages to confirm the release is live.
