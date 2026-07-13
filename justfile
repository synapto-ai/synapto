
# Run scenarios matching a filter
test-scenarios filter="":
    cargo test -p synapto-test --test scenario_tests -- {{ filter }} --test-threads=1 --nocapture
