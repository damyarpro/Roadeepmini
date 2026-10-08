// Persian strings for the global shortcut (Settings → General) and for the
// error codes the Rust side sends (src-tauri/src/errors.rs, read by
// core/error-text.ts). Merged into the main `fa` dictionary by the orchestrator.

export const miscFa: Record<string, string> = {
  "misc.shortcut": "میان‌بر گفتگو",
  "misc.shortcutHint": "گفتگوی جزیره را از هر برنامه‌ای باز می‌کند.",
  "misc.shortcutNone": "خاموش",
  "misc.shortcutRecord": "ثبت میان‌بر تازه",
  "misc.shortcutRecording": "کلیدها را فشار بده…",
  "misc.shortcutRecordingHint": "Esc برای لغو · Backspace برای خاموش کردن",
  "misc.shortcutLabel": "میان‌بر گفتگو: {combo}. برای تغییر کلیک کن.",
  "misc.shortcutActive": "فعال",
  "misc.shortcutOff": "خاموش",
  "misc.shortcutTaken": "در دسترس نیست — برنامهٔ دیگری آن را گرفته",
  "misc.shortcutSaved": "میان‌بر ذخیره شد: {combo}",
  "misc.shortcutCleared": "میان‌بر خاموش شد",
  "misc.keyWin": "Win",

  "err.int.invalidKey": "کلید API نامعتبر است (خطای ۴۰۱)",
  "err.int.http": "خطای API ({status})",
  "err.int.stripeSecretKey": "از کلید محرمانه استفاده کن (sk_live_…، نه pk_live_…)",
  "err.int.tokenScope": "توکن دسترسی‌های لازم را ندارد",
  "err.int.tokenAccess": "توکن دسترسی ندارد",
  "err.int.keyAccess": "کلید دسترسی ندارد",
  "err.int.notionAccess": "یکپارچه‌سازی دسترسی ندارد — در Notion یک صفحه را با آن به اشتراک بگذار",
  "err.int.noConnection": "اتصال برقرار نشد: {detail}",
  "err.int.badField": "«{field}» را بررسی کن — درست به نظر نمی‌رسد",
  "err.int.notFound": "پیدا نشد (۴۰۴) — شناسه‌ها و آدرس‌هایی را که وارد کرده‌ای بررسی کن",
  "err.int.rateLimited": "درخواست‌ها زیاد شده (۴۲۹). کمی بعد دوباره امتحان می‌شود.",
  "err.int.server": "سرویس به مشکل خورده ({status})",
  "err.int.tooLarge": "پاسخ سرویس برای خواندن خیلی بزرگ بود",
  "err.int.badResponse": "پاسخ سرویس شکل غیرمنتظره‌ای داشت",
  "err.int.hostBlocked": "جلویش گرفته شد: درخواست به آدرسی می‌رفت که این سرویس از آن استفاده نمی‌کند",
  "err.n8n.items": "← {node} · {count} مورد",

  "err.file.notDropped": "فایل را دوباره روی جزیره رها کن.",
  "err.file.isFolder": "فعلاً نمی‌شود پوشه رها کرد.",
  "err.file.unreadable": "خواندن {path} ممکن نشد: {detail}",
  "err.file.copy": "کپی فایل ممکن نشد: {detail}",

  "err.cfg.unreadable": "خواندن {path} ممکن نشد: {detail}",
  "err.cfg.notObject": "{path} یک شیء JSON نیست — رودیپ به آن دست نمی‌زند.",
  "err.cfg.invalidJson": "{path} یک JSON معتبر نیست ({detail}). درستش کن یا جابه‌جایش کن و دوباره امتحان کن — رودیپ آن را بازنویسی نمی‌کند.",
  "err.cfg.changed": "{path} بعد از پیش‌نمایش تغییر کرده است. چیزی نوشته نشد — تفاوت‌های تازه را مرور کن.",
  "err.cfg.backup": "گرفتن نسخهٔ پشتیبان ممکن نشد: {detail}",
  "err.cfg.write": "نوشتن ممکن نشد: {detail}",
  "err.cfg.noChange": "تغییری نیست.",
  "err.mcp.exeMissing": "{file} هنوز سر جایش نیست. رودیپ را دوباره اجرا کن و باز امتحان کن.",

  "err.shortcut.invalid": "این میان‌بری نیست که ویندوز بتواند از آن استفاده کند.",
  "err.shortcut.noModifier": "کلید را همراه Ctrl، Alt یا Win بزن.",
  "err.shortcut.taken": "در دسترس نیست — برنامهٔ دیگری آن را گرفته",
};
