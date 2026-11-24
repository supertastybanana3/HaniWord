import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select"
import { Pencil, Trash2, Check, X, FileUp, FileDown } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner"
import { invoke } from '@tauri-apps/api/core';

// 2026-11-04: 単語登録 (%APPDATA%\HaniWord\user_dictionary.tsv)。変換サーバーは保存のたびに読み直す
// 品詞・読みの長さは shared::user_dictionary と同じ
type Entry = { reading: string; word: string; pos: string; memo: string };
type Parsed = { entries: Entry[]; errors: string[] };

const PARTS_OF_SPEECH = ["名詞", "固有名詞", "人名", "地名", "短縮よみ"];
const MAX_READING = 20;
const MAX_BYTES = 4 << 20;
const MAX_ENTRIES = 50_000;
// 一度に並べる数 (それより多いときは絞り込んで探す)
const SHOWN = 200;

const EMPTY: Entry = { reading: "", word: "", pos: "名詞", memo: "" };

// カタカナはひらがなにそろえる (ヴ → ゔ)
const normalizeReading = (text: string) =>
    text.trim().replace(/[ァ-ヶ]/g, (c) => String.fromCharCode(c.charCodeAt(0) - 0x60));

// 直す前の形で確かめる。よければ null
const problemOf = (entry: Entry, others: Entry[]) => {
    if (entry.reading === "") return "読みを入れてください";
    if (!/^[ぁ-ゖーゔ]+$/.test(entry.reading)) return "読みはひらがなで入れてください";
    if ([...entry.reading].length > MAX_READING) return `読みは ${MAX_READING} 字までです`;
    if (entry.word === "") return "語を入れてください";
    if (others.some((e) => e.reading === entry.reading && e.word === entry.word)) return "同じ読みと語がもう登録されています";
    return null;
};

const tidy = (entry: Entry): Entry => ({
    reading: normalizeReading(entry.reading),
    word: entry.word.trim(),
    pos: entry.pos,
    memo: entry.memo.trim(),
});

const PosSelect = ({ value, onChange }: { value: string; onChange: (pos: string) => void }) => (
    <Select value={value} onValueChange={onChange}>
        <SelectTrigger className="w-28 shrink-0">
            <SelectValue />
        </SelectTrigger>
        <SelectContent>
            {PARTS_OF_SPEECH.map((pos) => <SelectItem key={pos} value={pos}>{pos}</SelectItem>)}
        </SelectContent>
    </Select>
);

export const UserDictionary = () => {
    const [entries, setEntries] = useState<Entry[]>([]);
    const [unreadable, setUnreadable] = useState<string[]>([]);
    const [draft, setDraft] = useState<Entry>(EMPTY);
    const [draftProblem, setDraftProblem] = useState<string | null>(null);
    const [editing, setEditing] = useState<{ index: number; original: Entry; entry: Entry; problem: string | null } | null>(null);
    const [filter, setFilter] = useState("");
    const fileInput = useRef<HTMLInputElement>(null);
    const readingInput = useRef<HTMLInputElement>(null);
    // 保存した最新の登録 (続けて操作しても前の変更を消さないように、保存は 1 つずつ順に、いつも最新の登録から作る)
    const saved = useRef<Entry[]>([]);
    const queue = useRef<Promise<unknown>>(Promise.resolve());

    useEffect(() => {
        invoke<Parsed>("user_dictionary_list")
            .then((parsed) => {
                saved.current = parsed.entries;
                setEntries(parsed.entries);
                setUnreadable(parsed.errors);
            })
            .catch(() => toast("単語登録を読めませんでした"));
    }, []);

    // update は最新の登録から次の登録を作る (作れないときは理由の文字列)。保存できたときだけ画面を変える。結果は保存できたか・理由
    const persist = (update: (current: Entry[]) => Entry[] | string): Promise<true | string> => {
        const run = queue.current.then(async (): Promise<true | string> => {
            const next = update(saved.current);
            if (typeof next === "string") return next;
            try {
                await invoke("user_dictionary_save", { entries: next });
            } catch (error) {
                toast("保存できませんでした", { description: String(error) });
                return "";
            }
            saved.current = next;
            setEntries(next);
            setUnreadable([]);
            return true;
        });
        queue.current = run;
        return run;
    };
    const same = (a: Entry, b: Entry) => a.reading === b.reading && a.word === b.word;

    const add = async (event: React.FormEvent) => {
        event.preventDefault();
        const entry = tidy(draft);
        const result = await persist((current) => problemOf(entry, current) ?? [...current, entry]);
        if (result === true) {
            setDraftProblem(null);
            setDraft({ ...EMPTY, pos: draft.pos });
            readingInput.current?.focus();
        } else if (result !== "") {
            setDraftProblem(result);
        }
    };

    const saveEdit = async () => {
        if (!editing) return;
        const entry = tidy(editing.entry);
        const original = editing.original;
        const result = await persist((current) => {
            const index = current.findIndex((e) => same(e, original));
            if (index < 0) return "直している語が見つかりません";
            return problemOf(entry, current.filter((_, i) => i !== index)) ?? current.map((e, i) => i === index ? entry : e);
        });
        if (result === true) setEditing(null);
        else if (result !== "") setEditing({ ...editing, problem: result });
    };

    const remove = async (removed: Entry) => {
        let at = 0;
        const result = await persist((current) => {
            at = current.findIndex((e) => same(e, removed));
            return at < 0 ? "" : current.filter((_, i) => i !== at);
        });
        if (result === true) {
            toast(`「${removed.word}」を消しました`, {
                action: {
                    label: "元に戻す",
                    onClick: () => {
                        persist((current) => current.some((e) => same(e, removed)) ? ""
                            : [...current.slice(0, at), removed, ...current.slice(at)]);
                    },
                },
            });
        }
    };

    const importFile = async (event: React.ChangeEvent<HTMLInputElement>) => {
        const file = event.target.files?.[0];
        event.target.value = "";
        if (!file) return;
        if (file.size > MAX_BYTES) {
            toast("ファイルが大きすぎます", { description: `${MAX_BYTES >> 20} MB までです` });
            return;
        }
        try {
            const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
            const parsed = await invoke<Parsed>("user_dictionary_parse", { bytes });
            let added: Entry[] = [];
            const result = await persist((current) => {
                const seen = new Set(current.map((e) => e.reading + "\t" + e.word));
                added = parsed.entries.filter((e) => {
                    const key = e.reading + "\t" + e.word;
                    if (seen.has(key)) return false;
                    seen.add(key);
                    return true;
                }).slice(0, Math.max(0, MAX_ENTRIES - current.length));
                return added.length > 0 ? [...current, ...added] : "";
            });
            if (result !== true && added.length > 0) return;
            const skipped = parsed.entries.length - added.length;
            const notes = [
                skipped > 0 ? `登録済みの語・上限 (${MAX_ENTRIES.toLocaleString()} 語) を超えた ${skipped} 語は飛ばしました` : "",
                parsed.errors.length > 0 ? `読めなかった行が ${parsed.errors.length} 行あります（${parsed.errors.slice(0, 3).join("、")}${parsed.errors.length > 3 ? " ほか" : ""}）` : "",
            ].filter((note) => note !== "");
            toast(`${added.length} 語を読み込みました`, { description: notes.join("。") || undefined, duration: notes.length > 0 ? 10000 : 4000 });
        } catch (error) {
            toast("読み込めませんでした", { description: String(error) });
        }
    };

    const exportFile = async () => {
        try {
            const path = await invoke<string>("user_dictionary_export", { entries });
            toast("書き出しました", { description: path });
        } catch (error) {
            toast("書き出せませんでした", { description: String(error) });
        }
    };

    // 新しく登録した語を上に
    const query = normalizeReading(filter);
    const matching = entries
        .map((entry, index) => ({ entry, index }))
        .reverse()
        .filter(({ entry }) => query === "" || entry.reading.includes(query) || entry.word.includes(filter.trim()) || entry.memo.includes(filter.trim()));
    const shown = matching.slice(0, SHOWN);

    return (
        <div className="space-y-8">
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">単語登録</h1>
                <p className="text-xs text-muted-foreground">
                    変換で出したい語を登録します。AI の変換でも、登録した語は書いたとおりに出ます
                </p>
                <form className="flex flex-wrap items-center gap-2 rounded-md border p-4" onSubmit={add}>
                    <Input ref={readingInput} className="w-40" placeholder="読み（ひらがな）" value={draft.reading}
                        aria-invalid={draftProblem !== null}
                        onChange={(e) => { setDraft({ ...draft, reading: e.target.value }); setDraftProblem(null); }} />
                    <Input className="w-44 flex-1" placeholder="語" value={draft.word}
                        onChange={(e) => { setDraft({ ...draft, word: e.target.value }); setDraftProblem(null); }} />
                    <PosSelect value={draft.pos} onChange={(pos) => setDraft({ ...draft, pos })} />
                    <Input className="w-32" placeholder="メモ" value={draft.memo}
                        onChange={(e) => setDraft({ ...draft, memo: e.target.value })} />
                    <Button type="submit" size="sm">登録</Button>
                    {draftProblem && <p className="w-full text-xs text-destructive">{draftProblem}</p>}
                </form>
            </section>

            <section className="space-y-2">
                <div className="flex items-center gap-2">
                    <h2 className="text-sm font-bold text-foreground">登録した語</h2>
                    <span className="text-xs text-muted-foreground">{entries.length} 語</span>
                    <div className="flex-1" />
                    <Button variant="ghost" size="sm" onClick={() => fileInput.current?.click()}>
                        <FileUp />読み込む
                    </Button>
                    <Button variant="ghost" size="sm" disabled={entries.length === 0} onClick={exportFile}>
                        <FileDown />書き出す
                    </Button>
                    <input ref={fileInput} type="file" accept=".txt,.tsv,.csv" className="hidden" onChange={importFile} />
                </div>
                {unreadable.length > 0 && (
                    <p className="text-xs text-destructive">
                        読めなかった行が {unreadable.length} 行あります（{unreadable.slice(0, 3).join("、")}）。ここで保存すると、その行は消えます
                    </p>
                )}
                {entries.length > 20 && (
                    <Input placeholder="読み・語・メモで絞り込む" value={filter} onChange={(e) => setFilter(e.target.value)} />
                )}
                {entries.length === 0 ? (
                    <p className="text-xs text-muted-foreground">
                        まだありません。ほかの IME の辞書（テキスト形式で書き出したもの）も読み込めます
                    </p>
                ) : (
                    <div className="divide-y rounded-md border">
                        {shown.map(({ entry, index }) => editing?.index === index ? (
                            <div key={index} className="space-y-1 px-3 py-2"
                                onKeyDown={(e) => { if (e.key === "Escape") setEditing(null); if (e.key === "Enter") saveEdit(); }}>
                                <div className="flex flex-wrap items-center gap-2">
                                    <Input className="w-40" value={editing.entry.reading} aria-invalid={editing.problem !== null} autoFocus
                                        onChange={(e) => setEditing({ ...editing, entry: { ...editing.entry, reading: e.target.value }, problem: null })} />
                                    <Input className="w-44 flex-1" value={editing.entry.word}
                                        onChange={(e) => setEditing({ ...editing, entry: { ...editing.entry, word: e.target.value }, problem: null })} />
                                    <PosSelect value={editing.entry.pos} onChange={(pos) => setEditing({ ...editing, entry: { ...editing.entry, pos } })} />
                                    <Input className="w-32" placeholder="メモ" value={editing.entry.memo}
                                        onChange={(e) => setEditing({ ...editing, entry: { ...editing.entry, memo: e.target.value } })} />
                                    <Button variant="ghost" size="icon" className="size-8" title="保存" onClick={saveEdit}><Check /></Button>
                                    <Button variant="ghost" size="icon" className="size-8" title="やめる" onClick={() => setEditing(null)}><X /></Button>
                                </div>
                                {editing.problem && <p className="text-xs text-destructive">{editing.problem}</p>}
                            </div>
                        ) : (
                            <div key={index} className="group flex items-center gap-3 px-3 py-1.5 text-sm">
                                <span className="w-40 shrink-0 truncate text-muted-foreground">{entry.reading}</span>
                                <span className="flex-1 truncate">{entry.word}</span>
                                <span className="w-16 shrink-0 text-xs text-muted-foreground">{entry.pos}</span>
                                <span className="w-32 shrink-0 truncate text-xs text-muted-foreground">{entry.memo}</span>
                                <div className="flex opacity-0 group-hover:opacity-100 focus-within:opacity-100">
                                    <Button variant="ghost" size="icon" className="size-7" title="直す"
                                        onClick={() => setEditing({ index, original: entry, entry, problem: null })}><Pencil /></Button>
                                    <Button variant="ghost" size="icon" className="size-7" title="消す"
                                        onClick={() => remove(entry)}><Trash2 /></Button>
                                </div>
                            </div>
                        ))}
                    </div>
                )}
                {matching.length > SHOWN && (
                    <p className="text-xs text-muted-foreground">ほか {matching.length - SHOWN} 語（絞り込むと出ます）</p>
                )}
            </section>
        </div>
    )
}
