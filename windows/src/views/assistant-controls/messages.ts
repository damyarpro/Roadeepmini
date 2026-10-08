import { registerMessages, t } from "../../core/i18n";
const en = {
 "assistant.voice": "Voice", "assistant.computer": "Computer", "assistant.title": "Work together",
 "assistant.close":"Close panel",
 "assistant.loading":"Checking connection…", "assistant.loadFailed":"Could not check the connection.", "assistant.retry":"Retry", "assistant.saving":"Saving…", "assistant.removing":"Removing key…", "assistant.reenter":"The key input was cleared for security. Enter it again to retry.", "assistant.working":"Operation in progress…", "assistant.starting":"Starting computer…", "assistant.stopping":"Stopping computer…", "assistant.pausing":"Pausing computer…", "assistant.resuming":"Resuming computer…", "assistant.resetting":"Resetting workspace…", "assistant.taking":"Taking control…", "assistant.releasing":"Returning control…", "assistant.coordinateX":"Horizontal position (0–1023)", "assistant.coordinateY":"Vertical position (0–639)", "assistant.clickPosition":"Click position", "assistant.browserControls":"Keyboard & pointer controls",
 "assistant.setup": "Open setup", "assistant.voiceMissing": "Add your OpenAI API key under Settings → Live conversation to start talking.",
 "assistant.preview": "Preview only · no microphone, provider or computer is connected",
 "assistant.native": "Available in the Windows app. Open Settings to configure voice and computer access.",
 "assistant.refresh": "Refresh", "assistant.start": "Start computer", "assistant.pause": "Pause", "assistant.resume": "Resume", "assistant.stop": "Stop",
 "assistant.reset": "Reset workspace", "assistant.resetConfirm": "Delete this agent’s isolated workspace and browser session? This cannot be undone.",
 "assistant.take": "Take control", "assistant.release": "Return to agent", "assistant.owner": "You have control · agent tools wait", "assistant.agent": "Agent has control",
 "assistant.running": "Computer running", "assistant.stopped": "Computer stopped", "assistant.paused": "Computer paused", "assistant.unavailable": "Computer needs setup",
 "assistant.computerHint": "A separate workspace for this agent. Start it when you need browser, files or terminal tools.",
 "assistant.dockerMissing": "Start Docker Desktop, then build the computer image in Settings. Refresh when ready.",
 "assistant.browser": "Browser", "assistant.files": "Files", "assistant.terminal": "Terminal", "assistant.url": "Website URL", "assistant.go": "Open website",
 "assistant.screenshot": "Refresh screen", "assistant.type": "Text to type", "assistant.typeAction": "Type", "assistant.key": "Key", "assistant.sendKey": "Send key", "assistant.scrollUp": "Scroll up", "assistant.scrollDown": "Scroll down",
 "assistant.path": "Workspace path", "assistant.list": "List", "assistant.read": "Read", "assistant.write": "Save file", "assistant.contents": "File contents", "assistant.command": "Command in isolated computer", "assistant.run": "Run command", "assistant.output": "Result",
 "assistant.controlHint": "Take control to use the browser. Click its screen to interact.", "assistant.busy": "Wait for the current operation to finish.",
 "assistant.failed": "The operation failed. Refresh or check setup and try again.", "assistant.callFailed": "The live conversation could not connect. Check your microphone, API key and internet connection.", "assistant.voiceNetwork":"Could not reach OpenAI. Check your internet connection and try again.", "assistant.voiceQuota":"OpenAI refused the session because of rate or credit limits. Check your OpenAI account balance.", "assistant.voiceBusy":"A live conversation is already running.",
 "assistant.micDenied":"Microphone access was denied. Allow microphone access in Windows and try again.", "assistant.voiceRejected":"OpenAI rejected the API key. Check it under Settings → Live conversation.", "assistant.invalidInput":"Check the website, relative workspace path or command and try again.", "assistant.stopConfirm":"Stop this computer and delete its temporary files and browser session?", "assistant.runtimeError":"The computer runtime did not respond correctly. Stop it, check setup and restart.",
 "assistant.settingsTitle": "Voice & computer", "assistant.settingsDesc": "Talk with Roadeep live and give agents an isolated workspace.",
 "assistant.liveTitle": "Live conversation", "assistant.liveDesc": "Press the microphone in the island header and talk naturally. Roadeep answers by voice and can use the app for you; every change waits for your approval.",
 "assistant.model": "Thinking model", "assistant.voiceName": "Voice", "assistant.apiKey": "OpenAI API key", "assistant.apiKeyPlaceholder": "Leave blank to keep the saved key", "assistant.keyHint": "The key goes straight to Windows Credential Manager on this PC. It is never shown again, written to a file or sent anywhere except OpenAI.",
 "assistant.costHint": "Live conversation is billed to your OpenAI account: about $0.05 per minute, charged per second while a conversation is open. Each conversation ends on its own after 10 minutes.",
 "assistant.nameFixed": "The assistant is always called Roadeep; the voice only changes how it sounds.", "assistant.endFirst": "End the live conversation before changing these settings.", "assistant.save": "Save", "assistant.clearKey": "Remove key", "assistant.saved": "Live conversation settings saved", "assistant.keyRemoved": "OpenAI key removed", "assistant.keyPresent": "Ready · key saved", "assistant.keyAbsent": "Not set up · add your OpenAI API key",
 "assistant.computerSetup": "Computer setup", "assistant.buildHint": "Start Docker Desktop, then run this command in PowerShell once. The app never downloads or builds the runtime automatically.", "assistant.isolationHint": "Each agent gets a separate container and workspace. Your host files are not mounted. Computer tool actions follow chat approvals.",
};
const fa: Record<keyof typeof en, string> = {
 "assistant.voice": "صدا", "assistant.computer": "رایانه", "assistant.title": "با هم کار کنیم",
 "assistant.close":"بستن پنل",
 "assistant.loading":"در حال بررسی اتصال…", "assistant.loadFailed":"بررسی اتصال انجام نشد.", "assistant.retry":"تلاش دوباره", "assistant.saving":"در حال ذخیره…", "assistant.removing":"در حال حذف کلید…", "assistant.reenter":"کادر کلید برای امنیت پاک شد؛ برای تلاش دوباره آن را وارد کنید.", "assistant.working":"در حال انجام عملیات…", "assistant.starting":"در حال روشن کردن رایانه…", "assistant.stopping":"در حال خاموش کردن رایانه…", "assistant.pausing":"در حال توقف رایانه…", "assistant.resuming":"در حال ادامهٔ کار…", "assistant.resetting":"در حال پاک کردن محیط کار…", "assistant.taking":"در حال گرفتن کنترل…", "assistant.releasing":"در حال بازگرداندن کنترل…", "assistant.coordinateX":"موقعیت افقی (۰ تا ۱۰۲۳)", "assistant.coordinateY":"موقعیت عمودی (۰ تا ۶۳۹)", "assistant.clickPosition":"کلیک روی موقعیت", "assistant.browserControls":"کنترل صفحه‌کلید و اشاره‌گر",
 "assistant.setup": "تنظیمات اتصال", "assistant.voiceMissing": "برای شروع گفتگو، کلید API اوپن‌ای‌آی را در تنظیمات ← گفتگوی زنده وارد کنید.",
 "assistant.preview": "فقط پیش‌نمایش · میکروفون، سرویس صدا و رایانه متصل نیستند",
 "assistant.native": "در نسخهٔ ویندوز در دسترس است. اتصال صدا و رایانه را در تنظیمات انجام دهید.",
 "assistant.refresh": "تازه‌سازی", "assistant.start": "روشن کردن رایانه", "assistant.pause": "توقف موقت", "assistant.resume": "ادامه", "assistant.stop": "خاموش کردن",
 "assistant.reset": "پاک کردن محیط کار", "assistant.resetConfirm": "محیط کار و نشست مرورگر این ایجنت پاک شود؟ این کار قابل بازگشت نیست.",
 "assistant.take": "کنترل را بگیر", "assistant.release": "بازگرداندن به ایجنت", "assistant.owner": "کنترل با شماست؛ ابزارهای ایجنت منتظر می‌مانند", "assistant.agent": "کنترل با ایجنت است",
 "assistant.running": "رایانه روشن است", "assistant.stopped": "رایانه خاموش است", "assistant.paused": "رایانه متوقف است", "assistant.unavailable": "رایانه به راه‌اندازی نیاز دارد",
 "assistant.computerHint": "محیط کاری جدا برای این ایجنت؛ هنگام نیاز به مرورگر، فایل یا ترمینال آن را روشن کنید.",
 "assistant.dockerMissing": "Docker Desktop را روشن کنید و تصویر رایانه را طبق تنظیمات بسازید؛ سپس تازه‌سازی کنید.",
 "assistant.browser": "مرورگر", "assistant.files": "فایل‌ها", "assistant.terminal": "ترمینال", "assistant.url": "نشانی وب‌سایت", "assistant.go": "باز کردن وب‌سایت",
 "assistant.screenshot": "تازه‌سازی تصویر", "assistant.type": "متن برای تایپ", "assistant.typeAction": "تایپ", "assistant.key": "کلید", "assistant.sendKey": "فرستادن کلید", "assistant.scrollUp": "پیمایش بالا", "assistant.scrollDown": "پیمایش پایین",
 "assistant.path": "مسیر در محیط کار", "assistant.list": "فهرست", "assistant.read": "خواندن", "assistant.write": "ذخیرهٔ فایل", "assistant.contents": "محتوای فایل", "assistant.command": "دستور در رایانهٔ جدا", "assistant.run": "اجرای دستور", "assistant.output": "نتیجه",
 "assistant.controlHint": "برای کار با مرورگر کنترل را بگیرید؛ سپس روی تصویر کلیک کنید.", "assistant.busy": "تا پایان عملیات فعلی صبر کنید.",
 "assistant.failed": "عملیات انجام نشد. اتصال را تازه‌سازی کنید یا تنظیمات را بررسی کنید.", "assistant.callFailed": "گفتگوی زنده وصل نشد. میکروفون، کلید API و اتصال اینترنت را بررسی کنید.", "assistant.voiceNetwork":"اتصال به OpenAI برقرار نشد. اینترنت را بررسی کنید و دوباره تلاش کنید.", "assistant.voiceQuota":"OpenAI به‌دلیل محدودیت درخواست یا اعتبار، گفتگو را نپذیرفت. موجودی حساب OpenAI را بررسی کنید.", "assistant.voiceBusy":"یک گفتگوی زنده در حال اجراست.",
 "assistant.micDenied":"دسترسی میکروفون رد شد. آن را در تنظیمات ویندوز فعال کنید و دوباره تلاش کنید.", "assistant.voiceRejected":"OpenAI کلید API را نپذیرفت. آن را در تنظیمات ← گفتگوی زنده بررسی کنید.", "assistant.invalidInput":"نشانی وب‌سایت، مسیر نسبی محیط کار یا دستور را بررسی کنید و دوباره تلاش کنید.", "assistant.stopConfirm":"رایانه خاموش و فایل‌های موقت و نشست مرورگر آن پاک شود؟", "assistant.runtimeError":"محیط رایانه درست پاسخ نداد. آن را خاموش کنید، تنظیمات را بررسی کنید و دوباره روشن کنید.",
 "assistant.settingsTitle": "صدا و رایانه", "assistant.settingsDesc": "با رودیپ زنده گفتگو کنید و به ایجنت‌ها محیط کاری جدا بدهید.",
 "assistant.liveTitle": "گفتگوی زنده", "assistant.liveDesc": "میکروفون بالای جزیره را بزنید و طبیعی صحبت کنید. رودیپ با صدا جواب می‌دهد و می‌تواند برایتان با اپ کار کند؛ هر تغییری منتظر تأیید شما می‌ماند.",
 "assistant.model": "مدل فکرکردن", "assistant.voiceName": "صدا", "assistant.apiKey": "کلید API اوپن‌ای‌آی", "assistant.apiKeyPlaceholder": "برای حفظ کلید ذخیره‌شده خالی بگذارید", "assistant.keyHint": "کلید مستقیم در Credential Manager ویندوز روی همین سیستم ذخیره می‌شود؛ دیگر نمایش داده نمی‌شود، در فایل نوشته نمی‌شود و جز به OpenAI به جایی فرستاده نمی‌شود.",
 "assistant.costHint": "هزینهٔ گفتگوی زنده از حساب OpenAI شما کم می‌شود: حدود ۰٫۰۵ دلار برای هر دقیقه، به‌صورت ثانیه‌ای و فقط وقتی گفتگو باز است. هر گفتگو پس از ۱۰ دقیقه خودکار تمام می‌شود.",
 "assistant.nameFixed": "نام دستیار همیشه رودیپ است؛ صدا فقط لحن او را تغییر می‌دهد.", "assistant.endFirst": "پیش از تغییر این تنظیمات، گفتگوی زنده را تمام کنید.", "assistant.save": "ذخیره", "assistant.clearKey": "حذف کلید", "assistant.saved": "تنظیمات گفتگوی زنده ذخیره شد", "assistant.keyRemoved": "کلید OpenAI حذف شد", "assistant.keyPresent": "آماده · کلید ذخیره شده", "assistant.keyAbsent": "راه‌اندازی نشده · کلید API اوپن‌ای‌آی را وارد کنید",
 "assistant.computerSetup": "راه‌اندازی رایانه", "assistant.buildHint": "Docker Desktop را روشن کنید و این دستور را یک بار در PowerShell اجرا کنید. برنامه محیط رایانه را خودکار دانلود یا ایجاد نمی‌کند.", "assistant.isolationHint": "هر ایجنت کانتینر و محیط کاری جدا دارد؛ فایل‌های رایانهٔ شما به آن متصل نمی‌شوند. ابزارهای رایانه از تأییدهای گفت‌وگو پیروی می‌کنند.",
};
registerMessages(en, fa);
export function assistantError(error: unknown): string {
 const code = String(error);
 if (code.includes("ASSISTANT_NATIVE_REQUIRED")) return t("assistant.native");
 if (/DOCKER|IMAGE|COMPUTER_UNAVAILABLE/i.test(code)) return t("assistant.dockerMissing");
 if (/VOICE_KEY|NOT_CONFIGURED|Configure the voice API key/i.test(code)) return t("assistant.voiceMissing");
 if (/computer-busy/i.test(code)) return t("assistant.busy");
 if (/computer-invalid-operation/i.test(code)) return t("assistant.invalidInput");
 if (/computer-runtime|computer-operation-failed/i.test(code)) return t("assistant.runtimeError");
 return t("assistant.failed");
}
/** Live voice failures (native gateway strings, getUserMedia/WebRTC names) as one actionable sentence. */
export function voiceError(error:unknown):string {
 const text=error instanceof Error?`${error.name} ${error.message}`:String(error);
 if(/NotAllowed|Permission|denied/i.test(text))return t("assistant.micDenied");
 if(/Configure the voice API key|not-configured|NOT_CONFIGURED/i.test(text))return t("assistant.voiceMissing");
 if(/HTTP (401|403)/.test(text))return t("assistant.voiceRejected");
 if(/HTTP 429/.test(text))return t("assistant.voiceQuota");
 if(/Cannot connect to voice provider|network|offline/i.test(text))return t("assistant.voiceNetwork");
 if(/Voice already active/i.test(text))return t("assistant.voiceBusy");
 if(/ASSISTANT_NATIVE_REQUIRED/.test(text))return t("assistant.native");
 return t("assistant.callFailed");
}
