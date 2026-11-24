import { Button } from "@/components/ui/button";
import { Palette, Type, CaseSensitive, ALargeSmall, LayoutList, Highlighter, SquareDashed, ArrowUp, ArrowDown, X } from "lucide-react";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select"
import { useEffect, useState } from "react";
import { toast } from "sonner"
import { invoke } from '@tauri-apps/api/core';

// 2026-11-06 (仕事 37): 候補の窓の外観 (settings.json の appearance)。どれも初期値は今のまま。表示の窓 (ui.exe) が、次に候補の窓を出したときから使う。
// 値の決まりは shared の AppearanceConfig と同じ (知らない値は今のまま)
type Appearance = {
    font: string;
    latin_font: string;
    font_size: number;
    theme: string;
    selected_color: string;
    layout: string;
    // 補うフォントの並び。undefined は今のまま (候補のフォントが今のままなら Meiryo UI、そのあと Windows が補う)。"@notdef" でそこで止めて □ で出す
    fallback?: string[];
};
type FontFamily = { name: string; japanese: boolean };

const DEFAULTS: Appearance = { font: "", latin_font: "", font_size: 14, theme: "light", selected_color: "", layout: "standard" };
const STOP = "@notdef";
const SIZES = Array.from({ length: 19 }, (_, i) => 10 + i);
const fromConfig = (data: any): Appearance => ({ ...DEFAULTS, ...(data?.appearance ?? {}) });

// 候補の窓と同じ色の組 (crates/ui/src/candidate.rs の CSS の変数と同じ値)
const LIGHT = {
    bg: "#FFFFFF", border: "rgb(150, 150, 150)", text: "#000000", shortcutBg: "rgb(243, 244, 255)", shortcutText: "rgb(97, 97, 97)",
    desc: "rgb(136, 136, 136)", selBg: "rgb(209, 234, 255)", selBorder: "rgb(127, 172, 221)", footerBorder: "rgb(96, 96, 96)",
    footerBg: "linear-gradient(rgb(255, 255, 255), rgb(238, 238, 238))", footerText: "rgb(76, 76, 76)",
};
const DARK = {
    bg: "#2B2B2B", border: "rgb(110, 110, 110)", text: "#F2F2F2", shortcutBg: "rgb(52, 54, 64)", shortcutText: "rgb(190, 190, 190)",
    desc: "rgb(160, 160, 160)", selBg: "rgb(38, 79, 120)", selBorder: "rgb(86, 140, 200)", footerBorder: "rgb(110, 110, 110)",
    footerBg: "linear-gradient(rgb(52, 52, 52), rgb(43, 43, 43))", footerText: "rgb(200, 200, 200)",
};
const mix = (hex: string, base: string, t: number) => {
    const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
    const b = [1, 3, 5].map((i) => parseInt(base.slice(i, i + 2), 16));
    return `rgb(${c.map((v, k) => Math.round(v * t + b[k] * (1 - t))).join(", ")})`;
};
const fontStack = (a: Appearance) => {
    const fonts: string[] = [];
    if (a.latin_font) fonts.push(JSON.stringify(a.latin_font));
    fonts.push(a.font ? JSON.stringify(a.font) : '"Yu Gothic UI"');
    // 候補の窓 (candidate.rs の applyAppearance) と同じ決まり
    let stop = false;
    if (Array.isArray(a.fallback)) {
        for (const f of a.fallback) {
            if (f === STOP) { stop = true; break; }
            fonts.push(JSON.stringify(f));
        }
    } else if (!a.font) {
        fonts.push('"Meiryo UI"');
    }
    fonts.push(...(stop ? ['"HW Emoji"', '"HW NotDef"'] : ["sans-serif"]));
    return fonts.join(", ");
};

// 候補の窓の見本 (候補の窓と同じ形・色・字。選んだ候補は 2 番目)
const Preview = ({ a, accent }: { a: Appearance; accent: string }) => {
    const prefersDark = typeof window !== "undefined" && window.matchMedia?.("(prefers-color-scheme: dark)").matches;
    const dark = a.theme === "dark" || (a.theme === "system" && prefersDark);
    const p = { ...(dark ? DARK : LIGHT) };
    if (a.layout === "flat") { p.shortcutBg = "transparent"; p.footerBg = p.bg; p.footerBorder = p.border; }
    const color = a.selected_color === "accent" ? accent : a.selected_color;
    if (/^#[0-9a-fA-F]{6}$/.test(color)) { p.selBorder = color; p.selBg = mix(color, dark ? "#2B2B2B" : "#FFFFFF", dark ? 0.45 : 0.25); }
    const pad = a.layout === "compact" ? "0px" : "1px";
    const rows: [string, string][] = [["変換", ""], ["返還", ""], ["偏寛", ""], ["ﾍﾝｶﾝ", "[半]"], ["Hello 😀", ""], ["蕅椂", "環境依存"]];
    return (
        <div style={{ fontFamily: fontStack(a), fontSize: a.font_size, lineHeight: a.layout === "compact" ? 1.2 : 1.35, display: "inline-block",
            border: `1px solid ${p.border}`, background: p.bg, userSelect: "none" }}>
            <table style={{ borderCollapse: "separate", borderSpacing: 0 }}>
                <tbody>
                    {rows.map(([text, note], i) => {
                        const focused = i === 1;
                        const cell = { padding: `${pad} 0`, whiteSpace: "nowrap" as const, borderTop: `1px solid ${focused ? p.selBorder : "transparent"}`,
                            borderBottom: `1px solid ${focused ? p.selBorder : "transparent"}`, background: focused ? p.selBg : undefined };
                        return (
                            <tr key={i}>
                                <td style={{ ...cell, background: focused ? p.selBg : p.shortcutBg, color: p.shortcutText, padding: `${pad} 8px`, textAlign: "center",
                                    borderLeft: `1px solid ${focused ? p.selBorder : "transparent"}` }}>{i + 1}</td>
                                <td style={{ ...cell, color: p.text, paddingLeft: 4, paddingRight: 8 }}>{text}</td>
                                <td style={{ ...cell, color: p.desc, fontSize: Math.max(9, a.font_size - 2), paddingRight: 8, textAlign: "right",
                                    borderRight: `1px solid ${focused ? p.selBorder : "transparent"}` }}>{note}</td>
                            </tr>
                        );
                    })}
                </tbody>
            </table>
            <div style={{ borderTop: `1px solid ${p.footerBorder}`, background: p.footerBg, color: p.footerText, textAlign: "right", padding: "0 4px" }}>2/6</div>
        </div>
    );
};

const Row = ({ icon, title, description, children }: { icon: React.ReactNode; title: string; description: string; children: React.ReactNode }) => (
    <div className="flex items-center space-x-4 rounded-md border p-4">
        {icon}
        <div className="flex-1 space-y-1">
            <p className="text-sm font-medium leading-none">{title}</p>
            <p className="text-xs text-muted-foreground">{description}</p>
        </div>
        {children}
    </div>
);

export const Appearance = () => {
    const [a, setA] = useState<Appearance>(DEFAULTS);
    const [fonts, setFonts] = useState<FontFamily[]>([]);
    const [accent, setAccent] = useState("#0078d4");

    useEffect(() => {
        invoke<any>("get_config").then((data) => setA(fromConfig(data))).catch(() => {});
        invoke<FontFamily[]>("system_fonts").then(setFonts).catch(() => {});
        invoke<string>("accent_color").then(setAccent).catch(() => {});
    }, []);

    const update = async (updater: (a: any) => void) => {
        try {
            const data = await invoke<any>("get_config");
            data.appearance = { ...DEFAULTS, ...(data.appearance ?? {}) };
            updater(data.appearance);
            await invoke("update_config", { newConfig: data });
            setA(fromConfig(data));
            toast("設定を変更しました", { description: "次に候補の窓を出したときから有効になります" });
        } catch (error) {
            toast("設定の更新に失敗しました");
        }
    };
    // Select は空の値を持てないので、今のまま ("") を "default" で表す
    const fromSelect = (v: string) => (v === "default" ? "" : v);
    const japanese = fonts.filter((f) => f.japanese);
    // 補うフォントの並び (今のままのときは、今の決まりで出る並びを見せる)。足す・動かす・消すと、今の並びを書いて今のままでなくなる
    const chain: string[] = a.fallback ?? (a.font ? [] : ["Meiryo UI"]);
    // 止めの印より後ろのフォントも並びには残す (候補の窓は止めの印までしか使わない。印を下へ戻せばまた使う)
    const setChain = (next: string[]) => update((x) => { x.fallback = next; });
    const stopAt = chain.indexOf(STOP);
    // 足すフォントは止めの印の前に入れる (印より後ろに足しても使われないため)
    const add = (v: string) => {
        if (!v || chain.includes(v)) return;
        setChain(v !== STOP && stopAt >= 0 ? [...chain.slice(0, stopAt), v, ...chain.slice(stopAt)] : [...chain, v]);
    };
    const move = (i: number, d: number) => { const next = [...chain]; [next[i], next[i + d]] = [next[i + d], next[i]]; setChain(next); };
    const remove = (i: number) => setChain(chain.filter((_, k) => k !== i));
    const customColor = a.selected_color.startsWith("#");

    return (
        <div className="space-y-8">
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">外観</h1>
                <p className="text-xs text-muted-foreground">候補の窓の字・色・並べ方を選びます。初期値は、どれも「標準」です。</p>
                <div className="rounded-md border p-4 flex justify-center bg-muted/40">
                    <Preview a={a} accent={accent} />
                </div>
                <Row icon={<Type />} title="候補のフォント" description="候補の字のフォントです。標準は Yu Gothic UI です (無いときは Meiryo UI)">
                    <Select value={a.font || "default"} onValueChange={(v) => update((x) => { x.font = fromSelect(v); })}>
                        <SelectTrigger className="w-56"><SelectValue /></SelectTrigger>
                        <SelectContent>
                            <SelectItem value="default">標準 (Yu Gothic UI)</SelectItem>
                            {japanese.map((f) => <SelectItem key={f.name} value={f.name} style={{ fontFamily: JSON.stringify(f.name) }}>{f.name}</SelectItem>)}
                            {a.font && !japanese.some((f) => f.name === a.font) && <SelectItem value={a.font}>{a.font}</SelectItem>}
                        </SelectContent>
                    </Select>
                </Row>
                <Row icon={<CaseSensitive />} title="英字のフォント" description="英字・数字・記号だけ別のフォントにします。選んだフォントに日本語の字もあれば、日本語もそのフォントで出ます">
                    <Select value={a.latin_font || "default"} onValueChange={(v) => update((x) => { x.latin_font = fromSelect(v); })}>
                        <SelectTrigger className="w-56"><SelectValue /></SelectTrigger>
                        <SelectContent>
                            <SelectItem value="default">候補のフォントと同じ</SelectItem>
                            {fonts.map((f) => <SelectItem key={f.name} value={f.name} style={{ fontFamily: JSON.stringify(f.name) }}>{f.name}</SelectItem>)}
                            {a.latin_font && !fonts.some((f) => f.name === a.latin_font) && <SelectItem value={a.latin_font}>{a.latin_font}</SelectItem>}
                        </SelectContent>
                    </Select>
                </Row>
                <Row icon={<ALargeSmall />} title="字の大きさ" description="候補の字の大きさです。標準は 14 です">
                    <Select value={String(a.font_size)} onValueChange={(v) => update((x) => { x.font_size = Number(v); })}>
                        <SelectTrigger className="w-56"><SelectValue /></SelectTrigger>
                        <SelectContent>
                            {SIZES.map((n) => <SelectItem key={n} value={String(n)}>{n === 14 ? "14 (標準)" : String(n)}</SelectItem>)}
                        </SelectContent>
                    </Select>
                </Row>
                <Row icon={<Palette />} title="色の組" description="候補の窓の地と字の色です。Windows に合わせると、Windows のアプリのモード (明るい・暗い) に合わせます">
                    <Select value={a.theme} onValueChange={(v) => update((x) => { x.theme = v; })}>
                        <SelectTrigger className="w-56"><SelectValue /></SelectTrigger>
                        <SelectContent>
                            <SelectItem value="light">明るい (標準)</SelectItem>
                            <SelectItem value="dark">暗い</SelectItem>
                            <SelectItem value="system">Windows に合わせる</SelectItem>
                        </SelectContent>
                    </Select>
                </Row>
                <Row icon={<Highlighter />} title="選んだ候補の色" description="選んでいる候補の枠の色です。地はこの色を薄くした色になります">
                    <div className="flex items-center gap-2">
                        {customColor && <input type="color" aria-label="選んだ候補の色" value={a.selected_color} className="h-9 w-10 cursor-pointer rounded border"
                            onChange={(e) => update((x) => { x.selected_color = e.target.value; })} />}
                        <Select value={customColor ? "custom" : (a.selected_color || "default")}
                            onValueChange={(v) => update((x) => { x.selected_color = v === "custom" ? (customColor ? a.selected_color : "#2f7bd6") : fromSelect(v); })}>
                            <SelectTrigger className="w-44"><SelectValue /></SelectTrigger>
                            <SelectContent>
                                <SelectItem value="default">薄い青 (標準)</SelectItem>
                                <SelectItem value="accent">Windows のアクセントの色</SelectItem>
                                <SelectItem value="custom">色を選ぶ</SelectItem>
                            </SelectContent>
                        </Select>
                    </div>
                </Row>
                <Row icon={<LayoutList />} title="並べ方" description="すっきりは番号の列に色を付けません。詰めるは行の間を狭くします">
                    <Select value={a.layout} onValueChange={(v) => update((x) => { x.layout = v; })}>
                        <SelectTrigger className="w-56"><SelectValue /></SelectTrigger>
                        <SelectContent>
                            <SelectItem value="standard">標準</SelectItem>
                            <SelectItem value="flat">すっきり</SelectItem>
                            <SelectItem value="compact">詰める</SelectItem>
                        </SelectContent>
                    </Select>
                </Row>
                <div className="space-y-3 rounded-md border p-4">
                    <div className="flex items-center space-x-4">
                        <SquareDashed />
                        <div className="flex-1 space-y-1">
                            <p className="text-sm font-medium leading-none">足りない字を補うフォントの順</p>
                            <p className="text-xs text-muted-foreground">
                                候補のフォントに無い字を、上から順に補います。「なし (□)」を置くとそこで止め、無い字を □ で出します (どの字がそのフォントで出ないか分かります)。
                                止めなければ、最後に Windows がほかのフォントで補います。絵文字はいつも Segoe UI Emoji で出します
                            </p>
                        </div>
                        {a.fallback !== undefined && <Button variant="secondary" onClick={() => update((x) => { delete x.fallback; })}>標準に戻す</Button>}
                    </div>
                    <ol className="space-y-1 text-sm">
                        <li className="flex items-center gap-2 rounded px-2 py-1 bg-muted/50">
                            <span className="w-5 text-right text-muted-foreground">1</span>
                            <span className="flex-1">{a.font || "Yu Gothic UI"} <span className="text-xs text-muted-foreground">(候補のフォント)</span></span>
                        </li>
                        {chain.map((f, i) => (
                            <li key={f} className="flex items-center gap-2 rounded px-2 py-1 border">
                                <span className="w-5 text-right text-muted-foreground">{i + 2}</span>
                                <span className={"flex-1" + (stopAt >= 0 && i > stopAt ? " text-muted-foreground line-through" : "")}
                                    style={f === STOP ? undefined : { fontFamily: JSON.stringify(f) }}>{f === STOP ? "なし (□ で出す。ここで止める)" : f}</span>
                                {stopAt >= 0 && i > stopAt && <span className="text-xs text-muted-foreground">止めたので使わない</span>}
                                <Button size="icon" variant="ghost" aria-label="上へ" disabled={i === 0} onClick={() => move(i, -1)}><ArrowUp /></Button>
                                <Button size="icon" variant="ghost" aria-label="下へ" disabled={i === chain.length - 1} onClick={() => move(i, 1)}><ArrowDown /></Button>
                                <Button size="icon" variant="ghost" aria-label="消す" onClick={() => remove(i)}><X /></Button>
                            </li>
                        ))}
                        {!chain.includes(STOP) && (
                            <li className="flex items-center gap-2 rounded px-2 py-1 text-muted-foreground">
                                <span className="w-5 text-right">{chain.length + 2}</span>
                                <span className="flex-1">Windows がほかのフォントで補う{a.fallback === undefined ? " (標準)" : ""}</span>
                            </li>
                        )}
                    </ol>
                    <div className="flex justify-end">
                        <Select value="" onValueChange={(v) => add(v)}>
                            <SelectTrigger className="w-56"><SelectValue placeholder="順に足す" /></SelectTrigger>
                            <SelectContent>
                                {!chain.includes(STOP) && <SelectItem value={STOP}>なし (□ で出す。ここで止める)</SelectItem>}
                                {fonts.filter((f) => !chain.includes(f.name) && f.name !== (a.font || "Yu Gothic UI")).map((f) =>
                                    <SelectItem key={f.name} value={f.name} style={{ fontFamily: JSON.stringify(f.name) }}>{f.name}</SelectItem>)}
                            </SelectContent>
                        </Select>
                    </div>
                </div>
            </section>
        </div>
    )
}
