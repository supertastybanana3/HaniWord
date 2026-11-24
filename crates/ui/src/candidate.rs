use anyhow::{Context as _, Result};
use tao::{
    event_loop::EventLoop,
    platform::windows::{WindowBuilderExtWindows, WindowExtWindows},
    window::{Window, WindowBuilder},
};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        SetWindowLongW, GWL_EXSTYLE, GWL_STYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_POPUP,
    },
};
use wry::WebViewBuilder;

use crate::UserEvent;

pub fn create_candidate_window(event_loop: &EventLoop<UserEvent>) -> Result<Window> {
    let window = WindowBuilder::new()
        .with_decorations(false)
        .with_title("CandidateList")
        .with_focused(false)
        .with_visible(false)
        .with_undecorated_shadow(false)
        .with_transparent(true)
        .build(&event_loop)
        .context("Failed to create window")?;

    let hwnd = window.hwnd() as *mut std::ffi::c_void;

    // set extended window style
    // https://docs.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles
    // https://docs.microsoft.com/en-us/windows/win32/winmsg/window-styles
    unsafe {
        let exnewstyle = WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOPMOST.0;
        SetWindowLongW(HWND(hwnd), GWL_EXSTYLE, exnewstyle as i32);

        let style = WS_POPUP.0;
        SetWindowLongW(HWND(hwnd), GWL_STYLE, style as i32);
    };

    Ok(window)
}

/// 2026-11-06 (仕事 37): 足りない字を □ で出すフォント (Adobe NotDef、SIL Open Font License 1.1。crates/ui/THIRD_PARTY_AdobeNotDef.md)
const NOTDEF_FONT: &[u8] = include_bytes!("../res/AND-Regular.otf");

/// 表示側に渡す外観の値 (shared::AppearanceConfig を sanitized したもの) と、Windows のアクセントの色
pub fn appearance_json(appearance: &shared::AppearanceConfig, accent: &str) -> String {
    let a = appearance.sanitized();
    serde_json::json!({
        "font": a.font, "latinFont": a.latin_font, "fontSize": a.font_size, "theme": a.theme,
        "selectedColor": a.selected_color, "layout": a.layout, "fallback": a.fallback, "accent": accent,
    }).to_string()
}

pub fn create_candidate_webview<'a>(appearance: &str) -> Result<WebViewBuilder<'a>> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let html = CANDIDATE_HTML
        .replace("{{NOTDEF_FONT}}", &STANDARD.encode(NOTDEF_FONT))
        .replace("{{APPEARANCE}}", appearance);
    let webview_builder = WebViewBuilder::new()
        .with_transparent(true)
        .with_navigation_handler(crate::navigation::embedded_html_navigation(&html))
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_html(html);

    Ok(webview_builder)
}

/// 外観を当てる命令 (設定が変わったとき)。値は JSON として渡す (式として組み立てない)
pub fn appearance_script(json: &str) -> String {
    format!("applyAppearance({json})")
}

const CANDIDATE_HTML: &str = r##"
        <html lang="ja">
            <head>
                <style>
                    /* Google 日本語入力 (Mozc) の候補ウィンドウに合わせる。
                       値は mozc/src/renderer/renderer_style.textproto より。
                       2026-11-06 (仕事 37): 色・フォント・大きさ・並べ方を設定 (外観) で替えられるように、値を変数にした。初期値は今のまま */
                    @font-face {
                        font-family: "HW NotDef";
                        font-display: swap;
                        src: url(data:font/otf;base64,{{NOTDEF_FONT}}) format("opentype");
                    }
                    /* 足りない字を補わないときも、絵文字は Segoe UI Emoji で出す */
                    @font-face {
                        font-family: "HW Emoji";
                        font-display: swap;
                        src: local("Segoe UI Emoji");
                        unicode-range: U+00A9, U+00AE, U+203C, U+2049, U+2122, U+2139, U+2194-2199, U+21A9-21AA, U+231A-231B, U+2328, U+23CF,
                            U+23E9-23F3, U+23F8-23FA, U+24C2, U+25AA-25AB, U+25B6, U+25C0, U+25FB-25FE, U+2600-27BF, U+2934-2935,
                            U+2B05-2B07, U+2B1B-2B1C, U+2B50, U+2B55, U+3030, U+303D, U+3297, U+3299, U+FE0F, U+200D, U+20E3, U+E0020-E007F,
                            U+1F000-1FAFF;
                    }
                    :root {
                        --font: "Yu Gothic UI", "Meiryo UI", sans-serif;
                        --size: 14px;
                        --desc-size: 12px;
                        --line: 1.35;
                        --row-pad: 1px;
                        --bg: #FFFFFF;
                        --border: rgb(150, 150, 150);
                        --text: #000000;
                        --shortcut-bg: rgb(243, 244, 255);
                        --shortcut-text: rgb(97, 97, 97);
                        --desc: rgb(136, 136, 136);
                        --sel-bg: rgb(209, 234, 255);
                        --sel-border: rgb(127, 172, 221);
                        --scroll-bg: rgb(224, 224, 224);
                        --scroll-thumb: rgb(117, 144, 184);
                        --footer-border: rgb(96, 96, 96);
                        --footer-bg: linear-gradient(rgb(255, 255, 255), rgb(238, 238, 238));
                        --footer-text: rgb(76, 76, 76);
                    }
                    /* 暗い色の組 (theme が dark か、system で Windows のアプリのモードが暗いとき) */
                    body[data-theme="dark"] {
                        --bg: #2B2B2B; --border: rgb(110, 110, 110); --text: #F2F2F2; --shortcut-bg: rgb(52, 54, 64); --shortcut-text: rgb(190, 190, 190);
                        --desc: rgb(160, 160, 160); --sel-bg: rgb(38, 79, 120); --sel-border: rgb(86, 140, 200); --scroll-bg: rgb(70, 70, 70);
                        --scroll-thumb: rgb(130, 160, 200); --footer-border: rgb(110, 110, 110); --footer-bg: linear-gradient(rgb(52, 52, 52), rgb(43, 43, 43));
                        --footer-text: rgb(200, 200, 200);
                    }
                    @media (prefers-color-scheme: dark) {
                        body[data-theme="system"] {
                            --bg: #2B2B2B; --border: rgb(110, 110, 110); --text: #F2F2F2; --shortcut-bg: rgb(52, 54, 64); --shortcut-text: rgb(190, 190, 190);
                            --desc: rgb(160, 160, 160); --sel-bg: rgb(38, 79, 120); --sel-border: rgb(86, 140, 200); --scroll-bg: rgb(70, 70, 70);
                            --scroll-thumb: rgb(130, 160, 200); --footer-border: rgb(110, 110, 110); --footer-bg: linear-gradient(rgb(52, 52, 52), rgb(43, 43, 43));
                            --footer-text: rgb(200, 200, 200);
                        }
                    }
                    /* 並べ方: flat は番号の列に色を付けず、下の欄も平らに。compact は行の間を詰める */
                    body[data-layout="flat"] { --shortcut-bg: transparent; --footer-bg: var(--bg); }
                    body[data-layout="compact"] { --row-pad: 0px; --line: 1.2; }
                    html, body {
                        margin: 0;
                        padding: 0;
                        overflow: hidden;
                        background: transparent;
                    }
                    body {
                        font-family: var(--font);
                        font-size: var(--size);
                        line-height: var(--line);
                        user-select: none;
                        cursor: default;
                    }
                    main {
                        display: inline-block;
                        border: 1px solid var(--border);
                        background: var(--bg);
                    }
                    #rows {
                        display: flex;
                    }
                    table {
                        border-collapse: separate;
                        border-spacing: 0;
                    }
                    td {
                        padding: var(--row-pad) 0;
                        white-space: nowrap;
                        border-top: 1px solid transparent;
                        border-bottom: 1px solid transparent;
                    }
                    td.shortcut {
                        background: var(--shortcut-bg);
                        color: var(--shortcut-text);
                        padding-left: 8px;
                        padding-right: 8px;
                        text-align: center;
                        border-left: 1px solid transparent;
                    }
                    td.value {
                        color: var(--text);
                        padding-left: 4px;
                        padding-right: 8px;
                        border-right: 1px solid transparent;
                    }
                    td.desc {
                        color: var(--desc);
                        font-size: var(--desc-size);
                        padding-right: 8px;
                        text-align: right;
                        border-right: 1px solid transparent;
                    }
                    tr.focused td.desc {
                        border-right-color: var(--sel-border);
                    }
                    td.value span.min {
                        display: block;
                        height: 0;
                        overflow: hidden;
                        visibility: hidden;
                    }
                    tr.focused td {
                        background: var(--sel-bg);
                        border-top-color: var(--sel-border);
                        border-bottom-color: var(--sel-border);
                    }
                    tr.focused td.shortcut {
                        border-left-color: var(--sel-border);
                    }
                    #scrollbar {
                        width: 4px;
                        background: var(--scroll-bg);
                        position: relative;
                    }
                    #scrollbar.hidden {
                        display: none;
                    }
                    #indicator {
                        position: absolute;
                        left: 0;
                        width: 4px;
                        background: var(--scroll-thumb);
                    }
                    footer {
                        border-top: 1px solid var(--footer-border);
                        background: var(--footer-bg);
                        color: var(--footer-text);
                        text-align: right;
                        padding: 0 4px;
                        font-size: var(--size);
                    }
                    body[data-layout="flat"] footer { border-top-color: var(--border); }
                </style>
                <script>
                    const PAGE_SIZE = 9;
                    let candidates = [];
                    let selection = 0;

                    // 2026-11-06 (仕事 37): 外観 (設定アプリの「外観」、Rust が sanitized した値を JSON で渡す)。初期値は今のまま
                    const FALLBACK_STOP = '@notdef';
                    function mix(hex, base, t) {
                        const c = [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16));
                        const b = [1, 3, 5].map(i => parseInt(base.slice(i, i + 2), 16));
                        return 'rgb(' + c.map((v, k) => Math.round(v * t + b[k] * (1 - t))).join(', ') + ')';
                    }
                    function applyAppearance(a) {
                        if (!a || typeof a !== 'object') return;
                        const root = document.documentElement.style, body = document.body;
                        const hexOk = (x) => typeof x === 'string' && /^#[0-9a-fA-F]{6}$/.test(x);
                        const name = (x) => (typeof x === 'string' && x !== '' ? JSON.stringify(x) : null);
                        const fonts = [];
                        if (name(a.latinFont)) fonts.push(name(a.latinFont));
                        fonts.push(name(a.font) || '"Yu Gothic UI"');
                        // 補うフォントの並び: 書いていなければ今のまま (候補のフォントが今のままなら Meiryo UI、そのあと Windows が補う)。
                        // 止めの印があればそこで止め、無い字を □ で出す (絵文字だけは Segoe UI Emoji)。止めなければ並びのあとに Windows が補う
                        let stop = false;
                        if (Array.isArray(a.fallback)) {
                            for (const f of a.fallback) {
                                if (f === FALLBACK_STOP) { stop = true; break; }
                                if (name(f)) fonts.push(name(f));
                            }
                        } else if (!name(a.font)) {
                            fonts.push('"Meiryo UI"');
                        }
                        if (stop) fonts.push('"HW Emoji"', '"HW NotDef"'); else fonts.push('sans-serif');
                        root.setProperty('--font', fonts.join(', '));
                        const size = Number.isInteger(a.fontSize) ? Math.min(28, Math.max(10, a.fontSize)) : 14;
                        root.setProperty('--size', size + 'px');
                        root.setProperty('--desc-size', Math.max(9, size - 2) + 'px');
                        body.dataset.theme = ['light', 'dark', 'system'].includes(a.theme) ? a.theme : 'light';
                        body.dataset.layout = ['standard', 'flat', 'compact'].includes(a.layout) ? a.layout : 'standard';
                        // 選んだ候補の色: 枠はその色、地は今の地の色と混ぜて薄くする (字が読めるように)
                        const color = a.selectedColor === 'accent' ? a.accent : a.selectedColor;
                        if (hexOk(color)) {
                            const dark = body.dataset.theme === 'dark' || (body.dataset.theme === 'system' && matchMedia('(prefers-color-scheme: dark)').matches);
                            root.setProperty('--sel-border', color);
                            root.setProperty('--sel-bg', mix(color, dark ? '#2B2B2B' : '#FFFFFF', dark ? 0.45 : 0.25));
                        } else {
                            root.removeProperty('--sel-border');
                            root.removeProperty('--sel-bg');
                        }
                        if (candidates.length) render();
                    }

                    // 2026-11-08 (仕事 52): 下の欄に添える文言 (重いので AI を休んでいる間)
                    let footerNote = '';

                    function updateCandidates(list, note) {
                        candidates = list;
                        footerNote = note || '';
                        if (selection >= candidates.length) {
                            selection = 0;
                        }
                        render();
                    }

                    function updateSelection(index) {
                        selection = index;
                        render();
                    }

                    function render() {
                        const tbody = document.getElementById('candidate-list');
                        tbody.innerHTML = '';
                        const total = candidates.length;
                        const page = Math.floor(Math.max(selection, 0) / PAGE_SIZE);
                        const begin = page * PAGE_SIZE;
                        const end = Math.min(begin + PAGE_SIZE, total);

                        for (let i = begin; i < end; i++) {
                            const tr = document.createElement('tr');
                            if (i === selection) {
                                tr.className = 'focused';
                            }
                            const shortcut = document.createElement('td');
                            shortcut.className = 'shortcut';
                            shortcut.textContent = String(i - begin + 1);
                            // 「候補\u001f注記」の形で届く (注記は [全] [半] カタカナ 環境依存 など)。
                            // 2026-11-08: 注記のあとの「\u001e案内」(学習した候補の「Ctrl+Delete で忘れる」) は、選んでいる行だけに出す
                            const [text, rest] = String(candidates[i]).split('\u001f');
                            const [note, hint] = String(rest ?? '').split('\u001e');
                            const value = document.createElement('td');
                            value.className = 'value';
                            value.textContent = text;
                            const min = document.createElement('span');
                            min.className = 'min';
                            min.textContent = 'そのほかの文字種';
                            value.appendChild(min);
                            const desc = document.createElement('td');
                            desc.className = 'desc';
                            desc.textContent = (i === selection && hint) ? (note ? note + ' ' + hint : hint) : (note || '');
                            tr.appendChild(shortcut);
                            tr.appendChild(value);
                            tr.appendChild(desc);
                            tbody.appendChild(tr);
                        }

                        const scrollbar = document.getElementById('scrollbar');
                        if (total > PAGE_SIZE) {
                            scrollbar.className = '';
                            const indicator = document.getElementById('indicator');
                            const height = tbody.parentElement.offsetHeight;
                            indicator.style.top = (height * begin / total) + 'px';
                            indicator.style.height = Math.max(1, height * (end - begin) / total) + 'px';
                        } else {
                            scrollbar.className = 'hidden';
                        }

                        document.getElementById('footer').textContent =
                            (total > 0 ? `${selection + 1}/${total}` : '') + (footerNote ? `　${footerNote}` : '');

                        const main = document.querySelector('main');
                        window.ipc.postMessage(JSON.stringify({
                            type: 'resize',
                            width: main.offsetWidth,
                            height: main.offsetHeight
                        }));
                    }
                </script>
            </head>
            <body>
                <script>applyAppearance({{APPEARANCE}});</script>
                <main>
                    <div id="rows">
                        <table><tbody id="candidate-list"></tbody></table>
                        <div id="scrollbar" class="hidden"><div id="indicator"></div></div>
                    </div>
                    <footer id="footer"></footer>
                </main>
            </body>
        </html>"##;
