use super::input_mode::InputMode;

#[derive(Debug, PartialEq)]
pub enum ClientAction {
    StartComposition,
    /// 表示中の未確定文字列を確定して終える
    EndComposition,
    /// 未確定文字列を消して終える
    Cancel,
    /// 未確定を経ずにそのまま確定する (全角スペースなど)
    CommitText(String),

    // 入力中 (Composing)
    Insert(String),
    DeleteBackward,
    DeleteForward,
    MoveCaret(i32),
    CaretToEdge(bool), // true = 末尾

    // 変換中 (Converting)
    Convert,
    /// Ctrl+変換: 辞書変換と切り替え前の表記を往復する
    ToggleDictionaryConversion,
    /// Ctrl+無変換: 表記の自動変換 (H₂O・α) を外す・戻す (2026-10-13)
    ToggleNotation,
    /// 行頭の / のあとで素通しにしていた文字を、日本語の入力に戻す: アプリへ渡した delete 文字を Backspace で消し、text を打ち直す (2026-10-13)
    RetypeAsKana(usize, String),
    /// Ctrl+Shift+変換: 選択した漢字の登録済み IVS を切り替える
    CycleIvs,
    /// 区切らずに全体を 1 文節にする (入力中に F6〜F10 を押したとき)
    ConvertAsOne,
    CancelConversion,
    /// 2026-11-10 (Fable の諮問 ux 1): ライブ変換で、表示 (漢字) が読みと違うときの Esc。読み (かな) の表示に戻す (未確定は消さない。
    /// もう一度 Esc で消す)。次に読みを変える・動かすキーで、ライブ変換の表示に戻る
    ShowReading,
    NextCandidate,
    PrevCandidate,
    NextPage,
    PrevPage,
    SelectNumber(i32), // 候補ウィンドウの 1〜9
    MoveFocus(i32),
    FocusEdge(bool), // true = 最後の文節
    Resize(i32),
    ConvertType(SetTextType),
    /// 無変換キー: ひらがな → カタカナ → 半角カタカナ を巡回
    SwitchKanaType,
    /// 最初の文節だけ確定する
    CommitFirstSegment,
    /// 2026-10-14: 変換中にカーソルを文節の間へ動かして文字を打った: カーソルより左の文節を確定し、右側をかなの入力に戻してカーソルを頭に置く
    /// (このあとの Insert がカーソルの位置に入る)
    CommitBeforeCursor,
    /// 2026-10-14: 変換中の Backspace: カーソルの左の 1 字 (カーソルのある文節の読みの最後の字) を消して、その文節だけ変換し直す
    DeleteAtCursor,
    /// ライブ変換: 手が止まったら Zenzai で変換し直す
    RefineLive,
    /// 2026-11-06 (仕事 29 の 2): 上限で切った清書の答えが遅れて届いた (読みが同じなら仮表示を清書に替える)
    ApplyLateRefine,
    /// 2026-11-06 (設定 input.smart_punctuation): 半角英数で - を 2 回続けた。直前の - を — (em dash) に入れ替える
    SmartDash,
    /// 2026-11-06 (仕事 34、input.period_shortcut): 半角英数で空白に続けて Space。直前の空白を「. 」に入れ替える
    PeriodShortcut,
    /// 「単漢字…」を選んで Enter: 単漢字の一覧に切り替える
    OpenSingleKanji,
    /// 確定直後の Ctrl+Backspace: 直前の確定を取り消して変換中に戻す
    UndoCommit,
    /// 2026-11-05 (仕事 24): 英語の文のつづりの自動修正で確定した直後の Backspace: 打ったとおりの綴りに戻し、その綴りを次から直さない
    UndoAutocorrect,
    /// 2026-10-28 (試験中、設定 input.punctuation_convert): 。、？！ まで変換する (候補の窓は開かない。確定は次の字か Enter)
    PunctuationConvert,
    /// 2026-10-29: 候補の一覧を開いているときの Ctrl+Delete。選んでいる候補が学習した語なら忘れて一覧から外し、次の候補を選ぶ (辞書の語は何もしない)
    ForgetCandidate,
    /// 2026-10-28 (試験中、設定 input.typo_repair): 母音の抜けを直した直後の Shift+Backspace。打ったとおりに戻して変換し直す (戻した綴りは次から直さない)
    UndoTypoRepair,
    /// TeX: 日本語を確定したあと $ を打った (行内の数式に入る)
    TexEnterMath,
    /// 次の確定の末尾に足す文字
    CommitSuffix(String),
    /// 長押し: アクセント付きの文字の候補を出す / 選ぶ (1 始まり) / やめる
    ShowAccents(char),
    PickAccent(i32),
    /// 長押しの候補の選択を動かす (+1 = 次)
    MoveAccent(i32),
    /// 長押しの候補でいま選んでいるものに決める
    PickCurrentAccent,
    CloseAccents,
    /// 予測変換: 手が止まったら、打ちかけの読みの続きの候補を出す (選ばない状態で)
    ShowPredictions,
    /// 予測変換の候補を選ぶ (Tab = 先頭から、Shift+Tab = 最後から)
    UsePrediction(bool),
    /// 2026-10-19: 次の語の候補の n 番目で、打ちかけの英単語を補う (Tab / Shift+Tab)
    CompleteNextWord(usize),
    /// 2026-10-19: 英数モードで、キャレットの前の打ちかけの語を次の語の候補の n 番目に置き換える (Tab / Shift+Tab)
    CompleteLatinWord(usize),
    /// Space selects the contextual width; Shift+Space inverts that same decision.
    CommitSmartSpace { reverse: bool },

    SetIMEMode(InputMode),
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SetTextType {
    Hiragana,     // F6
    Katakana,     // F7
    HalfKatakana, // F8
    FullLatin,    // F9
    HalfLatin,    // F10
}
