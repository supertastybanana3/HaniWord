# Mozc の入力モードの表示から移した部分（2026-11-06）

設定「入力モードの切り替えの表示」の「カーソルのそばに小さく」（`input.mode_indicator` が `"compact"`）は、Mozc の Windows 版の入力モードの表示（indicator）を移したもの。
`src/indicator.html` の `#compact`（絵と消える動き）と、`src/indicator.rs` の `compact_origin`・`message_font`・`COMPACT_WIDTH`・`COMPACT_HEIGHT`（出る位置・字のフォント・窓の大きさ）にあたる。

- 元: https://github.com/google/mozc （コミット `c4ed4683fb400b240e81d0a3895d024e3e21325e`、2026-11-03）
- ライセンス: BSD 3-Clause License（下に元のファイルの表示そのまま）
- 元にしたファイル: `src/renderer/win32/indicator_window.cc`（色・大きさ・字・出る位置・消える動き）、`src/renderer/win32/win32_image_util.cc`（`BalloonImage`: 吹き出しの形・ぼかし・字の大きさ）、`src/renderer/win32/win32_renderer_util.cc`（`LayoutIndicatorWindowByCompositionTarget`: 位置の基準）

## 元の値のまま移したもの

- 吹き出し: 45×45（DPI に合わせて広がる）、角は丸めない、枠 1、上の真ん中に幅 10・高さ 5 の尖り、中は白
- あ（ひらがな）: 枠の色 RGB(1, 122, 204)、まわりのぼかしは同じ色で σ 3・不透明度 0.5
- A（英数。元の表示の IME オフ「A」）: 枠の色 RGB(186, 186, 186)、ぼかしなし
- 字: 黒、メッセージ ボックスのフォント（`SystemParametersInfo` の `lfMessageFont`）の 13 ポイント、吹き出しの枠の内側の真ん中
- 出る位置: 尖りの先を、カーソルの位置の字の左端と行の下端に合わせる。画面の端に近くても動かさない
- 消える動き: 出てから 2.5 秒そのまま、そのあと 16 ミリ秒ごとに不透明度を 32/255 ずつ下げ、下げたぶんの 1/32（物理ピクセル）だけ下へずらす

## 変えたところ

- Win32 の層つきウィンドウ（`UpdateLayeredWindow`）と GDI で描いた絵を、WebView2 の HTML/CSS（SVG の吹き出しと CSS の `drop-shadow`）と JavaScript のタイマーに書き直した。字は元のように 1 ビットの字形を細かく分けて数えず、ブラウザの描き方（なめらか）で描く
- 出すモードは あ と A の 2 つだけ（全角カタカナ・全角英数・半角カタカナは HaniWord に無い）
- 位置の基準は、IME（TSF）から届くカーソルの矩形にした。幅のある矩形（変換中の文字列）のときは右端（打っている位置）に合わせる
- Caps Lock がオンの間は、同じ吹き出しに Caps Lock の矢印（CaretCue の形、`THIRD_PARTY_CaretCue.md`）を出したままにする。元の表示には無い。モードを切り替えたら、モードの字が消えたあとに矢印に戻る
- 候補ウィンドウが出ている間は隠す（札と同じ）
- 縦書きの位置合わせ・アクティブ ウィンドウの追従の回避（`SPI_GETACTIVEWINDOWTRACKING`）は移していない

## BSD 3-Clause License（元のファイルの先頭の表示そのまま）

```
Copyright 2010-2021, Google Inc.
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

    * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
    * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
    * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```
