# Authentication

[Documentation](README.md) · [Environments and secrets](environments-and-secrets.md)

Open a request's **Auth** tab and choose the scheme. Save the request with **⌘S** after configuring it.

| Scheme | Setup |
| --- | --- |
| None | Send without generated authentication |
| Basic | Enter the username and password in the credential fields |
| Bearer | Enter the token in the Bearer field |
| API key | Choose Header or Query placement, enter the parameter name and a Keychain reference in **Value Secret** |
| OAuth 2.0 | Configure Authorization Code + PKCE or Client Credentials, then acquire a token |

Basic and Bearer edits use Keychain-backed references. For API keys, **Value Secret is a reference name, not the API key itself**. Provision its value using the [Keychain instructions](environments-and-secrets.md#credentials-and-secret-references). There is no general API-key secret creation screen yet.

## OAuth 2.0

For **Authorization Code + PKCE**, enter the provider's Authorization URL, Token URL, Client ID and registered Redirect URI. Add scopes and audience if the provider requires them. Use **Get New Access Token** and complete the provider's browser flow.

For **Client Credentials**, enter the Token URL and Client ID. Choose a Client Secret Reference, enter the secret in **Client Secret (Keychain only)** and click **Store Client Secret in Keychain** before requesting a token.

The Access Token Reference identifies where the acquired token is stored. A token receipt is local state; sharing the request does not share the token. Configure credentials separately on each Mac.

Provider requirements vary. A configured form does not guarantee compatibility with every identity provider. If token acquisition fails, check the provider's redirect registration, grant permissions, scopes and endpoint response. Avoid posting tokens or authorization codes in issue reports.

## Export compatibility

Wirebolt JSON preserves API key and OAuth configuration in Wirebolt-specific metadata. Reimport through the Wirebolt format to retain it. Other clients may not interpret these extensions; credential values remain in Keychain. [Import and export](import-export.md) explains the boundary.
