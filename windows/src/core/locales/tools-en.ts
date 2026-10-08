// English strings for MCP tools in the island chat: the composer's tools chip,
// the tool-step rows, the one-time credit hint and the TOOL_* errors of
// chat_send. Registered with registerMessages by views/chat.ts.

export const toolsEn: Record<string, string> = {
  "tools.chip": "Tools",
  "tools.chipCount": "Tools · {n}",
  "tools.chipOn": "MCP tools are on for this chat. Click to turn them off.",
  "tools.chipOnCount": "MCP tools are on for this chat ({n} available). Click to turn them off.",
  "tools.chipOff": "MCP tools are off for this chat. Click to turn them on.",
  "tools.turnedOn": "MCP tools on for this chat",
  "tools.turnedOff": "MCP tools off for this chat",

  "tools.state.waiting": "Waiting for your approval",
  "tools.state.running": "Running…",
  "tools.state.done": "Done",
  "tools.state.error": "Failed",
  "tools.state.declined": "Declined",
  "tools.state.stopped": "Stopped",
  "tools.arguments": "Input",
  "tools.result": "Result",
  "tools.error": "Error",
  "tools.noArguments": "No input",
  "tools.details": "Details of {name}",
  "tools.approvalServer": "Server",
  "tools.approvalTool": "Tool",
  "tools.approvalArguments": "The exact input the tool will get",
  "tools.creditHint": "Each tool step is one more message to Roadeep, so it uses credit too.",

  "rerr.TOOL_MEMORY_TRIGGER":
    "Roadeep took a tool's output for a request to save something, so the assistant never saw it. Ask again, or turn tools off for this chat.",
  "rerr.TOOL_STEP_LIMIT": "The assistant kept calling tools and didn't answer. Try a narrower question.",
  "rerr.TOOL_APPROVAL_TIMEOUT": "Nobody answered the tool's permission request, so the reply stopped.",
};
