// Persian strings for Settings → Shortcuts (the global shortcuts and the
// island's own keys). Registered through registerMessages by
// settings/shortcuts-section.ts.

export const r3Fa: Record<string, string> = {
  "shortcuts.title": "میان‌برها",
  "shortcuts.desc": "کلیدهایی که از هر برنامه‌ای کار می‌کنند و کلیدهایی که جزیرهٔ باز به آن‌ها پاسخ می‌دهد.",

  "shortcuts.globalHead": "از هر برنامه‌ای",
  "shortcuts.globalHint":
    "روی میان‌بر کلیک کن و کلیدهای تازه را بزن؛ Esc لغو می‌کند و Backspace خاموشش می‌کند. میان‌بری که روشن کنی، در برنامه‌های دیگری که همین کلیدها را دارند دیگر کار نمی‌کند.",
  "shortcuts.islandHead": "در جزیرهٔ باز",
  "shortcuts.islandHint": "وقتی صفحه‌کلید در اختیار جزیره است کار می‌کنند؛ مثلاً در گفتگو و برنامه‌ریز.",

  "shortcuts.action.openChat": "باز کردن گفتگو",
  "shortcuts.action.toggleIsland": "باز و بسته کردن جزیره",
  "shortcuts.action.goToAlert": "رفتن به اجازه یا پرسشِ منتظر",
  "shortcuts.action.jumpToTerminal": "رفتن به ترمینالِ جلسه",
  "shortcuts.action.nextPill": "مورد بعدی در جزیره",
  "shortcuts.action.prevPill": "مورد قبلی در جزیره",
  "shortcuts.action.muteToggle": "قطع و وصل صداها",

  "shortcuts.status.active": "فعال",
  "shortcuts.status.off": "خاموش",
  "shortcuts.status.inUse": "در اختیار برنامهٔ دیگری است",
  "shortcuts.status.duplicate": "در اختیار «{name}»",
  "shortcuts.status.invalid": "میان‌بر معتبری نیست",
  "shortcuts.status.typesCharacter": "«{char}» را تایپ می‌کند",
  "shortcuts.status.unavailable": "در دسترس نیست",
  "shortcuts.status.paused": "تا پایان ضبط متوقف است",

  "shortcuts.none": "هیچ",
  "shortcuts.recordLabel": "{name}: {combo}. برای تغییر کلیک کن.",
  "shortcuts.recording": "کلیدها را فشار بده…",
  "shortcuts.recordingHint": "Esc برای لغو · Backspace برای خاموش کردن",
  "shortcuts.saved": "میان‌بر ذخیره شد: {combo}",
  "shortcuts.turnedOff": "«{name}» خاموش شد",
  "shortcuts.needsModifier": "کلید را همراه Ctrl، Alt یا Win بزن.",
  "shortcuts.unsupportedKey": "این کلید نمی‌تواند بخشی از میان‌بر باشد.",
  "shortcuts.typesNote":
    "{combo} در یکی از چیدمان‌های صفحه‌کلیدت «{char}» را تایپ می‌کند، پس نمی‌تواند میان‌بر باشد. کلید دیگری انتخاب کن.",
  "shortcuts.clash": "{combo} الان برای «{name}» است. اول آن را عوض کن یا کلیدهای دیگری انتخاب کن.",
  "shortcuts.reset": "بازگرداندن پیش‌فرض‌ها",
  "shortcuts.resetDone": "میان‌برها به حالت پیش‌فرض برگشتند",

  "shortcuts.island.cycle": "مورد بعدی یا قبلی در جزیره",
  "shortcuts.island.byNumber": "رفتن به مورد ۱ تا ۹",
  "shortcuts.island.pin": "باز نگه داشتن جزیره",
  "shortcuts.island.settings": "باز کردن تنظیمات",
  "shortcuts.island.send": "فرستادن پیام",
  "shortcuts.island.close": "بستن جزیره",
  "shortcuts.or": "یا",
  "shortcuts.to": "تا",
};
