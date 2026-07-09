test-scenarios filter="*":
    #!/usr/bin/env bash
    set -euo pipefail
    # Check for direct file match, directory/scenario.yaml, or wildcard match
    found=0
    for scenario in scenarios/{{ filter }}.yaml scenarios/{{ filter }}/scenario.yaml scenarios/{{ filter }}; do
        if [ -f "$scenario" ]; then
            echo "========================================================================"
            echo "RUNNING SCENARIO: $scenario"
            echo "========================================================================"
            cargo run -p test-bundle -- "$scenario"
            found=1
        fi
    done
    if [ "$found" -eq 0 ]; then
        # Try wildcard expansion if no direct match found
        for scenario in scenarios/{{ filter }}*.yaml; do
            if [ -f "$scenario" ]; then
                echo "========================================================================"
                echo "RUNNING SCENARIO: $scenario"
                echo "========================================================================"
                cargo run -p test-bundle -- "$scenario"
                found=1
            fi
        done
    fi
