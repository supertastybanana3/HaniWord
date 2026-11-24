// 英数モードで打った語を覚える (keltype のような語をかなで打つと けltype になり、変換でも戻せないため)。
//
// パスワードを覚えないように、和文の中で打った語だけをファイルに書く:
//   - 語の直前に日本語を確定していた
//   - または、語のあと Enter・Tab・Esc・フォーカス移動を挟まずに日本語を確定した
// どちらでもない語は、そのアプリを開いている間だけメモリで覚える (auto_english::note_latin_word)。
// 2026-10-20 (Codex の再調査 M1): パスワード・IS_PRIVATE などの欄で打った語は、メモリにもファイルにも覚えない。
// 語の打ち始めに欄を確かめ、日本語を確定したときも今の欄が IS_PRIVATE なら保留の語ごと捨てる

use std::cell::RefCell;

use super::auto_english::{note_latin_word, persist_english};
use super::user_action::UserAction;

#[derive(Default)]
struct State {
    /// 英数モードで打ちかけの語
    word: String,
    /// その語を打ち始めた欄が、覚えてはいけない欄 (パスワード・IS_PRIVATE など) か
    word_private: bool,
    /// その語を打ち始めたとき、直前に日本語を確定していたか
    japanese_before: bool,
    /// 直前に日本語を確定したか (Enter などで忘れる)
    japanese_recent: bool,
    /// 和文の中か分からない語。日本語の確定が続けばファイルに書く
    pending: Vec<String>,
    /// 同じキーの 2 回目の呼び出しを除く (メッセージの時刻, 仮想キー)
    last_key: Option<(i32, usize)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn finish_word(state: &mut State) {
    let word = std::mem::take(&mut state.word);
    let private = std::mem::take(&mut state.word_private);
    // 2026-11-24: 行頭の / のあとで日本語に戻した綴りを、利用者が消して英数モードで打ち直した → コマンド名として覚える (slash_commands)
    if let Some(command) = super::slash_commands::latin_word_typed(&word, private) {
        super::slash_commands::learn(&command);
    }
    if private {
        return;
    }
    if note_latin_word(&word) {
        if state.japanese_before {
            persist_english(&word);
        } else {
            state.pending.push(word);
        }
    }
}

/// 未確定文字列がないときのキーを見る (英数モードの打鍵と、和文の切れ目)。
/// `field_private` は語の打ち始めにだけ呼ぶ (今の欄が覚えてはいけない欄か)
pub fn observe_key(wparam: usize, action: &UserAction, latin_mode: bool, field_private: impl FnOnce() -> bool) {
    let time = unsafe { windows::Win32::UI::WindowsAndMessaging::GetMessageTime() };
    // 欄の確かめは編集セッションを開くので、STATE を借りる前に済ませる
    let starts_word = latin_mode && matches!(action, UserAction::Input(c) if c.is_ascii_alphabetic())
        && STATE.with_borrow(|state| state.word.is_empty() && state.last_key != Some((time, wparam)));
    let private = starts_word && field_private();
    STATE.with_borrow_mut(|state| {
        if state.last_key == Some((time, wparam)) {
            return;
        }
        state.last_key = Some((time, wparam));
        match action {
            UserAction::Unknown => {}
            UserAction::Input(c) if latin_mode && c.is_ascii_alphabetic() => {
                if state.word.is_empty() {
                    state.japanese_before = state.japanese_recent;
                    state.word_private = private;
                }
                state.word.push(*c);
            }
            UserAction::Backspace if latin_mode && !state.word.is_empty() => {
                state.word.pop();
            }
            // 送信・欄の移動: 和文の続きではない
            UserAction::Enter | UserAction::Tab | UserAction::Escape => forget(state),
            _ => {
                if latin_mode {
                    finish_word(state);
                }
            }
        }
    });
}

fn forget(state: &mut State) {
    finish_word(state);
    state.pending.clear();
    state.japanese_recent = false;
}

/// 日本語 (英数字だけでない文字列) を確定した
pub fn japanese_committed() {
    STATE.with_borrow_mut(|state| {
        if super::direct_field::private_field() {
            state.word.clear();
            state.word_private = false;
            state.pending.clear();
            state.japanese_recent = false;
            return;
        }
        finish_word(state);
        for word in state.pending.drain(..) {
            persist_english(&word);
        }
        state.japanese_recent = true;
    });
}

/// フォーカスが移った・英数モードに切り替えた等で、打ちかけの語を区切る
pub fn focus_changed() {
    STATE.with_borrow_mut(forget);
    super::slash_commands::forget_switched();
}

/// 英数モードからかなモードに戻った (打ちかけの語はここで終わり。和文の続きかは次の確定で決まる)
pub fn mode_changed() {
    STATE.with_borrow_mut(finish_word);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_word(word: &str, private: bool) {
        for (i, c) in word.chars().enumerate() {
            observe_key(0x7000 + i, &UserAction::Input(c), true, || private);
        }
        focus_changed();
    }

    #[test]
    fn words_typed_in_private_fields_are_not_remembered() {
        type_word("zxqvwk", false);
        assert!(crate::engine::auto_english::session_english_for_test("zxqvwk"));
        type_word("zxqvwj", true);
        assert!(!crate::engine::auto_english::session_english_for_test("zxqvwj"));
    }

    #[test]
    fn japanese_commit_in_a_private_field_drops_pending_words() {
        observe_key(0x7100, &UserAction::Input('q'), true, || false);
        super::super::direct_field::note_scopes(Some(&[windows::Win32::UI::TextServices::IS_PRIVATE.0]));
        japanese_committed();
        let empty = STATE.with_borrow(|state| state.word.is_empty() && state.pending.is_empty() && !state.japanese_recent);
        super::super::direct_field::note_scopes(None);
        assert!(empty);
    }
}
