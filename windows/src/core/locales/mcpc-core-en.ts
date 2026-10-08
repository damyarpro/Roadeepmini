// English strings for the E_MCPC_* codes of the MCP client core
// (src-tauri/src/mcpc/: store, transports, tool calls). Sign-in errors are in
// mcpc-oauth-en.ts. Registered through registerMessages by core/error-text.ts.

export const mcpcCoreEn: Record<string, string> = {
  "err.mcpc.store": "Couldn't read or save your MCP servers: {detail}",
  "err.mcpc.unknownServer": "This server no longer exists.",
  "err.mcpc.invalid": "Check “{field}” — it isn't valid here.",
  "err.mcpc.limit": "You've reached the limit ({limit}).",
  "err.mcpc.needsAuth": "The server needs a token or a sign-in.",
  "err.mcpc.needsApproval": "Confirm the command before it runs.",
  "err.mcpc.disabled": "The server is off.",
  "err.mcpc.commandNotFound": "Couldn't find {command}. Install it (or check PATH), then restart Roadeep.",
  "err.mcpc.unsafeArg": "This argument can't be passed safely to the command: {arg}",
  "err.mcpc.spawn": "Couldn't start the command: {detail}",
  "err.mcpc.exited": "The server stopped.",
  "err.mcpc.timeout": "The server didn't answer in time.",
  "err.mcpc.http": "The server answered with an error ({status}).",
  "err.mcpc.network": "Couldn't reach the server: {detail}",
  "err.mcpc.tooLarge": "The server's answer is too large.",
  "err.mcpc.protocol": "The server's answer wasn't understood: {detail}",
  "err.mcpc.rpc": "The server reported an error ({code}): {message}",
  "err.mcpc.unknownTool": "No connected server offers the tool {tool}.",
  "err.mcpc.toolOff": "The tool {tool} is switched off.",
  "err.mcpc.toolAsk": "The tool {tool} now asks before it runs, and it wasn't approved.",
  "err.mcpc.argsTooLarge": "The tool's input is too long to show you in full (over {limit} characters), so it wasn't run.",
  "err.mcpc.commandChanged": "The command changed after it was shown. Check it again, then approve.",
};
