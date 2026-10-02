Read and follow all instructions in [CONTRIBUTION.md](CONTRIBUTION.md) before planning or executing any changes.

## Zero Environment Variables for Operational Credentials & Parameters Mandate

1. **Zero Secret Fallback:** Operational tasks, provisioning scripts, deployments, and automation executed via `mise` or `just` must NEVER read credentials, API tokens, or system configurations from ambient environment variables.
2. **Deterministic Sources:** All operational secrets and parameters must be loaded strictly from:
   - SOPS-encrypted configuration files (`encrypted.*.json`, `clients/encrypted.clients.json`).
   - Explicit, validated positional command-line arguments.
3. **Prohibited Pattern:** Scripts must not use fallback expansions for credentials (for example: `TOKEN="${API_TOKEN:-...}"` or `ACCOUNT_ID="${ACCOUNT_ID:-...}"`). Missing credentials must immediately cause a hard failure.
4. **Permitted Exception:** Environment variables are permitted exclusively for transient, non-secret developer controls (such as `DEBUG`, `LOG_LEVEL`, or `ASSISTANT_LOG_LEVEL`).
