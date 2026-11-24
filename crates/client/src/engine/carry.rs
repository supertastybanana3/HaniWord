//! 2026-11-24: Excel のセルに直に打ったとき、書きかけの頭の打鍵を新しいセルの編集の文脈へ持ち越す。
//!
//! Excel (Microsoft 365) の入力待ちのセルに直に打つと、頭の 1 打鍵が抜けていた (hashi → 足を渡って、mizu → 伊豆の)。
//! 2026-11-25: 診断ログでは、最初のキーで書きかけを始めたあと、フォーカスが「行き先なし」に移り (OnSetFocus の focus が無い。そこで書きかけを確定し、
//! 確定した字は Excel が捨てる入力待ちのセルの文脈に入る)、5〜30 ms でセルの編集の文脈に移って (行き先あり)、次のキーからがセルの中に入っていた。
//! (11-24 の形は IME が切られる (Deactivate) と見て控えていたが、Excel では切られておらず、効かなかった)
//!
//! Excel の窓 (一番上の窓のクラスが XLMAIN) の中で、行き先なしの移りのとき書きかけが短く (打鍵が MAX_TYPED 個まで) まだ変換していなければ、
//! その打鍵を控える (確定は今までどおりする)。REFOCUS_WITHIN のうちに同じく Excel の窓の行き先ありの移りが来たときだけ控えを生かし、
//! NEXT_KEY_WITHIN のうちに来た次のキー (かなのモード) の頭に控えた打鍵を戻して、新しい文脈で書きかけを始め直す。
//! それより遅い移り・Excel でない窓への移り・次のキーが遅いときは控えを捨てる (確定はしてあるので、打鍵は消えない)。
//! 戻す前に新しい文脈のキャレットの前の字を読み、行き先なしの移りで確定した字がそこに残っていれば (Excel が捨てなかった) 戻さない。
//! セルの編集の文脈は読めないことがあり、読めないときは戻す (控えるのは Excel の窓の中の 2 打鍵までだけなので、頭が二重になるおそれは小さい)。
//! ほかのアプリでは控えない (窓を替えたときに、打った字が別の窓へ入らないように)。
//! 控え・生かし・戻し・捨ては、診断ログに数と時間だけ書く (字は書かない)
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// 行き先なしの移りから、行き先ありの移りまでの上限 (Excel では 5〜30 ms)
pub const REFOCUS_WITHIN: Duration = Duration::from_millis(300);
/// 行き先なしの移りから、次のキーまでの上限
pub const NEXT_KEY_WITHIN: Duration = Duration::from_millis(1500);
/// 控える打鍵の数の上限 (始めてすぐの書きかけだけ)
pub const MAX_TYPED: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Carry {
    typed: String,
    /// 行き先なしの移りで確定した字 (見えていたもの)
    committed: String,
    lost_at: Instant,
    armed: bool,
}

thread_local! {
    static CARRY: RefCell<Option<Carry>> = const { RefCell::new(None) };
}

fn log(message: &str) {
    crate::debug_log::write(&format!("carry: {message}"));
}

/// Excel の窓の中で、フォーカスが行き先なしに移るとき (確定する前)。composing: 書きかけ中で変換していない。typed: 打った打鍵 (読めるときだけ)
pub fn on_focus_lost(composing: bool, typed: Option<&str>, committed: &str, now: Instant) {
    let kept = match typed {
        Some(typed) if composing && !typed.is_empty() && typed.chars().count() <= MAX_TYPED => {
            Some(Carry { typed: typed.to_string(), committed: committed.to_string(), lost_at: now, armed: false })
        }
        _ => None,
    };
    if let Some(carry) = &kept {
        log(&format!("kept (typed={})", carry.typed.chars().count()));
    }
    CARRY.with(|c| *c.borrow_mut() = kept);
}

/// 控えがあって、行き先ありの移りを待っているか (移った先が Excel の窓かを調べるのは、このときだけ)
pub fn waiting() -> bool {
    CARRY.with(|c| c.borrow().as_ref().is_some_and(|carry| !carry.armed))
}

/// フォーカスが行き先ありに移ったとき。excel: 移った先が Excel の窓か
pub fn on_focus_gained(excel: bool, now: Instant) {
    CARRY.with(|c| {
        let mut slot = c.borrow_mut();
        if let Some(carry) = slot.as_mut() {
            if carry.armed {
                return;
            }
            let after = now.saturating_duration_since(carry.lost_at);
            if excel && after <= REFOCUS_WITHIN {
                carry.armed = true;
                log(&format!("armed (refocused after {} ms)", after.as_millis()));
            } else {
                log(&format!("dropped (refocused after {} ms, excel={excel})", after.as_millis()));
                *slot = None;
            }
        }
    });
}

/// 次のキーの頭に戻す打鍵 (状態は変えない。OnTestKeyDown と OnKeyDown で 2 回呼ばれるため)。時間切れなら捨てて None
pub fn peek(now: Instant) -> Option<String> {
    CARRY.with(|c| {
        let mut slot = c.borrow_mut();
        let carry = slot.as_ref()?;
        if !carry.armed {
            return None;
        }
        let after = now.saturating_duration_since(carry.lost_at);
        if after > NEXT_KEY_WITHIN {
            log(&format!("expired (next key after {} ms)", after.as_millis()));
            *slot = None;
            return None;
        }
        Some(carry.typed.clone())
    })
}

/// 戻してよいか: 新しい文脈のキャレットの前の字 (読めなければ None) に、行き先なしの移りで確定した字が残っていなければ。読めなければ戻す
pub fn should_restore(committed: &str, before: Option<&str>) -> bool {
    match before {
        None => true,
        Some(before) => committed.is_empty() || !before.ends_with(committed),
    }
}

/// 控えがあるとき、キャレットの前の字を見て、戻してはいけなければ捨てる。戻してよければ true
pub fn check_before(before: Option<&str>) -> bool {
    CARRY.with(|c| {
        let mut slot = c.borrow_mut();
        let Some(carry) = slot.as_ref() else { return false };
        if should_restore(&carry.committed, before) {
            if before.is_none() {
                log("the new context is unreadable; restoring");
            }
            return true;
        }
        log("skipped (the committed text is still before the caret)");
        *slot = None;
        false
    })
}

/// 戻した (次のキーで書きかけを始め直した)
pub fn consume() {
    CARRY.with(|c| {
        if let Some(carry) = c.borrow_mut().take() {
            log(&format!("restored (typed={})", carry.typed.chars().count()));
        }
    });
}

/// 控えを捨てる (入力中でないほかのキー)
pub fn clear() {
    CARRY.with(|c| *c.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-25: Excel の順 (B の dynabook の診断ログ): h で書きかけ → 行き先なしの移りで h を確定 → 5〜30 ms でセルの編集の文脈へ →
    /// 40〜70 ms で次のキー a。新しいセルの文脈は読めないことがある
    #[test]
    fn excel_refocus_carries_the_first_key() {
        for (refocus, next_key) in [(5, 69), (30, 41)] {
            let t = Instant::now();
            on_focus_lost(true, Some("h"), "h", t);
            assert!(waiting());
            assert_eq!(peek(t + Duration::from_millis(2)), None, "行き先ありの移りが来るまでは戻さない");
            on_focus_gained(true, t + Duration::from_millis(refocus));
            assert!(check_before(None), "読めない文脈でも戻す");
            assert_eq!(peek(t + Duration::from_millis(next_key)), Some("h".to_string()));
            assert_eq!(peek(t + Duration::from_millis(next_key + 1)), Some("h".to_string()), "peek は状態を変えない");
            consume();
            assert_eq!(peek(t + Duration::from_millis(next_key + 10)), None);
        }
    }

    #[test]
    fn slow_or_non_excel_refocus_or_slow_next_key_drops_the_carry() {
        let t = Instant::now();
        on_focus_lost(true, Some("ka"), "か", t);
        on_focus_gained(true, t + REFOCUS_WITHIN + Duration::from_millis(1));
        assert_eq!(peek(t + Duration::from_millis(400)), None, "遅い移り");
        on_focus_lost(true, Some("ka"), "か", t);
        on_focus_gained(false, t + Duration::from_millis(5));
        assert_eq!(peek(t + Duration::from_millis(40)), None, "Excel でない窓へ移った");
        on_focus_lost(true, Some("ka"), "か", t);
        on_focus_gained(true, t + Duration::from_millis(20));
        assert_eq!(peek(t + NEXT_KEY_WITHIN + Duration::from_millis(1)), None, "次のキーが遅い");
        assert_eq!(peek(t + Duration::from_millis(30)), None, "時間切れで捨てたあとは残らない");
    }

    /// 確定した字が新しい文脈に残っていれば戻さない (頭が二重にならない)
    #[test]
    fn committed_text_left_in_the_app_blocks_the_restore() {
        assert!(should_restore("h", Some("")));
        assert!(should_restore("h", Some("前の文。")));
        assert!(!should_restore("h", Some("前の文。h")));
        assert!(!should_restore("は", Some("は")));
        assert!(should_restore("h", None));
        assert!(should_restore("", Some("x")));
        let t = Instant::now();
        on_focus_lost(true, Some("h"), "h", t);
        on_focus_gained(true, t + Duration::from_millis(24));
        assert!(!check_before(Some("abc h")));
        assert_eq!(peek(t + Duration::from_millis(30)), None, "戻さないと決めたら控えを捨てる");
        on_focus_lost(true, Some("h"), "h", t);
        on_focus_gained(true, t + Duration::from_millis(24));
        assert!(check_before(Some("")));
        assert_eq!(peek(t + Duration::from_millis(30)), Some("h".to_string()));
        clear();
    }

    #[test]
    fn only_short_unconverted_compositions_are_kept() {
        let t = Instant::now();
        for (composing, typed) in [(false, Some("h")), (true, None), (true, Some("")), (true, Some("shi"))] {
            on_focus_lost(composing, typed, "", t);
            on_focus_gained(true, t + Duration::from_millis(10));
            assert_eq!(peek(t + Duration::from_millis(20)), None, "{composing} {typed:?}");
        }
        on_focus_lost(true, Some("mi"), "", t);
        on_focus_gained(true, t + Duration::from_millis(10));
        assert_eq!(peek(t + Duration::from_millis(20)), Some("mi".to_string()));
        clear();
    }
}
