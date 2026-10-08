import type { ActivitySession } from "./types";

function literal(text: string): string { return text.replace(/[\\`*_{}\[\]<>#]/g, character => `\\${character}`).replace(/[\r\n]+/g, " "); }

export function recapMarkdown(session: ActivitySession, lang: string = "en"): string {
  const fa = lang === "fa";
  const labels = fa ? { heading: "جلسه کدنویسی", harness: "دستیار", session: "جلسه", status: "وضعیت", observed: "زمان مشاهده", caution: "این شواهد محلی محدود است؛ تضمین درستی یا متن کامل جلسه نیست.", files: "فایل‌های تغییرکرده", noFiles: "تغییر فایلی مشاهده نشده است.", tests: "شواهد تست", noTests: "نتیجه تستی مشاهده نشده است.", freshness: "تازگی", passed: "موفق", failed: "ناموفق", skipped: "ردشده", command: "فرمان", evidence: "شناسه شاهد", failures: "خطاها", unknown: "عملیات نامشخص", noFailures: "در بازه نگهداری‌شده خطایی مشاهده نشده است.", handoff: "ادامه کار", review: "پیش از ادامه، وضعیت فعلی پروژه و فایل‌های تغییرکرده را بررسی کن.", run: "تست‌های مرتبط را روی آخرین تغییرات اجرا کن؛ شاهد تازه‌ای از موفقیت تست‌ها نداریم." } : { heading: "Coding session", harness: "Harness", session: "Session", status: "Status", observed: "Observed", caution: "This is bounded local evidence, not proof of correctness or a complete transcript.", files: "Changed files", noFiles: "No file mutations were observed.", tests: "Test evidence", noTests: "No test result was observed.", freshness: "freshness", passed: "Passed", failed: "failed", skipped: "skipped", command: "Command", evidence: "Evidence ID", failures: "Failures", unknown: "Unknown operation", noFailures: "No failures were observed in the retained window.", handoff: "Handoff", review: "Confirm the current working tree and review the changed files before continuing.", run: "Run the relevant tests against the latest changes; current passing evidence is unavailable." };
  const translated: Record<string, string> = { archived: "بایگانی‌شده", active: "فعال", finished: "پایان‌یافته", cancelled: "متوقف شد", error: "خطا", passed: "موفق", failed: "ناموفق", unknown: "نامشخص", skipped: "ردشده", current: "تازه نسبت به تغییرات مشاهده‌شده", stale: "قدیمی" };
  const word = (value: string) => fa ? translated[value] ?? value : value === "cancelled" ? "Stopped" : value;
  const lines = [`# ${labels.heading}: ${literal(session.title)}`, "", `${labels.harness}: ${session.harness}`, `${labels.session}: ${literal(session.id)}`, `${labels.status}: ${word(session.status)}`, `${labels.observed}: ${new Date(session.updatedAt).toISOString()}`, "", labels.caution, "", `## ${labels.files}`];
  for (const file of session.changedFiles) lines.push(`- ${literal(file)}`);
  if (!session.changedFiles.length) lines.push(labels.noFiles);
  lines.push("", `## ${labels.tests}`);
  const reasons: Record<string, string> = { "Reported test failures": "تست ناموفق گزارش شده است", "Command exited unsuccessfully": "فرمان با خطا پایان یافته است", "Output is incomplete": "خروجی کامل نیست", "Exit status is unknown": "وضعیت خروج نامشخص است", "Successful exit with reported passing tests": "خروج موفق همراه با تعداد تست‌های موفق گزارش‌شده", "Only skipped tests were reported": "فقط تست‌های ردشده گزارش شده‌اند", "No executed test count was found": "تعداد تست اجراشده پیدا نشد" };
  if (session.test) lines.push(`- ${word(session.test.verdict)}; ${labels.freshness}: ${word(session.test.freshness)}`, `- ${labels.passed}: ${session.test.passed}; ${labels.failed}: ${session.test.failed}; ${labels.skipped}: ${session.test.skipped}`, `- ${labels.command}: ${literal(session.test.command)}`, `- ${labels.evidence}: ${literal(session.test.eventId)}`, `- ${fa ? reasons[session.test.reason] ?? session.test.reason : session.test.reason}`);
  else lines.push(labels.noTests);
  lines.push("", `## ${labels.failures}`);
  const errors = session.events.filter(event => event.kind === "error" || event.phase === "failed");
  for (const event of errors.slice(-12)) lines.push(`- ${literal(event.command || event.title || event.tool || labels.unknown)} (${labels.evidence}: ${literal(event.id)})`);
  if (!errors.length) lines.push(labels.noFailures);
  lines.push("", `## ${labels.handoff}`, `- ${labels.review}`);
  if (!session.test || session.test.freshness !== "current" || session.test.verdict !== "passed") lines.push(`- ${labels.run}`);
  for (const conflict of session.conflicts) lines.push(fa ? `- هم‌پوشانی احتمالی را بررسی کن: ${literal(conflict.file)} با جلسه ${literal(conflict.sessionId)}. این شاهد نوشتن هم‌زمان نیست.` : `- Check advisory file overlap: ${literal(conflict.file)} with session ${literal(conflict.sessionId)}. This does not establish concurrent writes.`);
  return lines.join("\n");
}
