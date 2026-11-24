import { Cpu, FileText, Info } from "lucide-react";
import { Button } from "@/components/ui/button";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { useEffect, useState } from "react";
import { AiBackendStatus, describeBackend, describeReasons, describeUpdated, fetchAiBackendStatus } from "@/lib/aiBackend";

export const About = () => {
    // 2026-11-19: 版と、実際に動いている AI の一式 (不具合の報告に書いてもらうため。launcher が書く ai_backend.json)
    const [version, setVersion] = useState("");
    const [status, setStatus] = useState<AiBackendStatus | null>(null);
    const [aiEnabled, setAiEnabled] = useState<boolean | null>(null);
    useEffect(() => {
        invoke<string>("app_version").then(setVersion).catch(() => {});
        fetchAiBackendStatus().then(setStatus);
        invoke<any>("get_config").then((data) => setAiEnabled(data?.zenzai?.enable ?? null)).catch(() => {});
    }, []);
    const openNotices = () => {
        invoke("open_notices").catch((error) => toast(String(error)));
    };
    const reasons = describeReasons(status);
    const updated = describeUpdated(status);
    return (
        <div className="space-y-8">
            <section className="space-y-2">
                <h1 className="text-sm font-bold text-foreground">このソフトについて</h1>
                <div className="flex items-center space-x-4 rounded-md border p-4">
                    <Info />
                    <div className="flex-1 space-y-1">
                        <p className="text-sm font-medium leading-none">HaniWord{version ? ` ${version}` : ""}</p>
                        <p className="text-xs text-muted-foreground">
                            azooKey-Windows (MIT License) をもとにした日本語入力です。不具合や質問は HaniWord の GitHub へ
                        </p>
                    </div>
                </div>
                <div className="flex items-center space-x-4 rounded-md border p-4">
                    <Cpu />
                    <div className="flex-1 space-y-1">
                        <p className="text-sm font-medium leading-none">AI の変換が動いている所</p>
                        <p className="text-xs text-muted-foreground">
                            {describeBackend(status, aiEnabled)}{updated ? `（${updated}）` : ""}
                        </p>
                        {reasons.map((reason) => (
                            <p key={reason} className="text-xs text-muted-foreground">{reason}</p>
                        ))}
                    </div>
                </div>
                <div className="flex items-center space-x-4 rounded-md border p-4">
                    <FileText />
                    <div className="flex-1 space-y-1">
                        <p className="text-sm font-medium leading-none">ライセンス</p>
                        <p className="text-xs text-muted-foreground">
                            このソフトに含まれるソフトウェア・データのライセンスと出典
                        </p>
                    </div>
                    <Button variant="secondary" onClick={openNotices}>
                        表示する
                    </Button>
                </div>
            </section>
        </div>
    )
}
