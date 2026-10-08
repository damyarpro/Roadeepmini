// Persian strings for MCP tools in the island chat; see tools-en.ts. Same
// informal tone as the rest of the chat («بپرس»، «ببین»).

export const toolsFa: Record<string, string> = {
  "tools.chip": "ابزارها",
  "tools.chipCount": "ابزارها · {n}",
  "tools.chipOn": "ابزارهای MCP در این گفتگو روشن است. برای خاموش کردن کلیک کن.",
  "tools.chipOnCount": "ابزارهای MCP در این گفتگو روشن است ({n} ابزار). برای خاموش کردن کلیک کن.",
  "tools.chipOff": "ابزارهای MCP در این گفتگو خاموش است. برای روشن کردن کلیک کن.",
  "tools.turnedOn": "ابزارهای MCP برای این گفتگو روشن شد",
  "tools.turnedOff": "ابزارهای MCP برای این گفتگو خاموش شد",

  "tools.state.waiting": "منتظر تأیید تو",
  "tools.state.running": "در حال اجرا…",
  "tools.state.done": "انجام شد",
  "tools.state.error": "ناموفق",
  "tools.state.declined": "رد شد",
  "tools.state.stopped": "متوقف شد",
  "tools.arguments": "ورودی",
  "tools.result": "نتیجه",
  "tools.error": "خطا",
  "tools.noArguments": "بدون ورودی",
  "tools.details": "جزئیات {name}",
  "tools.approvalServer": "سرور",
  "tools.approvalTool": "ابزار",
  "tools.approvalArguments": "ورودی دقیقی که به ابزار داده می‌شود",
  "tools.creditHint": "هر مرحلهٔ ابزار یک پیام دیگر به رودیپ است و از اعتبارت هم کم می‌کند.",

  "rerr.TOOL_MEMORY_TRIGGER":
    "سرور رودیپ خروجی یک ابزار را درخواست نگه‌داشتن اطلاعات تشخیص داد و دستیار آن را ندید. دوباره بپرس یا ابزارها را برای این گفتگو خاموش کن.",
  "rerr.TOOL_STEP_LIMIT": "دستیار پشت سر هم ابزار صدا زد و پاسخی ننوشت. سؤال دقیق‌تری بپرس.",
  "rerr.TOOL_APPROVAL_TIMEOUT": "کسی به درخواست اجازهٔ ابزار پاسخ نداد، برای همین پاسخ متوقف شد.",
};
