// Persian strings for the MCP server sign-in errors; see mcpc-oauth-en.ts.

export const mcpcOauthFa: Record<string, string> = {
  "err.mcpc.oauth.discovery": "روش ورود این سرور پیدا نشد ({detail}).",
  "err.mcpc.oauth.insecure": "صفحه‌ی ورود این سرور روی {host} امن (https) نیست، پس استفاده نشد.",
  "err.mcpc.oauth.noDcr": "این سرویس اجازه نمی‌دهد برنامه‌ها خودشان وارد شوند. در خود سرویس یک توکن بسازید و احراز هویت سرور را روی توکن بگذارید.",
  "err.mcpc.oauth.register": "سرویس ثبت Roadeep را برای ورود نپذیرفت ({detail}).",
  "err.mcpc.oauth.listen": "پورت محلی برای پاسخ مرورگر باز نشد ({detail}).",
  "err.mcpc.oauth.denied": "ورود در مرورگر رد شد ({detail}).",
  "err.mcpc.oauth.state": "پاسخ مرورگر به این ورود تعلق نداشت و نادیده گرفته شد. دوباره امتحان کنید.",
  "err.mcpc.oauth.timeout": "ورود در مرورگر تا ۵ دقیقه کامل نشد. دوباره امتحان کنید.",
  "err.mcpc.oauth.cancelled": "ورود متوقف شد.",
  "err.mcpc.oauth.token": "سرویس پس از ورود دسترسی را تحویل نداد ({detail}).",
  "err.mcpc.oauth.keyring": "ورود در Credential Manager ویندوز ذخیره نشد ({detail}).",
};
