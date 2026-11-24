use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use std::{cell::Cell, hash::{Hash, Hasher}};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys(u64);

impl Keys {
    fn add(&mut self, c: char) {
        let bit = match c.to_ascii_lowercase() {
            'a'..='z' => c.to_ascii_lowercase() as u32 - 'a' as u32,
            '0'..='9' => c as u32 - '0' as u32 + 26,
            _ => return,
        };
        self.0 |= 1 << bit;
    }

    pub fn accepts(self, vk: usize) -> bool {
        let bit = match vk {
            0x41..=0x5a => vk - 0x41,
            0x30..=0x39 => vk - 0x30 + 26,
            0x60..=0x69 => vk - 0x60 + 26,
            0x0d | 0x1b | 0x09 | 0x20 | 0x25..=0x28 => return self.0 != 0,
            _ => return false,
        };
        self.0 & (1 << bit) != 0
    }
}

fn trim_frame(line: &str) -> &str {
    line.trim_matches(|c: char| c.is_whitespace() || "│┃╭╮╰╯┌┐└┘─━".contains(c))
}

fn action_label(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    ["yes", "no", "allow", "deny", "approve", "reject", "cancel", "proceed", "always", "once", "accept", "continue"]
        .iter().any(|word| lower.strip_prefix(word).is_some_and(|rest| rest.is_empty() || rest.starts_with(|c: char| !c.is_ascii_alphabetic())))
        || ["はい", "いいえ", "許可", "拒否", "承認", "キャンセル", "続行"].iter().any(|word| label.starts_with(word))
}

fn bracket_keys(line: &str, keys: &mut Keys) {
    for (open, close) in [('(', ')'), ('[', ']')] {
        for part in line.split(open).skip(1) {
            if let Some((key, _)) = part.split_once(close) {
                if key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric()) {
                    keys.add(key.chars().next().unwrap());
                }
            }
        }
    }
}

fn choice(line: &str) -> Option<(char, &str)> {
    let line = line.trim_start_matches(|c: char| c.is_whitespace() || "›❯>●○*".contains(c));
    let mut chars = line.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphanumeric() { return None; }
    let rest = chars.as_str();
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')).or_else(|| rest.strip_prefix(':'))?;
    if !rest.starts_with(char::is_whitespace) { return None; }
    Some((first, rest.trim_start()))
}

fn confirm_footer(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    (lower.contains("enter") && ["confirm", "select", "accept", "continue"].iter().any(|word| lower.contains(word)))
        || (line.contains("Enter") && ["確定", "選択", "承認"].iter().any(|word| line.contains(word)))
}

// Inspect only the current screen tail, never a terminal's scrollback or a transcript.
pub fn detect(text: &str) -> Keys {
    let lines: Vec<&str> = text.lines().map(trim_frame).filter(|line| !line.is_empty()).collect();
    let lines = &lines[lines.len().saturating_sub(16)..];
    let Some(last) = lines.last() else { return Keys::default(); };
    for (open, close) in [('(', ')'), ('[', ']')] {
        if let Some((_, suffix)) = last.rsplit_once(open) {
            if let Some((options, after)) = suffix.split_once(close) {
                if after.trim_matches(|c: char| c.is_whitespace() || ":?".contains(c)).is_empty()
                    && matches!(options.to_ascii_lowercase().as_str(), "y/n" | "y/n/a" | "y/n/q") {
                    let mut keys = Keys::default();
                    for c in options.chars().filter(|c| c.is_ascii_alphabetic()) { keys.add(c); }
                    return keys;
                }
            }
        }
    }
    let footer = confirm_footer(last);
    let mut keys = Keys::default();
    let mut count = 0;
    let mut actions = 0;
    let mut selected = false;
    for line in lines.iter().rev().skip(usize::from(footer)) {
        if let Some((key, label)) = choice(line) {
            keys.add(key);
            bracket_keys(label, &mut keys);
            count += 1;
            actions += usize::from(action_label(label));
            selected |= line.starts_with(['›', '❯', '>', '●']);
        } else if (line.starts_with('[') || line.starts_with('(')) && line.chars().nth(2).is_some_and(|c| c == ']' || c == ')') {
            let label = line.get(3..).unwrap_or_default().trim_start();
            if !action_label(label) { break; }
            bracket_keys(line, &mut keys);
            count += 1;
            actions += 1;
        } else if count > 0 {
            break;
        } else {
            return Keys::default();
        }
    }
    if count >= 2 && actions >= 2 && (footer || selected) { keys } else { Keys::default() }
}

struct Snapshot {
    window: isize,
    updated: Instant,
    keys: Keys,
    signature: u64,
}

impl Snapshot {
    fn accepts(&self, window: isize, vk: usize) -> bool {
        self.window == window && self.updated.elapsed() < Duration::from_millis(450) && self.keys.accepts(vk)
    }
}

static SNAPSHOT: Mutex<Option<Snapshot>> = Mutex::new(None);
static CONSUMED: Mutex<Option<(isize, u64)>> = Mutex::new(None);

thread_local! {
    static LAST_KEY: Cell<Option<(i32, usize, bool)>> = const { Cell::new(None) };
}

pub fn start() {
    if !super::prefix_direct::host_is_terminal() { return; }
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        if shared::AppConfig::read_or_default().input.latin_at_terminal_choices {
            let _ = std::thread::Builder::new().name("azk-terminal-choices".into()).spawn(observe);
        }
    });
}

pub fn pass_key(vk: usize) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetMessageTime};
    let time = unsafe { GetMessageTime() };
    if let Some((last_time, last_vk, pass)) = LAST_KEY.get() {
        if last_time == time && last_vk == vk { return pass; }
    }
    let window = unsafe { GetForegroundWindow() }.0 as isize;
    let pass = SNAPSHOT.try_lock().ok().and_then(|snapshot| snapshot.as_ref().map(|s| {
        if !s.accepts(window, vk) { return false; }
        let Ok(mut consumed) = CONSUMED.try_lock() else { return false; };
        if *consumed == Some((window, s.signature)) { return false; }
        if !matches!(vk, 0x09 | 0x25..=0x28) { *consumed = Some((window, s.signature)); }
        true
    })).unwrap_or(false);
    LAST_KEY.set(Some((time, vk, pass)));
    pass
}

pub fn focused_visible_text(automation: &windows::Win32::UI::Accessibility::IUIAutomation, process: u32) -> windows::core::Result<String> {
    use windows::Win32::{Foundation::BOOL, UI::Accessibility::{IUIAutomationTextPattern, IUIAutomationTextPattern2, TextPatternRangeEndpoint_Start, TextPatternRangeEndpoint_End, TextUnit_Line, UIA_TextPatternId, UIA_TextPattern2Id}};
    unsafe {
        let element = automation.GetFocusedElement()?;
        if element.CurrentProcessId()? != process as i32 || !element.CurrentHasKeyboardFocus()?.as_bool() {
            return Ok(String::new());
        }
        let pattern: IUIAutomationTextPattern = element.GetCurrentPatternAs(UIA_TextPatternId)?;
        let ranges = pattern.GetVisibleRanges()?;
        if ranges.Length()? != 1 { return Ok(String::new()); }
        let visible = ranges.GetElement(0)?;
        let selection = pattern.GetSelection()?;
        for index in 0..selection.Length()? {
            let selected = selection.GetElement(index)?;
            if selected.CompareEndpoints(TextPatternRangeEndpoint_Start, &selected, TextPatternRangeEndpoint_End)? != 0 {
                return Ok(String::new());
            }
        }
        let caret = if let Ok(pattern2) = element.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id) {
            let mut active = BOOL::default();
            let caret = pattern2.GetCaretRange(&mut active)?;
            if !active.as_bool() { return Ok(String::new()); }
            caret
        } else if selection.Length()? == 1 {
            selection.GetElement(0)?
        } else { return Ok(String::new()); };
        if caret.CompareEndpoints(TextPatternRangeEndpoint_Start, &visible, TextPatternRangeEndpoint_Start)? < 0
            || caret.CompareEndpoints(TextPatternRangeEndpoint_End, &visible, TextPatternRangeEndpoint_End)? > 0 {
            return Ok(String::new());
        }
        let range = visible.Clone()?;
        range.MoveEndpointByRange(TextPatternRangeEndpoint_Start, &range, TextPatternRangeEndpoint_End)?;
        range.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Line, -16)?;
        if range.CompareEndpoints(TextPatternRangeEndpoint_Start, &visible, TextPatternRangeEndpoint_Start)? < 0 {
            range.MoveEndpointByRange(TextPatternRangeEndpoint_Start, &visible, TextPatternRangeEndpoint_Start)?;
        }
        Ok(range.GetText(8192)?.to_string())
    }
}

fn observe() {
    use windows::Win32::{System::{Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED}, Threading::GetCurrentProcessId}, UI::{Accessibility::{CUIAutomation, IUIAutomation}, WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId}}};
    unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() { return; }
        let Ok(automation): windows::core::Result<IUIAutomation> = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) else { return; };
        let process = GetCurrentProcessId();
        loop {
            let queried = Instant::now();
            let window = GetForegroundWindow();
            let mut foreground_process = 0;
            GetWindowThreadProcessId(window, Some(&mut foreground_process));
            let text = if foreground_process == process {
                focused_visible_text(&automation, process).unwrap_or_default()
            } else { String::new() };
            let keys = detect(&text);
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut hasher);
            let signature = hasher.finish();
            // A blocked provider never blocks typing; the previous snapshot expires.
            if window == GetForegroundWindow() {
                if let Ok(mut snapshot) = SNAPSHOT.lock() {
                    *snapshot = Some(Snapshot { window: window.0 as isize, updated: queried, keys, signature });
                }
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_or_other_window_snapshots_do_not_pass_keys() {
        let mut snapshot = Snapshot { window: 7, updated: Instant::now(), keys: detect("Continue? [Y/n]"), signature: 1 };
        assert!(snapshot.accepts(7, 0x59));
        assert!(!snapshot.accepts(8, 0x59));
        assert!(!snapshot.accepts(7, 0x50));
        snapshot.updated = Instant::now() - Duration::from_secs(1);
        assert!(!snapshot.accepts(7, 0x59));
    }

    #[test]
    fn numbered_approval_includes_advertised_letters_and_rejection() {
        let keys = detect("Run this command?\n› 1. Yes, proceed (y)\n  2. Yes, remember permission (p)\n  3. No, reject (n)\nPress enter to confirm or esc to cancel");
        for key in [0x50, 0x59, 0x4e, 0x31, 0x33, 0x61, 0x0d, 0x1b] { assert!(keys.accepts(key), "{key}"); }
        assert!(!keys.accepts(0x41));
    }

    #[test]
    fn other_tools_and_japanese_choices_are_not_app_specific() {
        assert!(detect("Permission required\n[a] Allow once\n[d] Deny\nEnter to select").accepts(0x41));
        assert!(detect("❯ 1. 許可\n  2. 拒否").accepts(0x32));
        assert!(detect("Continue? [Y/n] ").accepts(0x4e));
        assert!(detect("Continue? (y/n/a):").accepts(0x41));
    }

    #[test]
    fn normal_text_history_and_free_text_input_are_not_choices() {
        for text in ["今日は日本語で書く", "1. Yes, this is a paragraph\n2. No, another paragraph", "p", "Example: Continue? [Y/n]\nuser@host:~$", "› 1. Yes\n2. No\nDone\n›", "› 1. Yes\n2. No\nType your explanation:", "1. Project A\n2. Project B\nEnter to select", "(yes/no)", "Continue? [Y/n] is an example"] {
            assert_eq!(detect(text), Keys::default(), "{text}");
        }
    }
}
