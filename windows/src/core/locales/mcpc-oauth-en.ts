// English strings for the MCP server sign-in errors (src-tauri/src/mcpc/oauth.rs,
// codes in errors.rs, read by core/error-text.ts, which registers this file).

export const mcpcOauthEn: Record<string, string> = {
  "err.mcpc.oauth.discovery": "Couldn't find how this server signs in ({detail}).",
  "err.mcpc.oauth.insecure": "This server's sign-in page isn't secure (https) on {host}, so it was not used.",
  "err.mcpc.oauth.noDcr": "This service doesn't let apps sign in on their own. Create a token in the service and switch the server's authentication to token.",
  "err.mcpc.oauth.register": "The service refused to register Roadeep for sign-in ({detail}).",
  "err.mcpc.oauth.listen": "Couldn't open a local port for the browser's answer ({detail}).",
  "err.mcpc.oauth.denied": "Sign-in was declined in the browser ({detail}).",
  "err.mcpc.oauth.state": "The browser's answer didn't belong to this sign-in, so it was ignored. Try again.",
  "err.mcpc.oauth.timeout": "Sign-in wasn't finished in the browser within 5 minutes. Try again.",
  "err.mcpc.oauth.cancelled": "Sign-in was stopped.",
  "err.mcpc.oauth.token": "The service didn't hand over access after sign-in ({detail}).",
  "err.mcpc.oauth.keyring": "Couldn't save the sign-in in Windows Credential Manager ({detail}).",
};
