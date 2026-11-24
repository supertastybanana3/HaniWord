import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Slash, CircleDot, Repeat, Replace, BookA, Pi, Space, CaseUpper, SpellCheck, RefreshCcw, ExternalLink, Languages, CaseSensitive, SquareTerminal, Keyboard, Terminal, ArrowBigUp, Power, Superscript, Sigma, FlaskConical, Languages as LanguagesIcon, Hash, JapaneseYen, ListPlus, Scale, MessageSquareWarning, MessageCircle, ShieldAlert, Megaphone, BookOpenText, BellRing, Type, Calculator, Trash2 } from "lucide-react";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select"
import {
    Tooltip,
    TooltipContent,
    TooltipProvider,
    TooltipTrigger,
} from "@/components/ui/tooltip"
import { useEffect, useState } from "react";
import { toast } from "sonner"
import { invoke } from '@tauri-apps/api/core';
import {
    AlertDialog,
    AlertDialogAction,
    AlertDialogCancel,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
    AlertDialogTrigger,
} from "@/components/ui/alert-dialog"

const DIRECT_PREFIXES = "/$@";

// 2026-10-20: 日本語入力のときに全角・半角を選べる記号 (shared::CONFIGURABLE_SYMBOLS と同じ並び)
const SYMBOLS = "!\"#$%&'()*+:;<=>?@\\^_`{|}~";
const SYMBOL_PRESETS: [string, string, string][] = [
    ["full", "全部全角", SYMBOLS],
    ["half", "全部半角", ""],
    ["marks", "！？～ だけ全角", "!?~"],
];
const sortSymbols = (text: string) => SYMBOLS.split("").filter((c) => text.includes(c)).join("");
const presetOf = (symbols: string) => SYMBOL_PRESETS.find(([, , value]) => sortSymbols(value) === sortSymbols(symbols))?.[0] ?? "custom";
const toFullwidth = (c: string) => c === "\\" ? "＼" : c === "\"" ? "”" : c === "'" ? "’" : String.fromCharCode(c.charCodeAt(0) + 0xfee0);

// 長押しで出すアクセント付きの文字の言語
const LATIN_LANGUAGES: [string, string, string][] = [
    ["fr", "フランス語", "é è ç œ"],
    ["de", "ドイツ語", "ä ö ü ß"],
    ["es", "スペイン語", "ñ á é"],
    ["it", "イタリア語", "à è ì"],
    ["pt", "ポルトガル語", "ã õ ç"],
    ["nordic", "北欧の言語", "å æ ø"],
    ["pl", "ポーランド語", "ą ł ż"],
    ["cs", "チェコ語", "č ř ů"],
    ["tr", "トルコ語", "ğ ı ş"],
    ["pinyin", "ピンイン (声調)", "ā á ǎ à"],
];

// 2026-10-20: 専門用語の辞書 (変換サーバーの domain\domains.txt に並ぶ名前)。初期値は全部オンで、オフにした名前を input.domains_off に入れる
// 2026-10-30: インストーラーの専門分野の画面と同じ並び・名前だけ。「よく打つ」に印を付けた名前を input.domains_focus に入れる
// (目印の語が無くても同音語の優遇を効かせる)
// 2026-10-31: 画面に出すのは、入っている分野辞書 (domain\domains.txt) にある分野だけ (配る一式に入れる・外すだけで、画面も付いてくる)
const DOMAINS: [string, string][] = [
    ["law", "法律"],
    ["medicine", "医学"],
    ["biology", "生物"],
    ["chemistry", "化学"],
    ["it", "情報"],
    ["geography", "地理"],
    ["history", "歴史"],
    ["earth", "地学"],
    ["architecture", "建築"],
    ["religion", "仏教・神道"],
    ["math", "数学"],
];

// 2026-11-06 (仕事 39): 方言の言い方の辞書 (変換サーバーの domain\dialects.txt に並ぶ名前、shared の DIALECT_NAMES と同じ)。
// 選んだ名前を input.dialects に入れる (初期値は空 = 使わない)。画面に出すのは、入っている方言 (dialects.txt にある名前) だけ
const DIALECTS: [string, string][] = [
    ["kansai", "関西"],
];

// 2026-11-05 (仕事 26): 分野の名前にマウスを乗せたときの説明
const domainDetail = (code: string, name: string) => (
    <div className="space-y-1">
        <p>オン: {name}の話の文 (前後に分野の言葉があるとき) だけ、同じ読みの中で用語を先に出します。{code === "law" ? "例: 契約の話のときの さいけん → 債権" : ""}</p>
        <p>よく打つ (オンのときに出る印): 話題に関係なく用語を先に出します。{code === "math" ? "式の a= や ax も英字のまま入ります (アプリを開き直すと効きます)。" : ""}</p>
        <p>オフ: この分野の用語を足しません。「よく打つ」の印は残り、オンに戻すと同じ印で使います。</p>
    </div>
);

// 2026-11-05 (仕事 28): 小さな選び (ドロップダウン) の行
const SelectRow = ({ icon, title, description, value, options, onChange, width = "w-44" }: {
    icon: React.ReactNode;
    title: string;
    description: string;
    value: string;
    options: [string, string][];
    onChange: (value: string) => void;
    width?: string;
}) => (
    <div className="flex items-center space-x-4 rounded-md border p-4">
        {icon}
        <div className="flex-1 space-y-1">
            <p className="text-sm font-medium leading-none">{title}</p>
            <p className="text-xs text-muted-foreground">{description}</p>
        </div>
        <Select value={value} onValueChange={onChange}>
            <SelectTrigger className={width}>
                <SelectValue />
            </SelectTrigger>
            <SelectContent>
                {options.map(([v, label]) => <SelectItem key={v} value={v}>{label}</SelectItem>)}
            </SelectContent>
        </Select>
    </div>
);

const SwitchRow = ({ icon, title, description, checked, onChange }: {
    icon: React.ReactNode;
    title: string;
    description: string;
    checked: boolean;
    onChange: () => void;
}) => (
    <div className="flex items-center space-x-4 rounded-md border p-4">
        {icon}
        <div className="flex-1 space-y-1">
            <p className="text-sm font-medium leading-none">{title}</p>
            <p className="text-xs text-muted-foreground">{description}</p>
        </div>
        <Switch checked={checked} onCheckedChange={onChange} />
    </div>
);

// 2026-11-08: 学習をまっさらにする。消す範囲を選び、確かめの窓で控えの場所を見せてから、変換サーバーに頼む (控えを取ってから消す。
// 控えを取れなければ何も消さない)。消したあとは、控えの場所を知らせる
const ResetLearningRow = () => {
    const [includeTables, setIncludeTables] = useState(false);
    const [busy, setBusy] = useState(false);
    const reset = async () => {
        setBusy(true);
        try {
            const backup = await invoke<string>("reset_learning", { includeTables });
            toast("学習を消しました", { description: `控え: ${backup}`, duration: 20000 });
        } catch (error) {
            toast("学習を消せませんでした", { description: String(error), duration: 20000 });
        } finally {
            setBusy(false);
        }
    };
    return (
        <div className="flex items-center space-x-4 rounded-md border p-4">
            <Trash2 />
            <div className="flex-1 space-y-1">
                <p className="text-sm font-medium leading-none">学習をまっさらにする</p>
                <p className="text-xs text-muted-foreground">変換で覚えた語を消して、入れたばかりのときの変換に戻します。消す前に控えを取ります。1 つの候補だけを忘れるときは、候補の窓でその候補を選んで Ctrl+Delete を押します</p>
            </div>
            <AlertDialog>
                <AlertDialogTrigger asChild>
                    <Button variant="secondary" disabled={busy}>消す…</Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>学習をまっさらにする</AlertDialogTitle>
                        <AlertDialogDescription>
                            消す範囲を選んでください。消す前に、%APPDATA%\HaniWord\memory_backup_(日時) に控えを取ります (控えを取れなければ、何も消しません)。
                            学習から外す語・自分で足した英単語・単語登録・設定は残ります
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <div className="space-y-2 text-sm">
                        <label className="flex items-start gap-x-2">
                            <input type="radio" name="reset-learning-scope" checked={!includeTables} onChange={() => setIncludeTables(false)} />
                            <span>変換の学習だけ (確定して覚えた語)</span>
                        </label>
                        <label className="flex items-start gap-x-2">
                            <input type="radio" name="reset-learning-scope" checked={includeTables} onChange={() => setIncludeTables(true)} />
                            <span>HaniWord の学習も一緒に (表記の選び・同音語の選び・・ と / の選び・数え方・英字の語と綴り・予測の候補・ローマ字の打ち方)</span>
                        </label>
                    </div>
                    <AlertDialogFooter>
                        <AlertDialogCancel>やめる</AlertDialogCancel>
                        <AlertDialogAction onClick={reset}>控えを取って消す</AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </div>
    );
};

// 2026-11-06 (仕事 32): ギリシャ文字 24 字 (小文字, 大文字)。字ごとに先に出す形を選ぶ (input.greek_case)。書いていない字は変換サーバーの既定
// (ウィキペディアの文で数えて決めた。今はどの字も小文字が先。変換サーバーの greekDefaultUpper と同じ)
const GREEK: [string, string][] = [
    ["α", "Α"], ["β", "Β"], ["γ", "Γ"], ["δ", "Δ"], ["ε", "Ε"], ["ζ", "Ζ"], ["η", "Η"], ["θ", "Θ"], ["ι", "Ι"], ["κ", "Κ"], ["λ", "Λ"], ["μ", "Μ"],
    ["ν", "Ν"], ["ξ", "Ξ"], ["ο", "Ο"], ["π", "Π"], ["ρ", "Ρ"], ["σ", "Σ"], ["τ", "Τ"], ["υ", "Υ"], ["φ", "Φ"], ["χ", "Χ"], ["ψ", "Ψ"], ["ω", "Ω"],
];
const GREEK_DEFAULT_UPPER: string[] = [];
const greekUpperFirst = (greekCase: Record<string, string>, lower: string) =>
    (greekCase[lower] ?? (GREEK_DEFAULT_UPPER.includes(lower) ? "upper" : "lower")) === "upper";

const fromConfig = (data: any) => ({
    autoEnglish: data.input.auto_english,
    caseFix: data.input.english_case_fix ?? true,
    // 2026-11-05 (仕事 28): 自動の直しの切り替え。項目の無い前の版の設定では今の動きのまま
    slashKey: (data.input.slash_key ?? "auto") as string,
    periodKey: (data.input.period_key ?? "auto") as string,
    slashLearning: data.input.slash_learning ?? true,
    // 2026-11-06: 。。。 → … の自動・半角英数のスマート句読点 (-- → —)。どちらも初期値オフ
    ellipsisAuto: data.input.ellipsis_auto ?? false,
    smartPunctuation: data.input.smart_punctuation ?? false,
    // 2026-11-06 (仕事 34): Space 2 回の「. 」・文の頭の大文字・計算の答えの候補。どれも初期値オフ
    periodShortcut: data.input.period_shortcut ?? false,
    autoCapitalize: data.input.auto_capitalize ?? false,
    calcCandidates: data.input.calc_candidates ?? false,
    homophoneReplace: data.input.homophone_replace ?? true,
    initialLetterWords: data.input.initial_letter_words ?? true,
    hyoukiLearning: data.input.hyouki_learning ?? true,
    spaceAfterLatin: data.input.space_after_latin ?? false,
    // 数学の式: 書いていなければ、数学か物理をよく打つ分野に選んでいるか (shared の math_expressions_on と同じ)
    mathExpressions: (data.input.math_expressions ?? ((data.input.domains_focus ?? []) as string[]).some((d) => d === "math" || d === "physics")) as boolean,
    latinSpell: (data.input.latin_spell ?? "off") as string,
    // 2026-11-06 (仕事 35): 入力モードの表示。知らない値は札 (shared の mode_indicator_style と同じ)
    modeIndicator: (["badge", "compact", "off"].includes(data.input.mode_indicator) ? data.input.mode_indicator : "badge") as string,
    direct: data.input.direct_prefixes !== "",
    shellLatin: data.input.latin_at_shell_prompt,
    choiceLatin: data.input.latin_at_terminal_choices ?? true,
    romajiStyle: data.input.romaji_style,
    shiftAscii: data.input.shift_ascii_mode,
    startKana: data.input.start_in_kana,
    scripts: data.input.caret_scripts,
    greek: data.input.greek_letters,
    greekCase: (data.input.greek_case ?? {}) as Record<string, string>,
    chem: data.input.chem_subscript,
    languages: data.input.latin_languages ?? [],
    accentsInKana: data.input.accents_in_kana ?? true,
    letterKeyRepeat: data.input.letter_key_repeat ?? false,
    numberPunct: data.input.number_punctuation ?? true,
    // 2026-11-05 (仕事 27): 打っている間の候補は日本語・英語とも選ぶ形で初期値オフ (前の next_word_suggestions は読まない)
    englishPredictions: data.input.english_predictions ?? false,
    japanesePredictions: data.input.japanese_predictions ?? false,
    symbols: data.input.fullwidth_symbols ?? SYMBOLS,
    yenKey: data.input.yen_key ?? "yen",
    // 2026-11-05 (仕事 23): 前の版の true / false は numbers / off として見せる
    fullwidthDigits: (data.input.fullwidth_digits === true ? "numbers" : data.input.fullwidth_digits === false ? "off" : data.input.fullwidth_digits ?? "off") as string,
    // 2026-11-06 (仕事 36): かなで打った数詞＋助数詞 (いっかい) の出し方。"arabic" (初期値) か "kanji"
    counterNumerals: (data.input.counter_numerals === "kanji" ? "kanji" : "arabic") as string,
    domainsOff: (data.input.domains_off ?? []) as string[],
    domainsFocus: (data.input.domains_focus ?? []) as string[],
    // 2026-11-06 (仕事 39): 今は 1 つだけ選ぶ形 (設定は一覧で持つので、あとで方言を足しても形を変えずに済む)
    dialect: (((data.input.dialects ?? []) as string[]).find((d) => DIALECTS.some(([code]) => code === d)) ?? "") as string,
    misuseNotes: data.input.misuse_notes ?? false,
    homophoneNotes: data.input.homophone_notes ?? false,
    // 2026-11-06 (仕事 40): 注意の注記の 2 段 (どちらも初期値オフ)
    cautionSlurs: data.input.caution_slurs ?? false,
    cautionBroadcast: data.input.caution_broadcast ?? false,
    // 2026-11-07 (仕事 43): ネットスラングの辞書 (初期値オフ)
    internetSlang: data.input.internet_slang ?? false,
    punctuationConvert: data.input.punctuation_convert ?? false,
    typoRepair: data.input.typo_repair ?? false,
    liveKeepRefined: data.input.live_keep_refined ?? true,
    liveShortDictionary: data.input.live_short_dictionary ?? true,
    // 2026-11-08 (仕事 45): カーソルの右の字も見て変換する (初期値オン)
    rightContext: data.input.right_context ?? true,
    // 2026-11-08 (仕事 12): 空白なしで打った英語の句に空白を入れる (初期値オン)
    englishPhraseSpaces: data.input.english_phrase_spaces ?? true,
    // 2026-11-08 (仕事 53): Ctrl+Space で あ／A を切り替える (初期値オフ)
    ctrlSpaceToggle: data.input.ctrl_space_toggle ?? false,
    // 2026-11-08 (仕事 52): 重いときは自動で AI を休む (2026-11-19 から初期値オフ)・休んでいる間の並べ方 (辞書だけ向け)・候補の窓の知らせ (初期値オン)
    aiAutoPause: data.input.ai_auto_pause ?? false,
    aiPauseOrder: (data.input.ai_pause_order === "preview" ? "preview" : "dictionary") as string,
    aiPauseNotice: data.input.ai_pause_notice ?? true,
});

export const General = () => {
    const [input, setInput] = useState({
        autoEnglish: true,
        caseFix: true,
        latinSpell: "off",
        modeIndicator: "badge",
        slashKey: "auto",
        periodKey: "auto",
        slashLearning: true,
        ellipsisAuto: false,
        smartPunctuation: false,
        periodShortcut: false,
        autoCapitalize: false,
        calcCandidates: false,
        homophoneReplace: true,
        initialLetterWords: true,
        hyoukiLearning: true,
        spaceAfterLatin: false,
        mathExpressions: false,
        direct: true,
        shellLatin: true,
        choiceLatin: true,
        romajiStyle: "auto",
        shiftAscii: true,
        startKana: true,
        scripts: true,
        greek: true,
        greekCase: {} as Record<string, string>,
        chem: true,
        languages: [] as string[],
        accentsInKana: true,
        letterKeyRepeat: false,
        numberPunct: true,
        englishPredictions: false,
        japanesePredictions: false,
        symbols: SYMBOLS,
        yenKey: "yen",
        fullwidthDigits: "off",
        counterNumerals: "arabic",
        domainsOff: [] as string[],
        domainsFocus: [] as string[],
        dialect: "",
        misuseNotes: false,
        homophoneNotes: false,
        cautionSlurs: false,
        cautionBroadcast: false,
        internetSlang: false,
        punctuationConvert: false,
        typoRepair: false,
        liveKeepRefined: true,
        liveShortDictionary: true,
        rightContext: true,
        englishPhraseSpaces: true,
        ctrlSpaceToggle: false,
        aiAutoPause: false,
        aiPauseOrder: "dictionary",
        aiPauseNotice: true,
    });
    const [symbolDetails, setSymbolDetails] = useState(false);
    // 2026-10-29: 新しい版の知らせ (settings.json の notify_updates、初期値オン) と、この版の番号
    const [notifyUpdates, setNotifyUpdates] = useState(true);
    const [version, setVersion] = useState("");
    // 2026-10-31: 入っている分野辞書の名前 (読めなければ null で、DOMAINS を全部出す)
    const [installedDomains, setInstalledDomains] = useState<string[] | null>(null);
    const shownDomains = installedDomains ? DOMAINS.filter(([code]) => installedDomains.includes(code)) : DOMAINS;
    // 2026-11-06 (仕事 39): 入っている方言の言い方の辞書の名前 (読めなければ null で、DIALECTS を全部出す)
    const [installedDialects, setInstalledDialects] = useState<string[] | null>(null);
    const shownDialects = installedDialects ? DIALECTS.filter(([code]) => installedDialects.includes(code)) : DIALECTS;
    // 2026-11-07 (仕事 43): 入っているくだけた言葉の辞書の名前 (読めなければ null で、ネットスラングの切り替えを出す)
    const [installedSlang, setInstalledSlang] = useState<string[] | null>(null);
    const showInternetSlang = installedSlang ? installedSlang.includes("internet") : true;
    // 2026-11-06: 次の語の候補の表が入っていない言語 (インストーラーで選ばなかった言語)。読めなければ null で、何も出さない
    const [missingNextWords, setMissingNextWords] = useState<string[] | null>(null);

    useEffect(() => {
        invoke<any>("get_config")
            .then((data) => {
                setInput(fromConfig(data));
                setNotifyUpdates(data.notify_updates ?? true);
            })
            .catch(() => {
                // 読めなければ初期値のまま
            });
        invoke<string>("app_version").then(setVersion).catch(() => {});
        invoke<string[] | null>("installed_domains").then(setInstalledDomains).catch(() => {});
        invoke<string[] | null>("installed_dialects").then(setInstalledDialects).catch(() => {});
        invoke<string[] | null>("installed_slang").then(setInstalledSlang).catch(() => {});
        invoke<string[] | null>("missing_next_words").then(setMissingNextWords).catch(() => {});
    }, []);

    const toggleNotifyUpdates = async () => {
        try {
            const data = await invoke<any>("get_config");
            data.notify_updates = !(data.notify_updates ?? true);
            await invoke("update_config", { newConfig: data });
            setNotifyUpdates(data.notify_updates);
            toast("設定を変更しました");
        } catch (error) {
            toast("設定の更新に失敗しました");
        }
    };

    // 設定は IME を新しく読み込んだアプリから効く
    // (入力モードの表示のように表示の窓が読む設定は、effect で知らせの文を替える)
    const updateInput = async (updater: (input: any) => void, effect = "これから開くアプリ (開き直したアプリ) で有効になります") => {
        try {
            const data = await invoke<any>("get_config");
            updater(data.input);
            await invoke("update_config", { newConfig: data });
            setInput(fromConfig(data));
            toast("設定を変更しました", { description: effect });
        } catch (error) {
            toast("設定の更新に失敗しました");
        }
    };

    // 2026-11-06 (仕事 32): 押した字の先に出す形を入れ替える。既定と同じ形に戻したら書いた値を消す
    const toggleGreekCase = (lower: string) => updateInput((i) => {
        const map: Record<string, string> = { ...(i.greek_case ?? {}) };
        const next = greekUpperFirst(map, lower) ? "lower" : "upper";
        if ((next === "upper") === GREEK_DEFAULT_UPPER.includes(lower)) delete map[lower]; else map[lower] = next;
        if (Object.keys(map).length === 0) delete i.greek_case; else i.greek_case = map;
    });

    const preset = presetOf(input.symbols);
    const toggleSymbol = (c: string) => updateInput((i) => {
        const current: string = i.fullwidth_symbols ?? SYMBOLS;
        i.fullwidth_symbols = sortSymbols(current.includes(c) ? current.replace(c, "") : current + c);
    });

    return (
        <div className="space-y-8">
            <section className="space-y-2">
                {/* 2026-10-30: ライブ変換を落ち着かせる 2 つ (初期値はオン。試験中から移した) */}
                <h1 className="text-sm font-bold text-foreground">ライブ変換</h1>
                <SwitchRow
                    icon={<Keyboard />}
                    title="ライブ変換で、清書した前のほうを戻さない"
                    description="ライブ変換で手が止まって AI が変換し直した前のほうを、次の字を打っても辞書だけの変換に戻さず、後ろだけを変え直します (打つたびに前のほうが変わるのを抑えます)"
                    checked={input.liveKeepRefined}
                    onChange={() => updateInput((i) => { i.live_keep_refined = !input.liveKeepRefined; })}
                />
                <SwitchRow
                    icon={<Keyboard />}
                    title="短い入力は辞書の変換のまま見せる"
                    description="ライブ変換で、短い入力 (読みが 6 字まで) は辞書の変換だけを見せ、手が止まっても変え直しません。AI の変換は Space の候補の一覧に混ぜます (一番上は見えていたもののまま)"
                    checked={input.liveShortDictionary}
                    onChange={() => updateInput((i) => { i.live_short_dictionary = !input.liveShortDictionary; })}
                />
                <SwitchRow
                    icon={<Keyboard />}
                    title="カーソルの右の文字も見て変換する"
                    description="文の途中に書き足すとき、カーソルの右にある字 (同じ行の 20 字まで) も手がかりにして、辞書の変換の語を選びます (例: 右が「を操作する」なら きかい は 機械)。オフにすると左の字だけを見ます"
                    checked={input.rightContext}
                    onChange={() => updateInput((i) => { i.right_context = !input.rightContext; })}
                />
                {/* 2026-11-08 (仕事 52): 重いときは自動で AI を休む */}
                <SwitchRow
                    icon={<Keyboard />}
                    title="重いときは自動で AI をオフにする"
                    description="ほかのアプリの負荷などで AI の変換がふだんよりずっと遅くなったら、辞書だけの変換にします。速さが戻ったら AI に戻します"
                    checked={input.aiAutoPause}
                    onChange={() => updateInput((i) => { i.ai_auto_pause = !input.aiAutoPause; })}
                />
                <SelectRow
                    icon={<Keyboard />}
                    title="AI がオフの間の並べ方"
                    description="辞書だけ向けは AI を切った設定と同じ直しをかけます (結果がそのまま残るので正しさを優先)。AI ありと同じは、打っている途中の仮表示と同じ並べ方です"
                    value={input.aiPauseOrder}
                    options={[["dictionary", "辞書だけ向け"], ["preview", "AI ありと同じ"]]}
                    onChange={(v) => updateInput((i) => { i.ai_pause_order = v; })}
                />
                <SwitchRow
                    icon={<Keyboard />}
                    title="AI がオフになったことを知らせる"
                    description="AI がオフの間、候補の窓の下に「負荷がかかっているためAIがオフになっています」と出します"
                    checked={input.aiPauseNotice}
                    onChange={() => updateInput((i) => { i.ai_pause_notice = !input.aiPauseNotice; })}
                />
            </section>
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">記号の入力</h1>
                <div className="flex items-center space-x-4 rounded-md border p-4">
                    <Hash />
                    <div className="flex-1 space-y-1">
                        <p className="text-sm font-medium leading-none">記号の全角・半角</p>
                        <p className="text-xs text-muted-foreground">
                            日本語入力 (あ) で打つ ！？（）：＃ などの記号。、。・「」ー はいつもこのまま
                        </p>
                    </div>
                    <Select
                        value={preset}
                        onValueChange={(value) => {
                            const found = SYMBOL_PRESETS.find(([key]) => key === value);
                            if (found) updateInput((i) => { i.fullwidth_symbols = found[2]; });
                            else setSymbolDetails(true);
                        }}
                    >
                        <SelectTrigger className="w-44">
                            <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                            {SYMBOL_PRESETS.map(([key, label]) => (
                                <SelectItem key={key} value={key}>{label}</SelectItem>
                            ))}
                            <SelectItem value="custom">記号ごとに選ぶ</SelectItem>
                        </SelectContent>
                    </Select>
                </div>
                <div className="flex items-center space-x-4 rounded-md border p-4">
                    <JapaneseYen />
                    <div className="flex-1 space-y-1">
                        <p className="text-sm font-medium leading-none">¥ キーで入る字</p>
                        <p className="text-xs text-muted-foreground">「ろ」キーはいつも \ です。全角にするかは、上の設定の \ に従います</p>
                    </div>
                    <Select
                        value={input.yenKey}
                        onValueChange={(value) => updateInput((i) => { i.yen_key = value; })}
                    >
                        <SelectTrigger className="w-44">
                            <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                            <SelectItem value="yen">¥ (円記号)</SelectItem>
                            <SelectItem value="backslash">\ (バックスラッシュ)</SelectItem>
                        </SelectContent>
                    </Select>
                </div>
                {/* 2026-11-05 (仕事 23): 数字の全角・半角 (3 択。初期値は半角) */}
                <div className="flex items-center space-x-4 rounded-md border p-4">
                    <Hash />
                    <div className="flex-1 space-y-1">
                        <p className="text-sm font-medium leading-none">数字の全角・半角</p>
                        <p className="text-xs text-muted-foreground">日本語入力 (あ) で打った数字。式 (3x+1=0) と URL・メール・パスの中はいつも半角です</p>
                    </div>
                    <Select
                        value={input.fullwidthDigits}
                        onValueChange={(value) => updateInput((i) => { i.fullwidth_digits = value; })}
                    >
                        <SelectTrigger className="w-44">
                            <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                            <SelectItem value="off">半角 (123・5kg)</SelectItem>
                            <SelectItem value="numbers">全角 (１２３・5kg)</SelectItem>
                            <SelectItem value="all">全角 (１２３・５kg)</SelectItem>
                        </SelectContent>
                    </Select>
                </div>
                {/* 2026-11-06 (仕事 36): かなで打った数詞＋助数詞を、算用数字と漢数字のどちらで先に出すか (初期値は算用数字) */}
                <div className="flex items-center space-x-4 rounded-md border p-4">
                    <Hash />
                    <div className="flex-1 space-y-1">
                        <p className="text-sm font-medium leading-none">かなで打った数の書き方</p>
                        <p className="text-xs text-muted-foreground">いっかい・ふたり・にじゅうさんにん のように読みで打った数。もう一方の形は候補の 2 番目に出ます。数字のキーで打った数字はそのままです</p>
                    </div>
                    <Select
                        value={input.counterNumerals}
                        onValueChange={(value) => updateInput((i) => { i.counter_numerals = value; })}
                    >
                        <SelectTrigger className="w-44">
                            <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                            <SelectItem value="arabic">算用数字 (1回・23人)</SelectItem>
                            <SelectItem value="kanji">漢数字 (一回・二十三人)</SelectItem>
                        </SelectContent>
                    </Select>
                </div>
                <div className="rounded-md border p-4 space-y-3">
                    <button
                        className="flex w-full items-center space-x-4 text-left"
                        onClick={() => setSymbolDetails(!symbolDetails)}
                    >
                        <ListPlus />
                        <span className="flex-1 text-sm font-medium">詳細設定: 記号ごとに選ぶ</span>
                        <span className="text-xs text-muted-foreground">{symbolDetails ? "閉じる" : "開く"}</span>
                    </button>
                    {symbolDetails && (
                        <div className="grid grid-cols-6 gap-2">
                            {SYMBOLS.split("").map((c) => {
                                const full = input.symbols.includes(c);
                                return (
                                    <Button
                                        key={c}
                                        variant={full ? "secondary" : "outline"}
                                        className="h-10 font-mono"
                                        onClick={() => toggleSymbol(c)}
                                        title={full ? "全角 (押すと半角)" : "半角 (押すと全角)"}
                                    >
                                        {full ? toFullwidth(c) : c}
                                        <span className="ml-1 text-[10px] text-muted-foreground">{full ? "全" : "半"}</span>
                                    </Button>
                                );
                            })}
                        </div>
                    )}
                </div>
                {/* 2026-11-05 (仕事 28): ・ と /、。 と . の打ち分けと、選び直しの学習 */}
                <SelectRow
                    icon={<Slash />}
                    title="・ と / の打ち分け"
                    description="自動なら、英字や数字のすぐ後ろは / (km/h・1/2)、そのほかは ・ にします"
                    value={input.slashKey}
                    options={[["auto", "自動"], ["dot", "いつも ・"], ["slash", "いつも /"]]}
                    onChange={(v) => updateInput((i) => { i.slash_key = v; })}
                />
                <SelectRow
                    icon={<CircleDot />}
                    title="。 と . の打ち分け"
                    description="自動なら、数字のあとの小数点とファイルの拡張子は . (2.5・資料.pdf)、そのほかは 。 にします"
                    value={input.periodKey}
                    options={[["auto", "自動"], ["kuten", "いつも 。"], ["period", "いつも ."]]}
                    onChange={(v) => updateInput((i) => { i.period_key = v; })}
                />
                <SwitchRow
                    icon={<Repeat />}
                    title="・ / 。 . の選び直しを覚える"
                    description="候補で選び直した形を、同じ前後の語の組で次から使います"
                    checked={input.slashLearning}
                    onChange={() => updateInput((i) => { i.slash_learning = !input.slashLearning; })}
                />
                <SwitchRow
                    icon={<CircleDot />}
                    title="。。。 を … にする"
                    description="同じ点が 3 つ (。。。・・・・...) なら …、6 つなら …… にします。オフでも候補の 2 番目に出します"
                    checked={input.ellipsisAuto}
                    onChange={() => updateInput((i) => { i.ellipsis_auto = !input.ellipsisAuto; })}
                />
                <SwitchRow
                    icon={<Slash />}
                    title="半角英数のスマート句読点"
                    description={`半角英数で - を 2 回続けると — (ダッシュ)、' と " を向きのある引用符 (“ ” ‘ ’) にします。端末とコードを書くアプリでは働きません`}
                    checked={input.smartPunctuation}
                    onChange={() => updateInput((i) => { i.smart_punctuation = !input.smartPunctuation; })}
                />
                <SwitchRow
                    icon={<CircleDot />}
                    title="Space 2 回で「. 」"
                    description="半角英数で、英字か数字のあとに Space を 2 回続けると「. 」にします。端末とコードを書くアプリでは働きません"
                    checked={input.periodShortcut}
                    onChange={() => updateInput((i) => { i.period_shortcut = !input.periodShortcut; })}
                />
                <SwitchRow
                    icon={<Type />}
                    title="文の頭を大文字に"
                    description="半角英数で、. ? ! と空白のあと・改行のあとの小文字を大文字にします。欄の頭は変えません。端末とコードを書くアプリでは働きません"
                    checked={input.autoCapitalize}
                    onChange={() => updateInput((i) => { i.auto_capitalize = !input.autoCapitalize; })}
                />
                <SwitchRow
                    icon={<Calculator />}
                    title="計算の答えを候補に出す"
                    description="1+2= のように式のあとに = を打つと、候補の 2 番目に「1+2=3」、3 番目に「3」を出します"
                    checked={input.calcCandidates}
                    onChange={() => updateInput((i) => { i.calc_candidates = !input.calcCandidates; })}
                />
            </section>
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">打っている間の候補</h1>
                <SwitchRow
                    icon={<ListPlus />}
                    title="日本語の予測候補"
                    description="手が止まったときに、続きの言い回しの候補を出します (Tab で補う)。ライブ変換の中でも出します"
                    checked={input.japanesePredictions}
                    onChange={() => updateInput((i) => { i.japanese_predictions = !input.japanesePredictions; })}
                />
                <SwitchRow
                    icon={<ListPlus />}
                    title="英単語の候補"
                    description="英単語を打っている途中に、前の語から続きそうな語を出します (Tab で補う)。英数モードでは端末・コードエディタでは出しません"
                    checked={input.englishPredictions}
                    onChange={() => updateInput((i) => { i.english_predictions = !input.englishPredictions; })}
                />
            </section>
            <section className="space-y-2">
                {/* 2026-11-06 (仕事 35): 入力モードを切り替えたときの表示を 3 つから選ぶ (表示の窓が読む。開き直さなくても次の切り替えから効く) */}
                <h1 className="text-sm font-bold text-foreground">入力モードの表示</h1>
                <SelectRow
                    icon={<CaseSensitive />}
                    title="入力モードの切り替えの表示"
                    description="あ・A を切り替えたときに、カーソルのそばに出す表示です。札は あ と A を並べて今のほうを塗り、Caps Lock がオンの間も出ます。小さくは今のモードの字だけを 2.5 秒ほど出します。出さないを選ぶと、タスクバーの右下の表示だけになり、Caps Lock の表示も出ません"
                    value={input.modeIndicator}
                    options={[["badge", "カーソルのそばに札"], ["compact", "カーソルのそばに小さく"], ["off", "出さない"]]}
                    width="w-56"
                    onChange={(v) => updateInput((i) => { i.mode_indicator = v; }, "次に入力モードを切り替えたときから有効になります")}
                />
            </section>
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">英字の入力</h1>
                <SwitchRow
                    icon={<Power />}
                    title="アプリを開いたときは「あ」"
                    description="オフにすると、開いたときは英数 (A) から始まります"
                    checked={input.startKana}
                    onChange={() => updateInput((i) => { i.start_in_kana = !input.startKana; })}
                />
                <SwitchRow
                    icon={<Keyboard />}
                    title="Ctrl + Space で あ／A を切り替える"
                    description="半角/全角 キーの無いキーボード (US 配列) 向けです。オフ (初期値) なら、入力中の Ctrl + Space は確定して空白のまま、入力中でなければアプリへ渡します (VS Code の補完などとぶつからないように)。Alt + ` (1 の左のキー) はいつでも切り替えに使えます"
                    checked={input.ctrlSpaceToggle}
                    onChange={() => updateInput((i) => { i.ctrl_space_toggle = !input.ctrlSpaceToggle; })}
                />
                <SwitchRow
                    icon={<ArrowBigUp />}
                    title="大文字を打ったら確定まで英字"
                    description="オフ（初期値）は大文字で始まる語だけ英字です。Googledekensaku → Googleで検索"
                    checked={input.shiftAscii}
                    onChange={() => updateInput((i) => { i.shift_ascii_mode = !input.shiftAscii; })}
                />
                <SwitchRow
                    icon={<Languages />}
                    title="英語と日本語を自動で打ち分ける"
                    description="google・windows のような英単語は、ローマ字にせず英字のまま入れます"
                    checked={input.autoEnglish}
                    onChange={() => updateInput((i) => { i.auto_english = !input.autoEnglish; })}
                />
                <div className="flex items-center space-x-4 rounded-md border p-4">
                    <Keyboard />
                    <div className="flex-1 space-y-1">
                        <p className="text-sm font-medium leading-none">ローマ字の打ち方</p>
                        <p className="text-xs text-muted-foreground">
                            ふだん使わない綴り (訓令式の人の shi・nn の人の n+子音 など) を英語のしるしに使います。自動にすると、日本語として確定した綴りの回数を音ごとに数え (文章は残しません)、まず使わないと分かった綴りだけをしるしにします
                        </p>
                    </div>
                    <Select value={input.romajiStyle} onValueChange={(v) => updateInput((i) => { i.romaji_style = v; })}>
                        <SelectTrigger className="w-56">
                            <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                            <SelectItem value="auto">自動 (音ごとに判断)</SelectItem>
                            <SelectItem value="kunrei_nn">訓令式＋ん は nn (正確)</SelectItem>
                            <SelectItem value="standard">ヘボン式も使う (精度は少し下がる)</SelectItem>
                        </SelectContent>
                    </Select>
                </div>
                <SwitchRow
                    icon={<CaseSensitive />}
                    title="英単語を決まった表記に直す"
                    description="小文字で打った github・pdf を GitHub・PDF にします。オフなら打ったまま"
                    checked={input.caseFix}
                    onChange={() => updateInput((i) => { i.english_case_fix = !input.caseFix; })}
                />
                <SwitchRow
                    icon={<Space />}
                    title="空白なしで打った英語の句に空白を入れる"
                    description="seeyoutomorrow のように続けて打った英語を、Space のときに see you tomorrow にします。分け方がはっきりしているときだけで、日本語のローマ字は分けません。直後の Backspace で空白なしに戻せます"
                    checked={input.englishPhraseSpaces}
                    onChange={() => updateInput((i) => { i.english_phrase_spaces = !input.englishPhraseSpaces; })}
                />
                <SwitchRow
                    icon={<CaseUpper />}
                    title="英字 1 字＋語を決まった形に"
                    description="小文字で打った jri-gu・bkyuu を Jリーグ・B級 にします"
                    checked={input.initialLetterWords}
                    onChange={() => updateInput((i) => { i.initial_letter_words = !input.initialLetterWords; })}
                />
                <SwitchRow
                    icon={<Space />}
                    title="英字の後ろに半角の空白"
                    description="英字の語の後ろに日本語が続くとき、間に半角の空白を入れます (Windows でコンピューター)"
                    checked={input.spaceAfterLatin}
                    onChange={() => updateInput((i) => { i.space_after_latin = !input.spaceAfterLatin; })}
                />
                {/* 2026-11-05 (仕事 24): 英語の文のつづりの指摘・自動修正 (かなの入力中に英字のまま入れる英語の文だけ。半角英数の直接入力は変えない) */}
                <div className="flex items-center space-x-4 rounded-md border p-4">
                    <SpellCheck />
                    <div className="flex-1 space-y-1">
                        <p className="text-sm font-medium leading-none">英語の文のつづり</p>
                        <p className="text-xs text-muted-foreground">英語の文の打ち間違い (becuase・tambien) を、候補の一番上に出すか、確実なときだけ Space で直します (直後の BackSpace で戻せます)</p>
                    </div>
                    <Select value={input.latinSpell} onValueChange={(v) => updateInput((i) => { i.latin_spell = v; })}>
                        <SelectTrigger className="w-44">
                            <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                            <SelectItem value="off">直さない</SelectItem>
                            <SelectItem value="suggest">候補に出すだけ</SelectItem>
                            <SelectItem value="autocorrect">自動で直す</SelectItem>
                        </SelectContent>
                    </Select>
                </div>
                <SwitchRow
                    icon={<SquareTerminal />}
                    title="コマンド入力をアプリに直接渡す"
                    description="行頭や空白の後に /・$・@ を打つと、続く語を変換せずにアプリへ渡します。空白で普通の入力に戻ります"
                    checked={input.direct}
                    onChange={() => updateInput((i) => { i.direct_prefixes = input.direct ? "" : DIRECT_PREFIXES; })}
                />
                <SwitchRow
                    icon={<Terminal />}
                    title="シェルの入力待ちでは英字"
                    description="端末で PowerShell・コマンド プロンプト・bash が入力を待っているときは英字のまま入れます (半角/全角 でその場だけ解除)"
                    checked={input.shellLatin}
                    onChange={() => updateInput((i) => { i.latin_at_shell_prompt = !input.shellLatin; })}
                />
                <label className="flex items-center space-x-4 rounded-md border p-4">
                    <Terminal />
                    <span className="flex-1 text-sm font-medium">端末の確認キーを英数で通す</span>
                    <Switch checked={input.choiceLatin} onCheckedChange={() => updateInput((i) => { i.latin_at_terminal_choices = !input.choiceLatin; })} />
                </label>
            </section>
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">外国語の入力</h1>
                <div className="space-y-3 rounded-md border p-4">
                    <div className="flex items-center space-x-4">
                        <LanguagesIcon />
                        <div className="flex-1 space-y-1">
                            <p className="text-sm font-medium leading-none">使う言語</p>
                            <p className="text-xs text-muted-foreground">
                                キーの長押しで é ñ ü などを入力できます。数字キーで選択、Esc で取り消し
                            </p>
                        </div>
                    </div>
                    <label className="flex items-center space-x-2 text-sm">
                        <Switch
                            checked={input.accentsInKana}
                            disabled={input.languages.length === 0}
                            onCheckedChange={() => updateInput((i) => { i.accents_in_kana = !input.accentsInKana; })}
                        />
                        <span>日本語入力 (あ) のときも長押しを使う</span>
                        <span className="text-xs text-muted-foreground">オフなら英数 (A) のときだけ</span>
                    </label>
                    {/* 2026-11-05: 英字のキーを押しっぱなしにしたときに同じ字を続けて入れるか (初期値オフ) */}
                    <label className="flex items-center space-x-2 text-sm">
                        <Switch
                            checked={input.letterKeyRepeat}
                            disabled={input.languages.length === 0}
                            onCheckedChange={() => updateInput((i) => { i.letter_key_repeat = !input.letterKeyRepeat; })}
                        />
                        <span>英字のキーを押し続けたら同じ字を続けて入れる</span>
                        <span className="text-xs text-muted-foreground">オフなら長押しの候補だけ (数字・記号・BackSpace は続けて入ります)</span>
                    </label>
                    <div className="grid grid-cols-2 gap-2">
                        {LATIN_LANGUAGES.map(([code, name, sample]) => (
                            <div key={code} className="space-y-1">
                                <label className="flex items-center space-x-2 text-sm">
                                    <Switch
                                        checked={input.languages.includes(code)}
                                        onCheckedChange={() => updateInput((i) => {
                                            const list: string[] = i.latin_languages ?? [];
                                            i.latin_languages = list.includes(code) ? list.filter((c) => c !== code) : [...list, code];
                                            if (!list.includes(code)) i.accents_in_kana = true;
                                        })}
                                    />
                                    <span>{name}</span>
                                    <span className="text-xs text-muted-foreground">{sample}</span>
                                </label>
                                {/* 2026-11-06: 選んである言語の次の語の候補の表が入っていなければ一言 (長押しは表が無くても使える) */}
                                {input.languages.includes(code) && missingNextWords?.includes(code) && (
                                    <p className="pl-11 text-xs text-amber-600 dark:text-amber-400">
                                        長押しは使えます。この言語の次の語の候補・つづりの直しの表は入っていません。インストーラーをもう一度実行して、言語のページでこの言語を選ぶと入ります
                                    </p>
                                )}
                            </div>
                        ))}
                    </div>
                </div>
            </section>
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">数式・化学式の入力</h1>
                <SwitchRow
                    icon={<Superscript />}
                    title="^ と _ で上付き・下付き"
                    description="x^2 → x²、H_2O → H₂O、10^{-3} → 10⁻³。{ } でまとめて指定できます"
                    checked={input.scripts}
                    onChange={() => updateInput((i) => { i.caret_scripts = !input.scripts; })}
                />
                <SwitchRow
                    icon={<Sigma />}
                    title="ギリシャ文字"
                    description="アルファ → α、ガンマ線 → γ線。プラスアルファ・アルファベットのようなカタカナ語は変えません。ぱい・ろー などは候補にだけ出します"
                    checked={input.greek}
                    onChange={() => updateInput((i) => { i.greek_letters = !input.greek; })}
                />
                {/* 2026-11-06 (仕事 32): 字ごとに先に出す形 (小文字か大文字か)。押すと入れ替わる */}
                <div className={`flex flex-wrap items-center gap-0.5 px-4 ${input.greek ? "" : "opacity-50"}`}>
                    <span className="mr-1 text-xs text-muted-foreground">先に出す形 (押すと大文字・小文字を入れ替え):</span>
                    {GREEK.map(([lower, upper]) => {
                        const upperFirst = greekUpperFirst(input.greekCase, lower);
                        return (
                            <button
                                key={lower}
                                type="button"
                                disabled={!input.greek}
                                className="h-6 w-6 rounded text-sm text-foreground hover:bg-accent disabled:cursor-default"
                                title={upperFirst ? `${upper} が先 (押すと ${lower} を先に)` : `${lower} が先 (押すと ${upper} を先に)`}
                                onClick={() => toggleGreekCase(lower)}
                            >
                                {upperFirst ? upper : lower}
                            </button>
                        );
                    })}
                </div>
                <SwitchRow
                    icon={<Hash />}
                    title="数字のあとの . , を小数点・桁区切りに"
                    description="2.0、python3.12、1,000円 (iOS と同じ。, は後ろがちょうど 3 桁のときだけで、1,00 は 1、00)。文末の 答えは2。 は句点のまま"
                    checked={input.numberPunct}
                    onChange={() => updateInput((i) => { i.number_punctuation = !input.numberPunct; })}
                />
                <SwitchRow
                    icon={<FlaskConical />}
                    title="化学式の数字を下付きに"
                    description="H2O → H₂O、Ca(OH)2 → Ca(OH)₂。元素記号として読める並びだけ (PS5・A4 などは変えません)"
                    checked={input.chem}
                    onChange={() => updateInput((i) => { i.chem_subscript = !input.chem; })}
                />
                {/* 2026-11-05 (仕事 28): 数学の式。オンにしたら分野の「数学」もオンにする (オフにしたときは分野に手を付けない) */}
                <SwitchRow
                    icon={<Pi />}
                    title="数学の式を英字のまま"
                    description="式の a= や ax (a と x のかけ算) を英字のまま入れます (あ= にしない)"
                    checked={input.mathExpressions}
                    onChange={() => updateInput((i) => {
                        i.math_expressions = !input.mathExpressions;
                        if (i.math_expressions) {
                            i.domains_off = ((i.domains_off ?? []) as string[]).filter((c) => c !== "math");
                        }
                    })}
                />
            </section>
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">専門用語の辞書</h1>
                <div className="space-y-3 rounded-md border p-4">
                    <div className="flex items-center space-x-4">
                        <Scale />
                        <div className="flex-1 space-y-1">
                            <p className="text-sm font-medium leading-none">分野ごとの用語</p>
                            <p className="text-xs text-muted-foreground">
                                分野の用語を、ふつうの言葉より控えめに足します。<br />
                                「よく打つ」に印を付けた分野は、話題に関係なく用語を先に出します。
                            </p>
                        </div>
                    </div>
                    {/* 2026-11-05 (仕事 26): 分野ごとのスイッチと「よく打つ」の印。印は分野がオンのときだけ出す (オフのときは灰色にせず隠し、場所は空けたまま)。
                        オフにしても「よく打つ」の印 (domains_focus) は消さない (オンに戻すと同じ印で使う。変換サーバーはオフの分野の印を使わない) */}
                    <TooltipProvider delayDuration={300}>
                        <div className="grid grid-cols-1 gap-2">
                            {shownDomains.map(([code, name]) => {
                                const off = input.domainsOff.includes(code);
                                return (
                                    <div key={code} className="flex items-center space-x-2 text-sm">
                                        <Switch
                                            checked={!off}
                                            onCheckedChange={() => updateInput((i) => {
                                                const list: string[] = i.domains_off ?? [];
                                                i.domains_off = list.includes(code) ? list.filter((c) => c !== code) : [...list, code];
                                                // 2026-11-05 (仕事 28): 分野の「数学」をオンにしたら、数学の式もオンにする (オフにしたときは式に手を付けない)
                                                if (code === "math" && list.includes(code)) {
                                                    i.math_expressions = true;
                                                }
                                            })}
                                        />
                                        <Tooltip>
                                            <TooltipTrigger asChild>
                                                <span className="flex-1 cursor-default">{name}</span>
                                            </TooltipTrigger>
                                            <TooltipContent side="right" className="max-w-xs">
                                                {domainDetail(code, name)}
                                            </TooltipContent>
                                        </Tooltip>
                                        <label className={"flex items-center space-x-1 text-xs text-muted-foreground" + (off ? " invisible" : "")}>
                                            <input
                                                type="checkbox"
                                                tabIndex={off ? -1 : undefined}
                                                checked={input.domainsFocus.includes(code)}
                                                onChange={() => updateInput((i) => {
                                                    const focus: string[] = i.domains_focus ?? [];
                                                    i.domains_focus = focus.includes(code) ? focus.filter((c) => c !== code) : [...focus, code];
                                                })}
                                            />
                                            <span>よく打つ</span>
                                        </label>
                                    </div>
                                );
                            })}
                        </div>
                    </TooltipProvider>
                </div>
            </section>
            {shownDialects.length > 0 && (
                <section className="space-y-2">
                    {/* 2026-11-06 (仕事 39): 方言の語尾。選んだ方言の文の終わり・助動詞のまとまりを辞書に足す (初期値は使わない) */}
                    <h1 className="text-sm font-bold text-foreground">方言</h1>
                    <SelectRow
                        icon={<MessageCircle />}
                        title="方言の語尾"
                        description="選んだ方言の文の終わりや助動詞を、かなのまま出しやすくします (行かへん・来はった・そうやねん・ええやん)。ふだん方言で打つ人向けです"
                        value={input.dialect || "none"}
                        options={[["none", "使わない"], ...shownDialects]}
                        onChange={(v) => updateInput((i) => {
                            const others = ((i.dialects ?? []) as string[]).filter((d) => !DIALECTS.some(([code]) => code === d));
                            i.dialects = v === "none" ? others : [...others, v];
                        })}
                    />
                </section>
            )}
            {showInternetSlang && (
                <section className="space-y-2">
                    {/* 2026-11-07 (仕事 43): ネットスラングの辞書。オンにした人だけ、変換サーバーが domain_slang_internet を読む (初期値はオフ) */}
                    <h1 className="text-sm font-bold text-foreground">くだけた言葉</h1>
                    <SwitchRow
                        icon={<Hash />}
                        title="ネットスラング"
                        description="ネットで使われる言葉 (オワコン・親ガチャ・ぴえん など) を変換に足します。ふだんの文章の変換に混ぜたくない人はオフのままにしてください"
                        checked={input.internetSlang}
                        onChange={() => updateInput((i) => { i.internet_slang = !input.internetSlang; })}
                    />
                </section>
            )}
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">変換の直し</h1>
                <SwitchRow
                    icon={<Replace />}
                    title="難読語の同音語に置き換える"
                    description="文脈で難しい語のほうが合うときに置き換えます (当局の商用 → 当局の慫慂)"
                    checked={input.homophoneReplace}
                    onChange={() => updateInput((i) => { i.homophone_replace = !input.homophoneReplace; })}
                />
                <SwitchRow
                    icon={<BookA />}
                    title="表記ゆれの選び方を覚える"
                    description="候補で選び直した書き方 (こと・事、および・及び) を、次の変換から使います"
                    checked={input.hyoukiLearning}
                    onChange={() => updateInput((i) => { i.hyouki_learning = !input.hyoukiLearning; })}
                />
            </section>
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">学習</h1>
                <ResetLearningRow />
            </section>
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">言葉の使い方の注記</h1>
                <SwitchRow
                    icon={<MessageSquareWarning />}
                    title="よくある誤用の注記"
                    description="候補窓で、よくある誤用の候補に「本来は〜」を添えます (的を得る → 本来は 的を射る)。出典: 文化庁「国語に関する世論調査」、Wikipedia「日本語の誤用」"
                    checked={input.misuseNotes}
                    onChange={() => updateInput((i) => { i.misuse_notes = !input.misuseNotes; })}
                />
                <SwitchRow
                    icon={<BookOpenText />}
                    title="異字同訓の使い分けの注記"
                    description="候補窓で、同じ読みの漢字 (会う・合う・遭う) に使い分けを添えます。出典: 文化審議会「「異字同訓」の漢字の使い分け例（報告）」"
                    checked={input.homophoneNotes}
                    onChange={() => updateInput((i) => { i.homophone_notes = !input.homophoneNotes; })}
                />
                {/* 2026-11-06 (仕事 40): 注意の注記。候補は替えず、横に「注意」を添えるだけ。出典は配布物の参考文献の一覧 (licenses) */}
                <SwitchRow
                    icon={<ShieldAlert />}
                    title="差別・侮辱の語の注意"
                    description="候補窓で、差別や強い侮辱の語とされる語に「注意」を添えます。候補はそのまま出します (チャットやゲームで弾かれやすい語に気づけるように)。出典は参考文献の一覧にあります"
                    checked={input.cautionSlurs}
                    onChange={() => updateInput((i) => { i.caution_slurs = !input.cautionSlurs; })}
                />
                <SwitchRow
                    icon={<Megaphone />}
                    title="放送などで言い換えられる語の注意"
                    description="候補窓で、放送や新聞などで言い換えられることがある語に「注意」を添えます。候補はそのまま出します。出典は参考文献の一覧にあります"
                    checked={input.cautionBroadcast}
                    onChange={() => updateInput((i) => { i.caution_broadcast = !input.cautionBroadcast; })}
                />
            </section>
            <section className="space-y-2">
                {/* 2026-10-28: 試験中の働き (初期値はオフ。ふだんの打ち方が変わるので、オンにする人だけ) */}
                <h1 className="text-sm font-bold text-foreground">試験中の働き</h1>
                <SwitchRow
                    icon={<Keyboard />}
                    title="句読点で変換する (試験中)"
                    description="。、？！ を打ったら、そこまでを変換します。確定は次の字か Enter で、Enter を押す回数は今までと同じです。変換の直後の BackSpace で入力中に戻ります"
                    checked={input.punctuationConvert}
                    onChange={() => updateInput((i) => { i.punctuation_convert = !input.punctuationConvert; })}
                />
                <SwitchRow
                    icon={<FlaskConical />}
                    title="ローマ字の打ち間違いを直す (試験中)"
                    description="変換するときに、母音を打ち落とした所 (souiuktodesu → そういうことです) と、母音を 1 つ多く打った所 (motsuuutsukushii → 持つ美しい) を直します。英単語・名前として打った字は直しません。打ったとおりも候補に残り、直した直後の Shift+BackSpace で打ったとおりに戻せます (戻した綴りは次から直しません)"
                    checked={input.typoRepair}
                    onChange={() => updateInput((i) => { i.typo_repair = !input.typoRepair; })}
                />
            </section>
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">バージョンと更新プログラム</h1>
                <div className="flex items-center space-x-4 rounded-md border p-4">
                    <RefreshCcw />
                    <div className="flex-1 space-y-1">
                        <p className="text-sm font-medium leading-none">
                            {version ? `v${version}` : "HaniWord"}
                        </p>
                    </div>
                    <Button  variant="secondary">
                        <a href="https://github.com/supertastybanana3/HaniWord/releases" className="flex items-center gap-x-2" target="_blank" rel="noopener noreferrer">
                            <ExternalLink />
                            更新を確認する
                        </a>
                    </Button>
                </div>
                <SwitchRow
                    icon={<BellRing />}
                    title="新しい版を知らせる"
                    description="1 日 1 回、GitHub に新しい版が出ていないかを確かめ、出ていれば Windows の通知で知らせます (入れ替えは自動ではしません)。送るのは GitHub への問い合わせだけで、打った字・学習・設定は送りません"
                    checked={notifyUpdates}
                    onChange={toggleNotifyUpdates}
                />
            </section>
            {/* <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">診断とフィードバック</h1>
                <div className="flex items-center space-x-4 rounded-md border p-4">
                    <FileChartColumn />
                    <div className="flex-1 space-y-1">
                        <p className="text-sm font-medium leading-none">
                            診断データ
                        </p>
                        <p className="text-xs text-muted-foreground">
                            診断データを保存し、バグの修正に役立てます
                        </p>
                    </div>
                    <Switch />
                </div>
            </section> */}
        </div>
    )
}
