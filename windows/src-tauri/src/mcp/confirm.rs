// The human gate in front of the paid step. Whatever the MCP client's model
// says about the user having agreed, a generation only starts after someone
// clicks Yes in a native Windows dialog shown by the Roadeep app itself.
//
//   * owner-less, topmost, set to the foreground, with No as the default
//     button — a stray Enter declines;
//   * in the UI language (Settings.language), right-to-left for Persian;
//   * one at a time: a second request while one is up gets BUSY, never a
//     stack of dialogs to click through;
//   * unanswered after DIALOG_TIMEOUT it closes itself and counts as No.
//
// MessageBoxW has no timeout of its own (MessageBoxTimeoutW is undocumented),
// so the dialog's thread arms a thread timer first: the message box's modal
// loop dispatches it, and the callback presses No on the box it finds among
// this thread's windows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::quotes::Quote;
use crate::log;
use crate::roadeep::generation::Target;

pub const USER_DECLINED: &str = "USER_DECLINED";

/// Under the app's per-call deadline (230 s) with room for the submit after it
/// (two attempts at 60 s each), so an approved generation is never cut off.
pub const DIALOG_TIMEOUT: Duration = Duration::from_secs(100);
/// If the dialog thread itself never comes back, stop waiting a little later.
const BACKSTOP: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Approved,
    Declined,
    TimedOut,
    /// Another confirmation is on screen.
    Busy,
    /// The dialog could not be shown; nothing is started.
    Failed,
}

pub struct DialogText {
    pub title: String,
    pub body: String,
    pub rtl: bool,
}

/// What the user reads. `language` is Settings.language: "fa", else English.
pub fn render(quote: &Quote, language: &str) -> DialogText {
    let (product_label, product) = match &quote.target {
        Target::Subtype(s) => (("Product", "محصول"), s.as_str()),
        Target::Model(m) => (("Model", "مدل"), m.as_str()),
    };
    if language == "fa" {
        let unknown = "اعلام نشده";
        DialogText {
            title: "رودیپ — تأیید تولید پولی".into(),
            body: format!(
                "یک برنامهٔ MCP (مثلاً Claude Code) می‌خواهد با حساب رودیپ تو یک تولید را شروع کند. این کار از اعتبار تو کم می‌کند.\n\n\
                 {}: {product}\nقابلیت: {}\nهزینهٔ برآوردشده: {}\nاعتبار لازم: {}\n\n\
                 شروع شود؟ اگر این درخواست را خودت نداده‌ای، «خیر» را بزن.",
                product_label.1,
                quote.capability,
                quote.cost.as_deref().unwrap_or(unknown),
                quote.credits.as_deref().unwrap_or(unknown),
            ),
            rtl: true,
        }
    } else {
        let unknown = "not stated";
        DialogText {
            title: "Roadeep — confirm a paid generation".into(),
            body: format!(
                "An MCP client (such as Claude Code) is asking to start a Roadeep generation with your account. It spends your Roadeep credits.\n\n\
                 {}: {product}\nCapability: {}\nQuoted cost: {}\nRequired credits: {}\n\n\
                 Start it? If you did not ask for this, choose No.",
                product_label.0,
                quote.capability,
                quote.cost.as_deref().unwrap_or(unknown),
                quote.credits.as_deref().unwrap_or(unknown),
            ),
            rtl: false,
        }
    }
}

static OPEN: AtomicBool = AtomicBool::new(false);

/// Holding one means the (single) dialog is ours; dropping it frees the slot.
struct Slot;

impl Slot {
    fn take() -> Option<Self> {
        OPEN.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| Slot)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        OPEN.store(false, Ordering::SeqCst);
    }
}

/// Shows the dialog on a blocking thread and waits for the click. The slot
/// lives on that thread, so even if this future is dropped (the call's
/// deadline) no second dialog can open while the first is still up.
pub async fn ask(quote: &Quote) -> Answer {
    let Some(slot) = Slot::take() else {
        log::line("mcp: generation confirmation busy");
        return Answer::Busy;
    };
    let quote = quote.clone();
    let shown = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        let text = render(&quote, &crate::settings::load().language);
        native::show(&text, DIALOG_TIMEOUT)
    });
    let answer = match tokio::time::timeout(DIALOG_TIMEOUT + BACKSTOP, shown).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(err)) => {
            log::line(format!("mcp: confirmation dialog thread failed: {err}"));
            Answer::Failed
        }
        Err(_) => Answer::TimedOut,
    };
    log::line(format!("mcp: generation confirmation {answer:?}"));
    answer
}

mod native {
    use std::cell::Cell;
    use std::time::Duration;

    use windows::core::{BOOL, HSTRING};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetClassNameW, KillTimer, MessageBoxW, PostMessageW, SetTimer, IDNO, IDYES, MB_DEFBUTTON2,
        MB_ICONWARNING, MB_RIGHT, MB_RTLREADING, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO, WM_COMMAND,
    };

    use super::{Answer, DialogText};
    use crate::log;

    thread_local! {
        static TIMED_OUT: Cell<bool> = const { Cell::new(false) };
    }

    /// The window class of every dialog box, message boxes included.
    const DIALOG_CLASS: &str = "#32770";

    pub fn show(text: &DialogText, timeout: Duration) -> Answer {
        TIMED_OUT.with(|t| t.set(false));
        let mut style = MB_YESNO | MB_ICONWARNING | MB_TOPMOST | MB_SETFOREGROUND | MB_DEFBUTTON2;
        if text.rtl {
            style |= MB_RTLREADING | MB_RIGHT;
        }
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: plain Win32 calls on this thread; the strings outlive MessageBoxW.
        unsafe {
            let timer = SetTimer(None, 0, millis, Some(on_timeout));
            if timer == 0 {
                // Without the timer the dialog could wait forever; refuse instead.
                log::line("mcp: cannot arm the confirmation timeout; generation not started");
                return Answer::Failed;
            }
            let result = MessageBoxW(None, &HSTRING::from(text.body.as_str()), &HSTRING::from(text.title.as_str()), style);
            let _ = KillTimer(None, timer);
            if TIMED_OUT.with(Cell::get) {
                Answer::TimedOut
            } else if result == IDYES {
                Answer::Approved
            } else if result == IDNO {
                Answer::Declined
            } else {
                log::line(format!("mcp: confirmation dialog failed (result {})", result.0));
                Answer::Failed
            }
        }
    }

    unsafe extern "system" fn on_timeout(_: HWND, _: u32, id: usize, _: u32) {
        let _ = KillTimer(None, id);
        TIMED_OUT.with(|t| t.set(true));
        let _ = EnumThreadWindows(GetCurrentThreadId(), Some(press_no), LPARAM(0));
    }

    unsafe extern "system" fn press_no(hwnd: HWND, _: LPARAM) -> BOOL {
        let mut class = [0u16; 16];
        let len = GetClassNameW(hwnd, &mut class).max(0) as usize;
        if String::from_utf16_lossy(&class[..len]) == DIALOG_CLASS {
            let _ = PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(IDNO.0 as usize), LPARAM(0));
        }
        true.into()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Puts a real dialog on screen for half a second, so it is opt-in:
        /// `cargo test -p roadeep -- --ignored unanswered_dialog`.
        #[test]
        #[ignore = "shows a native dialog on the desktop"]
        fn an_unanswered_dialog_closes_itself_as_no() {
            let text = DialogText { title: "Roadeep test".into(), body: "Closing by itself…".into(), rtl: false };
            let started = std::time::Instant::now();
            assert_eq!(show(&text, Duration::from_millis(500)), Answer::TimedOut);
            assert!(started.elapsed() < Duration::from_secs(5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::quotes;
    use crate::roadeep::generation::GenerationRequest;
    use serde_json::json;

    /// Through the real store, as `start_generation` gets it. Ids are unique
    /// because tests share the store and run in parallel.
    fn quote(target: &str, cost: Option<&str>) -> Quote {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = format!("confirm-test-{}", N.fetch_add(1, Ordering::SeqCst));
        let mut args = json!({ "capability": "text-to-image", "input": { "prompt": "a cat" } });
        args[target] = json!("fal-flux");
        let req = GenerationRequest::from_args(&args).unwrap();
        let mut shaped = json!({ "quote_id": id, "expires_at": "2099-01-01T00:00:00Z", "required_credits": 3 });
        if let Some(c) = cost {
            shaped["estimated_cost"] = json!({ "amount": c, "currency": "IRT" });
        }
        quotes::remember(&shaped, &req);
        let q = quotes::for_request(&id, &req).unwrap();
        quotes::forget(&id);
        q
    }

    #[test]
    fn english_dialog_states_what_and_how_much() {
        let text = render(&quote("subtype", Some("1200")), "en");
        assert!(!text.rtl);
        for part in ["MCP client", "Roadeep generation", "Product: fal-flux", "text-to-image", "1200 IRT", "Required credits: 3", "No"] {
            assert!(text.body.contains(part), "{part} missing from {}", text.body);
        }
        let model = render(&quote("model", None), "en");
        assert!(model.body.contains("Model: fal-flux") && model.body.contains("Quoted cost: not stated"), "{}", model.body);
    }

    #[test]
    fn persian_dialog_is_rtl_and_persian() {
        let text = render(&quote("subtype", Some("1200")), "fa");
        assert!(text.rtl);
        assert!(text.title.starts_with("رودیپ"), "{}", text.title);
        for part in ["MCP", "رودیپ", "محصول: fal-flux", "1200 IRT", "اعتبار لازم: 3", "«خیر»"] {
            assert!(text.body.contains(part), "{part} missing from {}", text.body);
        }
        assert!(!render(&quote("subtype", None), "de").rtl, "anything but fa is English");
    }

    #[test]
    fn only_one_dialog_at_a_time() {
        let first = Slot::take().expect("free");
        assert!(Slot::take().is_none(), "a second confirmation must be refused");
        drop(first);
        let again = Slot::take().expect("freed when the first one ends");
        drop(again);
    }
}
