// Roadeep errors in the UI language. Both windows use this, so a code reads the
// same in the island and in the settings window.

import { isRoadeepError } from "../core/bridge";
import { t } from "../core/i18n";
import { en } from "../core/locales/en";

/**
 * A known code is translated (THROTTLED with its wait); anything else falls
 * back to the message the server or Rust sent, which is better than nothing.
 */
export function roadeepErrorText(err: unknown): string {
  if (!isRoadeepError(err)) {
    const text = String(err).replace(/^Error:\s*/, "").trim();
    return text || t("rerr.UNKNOWN");
  }
  if (err.code === "THROTTLED" && err.retryAfter != null && err.retryAfter > 0) {
    return t("rerr.THROTTLED.wait", { s: Math.ceil(err.retryAfter) });
  }
  if (isInsufficientCredits(err)) return t("rerr.INSUFFICIENT_CREDITS");
  const key = `rerr.${err.code}`;
  if (key in en) return t(key);
  return err.message.trim() || t("rerr.UNKNOWN");
}

/** The first server message for each field, for inline errors under inputs. */
export function fieldErrors(err: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  if (!isRoadeepError(err) || !err.fieldErrors) return out;
  for (const [field, messages] of Object.entries(err.fieldErrors)) {
    const first = Array.isArray(messages) ? messages.find((m) => typeof m === "string" && m.trim()) : null;
    if (first) out[field] = first;
  }
  return out;
}

/** Codes that mean "you are not signed in (any more)". */
export function isSignedOutError(err: unknown): boolean {
  return isRoadeepError(err) && (err.code === "NOT_SIGNED_IN" || err.code === "SESSION_EXPIRED");
}

/**
 * Out of tokens. Like the web and mobile clients: HTTP 402, or any code
 * containing INSUFFICIENT_CREDIT (INSUFFICIENT_CREDITS, AD_DESIGN_INSUFFICIENT_CREDITS…).
 */
export function isInsufficientCredits(err: unknown): boolean {
  return isRoadeepError(err) && (err.status === 402 || /INSUFFICIENT_CREDIT/i.test(err.code));
}
