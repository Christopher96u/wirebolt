# Authentication

[Documentation](README.md) · [Environments and secrets](environments-and-secrets.md)

Open a request's **Auth** tab and choose the scheme. Save the request with **⌘S** after configuring it.

| Scheme | Setup |
| --- | --- |
| None | Send without generated authentication |
| Basic | Enter the username and password in the credential fields |
| Bearer | Enter the token in the Bearer field |
| API Key | Choose **Add To** (Header or Query Params), enter the **Key** name and the **Value** |
| OAuth 2.0 | Configure Authorization Code + PKCE or Client Credentials, then acquire a token |

Passwords, tokens, API key values and OAuth client secrets are stored in this Mac's Keychain. The request saves only a reference name, and every request gets its own names, so two requests never share or overwrite each other's credentials. Use the eye button to show a masked value. Typed credentials are written to Keychain when you send, save or request a token.

Choosing another scheme starts that scheme empty; switching back does not restore the previous credentials.

## OAuth 2.0

For **Authorization Code + PKCE**, enter the provider's Authorization URL, Token URL, Client ID and registered Redirect URI. Add scopes and audience if the provider requires them. Use **Get New Access Token** and complete the provider's browser flow.

For **Client Credentials**, enter the Token URL, Client ID and **Client Secret**, then use **Get New Access Token**.

The acquired access token is stored in Keychain for this request and sent as a Bearer token. A token receipt is local state; sharing the request does not share the token or the client secret. Configure credentials separately on each Mac.

Provider requirements vary. A configured form does not guarantee compatibility with every identity provider. If token acquisition fails, check the provider's redirect registration, grant permissions, scopes and endpoint response. Avoid posting tokens or authorization codes in issue reports.

## Export compatibility

Wirebolt JSON preserves API key and OAuth configuration in Wirebolt-specific metadata. Reimport through the Wirebolt format to retain it. Other clients may not interpret these extensions; credential values remain in Keychain. [Import and export](import-export.md) explains the boundary.
