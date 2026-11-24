// 2026-11-19 (まとめ役の README の下書きの確かめの 6): launcher が変換サーバーを起動する前に書く ai_backend.json (shared::AiBackendStatus) を、
// 設定画面の言い方にする。「HaniWord について」と Zenzai の画面のバックエンドの所で使う。設定のバックエンドは希望で、
// launcher は GPU で落ちたときや AVX2 の無い CPU のときに CPU の版へ下げるので、実際に動いている一式はこのファイルで見る
import { invoke } from "@tauri-apps/api/core";

export type AiBackendStatus = {
    llama: string;
    setting: string;
    reasons?: string[];
    ai_off?: string;
    updated?: number;
    /** 変換サーバーが AI のモデルを読めなかったとき launcher が足す (load_failed)。読み直して読めたら消える */
    model_error?: string;
};

const LLAMA_NAMES: Record<string, string> = {
    llama_vulkan: "GPU（Vulkan）",
    llama_cuda: "GPU（CUDA）",
    llama_cpu_avx2: "CPU（AVX2）",
    llama_cpu: "CPU（AVX 無し）",
};

const REASON_TEXTS: Record<string, string> = {
    gpu_failed_before: "前に GPU で起動できなかったので、CPU で動いています",
    no_avx2: "この CPU は AVX2 に対応していないので、AVX 無しの版で動いています",
    avx2_failed_before: "前に AVX2 の版で起動できなかったので、AVX 無しの版で動いています",
    gpu_missing: "GPU の版が見つからないので、CPU で動いています",
    avx2_missing: "AVX2 の版が見つからないので、AVX 無しの版で動いています",
    quick_exits: "続けて落ちたので、今回だけ CPU で動いています",
};

export const AI_BACKEND_UNKNOWN = "まだ分かりません（変換サーバーが一度起動すると出ます）";

/** 実際に動いている一式 (GPU（Vulkan）・CPU（AVX2）・CPU（AVX 無し）・AI は切っています)。aiEnabled: 設定の AI の入り切り (分からなければ null) */
export function describeBackend(status: AiBackendStatus | null, aiEnabled: boolean | null): string {
    if (!status) return AI_BACKEND_UNKNOWN;
    const name = LLAMA_NAMES[status.llama] ?? status.llama;
    const off = status.ai_off === "slow_cpu" || aiEnabled === false ? "・AI は切っています" : "";
    return name + off;
}

/** 設定から下げたわけ (起きた順。知らない値はそのまま) と、AI のモデルを読めなかったこと */
export function describeReasons(status: AiBackendStatus | null): string[] {
    const reasons = (status?.reasons ?? []).map((reason) => REASON_TEXTS[reason] ?? reason);
    if (status?.model_error === "load_failed") reasons.push("AI のモデルを読み込めませんでした（辞書だけで変換しています）");
    return reasons;
}

/** いつの起動の様子か (例: 11月19日 3:05 の起動のとき)。分からなければ空 */
export function describeUpdated(status: AiBackendStatus | null): string {
    if (!status?.updated) return "";
    const at = new Date(status.updated * 1000);
    return `${at.getMonth() + 1}月${at.getDate()}日 ${at.getHours()}:${String(at.getMinutes()).padStart(2, "0")} の起動のとき`;
}

/** ai_backend.json を読む (無い・読めないなら null) */
export function fetchAiBackendStatus(): Promise<AiBackendStatus | null> {
    return invoke<AiBackendStatus | null>("ai_backend_status").catch(() => null);
}
