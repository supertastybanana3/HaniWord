# crate にライセンスのファイルが入っていない Rust のライブラリのライセンス文（2026-11-06）

`scripts/release/package.py` は、crate にライセンスのファイル（LICENSE・COPYING など）が無いとき、ここの `<名前>-<版>` のフォルダのファイルを配布物の `licenses/rust/<名前>-<版>/` に写す。
文は、crate の `.cargo_vcs_info.json` にある公開したときのコミットの配布元から、そのまま取った（selectors はリポジトリにライセンスのファイルが無いので、MPL 2.0 の全文を https://www.mozilla.org/media/MPL/2.0/index.txt から取った）。

| crate | 版 | 配布元 | コミット | ファイル |
|---|---|---|---|---|
| alloc-stdlib | 0.2.2 | https://github.com/dropbox/rust-alloc-no-stdlib | 6032b6a9b20e03737135c55a0270ccffcc1438ef | LICENSE |
| selectors | 0.36.1 | https://github.com/servo/stylo | 635e1a19d02960588a00e189bd4bd5bdb150ec3d | LICENSE-MPL-2.0.txt（mozilla.org の MPL 2.0） |
| unic-char-property | 0.9.0 | https://github.com/open-i18n/rust-unic | 5878605364af97a3358368a6eaef02104af2e016 | COPYRIGHT.md・LICENSE-APACHE・LICENSE-MIT |
| unic-char-range | 0.9.0 | 同上 | 5878605364af97a3358368a6eaef02104af2e016 | 同上 |
| unic-common | 0.9.0 | 同上 | 5878605364af97a3358368a6eaef02104af2e016 | 同上 |
| unic-ucd-ident | 0.9.0 | 同上 | 8a6ce83063d90b91ae2ce59eddb803edd393fca9 | 同上 |
| unic-ucd-version | 0.9.0 | 同上 | 5878605364af97a3358368a6eaef02104af2e016 | 同上 |
| webview2-com | 0.38.2 | https://github.com/wravery/webview2-rs | b74dc5e2b394044bea5191052868ce7a106c202c | LICENSE |
| webview2-com-macros | 0.8.1 | 同上 | dffa41a8a46d3f5565eefbff2de57d38d399f158 | LICENSE |
| webview2-com-sys | 0.38.2 | 同上 | b74dc5e2b394044bea5191052868ce7a106c202c | LICENSE |

依存の版が変わったら、package.py の「ライセンス文のファイルが同梱されていなかったもの」に出た crate について、同じやり方でフォルダを足す。
