// Persian strings for the Updates row (Settings → General, settings/update-row.ts)
// and the E_UPDATE_* codes from src-tauri/src/updater.rs. Registered through
// registerMessages by core/error-text.ts.

export const updateFa: Record<string, string> = {
  "update.title": "به‌روزرسانی",
  "update.version": "نسخهٔ {version}",
  "update.check": "بررسی به‌روزرسانی",
  "update.checking": "در حال بررسی…",
  "update.upToDate": "به‌روز است",
  "update.lastChecked": "آخرین بررسی: {time}",
  "update.available": "نسخهٔ {version} آماده است",
  "update.notes": "تازه‌ها",
  "update.install": "نصب و راه‌اندازی دوباره",
  "update.installHint": "رودیپ بسته می‌شود، به‌روزرسانی نصب می‌شود و دوباره باز می‌شود.",
  "update.downloading": "در حال دریافت… {percent}",
  "update.downloadingNoSize": "در حال دریافت…",
  "update.installing": "در حال اجرای نصب‌کننده…",
  "update.retry": "تلاش دوباره",
  "update.autoCheck": "بررسی خودکار",
  "update.autoCheckHint": "روزی یک بار. بدون کلیک تو چیزی دریافت یا نصب نمی‌شود.",
  "update.disabled": "به‌روزرسانی خودکار در این نسخه فعال نیست",

  "err.update.disabled": "به‌روزرسانی خودکار در این نسخه فعال نیست.",
  "err.update.nothing": "به‌روزرسانی‌ای برای نصب نیست. دوباره بررسی کن.",
  "err.update.busy": "بررسی یا دریافت به‌روزرسانی از قبل در جریان است.",
  "err.update.network": "به سرور به‌روزرسانی وصل نشد. اتصال یا پروکسی را بررسی کن.",
  "err.update.timeout": "سرور به‌روزرسانی دیر جواب داد.",
  "err.update.noRelease": "نسخهٔ منتشرشده‌ای پیدا نشد.",
  "err.update.manifest": "اطلاعات نسخهٔ جدید خوانده نشد.",
  "err.update.signature": "فایل دریافت‌شده از بررسی امضا رد شد و کنار گذاشته شد.",
  "err.update.install": "نصب‌کننده اجرا نشد.",
  "err.update.failed": "به‌روزرسانی انجام نشد.",
};
