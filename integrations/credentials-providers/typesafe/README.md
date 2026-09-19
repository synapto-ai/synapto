# TypeSafe Credentials Provider (`synapto-credentials-typesafe`)

Pluggable credentials provider resolving TypeSafe API keys via `TypeSafeCredentialTarget`.

## Configuration

In your assistant bundle settings (e.g. `config.json`):

```json
{
  "credentials": {
    "synapto_credentials_typesafe": {
      "TypeSafeCredentials": {
        "api_key": "your-api-key"
      }
    }
  }
}
```

Or via environment variable using the structured prefix:

```bash
SYNAPTO__CREDENTIALS__synapto_credentials_typesafe__TypeSafeCredentials__api_key="your-api-key"
```
