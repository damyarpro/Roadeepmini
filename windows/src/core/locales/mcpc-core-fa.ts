// Persian strings for the E_MCPC_* codes of the MCP client core; see
// mcpc-core-en.ts.

export const mcpcCoreFa: Record<string, string> = {
  "err.mcpc.store": "فهرست سرورهای MCP خوانده یا ذخیره نشد: {detail}",
  "err.mcpc.unknownServer": "این سرور دیگر وجود ندارد.",
  "err.mcpc.invalid": "«{field}» را بررسی کن — این‌جا معتبر نیست.",
  "err.mcpc.limit": "به سقف مجاز رسیده‌ای ({limit}).",
  "err.mcpc.needsAuth": "سرور به توکن یا ورود نیاز دارد.",
  "err.mcpc.needsApproval": "پیش از اجرا، فرمان را تأیید کن.",
  "err.mcpc.disabled": "سرور خاموش است.",
  "err.mcpc.commandNotFound": "{command} پیدا نشد. نصبش کن (یا PATH را بررسی کن) و رودیپ را دوباره باز کن.",
  "err.mcpc.unsafeArg": "این آرگومان را نمی‌شود با اطمینان به فرمان داد: {arg}",
  "err.mcpc.spawn": "فرمان اجرا نشد: {detail}",
  "err.mcpc.exited": "سرور متوقف شد.",
  "err.mcpc.timeout": "سرور به‌موقع پاسخ نداد.",
  "err.mcpc.http": "سرور با خطا پاسخ داد ({status}).",
  "err.mcpc.network": "به سرور وصل نشد: {detail}",
  "err.mcpc.tooLarge": "پاسخ سرور بیش از حد بزرگ است.",
  "err.mcpc.protocol": "پاسخ سرور قابل فهم نبود: {detail}",
  "err.mcpc.rpc": "سرور خطا گزارش داد ({code}): {message}",
  "err.mcpc.unknownTool": "هیچ سرور متصلی ابزار {tool} را ندارد.",
  "err.mcpc.toolOff": "ابزار {tool} خاموش است.",
  "err.mcpc.toolAsk": "ابزار {tool} حالا پیش از اجرا اجازه می‌خواهد و تأیید نشده بود.",
  "err.mcpc.argsTooLarge": "ورودی ابزار بلندتر از آن است که کامل به تو نشان داده شود (بیش از {limit} نویسه)، برای همین اجرا نشد.",
  "err.mcpc.commandChanged": "فرمان پس از نمایش تغییر کرده است. دوباره بررسی‌اش کن و بعد تأیید کن.",
};
