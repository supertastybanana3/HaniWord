//! 2026-11-05 (仕事 24): ラテン文字のつづりの指摘と自動修正 (設定 input.latin_spell: "off"・"suggest"・"autocorrect"。初期値は off)。
//! 見るのは、かなの入力中に英字の並びと判定した英語の文の語だけ (半角英数の直接入力・日本語に混ざった英語・端末・コードの欄は見ない)。
//! - 指摘: 打っている語の次の語の候補の一番上に、直した形を出す (Tab で補う)。Space では打ったとおりに確定する
//! - 自動修正: Space で確定するとき、自動修正の線も越えた語だけ直した形にする (直後の BackSpace で戻し、戻した綴りは次から直さない)
//! 判断: 打った語がどの表にも無い (か、まれ) で、1 字違い (抜け・余り・置き換え・入れ替え。隣のキーは軽く) かアクセントの付け忘れの語が、
//! 前の語とのつながりを含めてずっと出やすいときだけ。2 番目の候補と差が小さい (迷う) ときは、直さず指摘もしない。
//! 表は次の語の候補の表 (next_<言語>.hwn。英語と、設定で選んだアクセントの言語) を引くだけで、毎回の判断は軽い。
//! 判断の本体はこのファイルの標準ライブラリだけの部分で、測りの道具 (tools/latin_spell) も同じものを読み込む。IME につなぐ所は下の ime (Windows だけ)

/// 表 1 つ (言語 1 つ) の引き方。点はどれも -ln P * 1000 (小さいほど出やすい)
pub trait Model {
    fn languages(&self) -> usize;
    /// その言語の小文字 (トルコ語は I → ı)
    fn lower(&self, lang: usize, word: &str) -> String;
    /// 小文字の語の番号
    fn find(&self, lang: usize, lower: &str) -> Option<usize>;
    /// 語だけの点
    fn uni(&self, lang: usize, id: usize) -> u32;
    /// 前の語のあとの点 (表に組が無ければ 語だけの点 + 前の語の係数)
    fn after(&self, lang: usize, prev: usize, id: usize) -> u32;
    /// 表に残っている組の点だけ (無ければ None。表の組は、前の語を見ると順位が変わるものだけを残してある)
    fn pair(&self, lang: usize, prev: usize, id: usize) -> Option<u32>;
    /// 書く形 (I・Monday・Vater)
    fn display(&self, lang: usize, id: usize) -> String;
    /// その言語の語に出てくる字 (小文字。アクセントつきの字も)
    fn alphabet(&self, lang: usize) -> &[char];
    /// 小文字の prefix で始まる、より長い語があるか (打っている途中の語は直さない)
    fn longer_word(&self, lang: usize, prefix: &str) -> bool;
    /// アクセントを外した形 (base_char で 1 字ずつ) が base になる語 (番号, 小文字の語)。élève・élevé は eleve から引ける
    fn accent_variants(&self, lang: usize, base: &str) -> Vec<(usize, String)>;
    /// 名詞の頭を大文字で書く言語 (ドイツ語) の大文字の表。無ければ None (ほかの言語)
    fn capitals(&self, _lang: usize) -> Option<&Capitals> {
        None
    }
}

/// 2026-11-05: ドイツ語の名詞の頭の大文字の表 (german_case.tsv。tools/latin_spell/german_case で作る)。どれも文の途中の語の割合
#[derive(Default, Debug)]
pub struct Capitals {
    /// 両方の書き方がある語が大文字だった割合 (前の語の割合を比べる元)
    pub global: f32,
    /// 語 → 大文字で始まった割合
    pub words: std::collections::HashMap<String, f32>,
    /// 前の語 → そのあとで、両方の書き方がある語が大文字だった割合
    pub after: std::collections::HashMap<String, f32>,
    /// "前の語 語" → その組で大文字だった割合と回数 (語の割合と大きく違う組だけ)
    pub pairs: std::collections::HashMap<String, (f32, u32)>,
}

impl Capitals {
    pub fn parse(text: &str) -> Capitals {
        let mut table = Capitals { global: 0.5, ..Default::default() };
        for line in text.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            let share = |i: usize| cols.get(i).and_then(|v| v.parse::<f32>().ok()).filter(|v| (0.0..=1.0).contains(v));
            match cols[0] {
                "#global" => if let Some(v) = share(1) { table.global = v },
                "w" if cols.len() >= 3 => if let Some(v) = share(2) { table.words.insert(cols[1].to_string(), v); },
                "a" if cols.len() >= 3 => if let Some(v) = share(2) { table.after.insert(cols[1].to_string(), v); },
                "p" if cols.len() >= 4 => if let (Some(v), Ok(n)) = (share(2), cols[3].parse::<u32>()) { table.pairs.insert(cols[1].to_string(), (v, n)); },
                _ => {}
            }
        }
        table
    }

    /// 前の語 prev (小文字。無ければ空) のあとの小文字の語 word が、大文字で始まる見込み (表に無い語は None)
    pub fn probability(&self, prev: &str, word: &str) -> Option<f32> {
        let share = *self.words.get(word)?;
        if !prev.is_empty() {
            if let Some(&(pair, n)) = self.pairs.get(&format!("{prev} {word}")) {
                // 組の割合を、語の割合で少しならす (回数の少ない組を信じすぎない)
                return Some((pair * n as f32 + 5.0 * share) / (n as f32 + 5.0));
            }
        }
        let logit = |p: f32| { let p = p.clamp(0.001, 0.999); (p / (1.0 - p)).ln() };
        let mut z = logit(share);
        if let Some(&after) = self.after.get(prev) {
            z += logit(after) - logit(self.global);
        }
        Some(1.0 / (1.0 + (-z).exp()))
    }
}

/// アクセントを外した形 (1 字ずつ base_char)
pub fn strip_accents(word: &str) -> String {
    word.chars().map(base_char).collect()
}

/// アクセントの違いだけの語か: 打った語のアクセントのある字はそのまま、アクセントの無い字にだけアクセントを足した形なら、足した字の数
fn added_accents(typed: &[char], candidate: &str) -> Option<usize> {
    let candidate: Vec<char> = candidate.chars().collect();
    if candidate.len() != typed.len() {
        return None;
    }
    let mut added = 0;
    for (&t, &c) in typed.iter().zip(&candidate) {
        if t == c {
            continue;
        }
        if base_char(t) != t || base_char(c) != t {
            return None;
        }
        added += 1;
    }
    (added > 0).then_some(added)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Off,
    Suggest,
    Autocorrect,
}

impl Level {
    pub fn from_setting(value: &str) -> Level {
        match value {
            "suggest" => Level::Suggest,
            "autocorrect" => Level::Autocorrect,
            _ => Level::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// アクセントの付け忘れ・付け違い (tambien → también)
    Accent,
    /// ドイツ語の名詞の頭の大文字 (haus → Haus、zum leben → zum Leben)
    Capital,
    /// 1 字違い (becuase → because)
    Edit,
}

/// 直す所。start..end は渡した文字列の中のバイトの位置
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub start: usize,
    pub end: usize,
    pub typed: String,
    pub fixed: String,
    pub kind: Kind,
    /// 自動修正の線 (指摘より厳しい) も越えたか
    pub autocorrect: bool,
    /// 打っている語ではなく、その前の語を直す指摘か (tu hablas → tú hablas)。start..end は before の中の位置。指摘だけで、自動修正しない
    pub previous: bool,
}

/// 線。点の単位は -ln P * 1000 (1000 で e 倍、2303 で 10 倍)
#[derive(Clone, Debug)]
pub struct Thresholds {
    /// 見る語の字数の下限
    pub min_len: usize,
    /// 自動修正する語の字数の下限
    pub auto_min_len: usize,
    /// 直した形の語だけの点の上限 (まれな語には直さない)
    pub max_candidate_uni: u32,
    /// 打った語が表にあっても、語だけの点がこれより大きい (まれ) なら、1 字違いも見る
    pub rare_known: u32,
    /// 打った語が表に無いとき: 2 番目の候補との差の下限 (指摘・自動修正)
    pub suggest_margin: u32,
    pub auto_margin: u32,
    /// 打った語が表にあるとき (まれな語・アクセントだけ違う語): 打った語の点との差の下限 (指摘・自動修正)
    pub known_suggest_margin: u32,
    pub known_auto_margin: u32,
    /// 直した形の点 (前の語のあとの点 + 打ち間違いの点) の上限
    pub max_total: u32,
    /// 打った語が表に無いときの、その語の点 (表に無い正しい語 (活用形・名前) もあるので、直した形はこれよりずっと出やすいときだけ)
    pub unknown_cost: u32,
    /// 語の終わりの 1 字を足す・消す直し (hungers → hunger) に足す点 (表に無い活用形を削らないように)
    pub end_edit: u32,
    /// 文の頭の大文字で始まる語も見るか (0 なら見ない)
    pub sentence_start_capital: u32,
    /// 前の語のアクセント (外すと別の正しい語になる組: tu・tú、a・à) の指摘の線: 前後の語とのつながりの点の差の下限
    pub previous_accent_margin: u32,
    /// ドイツ語の名詞の頭の大文字の線 (大文字で始まる見込み、千分率。指摘・自動修正)
    pub capital_suggest: u32,
    pub capital_auto: u32,
}

/// 2026-11-05: 線は Tatoeba の試験用の文 (14 言語) の dev (偶数行) で、指摘・自動修正・表に無い語の点の組み合わせ 13 通りから選んだ
/// (tools/latin_spell の README)。自動修正は、正しい文を変える数と違う形に直す数がほぼ 0 になる厳しい線
impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            min_len: 3,
            auto_min_len: 4,
            max_candidate_uni: 13_000,
            rare_known: 15_000,
            suggest_margin: 3_500,
            auto_margin: 6_900,
            known_suggest_margin: 4_600,
            known_auto_margin: 6_900,
            max_total: 20_000,
            unknown_cost: 15_000,
            end_edit: 2_000,
            sentence_start_capital: 1,
            // dev (13 言語 6.9 万組) で 2300〜13800 を比べた。11500 以上だと Tu hablas → Tú hablas (点の差 9341) が出ないので 9200
            // (誤った指摘 0.038%。残るのは el vino (ワイン)・él vino (来た) のように後ろの語だけでは決まらない組)
            previous_accent_margin: 9_200,
            // dev のドイツ語 (12,872 文) で 900〜990 を比べ、正しい文の小文字の語を大文字にした数が 指摘 5・自動 0 になる線 (直る名詞 58.3%・37.3%)
            capital_suggest: 980,
            capital_auto: 990,
        }
    }
}

/// 打ち間違いの点 (-ln P * 1000)
const COST_SAME: u32 = 100;
const COST_ACCENT: u32 = 700;
/// アクセントを 2 つ以上足すときの、1 つ増えるごとの点 (付け忘れは語ぜんぶに付けないことが多いので、少しずつしか増やさない)
const COST_ACCENT_MORE: u32 = 300;
const COST_TRANSPOSE: u32 = 3_000;
const COST_SUB_NEAR: u32 = 3_200;
const COST_SUB_FAR: u32 = 6_000;
const COST_DOUBLE: u32 = 2_800;
const COST_DELETE: u32 = 3_800;
const COST_INSERT_NEAR: u32 = 3_400;
const COST_INSERT_FAR: u32 = 5_000;
/// 前の語を知っている言語があるのに、この言語は前の語を知らないとき
const UNKNOWN_CONTEXT_PENALTY: u32 = 2_000;

/// QWERTY で隣り合うキー
fn near(a: char, b: char) -> bool {
    const ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
    let pos = |c: char| ROWS.iter().enumerate().find_map(|(r, row)| row.find(c).map(|i| (r as i32, i as i32)));
    let (Some((ra, ia)), Some((rb, ib))) = (pos(a), pos(b)) else { return false };
    // 下の段は上の段より半キー右にずれている (w の下は a と s、s の下は z と x)。上の段の i 番目と隣り合うのは、下の段の i 番目と i-1 番目
    (ra == rb && (ia - ib).abs() == 1) || ((ra - rb).abs() == 1 && {
        let (upper, lower) = if ra < rb { (ia, ib) } else { (ib, ia) };
        lower == upper || lower == upper - 1
    })
}

/// アクセントを外した字 (é → e、ß・æ・ø・ł などは 1 字の元の字)
pub fn base_char(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ą' | 'ǎ' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'ď' | 'ð' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => 'e',
        'ğ' => 'g',
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ı' | 'ǐ' => 'i',
        'ł' => 'l',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ǒ' => 'o',
        'ř' => 'r',
        'ś' | 'š' | 'ş' | 'ß' => 's',
        'ť' | 'þ' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ǔ' | 'ǖ' | 'ǘ' | 'ǚ' | 'ǜ' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        'æ' => 'a',
        _ => c,
    }
}

/// 1 字違いの候補 (小文字の語, 打ち間違いの点)。alphabet はその言語の字
fn edits(word: &[char], alphabet: &[char], end_edit: u32, mut visit: impl FnMut(String, u32, Kind)) {
    let n = word.len();
    let make = |chars: &[char]| chars.iter().collect::<String>();
    // 打った語のアクセントのある字 (é・ß・ı) は、その言語を意識して打った字なので、置き換え・削り・入れ替えをしない (daß → das にしない)
    let typed_mark = |c: char| base_char(c) != c;
    // 置き換え (アクセントの付け忘れ (e → é) を含む)
    for i in 0..n {
        let original = word[i];
        if typed_mark(original) {
            continue;
        }
        for &c in alphabet {
            if c == original {
                continue;
            }
            let (cost, kind) = if base_char(c) == original {
                (COST_ACCENT, Kind::Accent)
            } else if near(original, base_char(c)) {
                (COST_SUB_NEAR, Kind::Edit)
            } else {
                (COST_SUB_FAR, Kind::Edit)
            };
            let mut next = word.to_vec();
            next[i] = c;
            visit(make(&next), cost, kind);
        }
    }
    // 入れ替え
    for i in 0..n.saturating_sub(1) {
        if word[i] == word[i + 1] || typed_mark(word[i]) || typed_mark(word[i + 1]) {
            continue;
        }
        let mut next = word.to_vec();
        next.swap(i, i + 1);
        visit(make(&next), COST_TRANSPOSE, Kind::Edit);
    }
    // 余り (打った語の字を 1 つ消す)。語の終わりの 2 字の足し引きは活用の語尾の取り違え (bestehst → besteht) のことが多いので end_edit を足す
    for i in 0..n {
        if typed_mark(word[i]) {
            continue;
        }
        let doubled = (i > 0 && word[i - 1] == word[i]) || (i + 1 < n && word[i + 1] == word[i]);
        let close = (i > 0 && near(word[i - 1], word[i])) || (i + 1 < n && near(word[i + 1], word[i]));
        let cost = if doubled { COST_DOUBLE } else if close { COST_INSERT_NEAR } else { COST_INSERT_FAR };
        let mut next = word.to_vec();
        next.remove(i);
        visit(make(&next), cost + if i + 2 >= n { end_edit } else { 0 }, Kind::Edit);
    }
    // 抜け (字を 1 つ足す)
    for i in 0..=n {
        for &c in alphabet {
            let doubled = (i > 0 && word[i - 1] == c) || (i < n && word[i] == c);
            let mut next = word.to_vec();
            next.insert(i, c);
            visit(make(&next), (if doubled { COST_DOUBLE } else { COST_DELETE }) + if i + 1 >= n { end_edit } else { 0 }, Kind::Edit);
        }
    }
}

/// 文字列の中の語 (字と語の中の ' ’)。前後の記号は外し、語の中に字でないもの (数字・/・@・. など) がある塊は飛ばす (URL・メール・パス・数)
pub fn words(text: &str) -> Vec<(usize, usize, bool)> {
    // (start, end, 文の区切りのあと)
    let mut out = Vec::new();
    let mut sentence_start = true;
    let mut pos = 0;
    for chunk in text.split(char::is_whitespace) {
        let start = pos;
        pos += chunk.len() + text[pos + chunk.len()..].chars().next().map_or(0, |c| c.len_utf8());
        if chunk.is_empty() {
            continue;
        }
        let is_edge = |c: char| !c.is_alphanumeric() && !"/@\\_-".contains(c);
        let core_start = chunk.char_indices().find(|&(_, c)| !is_edge(c)).map(|(i, _)| i);
        let core_end = chunk.char_indices().rev().find(|&(_, c)| !is_edge(c)).map(|(i, c)| i + c.len_utf8());
        let ends_sentence = chunk.trim_end_matches(|c: char| "\"'”’)»".contains(c)).ends_with(['.', '!', '?']);
        if let (Some(a), Some(b)) = (core_start, core_end) {
            let core = &chunk[a..b];
            let wordlike = core.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '’');
            // 記号の付いた塊 (http://・a@b・~/x) は前後でも飛ばす
            let marked = chunk.contains("://") || chunk.contains('@') || chunk.starts_with(['/', '~', '\\', '#', '$', '<']);
            if wordlike && !marked {
                out.push((start + a, start + b, sentence_start));
            }
        }
        sentence_start = ends_sentence;
    }
    out
}

/// 打った語の大文字に合わせる (Teh → The)。表の書く形 (I・Monday) は、打った語が小文字なら使う
fn match_case(display: &str, typed: &str) -> String {
    let first_upper = typed.chars().next().is_some_and(|c| c.is_uppercase());
    if !first_upper {
        return display.to_string();
    }
    let mut chars = display.chars();
    chars.next().map(|head| head.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// 1 語を判断する。prev は同じ文の前の語 (無ければ空)。protected は直さない語 (固有名詞・利用者の語・戻された綴り。小文字で渡す)
pub fn judge(model: &dyn Model, th: &Thresholds, protected: &dyn Fn(&str) -> bool, prev: &str, typed: &str, sentence_start: bool,
             typing: bool) -> Option<Fix> {
    let letters: Vec<char> = typed.chars().collect();
    if letters.len() < th.min_len || letters.iter().any(|&c| !(c.is_alphabetic() || c == '\'' || c == '’')) {
        return None;
    }
    // 大文字が途中にある語 (camelCase)・全部大文字の語は直さない。文の頭でない大文字で始まる語は名前のことが多いので直さない
    if letters.iter().skip(1).any(|c| c.is_uppercase()) || (letters[0].is_uppercase() && (!sentence_start || th.sentence_start_capital == 0)) {
        return None;
    }
    // ’ は ' として引く (d’une と d'une は同じ語)
    let typed_plain = typed.replace('’', "'");
    let languages = model.languages();
    if languages == 0 {
        return None;
    }
    let lowered: Vec<String> = (0..languages).map(|l| model.lower(l, &typed_plain)).collect();
    if lowered.iter().any(|w| protected(w)) {
        return None;
    }
    // ドイツ語の名詞の頭の大文字 (haus → Haus)。打った語が表にあり全部小文字で、文の頭でないとき。打っている途中 (より長い語の頭) でも出す
    if let Some(fix) = capital_fix(model, th, prev, typed, sentence_start) {
        return Some(fix);
    }
    // 打っている途中の語 (より長い語の頭) は直さない
    if typing && (0..languages).any(|l| model.longer_word(l, &lowered[l])) {
        return None;
    }
    // 前の語。知っている言語があれば、知らない言語には点を足す (前の語から言語を見分ける)
    let prev_ids: Vec<Option<usize>> = (0..languages).map(|l| if prev.is_empty() { None } else { model.find(l, &model.lower(l, prev)) }).collect();
    let any_prev = prev_ids.iter().any(Option::is_some);
    let lm = |l: usize, id: usize| -> u32 {
        match prev_ids[l] {
            Some(p) => model.after(l, p, id),
            None => model.uni(l, id).saturating_add(if any_prev { UNKNOWN_CONTEXT_PENALTY } else { 0 }),
        }
    };
    // 打った語の点 (表にあれば)
    let mut typed_cost = u32::MAX;
    let mut typed_uni = u32::MAX;
    for l in 0..languages {
        if let Some(id) = model.find(l, &lowered[l]) {
            typed_cost = typed_cost.min(lm(l, id).saturating_add(COST_SAME));
            typed_uni = typed_uni.min(model.uni(l, id));
        }
    }
    let known = typed_cost != u32::MAX;
    // よく使う語 (表にあり、まれでない) は直さない (アクセントだけの違い (sólo・solo、tu・tú) も前後の語で決まるので、確実とは言えない)
    if known && typed_uni <= th.rare_known {
        return None;
    }
    // 候補 (書く形, 点, 語だけの点, 種類, 小文字の語)。同じ書く形は点の良いほう
    let mut best: std::collections::HashMap<String, (u32, u32, Kind, String)> = std::collections::HashMap::new();
    // アクセントの違いだけの、表にある語 (2 つ以上あれば、どれか 1 つとは言えないので、アクセントの直しは出さない。elève・élève)
    let mut accent_words: std::collections::HashSet<String> = std::collections::HashSet::new();
    for l in 0..languages {
        let chars: Vec<char> = lowered[l].chars().collect();
        edits(&chars, model.alphabet(l), th.end_edit, |candidate, cost, kind| {
            let Some(id) = model.find(l, &candidate) else { return };
            let uni = model.uni(l, id);
            if uni > th.max_candidate_uni {
                return;
            }
            let total = lm(l, id).saturating_add(cost);
            let entry = best.entry(model.display(l, id)).or_insert((u32::MAX, uni, kind, String::new()));
            if total < entry.0 {
                *entry = (total, uni, kind, candidate);
            }
        });
        // アクセントの付け忘れは 1 語に 2 つ以上のこともある (eleve → élève、informacao → informação)。アクセントを外した形で表を引く
        for (id, candidate) in model.accent_variants(l, &strip_accents(&lowered[l])) {
            let Some(added) = added_accents(&chars, &candidate) else { continue };
            let uni = model.uni(l, id);
            if uni > th.max_candidate_uni {
                continue;
            }
            accent_words.insert(candidate.clone());
            let total = lm(l, id).saturating_add(COST_ACCENT + COST_ACCENT_MORE * (added as u32 - 1));
            let entry = best.entry(model.display(l, id)).or_insert((u32::MAX, uni, Kind::Accent, String::new()));
            if total < entry.0 {
                *entry = (total, uni, Kind::Accent, candidate);
            }
        }
    }
    // 打った語と同じ語 (書く形の違いだけ) は候補にしない
    best.retain(|display, _| lowered.iter().all(|w| display.to_lowercase() != *w));
    // アクセントの違いだけの語が 2 つ以上 (elève・élève) なら、アクセントの直しは出さない (前後の語で決める前の語の指摘は別)
    if accent_words.len() >= 2 {
        best.retain(|_, v| v.2 != Kind::Accent);
    }
    let mut ranked: Vec<(String, (u32, u32, Kind, String))> = best.into_iter().collect();
    ranked.sort_by(|a, b| a.1 .0.cmp(&b.1 .0).then_with(|| a.0.cmp(&b.0)));
    let (display, (total, _, kind, lower)) = ranked.first()?.clone();
    if total > th.max_total {
        return None;
    }
    let second = ranked.get(1).map_or(u32::MAX, |r| r.1 .0);
    // 打った語が表に無いときも、表に無い正しい語かもしれないので unknown_cost と比べる
    let rival = second.min(if known { typed_cost } else { th.unknown_cost });
    let (suggest_margin, auto_margin) = if known { (th.known_suggest_margin, th.known_auto_margin) } else { (th.suggest_margin, th.auto_margin) };
    let margin = rival.saturating_sub(total);
    if margin < suggest_margin {
        return None;
    }
    let autocorrect = margin >= auto_margin && letters.len() >= th.auto_min_len;
    // アクセントの直しは字の大きさを打ったとおりに (lugen → lügen。表の書く形が Lügen でも)。1 字違いは表の書く形 (i → I)
    let mut fixed = if kind == Kind::Accent { keep_case(&lower, typed) } else { match_case(&display, typed) };
    let mut autocorrect = autocorrect;
    // アクセントを付けたあとで、ドイツ語の名詞の頭の大文字を決める (lugen → lügen → die Lügen)
    if kind == Kind::Accent {
        if let Some(capital) = capital_fix(model, th, prev, &fixed, sentence_start) {
            autocorrect = autocorrect && capital.autocorrect;
            fixed = capital.fixed;
        }
    }
    Some(Fix { start: 0, end: typed.len(), typed: typed.to_string(), fixed, kind, autocorrect, previous: false })
}

/// ドイツ語の名詞の頭の大文字。打った語が全部小文字で表にあり、文の頭でなく、前の語との組・語の割合から大文字の見込みが線を越えるとき
fn capital_fix(model: &dyn Model, th: &Thresholds, prev: &str, typed: &str, sentence_start: bool) -> Option<Fix> {
    if sentence_start || typed.chars().any(|c| !c.is_lowercase()) {
        return None;
    }
    let mut best: Option<f32> = None;
    for l in 0..model.languages() {
        let Some(capitals) = model.capitals(l) else { continue };
        let word = model.lower(l, typed);
        let previous = model.lower(l, prev);
        if let Some(p) = capitals.probability(&previous, &word) {
            best = Some(best.map_or(p, |b| b.max(p)));
        }
    }
    let p = (best? * 1000.0) as u32;
    if p < th.capital_suggest {
        return None;
    }
    let mut chars = typed.chars();
    let fixed: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
    Some(Fix { start: 0, end: typed.len(), typed: typed.to_string(), fixed, kind: Kind::Capital,
               autocorrect: p >= th.capital_auto && typed.chars().count() >= th.auto_min_len, previous: false })
}

/// アクセントを付けた小文字の語に、打った語の字の大きさを 1 字ずつ当てる (字の数が同じとき。違えば頭だけ)
fn keep_case(lower: &str, typed: &str) -> String {
    let t: Vec<char> = typed.chars().collect();
    let l: Vec<char> = lower.chars().collect();
    if t.len() != l.len() {
        return match_case(lower, typed);
    }
    t.iter().zip(&l).flat_map(|(&tc, &lc)| -> Vec<char> { if tc.is_uppercase() { lc.to_uppercase().collect() } else { vec![lc] } }).collect()
}

/// 前の語のアクセント (Tu hablas → Tú hablas)。外すと別の正しい語になる組 (tu・tú、el・él、a・à) は、その語だけでは決まらないので、
/// すぐ後ろの語 (current) とのつながりで決める。前の語が表にある語で、アクセントだけ違う語が同じ表にあり、その語のあとに current が来る組が
/// 表に残っていて (表の組は前の語で順位が変わるものだけなので、残っていること自体が強いしるし)、前後のつながりの点が線より良いときだけ。
/// 指摘だけで、自動修正しない。before_prev は前の語のさらに前の語 (無ければ空)
pub fn judge_previous(model: &dyn Model, th: &Thresholds, protected: &dyn Fn(&str) -> bool, before_prev: &str, prev: &str,
                      prev_sentence_start: bool, current: &str) -> Option<Fix> {
    let letters: Vec<char> = prev.chars().collect();
    if letters.is_empty() || letters.iter().any(|c| !c.is_alphabetic()) || current.chars().count() < 2 || current.chars().any(|c| !c.is_alphabetic()) {
        return None;
    }
    if letters.iter().skip(1).any(|c| c.is_uppercase()) || (letters[0].is_uppercase() && !prev_sentence_start)
        || current.chars().skip(1).any(|c| c.is_uppercase()) {
        return None;
    }
    let mut best: Option<(u32, String)> = None;
    for l in 0..model.languages() {
        let lowered = model.lower(l, prev);
        if protected(&lowered) {
            return None;
        }
        let (Some(p), Some(w)) = (model.find(l, &lowered), model.find(l, &model.lower(l, current))) else { continue };
        let pp = if before_prev.is_empty() { None } else { model.find(l, &model.lower(l, before_prev)) };
        let left = |id: usize| pp.map_or_else(|| model.uni(l, id), |pp| model.after(l, pp, id));
        let base = left(p).saturating_add(model.after(l, p, w));
        let chars: Vec<char> = lowered.chars().collect();
        edits(&chars, model.alphabet(l), 0, |candidate, cost, kind| {
            if kind != Kind::Accent {
                return;
            }
            let Some(v) = model.find(l, &candidate) else { return };
            let Some(stored) = model.pair(l, v, w) else { return };
            let variant = left(v).saturating_add(stored).saturating_add(cost);
            let margin = base.saturating_sub(variant);
            if margin >= th.previous_accent_margin && best.as_ref().map_or(true, |(m, _)| margin > *m) {
                best = Some((margin, model.display(l, v)));
            }
        });
        // 2 つ以上のアクセントの違い (mas → más は 1 つだが、esta → está のように 1 つとは限らない語も同じ形で)
        for (v, candidate) in model.accent_variants(l, &strip_accents(&lowered)) {
            let Some(added) = added_accents(&chars, &candidate) else { continue };
            if added < 2 {
                continue;  // 1 つは上の edits で見た
            }
            let Some(stored) = model.pair(l, v, w) else { continue };
            let variant = left(v).saturating_add(stored).saturating_add(COST_ACCENT + COST_ACCENT_MORE * (added as u32 - 1));
            let margin = base.saturating_sub(variant);
            if margin >= th.previous_accent_margin && best.as_ref().map_or(true, |(m, _)| margin > *m) {
                best = Some((margin, model.display(l, v)));
            }
        }
    }
    let (_, display) = best?;
    Some(Fix { start: 0, end: prev.len(), typed: prev.to_string(), fixed: keep_case(&display.to_lowercase(), prev), kind: Kind::Accent, autocorrect: false, previous: true })
}

/// 打っている語の指摘。before は打っている語より前の同じ欄の文字列 (キャレットの前。読めなければ最後に確定した文字列)、typed は打っている語。
/// 打っている語の直し (1 字違い・アクセント) が無ければ、前の語のアクセント (tu hablas → tú hablas) を見る (fix.previous、位置は before の中)
pub fn suggestion_with(model: &dyn Model, th: &Thresholds, protected: &dyn Fn(&str) -> bool, before: &str, typed: &str) -> Option<Fix> {
    let found = words(before);
    // 前の語は、打っている語との間が空白だけのとき (句読点を挟むと、つながりの手がかりにならない)
    let trimmed = before.trim_end();
    let spaced = trimmed.len() < before.len();
    let last = found.last().copied().filter(|&(_, b, _)| spaced && b == trimmed.len());
    let prev = last.map_or("", |(a, b, _)| &before[a..b]);
    if let Some(fix) = judge(model, th, protected, prev, typed, prev.is_empty(), true) {
        return Some(fix);
    }
    let (a, b, prev_start) = last?;
    let before_prev = if prev_start { "" } else { found.len().checked_sub(2).map_or("", |i| &before[found[i].0..found[i].1]) };
    let mut fix = judge_previous(model, th, protected, before_prev, &before[a..b], prev_start, typed)?;
    fix.start = a;
    fix.end = b;
    Some(fix)
}

/// 確定する文字列の中の語をまとめて見る (句読点の前の語も)。before は同じ文の前の文字列 (確定済みの分も)
pub fn check_text_with(model: &dyn Model, th: &Thresholds, protected: &dyn Fn(&str) -> bool, before: &str, text: &str) -> Vec<Fix> {
    let mut fixes = Vec::new();
    let mut prev = words(before).last().map(|&(a, b, _)| before[a..b].to_string()).unwrap_or_default();
    let mut start_of_sentence = before.trim_end().is_empty() || before.trim_end().ends_with(['.', '!', '?']);
    let mut first = true;
    for (a, b, after_end) in words(text) {
        let word = &text[a..b];
        let sentence_start = if first { start_of_sentence || (after_end && a > 0) } else { after_end };
        if sentence_start && !first {
            prev.clear();
        }
        if let Some(mut fix) = judge(model, th, protected, if sentence_start { "" } else { &prev }, word, sentence_start, false) {
            fix.start = a;
            fix.end = b;
            fixes.push(fix);
        }
        prev = word.to_string();
        first = false;
        start_of_sentence = false;
    }
    fixes
}

/// 直した形を文字列に当てる (後ろから)
pub fn apply(text: &str, fixes: &[Fix]) -> String {
    let mut out = text.to_string();
    for fix in fixes.iter().rev() {
        out.replace_range(fix.start..fix.end, &fix.fixed);
    }
    out
}

/// IME につなぐ所 (Windows だけ。測りの道具 tools/latin_spell は上の判断だけを使う)
#[cfg(windows)]
mod ime {
    use super::*;
    use crate::engine::{auto_english, next_word};
    use std::{cell::RefCell, collections::HashSet, sync::OnceLock};

    /// 設定 input.latin_spell
    pub fn level() -> Level {
        static LEVEL: OnceLock<Level> = OnceLock::new();
        *LEVEL.get_or_init(|| Level::from_setting(&shared::AppConfig::read_or_default().input.latin_spell))
    }

    /// 次の語の候補の表 (英語と、設定で選んだアクセントの言語)
    struct Tables {
        tables: &'static [next_word::Table],
        alphabets: Vec<Vec<char>>,
        /// アクセントを外した形 → 語の番号 (表を開いたときに 1 度だけ作る)
        accents: Vec<std::collections::HashMap<String, Vec<usize>>>,
        /// ドイツ語の表の、名詞の頭の大文字の表 (IME 本体の隣の german_case.tsv)
        capitals: Vec<Option<Capitals>>,
    }

    impl Model for Tables {
        fn languages(&self) -> usize { self.tables.len() }
        fn lower(&self, lang: usize, word: &str) -> String { self.tables[lang].spell_lower(word) }
        fn find(&self, lang: usize, lower: &str) -> Option<usize> { self.tables[lang].spell_find(lower) }
        fn uni(&self, lang: usize, id: usize) -> u32 { self.tables[lang].spell_uni(id) }
        fn after(&self, lang: usize, prev: usize, id: usize) -> u32 { self.tables[lang].spell_after(prev, id) }
        fn pair(&self, lang: usize, prev: usize, id: usize) -> Option<u32> { self.tables[lang].spell_pair(prev, id) }
        fn display(&self, lang: usize, id: usize) -> String { self.tables[lang].spell_display(id) }
        fn alphabet(&self, lang: usize) -> &[char] { &self.alphabets[lang] }
        fn longer_word(&self, lang: usize, prefix: &str) -> bool { self.tables[lang].spell_longer(prefix) }
        fn capitals(&self, lang: usize) -> Option<&Capitals> {
            self.capitals[lang].as_ref()
        }
        fn accent_variants(&self, lang: usize, base: &str) -> Vec<(usize, String)> {
            self.accents[lang].get(base).map_or(vec![], |ids| ids.iter().map(|&id| (id, self.tables[lang].spell_word(id))).collect())
        }
    }

    fn tables() -> &'static Tables {
        static TABLES: OnceLock<Tables> = OnceLock::new();
        TABLES.get_or_init(|| {
            let tables = next_word::spell_tables();
            let accents = tables.iter().map(|t| {
                let mut index: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
                for id in 0..t.spell_words() {
                    let word = t.spell_word(id);
                    let base = strip_accents(&word);
                    if base != word {
                        index.entry(base).or_default().push(id);
                    }
                }
                index
            }).collect();
            let capitals = tables.iter().map(|t| {
                (t.spell_language() == "de").then(|| {
                    auto_english::dll_dir().and_then(|d| std::fs::read_to_string(d.join("german_case.tsv")).ok()).map(|text| Capitals::parse(&text))
                }).flatten()
            }).collect();
            Tables { tables, alphabets: tables.iter().map(|t| t.spell_alphabet()).collect(), accents, capitals }
        })
    }

    /// 利用者専用の英単語 (english_terms_personal.txt。小文字)
    fn personal_terms() -> &'static HashSet<String> {
        static TERMS: OnceLock<HashSet<String>> = OnceLock::new();
        TERMS.get_or_init(|| {
            auto_english::data_dir().and_then(|d| std::fs::read_to_string(d.join("english_terms_personal.txt")).ok()).unwrap_or_default()
                .lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_lowercase).collect()
        })
    }

    thread_local! {
        // 戻された綴り (latin_spell_ng.txt。小文字で 1 行 1 つ)。初めて使うときに読む。
        // 2026-11-08: 読んだときの、学習をまっさらにした印 (auto_english::learning_reset_mark) も持ち、変わっていたら読み直す
        static DECLINED: RefCell<Option<(HashSet<String>, Option<std::time::SystemTime>)>> = const { RefCell::new(None) };
    }

    fn declined_path() -> Option<std::path::PathBuf> {
        Some(auto_english::data_dir()?.join("latin_spell_ng.txt"))
    }

    fn declined(lower: &str) -> bool {
        let mark = auto_english::learning_reset_mark();
        DECLINED.with(|d| {
            let mut d = d.borrow_mut();
            if d.as_ref().is_some_and(|(_, loaded)| *loaded != mark) {
                *d = None;
            }
            d.get_or_insert_with(|| {
                (declined_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default()
                    .lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect(), mark)
            }).0.contains(lower)
        })
    }

    /// 直さない語: 固有名詞・社名・表記直しの表の語、利用者専用の英単語、戻された綴り
    fn protected(lower: &str) -> bool {
        auto_english::canonical_case(lower).is_some() || auto_english::english_name_case(lower).is_some()
            || personal_terms().contains(lower) || declined(lower)
    }

    /// 打っている語の指摘 (次の語の候補の一番上に出す)。before は打っている語より前の文字列 (キャレットの前。読めなければ最後に確定した文字列)、
    /// typed は打っている語。打っている語の直しが無ければ、前の語のアクセント (Tu hablas → Tú hablas) を返すことがある
    /// (fix.previous が真。位置は before の中で、Tab で前の語を書き換える)。
    /// 設定がオフ・端末やコードエディタ・直すほど確かでないときは None。表を引くだけで、打鍵ごとに呼んでよい
    pub fn suggestion(before: &str, typed: &str) -> Option<Fix> {
        if level() == Level::Off || !next_word::app_allows_latin_help() {
            return None;
        }
        suggestion_with(tables(), &Thresholds::default(), &protected, before, typed)
    }

    /// Space で確定する文字列のうち、自動修正する所 (設定が autocorrect のときだけ。自動修正の線を越えたものだけ)。
    /// before はキャレットの前の文字列 (読めなければ最後に確定した文字列)。直した文字列は apply で作る
    pub fn autocorrections(before: &str, text: &str) -> Vec<Fix> {
        if level() != Level::Autocorrect || !next_word::app_allows_latin_help() {
            return vec![];
        }
        check_text_with(tables(), &Thresholds::default(), &protected, before, text).into_iter().filter(|f| f.autocorrect).collect()
    }

    /// 自動修正を直後の BackSpace で戻したとき: 打った綴りを次から直さない (%APPDATA%\HaniWord\latin_spell_ng.txt。配らない)。
    /// 学習してよい欄 (composition.rs の may_learn) のときだけ呼ぶ。打った文章は残さず、綴りだけ
    pub fn decline(typed: &str) {
        let Some(path) = declined_path() else { return };
        let word = typed.replace('’', "'").to_lowercase();
        if word.is_empty() || word.chars().any(|c| !(c.is_alphabetic() || c == '\'')) || word.chars().count() > 64 {
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if !lines.contains(&word.as_str()) {
            lines.push(&word);
        }
        if lines.len() > 5_000 {
            lines.drain(..lines.len() - 2_500);
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, lines.join("\n") + "\n");
        DECLINED.with(|d| *d.borrow_mut() = None);
    }
}

#[cfg(windows)]
pub use ime::{autocorrections, decline, suggestion};

#[cfg(test)]
mod tests {
    use super::*;

    /// 試験用の小さな表 (語, 語だけの点)。組 (前の語, 語, 点) があればそれを使い、無ければ語だけの点 + 1000
    struct Small {
        words: Vec<(&'static str, u32)>,
        pairs: Vec<(&'static str, &'static str, u32)>,
        alphabet: Vec<char>,
    }

    impl Model for Small {
        fn languages(&self) -> usize { 1 }
        fn lower(&self, _: usize, word: &str) -> String { word.to_lowercase() }
        fn find(&self, _: usize, lower: &str) -> Option<usize> { self.words.iter().position(|(w, _)| *w == lower) }
        fn uni(&self, _: usize, id: usize) -> u32 { self.words[id].1 }
        fn after(&self, _: usize, prev: usize, id: usize) -> u32 {
            self.pairs.iter().find(|(a, b, _)| *a == self.words[prev].0 && *b == self.words[id].0).map_or(self.words[id].1 + 1000, |p| p.2)
        }
        fn pair(&self, _: usize, prev: usize, id: usize) -> Option<u32> {
            self.pairs.iter().find(|(a, b, _)| *a == self.words[prev].0 && *b == self.words[id].0).map(|p| p.2)
        }
        fn display(&self, _: usize, id: usize) -> String { if self.words[id].0 == "i" { "I".into() } else { self.words[id].0.to_string() } }
        fn alphabet(&self, _: usize) -> &[char] { &self.alphabet }
        fn longer_word(&self, _: usize, prefix: &str) -> bool { self.words.iter().any(|(w, _)| w.len() > prefix.len() && w.starts_with(prefix)) }
        fn accent_variants(&self, _: usize, base: &str) -> Vec<(usize, String)> {
            self.words.iter().enumerate().filter(|(_, (w, _))| strip_accents(w) == base && *w != base).map(|(i, (w, _))| (i, w.to_string())).collect()
        }
    }

    fn small() -> Small {
        Small {
            words: vec![("because", 4000), ("the", 3000), ("they", 6000), ("form", 8000), ("from", 6000), ("también", 7000), ("tambien", 20000),
                        ("cat", 9000), ("cut", 9000), ("i", 4000), ("think", 7000), ("so", 5000), ("he", 5000), ("said", 7000),
                        ("tu", 6000), ("tú", 6500), ("hablas", 12000), ("casa", 8000), ("élève", 7000), ("informação", 7000)],
            pairs: vec![("i", "think", 3000), ("tú", "hablas", 1500)],
            alphabet: "abcdefghijklmnopqrstuvwxyzáéíóú".chars().collect(),
        }
    }

    fn check(before: &str, text: &str) -> Vec<(String, String, bool)> {
        let none = |_: &str| false;
        check_text_with(&small(), &Thresholds::default(), &none, before, text).into_iter().map(|f| (f.typed, f.fixed, f.autocorrect)).collect()
    }

    #[test]
    fn unknown_words_one_typo_away_from_a_clear_word_are_fixed() {
        assert_eq!(check("", "becuase"), vec![("becuase".into(), "because".into(), true)]);
        assert_eq!(check("He said", "Becuase"), vec![]);  // 文の途中の大文字で始まる語 (名前のことが多い) は直さない
        assert_eq!(check("", "Becuase"), vec![("Becuase".into(), "Because".into(), true)]);
        assert_eq!(check("I", "thnik"), vec![("thnik".into(), "think".into(), true)]);
    }

    #[test]
    fn known_words_ambiguous_fixes_and_marked_words_are_left_alone() {
        assert_eq!(check("", "form"), vec![]);  // 表にある語 (form と from は前後で決まる)
        assert_eq!(check("", "cxt"), vec![]);  // cat と cut の 2 つが同じくらい近い (どちらも遠いキーの置き換え)
        assert_eq!(check("", "gitHub"), vec![]);  // 途中に大文字
        assert_eq!(check("", "BECUASE"), vec![]);  // 全部大文字
        assert_eq!(check("", "becuase2"), vec![]);  // 数字
        assert_eq!(check("", "https://becuase.example"), vec![]);
        assert_eq!(check("", "becuase@example.com"), vec![]);
        let protected = |w: &str| w == "becuase";
        assert!(check_text_with(&small(), &Thresholds::default(), &protected, "", "becuase").is_empty());
    }

    #[test]
    fn missing_accents_are_restored_and_rare_known_words_are_checked() {
        // tambien は表にあるがまれ (20000) なので、アクセントの付け忘れとして también に
        assert_eq!(check("", "tambien"), vec![("tambien".into(), "también".into(), true)]);
        // 打ったアクセントは外さない (también の é を消す・ほかの字にする直しはしない)
        let none = |_: &str| false;
        let mut found = vec![];
        edits(&"tú".chars().collect::<Vec<_>>(), &['t', 'u', 'ú', 'a'], 0, |w, _, _| found.push(w));
        assert!(!found.contains(&"tu".to_string()) && !found.contains(&"t".to_string()) && found.contains(&"túa".to_string()));
        assert!(check_text_with(&small(), &Thresholds::default(), &none, "", "también").is_empty());
        // 2 つ以上のアクセントも全部戻す (一部だけの elève・informaçao は表に無いので出ない)
        assert_eq!(check("", "eleve"), vec![("eleve".into(), "élève".into(), true)]);
        assert_eq!(check("", "informacao"), vec![("informacao".into(), "informação".into(), true)]);
        assert_eq!(check("", "elève"), vec![("elève".into(), "élève".into(), true)]);
        assert_eq!(added_accents(&"eleve".chars().collect::<Vec<_>>(), "élève"), Some(2));
        // 字の大きさは打ったとおり (Eleve → Élève)
        assert_eq!(keep_case("élève", "Eleve"), "Élève");
        assert_eq!(keep_case("lügen", "lugen"), "lügen");
        assert_eq!(added_accents(&"élève".chars().collect::<Vec<_>>(), "eleve"), None);
    }

    #[test]
    fn words_keep_positions_and_punctuation() {
        let none = |_: &str| false;
        let fixes = check_text_with(&small(), &Thresholds::default(), &none, "", "becuase, teh");
        assert_eq!(apply("becuase, teh", &fixes), "because, the");
        let text = "¿Qué tal? http://x.y a@b.c 3rd don't. Bien";
        let found: Vec<(&str, bool)> = words(text).into_iter().map(|(a, b, start)| (&text[a..b], start)).collect();
        assert_eq!(found, vec![("Qué", true), ("tal", false), ("don't", false), ("Bien", true)]);
    }

    #[test]
    fn partial_words_are_not_judged_while_typing() {
        let none = |_: &str| false;
        assert!(judge(&small(), &Thresholds::default(), &none, "", "becaus", true, true).is_none());  // because の頭
        assert!(judge(&small(), &Thresholds::default(), &none, "", "becuase", true, true).is_some());
    }

    /// ドイツ語の名詞の頭の大文字は、組と語の割合で決める (文の頭・大文字を含む語は見ない)
    #[test]
    fn german_nouns_are_capitalized_from_the_table() {
        let table = Capitals::parse("#global\t0.5\na\tich\t0.09\nw\thaus\t0.99\nw\tleben\t0.77\np\tzum leben\t0.97\t536\np\twir leben\t0.03\t349\n");
        assert!(table.probability("", "haus").unwrap() > 0.98);
        assert!(table.probability("zum", "leben").unwrap() > 0.96);
        assert!(table.probability("wir", "leben").unwrap() < 0.05);
        assert!(table.probability("ich", "leben").unwrap() < 0.3);
        assert!(table.probability("", "gehen").is_none());
    }

    /// アクセントの違いだけの語が 2 つ以上 (élève・élevé) なら、打っている語のアクセントは直さない
    #[test]
    fn ambiguous_accent_variants_are_left_alone() {
        let mut model = small();
        model.words.push(("élevé", 7000));
        let none = |_: &str| false;
        assert!(check_text_with(&model, &Thresholds::default(), &none, "", "eleve").is_empty());
    }

    /// 外すと別の正しい語になる組 (tu・tú) は、すぐ後ろの語とのつながりで前の語を指摘する (自動修正はしない)
    #[test]
    fn previous_word_accent_is_suggested_from_the_next_word() {
        let none = |_: &str| false;
        let th = Thresholds::default();
        let fix = suggestion_with(&small(), &th, &none, "Tu ", "hablas").unwrap();
        assert_eq!((fix.previous, fix.autocorrect, fix.fixed.as_str(), &"Tu "[fix.start..fix.end]), (true, false, "Tú", "Tu"));
        // tu casa (あなたの家) は組が表に無いので、そのまま
        assert!(suggestion_with(&small(), &th, &none, "Tu ", "casa").is_none());
        // 前の語の途中 (空白が無い) なら前の語ではない
        assert!(suggestion_with(&small(), &th, &none, "Tu", "hablas").is_none());
    }

    #[test]
    fn neighbouring_keys() {
        assert!(near('a', 's') && near('s', 'w') && near('a', 'w') && near('z', 'a') && near('x', 's') && near('m', 'n') && near('b', 'g'));
        assert!(!near('a', 'p') && !near('q', 'm') && !near('a', 'x') && !near('a', 'e'));
    }
}
