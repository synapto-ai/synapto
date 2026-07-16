
# Run scenario tests.
# Usage:
#   just test-scenarios                        (run all)
#   just test-scenarios <filter>               (run specific test across workspace)
#   just test-scenarios -p <package>           (run all tests in a crate)
#   just test-scenarios -p <package> <filter>  (run specific test in a crate)
test-scenarios *ARGS:
    cargo test {{ if ARGS == "" { "--workspace" } else { ARGS } }} --test scenario_tests -- --ignored --test-threads=1 --nocapture

# Check for broken local and relative links in markdown documentation using lychee
link-check:
    lychee --offline "crates/**/*.md" "docs/**/*.md" "README.md"

# Run all pre-release checks (lints, tests, formatting, and links) to guarantee release readiness
pre-release-check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-targets
    just test-scenarios
    just link-check
