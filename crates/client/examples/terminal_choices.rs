#[allow(dead_code)]
mod prefix_direct {
    pub fn host_is_terminal() -> bool { false }
}

#[path = "../src/engine/terminal_choices.rs"]
#[allow(dead_code)]
mod terminal_choices;

fn main() -> windows::core::Result<()> {
    use windows::Win32::{System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED}, UI::{Accessibility::{CUIAutomation, IUIAutomation}, WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId}}};
    let expected: u32 = std::env::args().nth(1).expect("expected terminal PID").parse().expect("PID");
    unsafe {
        let mut process = 0;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut process));
        if process != expected { println!("Foreground changed; skipped"); return Ok(()); }
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        match terminal_choices::focused_visible_text(&automation, process) {
            Ok(text) => println!("{}", serde_json::json!({
                "terminal_pid": process, "visible_lines": text.lines().count(), "visible_chars": text.chars().count(),
                "pass_letters": ('A'..='Z').filter(|c| terminal_choices::detect(&text).accepts(*c as usize)).collect::<String>(),
                "pass_digits": ('0'..='9').filter(|c| terminal_choices::detect(&text).accepts(*c as usize)).collect::<String>()
            })),
            Err(error) => println!("Provider unavailable; normal IME behavior: {error}"),
        }
        Ok(())
    }
}
