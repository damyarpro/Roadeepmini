import {registerMessages,t} from "../core/i18n";
registerMessages({
 "local.apiModelHint":"Your selected chat model is used for requests sent to the Roadeep API.", "local.preparing":"Waiting for microphone access…", "local.title":"Roadeep’s local brain", "local.description":"Small requests stay on this PC. Complex work uses your selected Roadeep API model.",
 "local.install":"Download and install", "local.downloadHint":"About 1.67 GB. Includes the brain, speech recognition, admin speaker recognition and Persian voice. No separate app or API key needed.",
 "local.ready":"Installed and verified", "local.enabled":"Local brain enabled", "local.disabled":"Local brain disabled", "local.notInstalled":"Not installed", "local.cancel":"Cancel download", "local.retry":"Retry installation", "local.enabling":"Enabling local brain…", "local.disabling":"Disabling local brain…", "local.cancelling":"Cancelling download…", "local.startingInstall":"Starting download…", "local.enable":"Enable local brain", "local.disable":"Disable local brain", "local.cancelled":"Download cancelled", "local.statusError":"Could not check the local installation. Try checking again.", "local.loadingStatus":"Checking the local installation…", "local.reloadStatus":"Check again", "local.toggleError":"Could not change the local brain setting. Its previous state was kept.", "local.cancelError":"Cancellation could not be confirmed. Try again.", "local.error":"Installation failed. Retry when the connection and free disk space are available.",
 "local.downloading":"Downloading", "local.verifying":"Verifying downloaded files", "local.installing":"Installing", "local.sources":"Models, sources and licenses", "local.app":"App message", "local.offline":"Local · on this PC", "local.cloud":"Roadeep · API", "local.cpu":"Local · CPU fallback", "local.fallback":"Local processing is unavailable; continuing with the Roadeep API.", "local.cloudReason":"This request needs the Roadeep API.", "local.signIn":"Sign in to continue this complex request with Roadeep. Simple requests work locally.",
 "local.voice":"Local voice", "local.voiceHint":"Record a short message, then send it. Recognition and Persian speech run on this PC.", "local.record":"Record message", "local.sendRecording":"Stop and send", "local.stop":"Stop", "local.idle":"Ready to record · up to 30 seconds", "local.recording":"Recording · stop when finished", "local.transcribing":"Recognizing speech on this PC…", "local.thinking":"Preparing your answer…", "local.speaking":"Speaking…", "local.voiceError":"Local voice could not finish. Try again.", "local.micDenied":"Microphone access was denied. Enable access in Windows privacy settings.", "local.silence":"No clear speech was detected. Try again in a quiet place.", "local.tooLong":"Keep the recording under 30 seconds.", "local.setup":"Set up the local brain", "local.remoteVoice":"Advanced: cloud voice", "local.remoteVoiceHint":"Real-time voice uses the configured OpenAI service and requires its API key.", "local.preview":"Preview only · no model download, microphone or inference is active.", "local.component.brain":"Brain model", "local.component.speech-recognition":"Speech recognition", "local.component.persian-voice":"Persian voice", "local.component.voice-config":"Voice configuration", "local.component.voice-license":"Voice license", "local.component.llama-vulkan":"GPU engine", "local.component.llama-cpu":"CPU fallback engine", "local.component.whisper":"Recognition engine", "local.component.piper":"Speech engine",
},{
 "local.apiModelHint":"مدل انتخاب‌شدهٔ چت برای درخواست‌های API رودیپ استفاده می‌شود.", "local.preparing":"در انتظار دسترسی به میکروفون…", "local.title":"مغز داخلی رودیپ", "local.description":"درخواست‌های کوچک روی همین سیستم پردازش می‌شوند. کارهای پیچیده به مدل انتخاب‌شدهٔ API رودیپ می‌رسند.",
 "local.install":"دانلود و نصب", "local.downloadHint":"حدود ۱٫۶۷ گیگابایت؛ شامل مغز، تشخیص گفتار، تشخیص صدای ادمین و صدای فارسی. بدون اپ جداگانه یا کلید API.",
 "local.ready":"نصب و صحت فایل‌ها تأیید شد", "local.enabled":"مغز داخلی روشن است", "local.disabled":"مغز داخلی خاموش است", "local.notInstalled":"هنوز نصب نشده", "local.cancel":"لغو دانلود", "local.retry":"تلاش دوباره برای نصب", "local.enabling":"در حال روشن‌کردن مغز داخلی…", "local.disabling":"در حال خاموش‌کردن مغز داخلی…", "local.cancelling":"در حال لغو دانلود…", "local.startingInstall":"آغاز دانلود…", "local.enable":"روشن‌کردن مغز داخلی", "local.disable":"خاموش‌کردن مغز داخلی", "local.cancelled":"دانلود لغو شد", "local.statusError":"بررسی وضعیت نصب داخلی انجام نشد. دوباره بررسی کن.", "local.loadingStatus":"در حال بررسی نصب داخلی…", "local.reloadStatus":"بررسی دوباره", "local.toggleError":"تنظیم مغز داخلی تغییر نکرد. وضعیت قبلی حفظ شد.", "local.cancelError":"لغو دانلود تأیید نشد. دوباره تلاش کن.", "local.error":"نصب انجام نشد. اتصال اینترنت و فضای دیسک را بررسی و دوباره تلاش کن.",
 "local.downloading":"در حال دانلود", "local.verifying":"بررسی صحت فایل‌ها", "local.installing":"در حال نصب", "local.sources":"مدل‌ها، منابع و مجوزها", "local.app":"پیام اپ", "local.offline":"داخلی · همین سیستم", "local.cloud":"رودیپ · API", "local.cpu":"داخلی · پردازندهٔ مرکزی", "local.fallback":"پردازش داخلی در دسترس نیست؛ ادامه با API رودیپ.", "local.cloudReason":"این درخواست به API رودیپ نیاز دارد.", "local.signIn":"برای ادامهٔ این درخواست پیچیده وارد رودیپ شو. درخواست‌های ساده به‌صورت داخلی انجام می‌شوند.",
 "local.voice":"صدای داخلی", "local.voiceHint":"پیامت را ضبط کن و سپس بفرست. تشخیص گفتار و صدای فارسی روی همین سیستم اجرا می‌شوند.", "local.record":"ضبط پیام", "local.sendRecording":"پایان ضبط و ارسال", "local.stop":"توقف", "local.idle":"آمادهٔ ضبط · حداکثر ۳۰ ثانیه", "local.recording":"در حال ضبط · پس از صحبت پایان بده", "local.transcribing":"تشخیص گفتار روی همین سیستم…", "local.thinking":"آماده‌کردن پاسخ…", "local.speaking":"در حال صحبت…", "local.voiceError":"گفت‌وگوی صوتی داخلی کامل نشد. دوباره تلاش کن.", "local.micDenied":"دسترسی به میکروفون رد شد. آن را در تنظیمات حریم خصوصی ویندوز فعال کن.", "local.silence":"گفتار واضحی تشخیص داده نشد. در محیط آرام دوباره تلاش کن.", "local.tooLong":"زمان ضبط را کمتر از ۳۰ ثانیه نگه دار.", "local.setup":"راه‌اندازی مغز داخلی", "local.remoteVoice":"پیشرفته: صدای ابری", "local.remoteVoiceHint":"مکالمهٔ بلادرنگ از سرویس OpenAI تنظیم‌شده استفاده می‌کند و کلید API می‌خواهد.", "local.preview":"فقط پیش‌نمایش · دانلود مدل، میکروفون و پردازش فعال نیستند.", "local.component.brain":"مدل مغز", "local.component.speech-recognition":"تشخیص گفتار", "local.component.persian-voice":"صدای فارسی", "local.component.voice-config":"تنظیمات صدا", "local.component.voice-license":"مجوز صدا", "local.component.llama-vulkan":"موتور گرافیکی", "local.component.llama-cpu":"موتور جایگزین پردازنده", "local.component.whisper":"موتور تشخیص گفتار", "local.component.piper":"موتور تولید صدا",
});
export function localError(error:unknown):string{
 if(String(error).includes("local-mic-no-data")||String(error).includes("local-mic-start-timeout"))return t("local.inputUnavailable");
 const text=error instanceof Error?`${error.name} ${error.message}`:String(error);
 if(text.includes("local-native-required"))return t("adminVoice.installedRequired");
 if(text.includes("NotAllowedError")||text.includes("PermissionDenied"))return t("local.micDenied");
 if(text.includes("local-silence"))return t("local.silence");
 if(text.includes("local-duration"))return t("local.tooLong");
 if(text.includes("speaker-enrollment-inconsistent"))return t("adminVoice.enrollmentInconsistent");
 if(text.includes("speaker-enrollment-too-short")||text.includes("speaker-too-short"))return t("adminVoice.enrollmentTooShort");
 if(text.includes("speaker-enrollment-insufficient")||text.includes("speaker-insufficient")||text.includes("speaker-no-speech"))return t("adminVoice.enrollmentInsufficient");
 if(text.includes("speaker-enrollment"))return t("adminVoice.enrollmentError");
 if(text.includes("speaker-profile"))return t("adminVoice.missing");
 if(text.includes("local-disabled")||text.includes("local-runtime"))return t("local.setup");
 return t("local.voiceError");
}
registerMessages({"local.inputLevel":"Microphone input level","local.inputWaiting":"Waiting for microphone sound…","local.inputQuiet":"Input is quiet · speak closer to the microphone","local.inputDetected":"Microphone sound received","local.inputPercent":"Input level: {value}%","local.inputUnavailable":"No microphone audio arrived. Check your input device and Windows microphone access, then try again."},{"local.inputLevel":"شدت صدای میکروفون","local.inputWaiting":"منتظر صدای میکروفون…","local.inputQuiet":"صدا کم است؛ نزدیک‌تر به میکروفون صحبت کن","local.inputDetected":"صدای میکروفون دریافت می‌شود","local.inputPercent":"شدت صدا: {value} درصد","local.inputUnavailable":"صدایی از میکروفون دریافت نشد. ورودی صدا و دسترسی میکروفون در ویندوز را بررسی کن و دوباره ضبط کن."});

registerMessages({
 "adminVoice.title":"Your voice", "adminVoice.description":"Turn on the microphone on the home screen. Roadeep listens across the app and answers only the registered admin voice.", "adminVoice.enrollmentHint":"Read a few complete sentences for 12 seconds in a quiet room.", "adminVoice.enroll":"Register my voice", "adminVoice.rerecord":"Record my voice again", "adminVoice.remove":"Delete voice profile", "adminVoice.registered":"Admin voice registered", "adminVoice.missing":"Register your voice before enabling listening", "adminVoice.enrolling":"Speak naturally · recording for 12 seconds…", "adminVoice.saving":"Checking voice profile…", "adminVoice.turnOn":"Turn on admin voice listening", "adminVoice.turnOff":"Turn off voice listening", "adminVoice.preparing":"Preparing voice listening…", "adminVoice.recording":"Listening for the admin voice", "adminVoice.verifying":"Checking the speaker…", "adminVoice.transcribing":"Recognizing your message…", "adminVoice.thinking":"Preparing your answer…", "adminVoice.speaking":"Speaking · microphone paused", "adminVoice.idle":"Voice listening enabled", "adminVoice.homeHint":"Use the microphone button on the home screen for local admin voice listening."
},{
 "adminVoice.title":"صدای شما", "adminVoice.description":"میکروفون صفحهٔ اصلی را روشن کن. رودیپ در همهٔ بخش‌های اپ گوش می‌دهد و فقط به صدای ادمین ثبت‌شده پاسخ می‌دهد.", "adminVoice.enrollmentHint":"در محیط آرام، ۱۲ ثانیه چند جملهٔ کامل بخوان.", "adminVoice.enroll":"ثبت صدای من", "adminVoice.rerecord":"ثبت دوبارهٔ صدای من", "adminVoice.remove":"حذف پروفایل صدا", "adminVoice.registered":"صدای ادمین ثبت شده", "adminVoice.missing":"پیش از روشن‌کردن میکروفون، صدایت را ثبت کن", "adminVoice.enrolling":"طبیعی صحبت کن · ضبط به‌مدت ۱۲ ثانیه…", "adminVoice.saving":"در حال بررسی پروفایل صدا…", "adminVoice.turnOn":"روشن‌کردن گوش‌دادن به صدای ادمین", "adminVoice.turnOff":"خاموش‌کردن گوش‌دادن به صدا", "adminVoice.preparing":"آماده‌کردن مکالمهٔ صوتی…", "adminVoice.recording":"در حال گوش‌دادن به صدای ادمین", "adminVoice.verifying":"بررسی گوینده…", "adminVoice.transcribing":"تشخیص پیام شما…", "adminVoice.thinking":"آماده‌کردن پاسخ…", "adminVoice.speaking":"در حال پاسخ · میکروفون متوقف است", "adminVoice.idle":"گوش‌دادن به صدا روشن است", "adminVoice.homeHint":"برای مکالمهٔ داخلی با صدای ادمین، از دکمهٔ میکروفون صفحهٔ اصلی استفاده کن"
});

registerMessages({"adminVoice.paused":"Listening paused while registering your voice"},{"adminVoice.paused":"گوش‌دادن هنگام ثبت صدای شما متوقف است"});

registerMessages({"adminVoice.cancelEnrollment":"Cancel recording", "adminVoice.recordingProgress":"Voice registration recording progress", "adminVoice.recordedSeconds":"seconds recorded"},{"adminVoice.cancelEnrollment":"لغو ضبط", "adminVoice.recordingProgress":"پیشرفت ضبط برای ثبت صدا", "adminVoice.recordedSeconds":"ثانیه ضبط شده"});

registerMessages({"adminVoice.pausedShort":"Listening paused", "adminVoice.errorShort":"Voice stopped", "adminVoice.short.idle":"Listening", "adminVoice.short.preparing":"Preparing…", "adminVoice.short.recording":"Listening", "adminVoice.short.verifying":"Checking voice…", "adminVoice.short.transcribing":"Recognizing…", "adminVoice.short.thinking":"Thinking…", "adminVoice.short.speaking":"Speaking…"},{"adminVoice.pausedShort":"گوش‌دادن متوقف است", "adminVoice.errorShort":"صدا متوقف شد", "adminVoice.short.idle":"در حال گوش‌دادن", "adminVoice.short.preparing":"آماده‌سازی…", "adminVoice.short.recording":"در حال گوش‌دادن", "adminVoice.short.verifying":"بررسی صدا…", "adminVoice.short.transcribing":"تشخیص گفتار…", "adminVoice.short.thinking":"فکر می‌کنم…", "adminVoice.short.speaking":"در حال پاسخ…"});

registerMessages({"adminVoice.enrollmentError":"Voice registration could not be verified. Read several complete sentences for 12 seconds in a quiet room and try again."},{"adminVoice.enrollmentError":"ثبت صدا تأیید نشد. در محیط آرام ۱۲ ثانیه چند جملهٔ کامل بخوان و دوباره تلاش کن."});

registerMessages({"adminVoice.installedRequired":"Run the installed app"},{"adminVoice.installedRequired":"نسخهٔ نصب‌شده را اجرا کن"});

registerMessages({"adminVoice.recordingTime":"Recording time","adminVoice.recordedCount":"{value} of 12 seconds","adminVoice.privacy":"Your voice profile stays on this PC; the recording is discarded."},{"adminVoice.recordingTime":"زمان ضبط","adminVoice.recordedCount":"{value} از ۱۲ ثانیه","adminVoice.privacy":"پروفایل صدا روی همین سیستم می‌ماند و فایل ضبط‌شده حذف می‌شود."});

registerMessages({
 "adminVoice.enrollmentInconsistent":"The recorded sections did not produce a consistent voice profile. Try again in a quiet room, speaking continuously at a steady distance from the microphone.",
 "adminVoice.enrollmentInsufficient":"Not enough clear speech was received to register your voice. Move closer to the microphone, read several complete sentences, and try again.",
 "adminVoice.enrollmentTooShort":"The recording was too short to register your voice. Let the full 12-second recording finish, then try again."
},{
 "adminVoice.enrollmentInconsistent":"نمونهٔ صدا یکدست نبود. در محیط آرام، با فاصلهٔ ثابت از میکروفون چند جملهٔ کامل بخوان و دوباره ضبط کن.",
 "adminVoice.enrollmentInsufficient":"گفتار کافی و واضح برای ثبت صدا دریافت نشد. نزدیک‌تر به میکروفون، چند جملهٔ کامل بخوان و دوباره ضبط کن.",
 "adminVoice.enrollmentTooShort":"ضبط برای ثبت صدا کوتاه بود. اجازه بده ضبط ۱۲ ثانیه کامل شود و دوباره تلاش کن."
});

registerMessages({"local.desktopChecking":"Checking the requested action…","local.desktopServer":"This PC","local.desktopFailed":"The app could not open the requested window. Try again.","local.voiceSensitiveBlocked":"This action is sensitive and cannot be run by voice. It was blocked.","local.voiceTaskFailed":"The task could not be completed. Try again or check the chat for details."},{"local.desktopChecking":"بررسی کار درخواستی…","local.desktopServer":"این سیستم","local.desktopFailed":"اپ نتوانست پنجرهٔ درخواستی را باز کند. دوباره تلاش کن.","local.voiceSensitiveBlocked":"این کار حساس است و با فرمان صوتی انجام نمی‌شود. اجرای آن مسدود شد.","local.voiceTaskFailed":"کار درخواستی انجام نشد. دوباره تلاش کن یا جزئیات چت را بررسی کن."});

registerMessages({"adminVoice.modeTitle":"Voice activation","adminVoice.mode.manual":"Manual","adminVoice.mode.always":"Always ready","adminVoice.modeManualHint":"Press the microphone for one request.","adminVoice.modeAlwaysHint":"Listening starts with the app. The microphone button pauses it.","adminVoice.modeHelp":"Manual listens for one utterance and stops after the response. Always ready listens while the app is running, analyzes the registered admin's speech, and answers addressed requests. Tasks and notes are saved only after your confirmation in both modes.","adminVoice.modeError":"The voice mode could not be saved. Your previous choice was kept.","adminVoice.modeSaving":"Saving voice mode…","adminVoice.proposalTitle":"Voice proposal","adminVoice.proposalTask":"Proposed task","adminVoice.proposalNote":"Proposed note","adminVoice.proposalHint":"Save this proposal? Nothing has been saved yet.","adminVoice.proposalAccept":"Save","adminVoice.proposalReject":"Reject","adminVoice.proposalSaving":"Saving…","adminVoice.proposalError":"Could not save the decision. Check the proposal and try again.","adminVoice.proposalExpired":"This proposal is no longer pending. Speak again to create a new proposal.","adminVoice.proposalReady":"Proposal ready"},{"adminVoice.modeTitle":"فعال‌سازی صدا","adminVoice.mode.manual":"دستی","adminVoice.mode.always":"همیشه آماده","adminVoice.modeManualHint":"برای یک درخواست، دکمهٔ میکروفون را بزن.","adminVoice.modeAlwaysHint":"با اجرای اپ گوش‌دادن شروع می‌شود؛ دکمهٔ میکروفون آن را متوقف می‌کند.","adminVoice.modeHelp":"حالت دستی یک جمله را می‌شنود و پس از پاسخ متوقف می‌شود. همیشه آماده تا وقتی اپ باز است، صدای ادمین را تحلیل می‌کند و به درخواست‌های خطاب‌شده پاسخ می‌دهد. در هر دو حالت، تسک و یادداشت فقط با تأیید شما ثبت می‌شوند.","adminVoice.modeError":"حالت صدا ذخیره نشد؛ انتخاب قبلی حفظ شد.","adminVoice.modeSaving":"ذخیرهٔ حالت صدا…","adminVoice.proposalTitle":"پیشنهاد صوتی","adminVoice.proposalTask":"تسک پیشنهادی","adminVoice.proposalNote":"یادداشت پیشنهادی","adminVoice.proposalHint":"این پیشنهاد ثبت شود؟ هنوز چیزی ذخیره نشده است.","adminVoice.proposalAccept":"ثبت","adminVoice.proposalReject":"رد","adminVoice.proposalSaving":"در حال ثبت…","adminVoice.proposalError":"تصمیم ذخیره نشد. پیشنهاد را بررسی کن و دوباره تلاش کن.","adminVoice.proposalExpired":"این پیشنهاد دیگر در انتظار تأیید نیست. دوباره صحبت کن تا پیشنهاد تازه‌ای آماده شود.","adminVoice.proposalReady":"پیشنهاد آماده"});

registerMessages({"local.desktopOpen":"Open window"},{"local.desktopOpen":"بازکردن پنجره"});

registerMessages({"adminVoice.proposalQuestion":"Save this proposal: {text}? Nothing has been saved yet."},{"adminVoice.proposalQuestion":"این پیشنهاد ثبت شود: {text}؟ هنوز چیزی ذخیره نشده است."});
registerMessages({
 "adminVoice.wakeReply":"I'm listening. Tell me what to do.","adminVoice.wakeWaiting":"Listening for your request · 20 seconds",
 "adminVoice.notice.speaker-too-short":"Say a complete phrase, starting with Roadeep",
 "adminVoice.notice.speaker-not-enrolled":"Register your voice in Settings",
 "adminVoice.notice.speaker-uncertain":"Voice unclear · speak closer to the microphone",
 "adminVoice.notice.speaker-different":"The registered admin voice was not recognized",
 "adminVoice.notice.speaker-enrollment-active":"Voice registration is in progress",
 "adminVoice.notice.speech-unrecognized":"Speech unclear · say Roadeep and your request again"
},{
 "adminVoice.wakeReply":"جانم، بگو چه کاری انجام بدهم.","adminVoice.wakeWaiting":"منتظر درخواست شما · ۲۰ ثانیه",
 "adminVoice.notice.speaker-too-short":"یک جملهٔ کامل با «رودیپ» بگو",
 "adminVoice.notice.speaker-not-enrolled":"صدایت را در تنظیمات ثبت کن",
 "adminVoice.notice.speaker-uncertain":"صدا واضح نبود؛ نزدیک‌تر به میکروفون صحبت کن",
 "adminVoice.notice.speaker-different":"صدای ادمین ثبت‌شده تشخیص داده نشد",
 "adminVoice.notice.speaker-enrollment-active":"ثبت صدا در حال انجام است",
 "adminVoice.notice.speech-unrecognized":"گفتار واضح نبود؛ «رودیپ» و درخواستت را دوباره بگو"
});

registerMessages({
 "liveVoice.start":"Start live conversation","liveVoice.end":"End live conversation",
 "liveVoice.idle":"Talk with Roadeep","liveVoice.connecting":"Connecting…","liveVoice.listening":"Listening","liveVoice.thinking":"Thinking…","liveVoice.speaking":"Speaking","liveVoice.closing":"Ending…","liveVoice.error":"Live conversation stopped","liveVoice.muted":"Microphone muted",
 "liveVoice.you":"You","liveVoice.assistant":"Roadeep",
 "liveVoice.approvalTitle":"Needs your approval","liveVoice.approve":"Approve","liveVoice.reject":"Decline",
 "liveVoice.approvalHint":"Say “Approve” or “Decline” — or any answer like “yes, do it” / “no, leave it”.","liveVoice.approvalLeft":"{value} s left","liveVoice.heard":"Heard: {text}",
 "liveVoice.setupNeeded":"Add your OpenAI API key in Settings to start a live conversation."
},{
 "liveVoice.start":"شروع گفتگوی زنده","liveVoice.end":"پایان گفتگوی زنده",
 "liveVoice.idle":"گفتگو با رودیپ","liveVoice.connecting":"در حال اتصال…","liveVoice.listening":"در حال گوش‌دادن","liveVoice.thinking":"در حال فکرکردن…","liveVoice.speaking":"در حال صحبت","liveVoice.closing":"در حال پایان…","liveVoice.error":"گفتگوی زنده متوقف شد","liveVoice.muted":"میکروفون قطع است",
 "liveVoice.you":"شما","liveVoice.assistant":"رودیپ",
 "liveVoice.approvalTitle":"نیاز به تأیید شما","liveVoice.approve":"تأیید","liveVoice.reject":"رد",
 "liveVoice.approvalHint":"بگویید «تأیید» یا «رد» — یا هر جوابی مثل «آره، انجامش بده» / «نه، ولش کن».","liveVoice.approvalLeft":"{value} ثانیه","liveVoice.heard":"شنیدم: {text}",
 "liveVoice.setupNeeded":"برای شروع گفتگوی زنده، کلید API اوپن‌ای‌آی را در تنظیمات وارد کنید."
});

registerMessages({
 "liveVoice.draft.waiting":"Waiting for your approval","liveVoice.draft.done":"Done",
 "liveVoice.draft.new.task":"New task","liveVoice.draft.new.note":"New note","liveVoice.draft.new.reminder":"New reminder","liveVoice.draft.new.habit":"Habit","liveVoice.draft.new.focus":"Focus timer"
},{
 "liveVoice.draft.waiting":"در انتظار تأیید شما","liveVoice.draft.done":"انجام شد",
 "liveVoice.draft.new.task":"کار جدید","liveVoice.draft.new.note":"یادداشت جدید","liveVoice.draft.new.reminder":"یادآور جدید","liveVoice.draft.new.habit":"عادت","liveVoice.draft.new.focus":"تایمر تمرکز"
});
registerMessages({"liveVoice.draft.working":"Doing it…","liveVoice.draft.failed":"It didn't go through.","liveVoice.details":"Full details of this request"},{"liveVoice.draft.working":"در حال انجام…","liveVoice.draft.failed":"انجام نشد.","liveVoice.details":"جزئیات کامل این درخواست"});

registerMessages({
 "memory.title":"Roadeep's memory","memory.description":"What Roadeep has learned about you in live conversations — preferences, habits and facts you shared — so its suggestions fit you.",
 "memory.enabled":"Let Roadeep learn and remember","memory.on":"On — {count} things remembered","memory.off":"Off — nothing new is learned ({count} kept until you clear them)",
 "memory.factsTitle":"What Roadeep remembers","memory.usageTitle":"How you use Roadeep","memory.empty":"Nothing yet. Tell Roadeep something like “I prefer short answers”.",
 "memory.delete":"Forget “{text}”","memory.clearAll":"Clear everything","memory.clearConfirm":"Delete everything Roadeep remembers about you, including usage history? This cannot be undone.",
 "memory.category.preference":"Preference","memory.category.habit":"Habit","memory.category.style":"Style","memory.category.fact":"Fact",
 "memory.topTools":"Most used: {list}","memory.noUsage":"No usage recorded yet.","memory.hours":"Usually active: {list}","memory.hourRange":"{from}–{to}",
 "memory.privacy":"Roadeep keeps the facts it learned and short summaries of your recent requests (these can include the text of tasks and notes) in a file on this PC. They are sent to the voice provider you configured (OpenAI) as part of every live conversation, and nowhere else. Turning memory off stops new learning; “Clear everything” erases them.",
 "memory.tool.add_task":"adding tasks","memory.tool.list_tasks":"checking tasks","memory.tool.complete_task":"completing tasks","memory.tool.add_note":"writing notes","memory.tool.list_notes":"reading notes","memory.tool.update_note":"editing notes","memory.tool.add_reminder":"reminders","memory.tool.control_focus":"focus timer","memory.tool.ask_roadeep":"questions","memory.tool.log_habit":"habits","memory.tool.show_view":"opening pages",
 "liveVoice.remembered":"Remembered: {text}"
},{
 "memory.title":"حافظهٔ رودیپ","memory.description":"آنچه رودیپ در گفتگوهای زنده دربارهٔ شما یاد گرفته — ترجیح‌ها، عادت‌ها و نکته‌هایی که گفته‌اید — تا پیشنهادهایش به کار شما بیاید.",
 "memory.enabled":"رودیپ یاد بگیرد و به خاطر بسپارد","memory.on":"روشن؛ {count} مورد در حافظه","memory.off":"خاموش؛ چیز تازه‌ای یاد گرفته نمی‌شود ({count} مورد تا پاک کردن می‌ماند)",
 "memory.factsTitle":"آنچه رودیپ به خاطر دارد","memory.usageTitle":"نحوهٔ استفادهٔ شما","memory.empty":"هنوز چیزی نیست. مثلاً به رودیپ بگویید «جواب‌های کوتاه را ترجیح می‌دهم».",
 "memory.delete":"فراموش کردن «{text}»","memory.clearAll":"پاک کردن همه","memory.clearConfirm":"همهٔ آنچه رودیپ دربارهٔ شما به خاطر دارد، همراه با سابقهٔ استفاده، پاک شود؟ این کار برگشت‌پذیر نیست.",
 "memory.category.preference":"ترجیح","memory.category.habit":"عادت","memory.category.style":"سبک","memory.category.fact":"نکته",
 "memory.topTools":"بیشترین استفاده: {list}","memory.noUsage":"هنوز استفاده‌ای ثبت نشده.","memory.hours":"ساعت‌های معمول: {list}","memory.hourRange":"{from} تا {to}",
 "memory.privacy":"رودیپ نکته‌هایی را که یاد گرفته و خلاصهٔ کوتاه درخواست‌های اخیر شما (که ممکن است متن کارها و یادداشت‌ها را هم داشته باشد) در فایلی روی همین سیستم نگه می‌دارد. این‌ها در هر گفتگوی زنده برای سرویس صوتی‌ای که تنظیم کرده‌اید (OpenAI) فرستاده می‌شوند و به هیچ جای دیگری نه. خاموش کردن حافظه یادگیری تازه را متوقف می‌کند و «پاک کردن همه» همه را حذف می‌کند.",
 "memory.tool.add_task":"افزودن کار","memory.tool.list_tasks":"دیدن کارها","memory.tool.complete_task":"انجام کارها","memory.tool.add_note":"نوشتن یادداشت","memory.tool.list_notes":"خواندن یادداشت‌ها","memory.tool.update_note":"ویرایش یادداشت","memory.tool.add_reminder":"یادآورها","memory.tool.control_focus":"تایمر تمرکز","memory.tool.ask_roadeep":"پرسش‌ها","memory.tool.log_habit":"عادت‌ها","memory.tool.show_view":"باز کردن صفحه‌ها",
 "liveVoice.remembered":"به خاطر سپردم: {text}"
});
