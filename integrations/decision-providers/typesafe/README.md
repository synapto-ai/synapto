# TypeSafe Decision Provider (`synapto-decision-typesafe`)

Dedicated decision provider providing fast, typed System One judgments (`Choice`, `Noul`, `Score`) for Synapto assistants using the [TypeSafe](https://typesafe.ai) API.

## Configuration

In your assistant bundle settings:

```yaml
typesafe_decision:
  model: "jev-latest"
  api_endpoint: "https://api.typesafe.ai/v1/systemone"
```

## Credentials

The decision provider resolves its API key from the ambient `CredentialsHandle` using `TypeSafeCredentialTarget`. Ensure that `synapto-credentials-typesafe::TypeSafeCredentials` is included in your bundle's `.credentials()` tuple.
