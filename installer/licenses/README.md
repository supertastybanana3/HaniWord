# installer\licenses の出どころ（2026-11-06 に書いた。package.py が配布物の licenses と THIRD_PARTY_NOTICES.txt に入れる）

- CMUdict_LICENSE.txt・Mozc_LICENSE.txt: 前からあるもの（english_words.tsv の作成・候補窓の配色の参考）
- WebView2_LICENSE.txt・WebView2_NOTICE.txt: NuGet の Microsoft.Web.WebView2 1.0.3650.58（https://www.nuget.org/packages/Microsoft.Web.WebView2/1.0.3650.58）の LICENSE.txt・NOTICE.txt そのまま。
  webview2-com-sys 0.38.2 の x64\WebView2LoaderStatic.lib は、このパッケージの build\native\x64\WebView2LoaderStatic.lib とバイト単位で同じ（2026-11-06 に cmp で確かめた）
- Vulkan-Headers_LICENSE.md・Vulkan-Headers_MIT.txt: https://github.com/KhronosGroup/Vulkan-Headers のタグ vulkan-sdk-1.4.363.0 の LICENSE.md・LICENSES/MIT.txt そのまま。
  llama.cpp を組み立てた Vulkan SDK 1.4.363.0 の Include\vulkan\vulkan.hpp は、このタグのものと改行のほかは同じ
- YaRN_LICENSE.txt: https://github.com/jquesnelle/yarn のコミット a18e54988639030e706f6ff9db224cf74e08a65a の LICENSE そのまま
- ggllm.cpp_LICENSE.txt: https://github.com/cmp-nct/ggllm.cpp のコミット 3e4c0214c749df55c98c4cc161c7df862b4bb66c の LICENSE そのまま
- Swift_LICENSE.txt（2026-11-23）: https://github.com/swiftlang/swift のタグ swift-6.4.0-RELEASE（コミット b8189d766d86ad7fc8106787d6ce9e402f38dd72）の LICENSE.txt そのまま。
  配る Swift の実行環境の DLL の許諾（Apache License 2.0 と Runtime Library Exception）。同じタグの swift-corelibs-foundation・swift-foundation・
  swift-corelibs-libdispatch・swift-foundation-icu・swift-experimental-string-processing の LICENSE も、見出しの # の数と末尾の改行のほかは同じ文（2026-11-23 に GitHub の API で取って比べた）
- rust\: crate にライセンスのファイルが無いもの（rust\README.md）
- javascript\: 設定アプリの JavaScript の依存のうち、パッケージにライセンスのファイルが入っていないもの（package.py は `<名前>`（/ は __）か、`@` で始まる組の名前のフォルダを使う）
  - @radix-ui\LICENSE: https://github.com/radix-ui/primitives のコミット ee4e3197bffe7dde669d6bbf8c30fef97b65e49d（2022-06-01。LICENSE を最後に変えたコミット）の LICENSE そのまま。
    設定アプリが使う @radix-ui の 31 のパッケージは、どれも package.json の repository がこのリポジトリで、license は MIT
  - react-remove-scroll-bar\LICENSE: https://github.com/theKashey/react-remove-scroll-bar のコミット 7301c160fda44cb8cf2b9fdfde61efad35736196（2025-05-21、LICENSE を足したコミット）の LICENSE そのまま。
    使っている 2.3.8（2024-12-15 公開）はこの前の版で、package.json の license は MIT・作者は同じ Anton Korzunov
  - @esbuild\LICENSE.md・@rollup\LICENSE.md（2026-11-06）: 設定アプリの組み立てだけに使う道具の、OS ごとの本体のパッケージ（@esbuild/win32-x64 0.25.12・
    @rollup/rollup-win32-x64-gnu と -msvc 4.64.5）にはライセンスのファイルが無い。本番の依存の @tailwindcss/vite が vite を求めるので、npm の本番の依存の
    一覧に入る（配る画面には入らない）。同じ版の esbuild 0.25.12・rollup 4.64.5 のパッケージの LICENSE.md そのまま（配布元はどちらも同じリポジトリ、MIT）
- caution_lists\（2026-11-07、仕事 40 の注意の注記）: 注意の注記の語を選んだ NG 語の一覧の許諾の文。配布物では licenses\caution_list_<ファイルの名前> に平らに置く。Nextremer のもの (下) のほかは、配布元の、使った版のものをそのまま。
  - MosasoM_inappropriate-words-ja_LICENSE.txt: https://github.com/MosasoM/inappropriate-words-ja のコミット b93b76d61cbc80542da4e16c4a5925256344fdee の LICENSE（MIT）
  - ZachHandley_ContentShield_LICENSE.txt: https://github.com/ZachHandley/ContentShield のコミット a0e7c6eb66ef5c28ca84701d71e8d41c2c2cd43e の LICENSE（MIT）
  - yomiplush_outrage-mute_LICENSE.txt: https://github.com/yomiplush/outrage-mute のコミット 8dc2583d04e2e7f79fdfae8c82872548189b4638 の LICENSE（MIT）
  - YukimaruGit_student-id-generator_LICENSE.txt: https://github.com/YukimaruGit/student-id-generator のコミット 8e1020fca8e53bd7f638ec603d2c844018991aba の LICENSE.txt（MIT）
  - HojiChar_HojiChar_LICENSE.txt: https://github.com/HojiChar/HojiChar のコミット 2df85ad6b1ba86752a36e0513e4d334caddbc355 の LICENSE（Apache License 2.0。この版に NOTICE は無い）。
    使った discrimination_keywords_ja.txt は、前のコミット 7c6779393bb492a0738107d1bd62dccec17e99b1（LICENSE が無い）と中身が同じ（2026-11-07 に cmp で確かめた）
  - Nextremer_minarai-cs-chat-webui_LICENSE.txt: https://github.com/Nextremer/minarai-cs-chat-webui には LICENSE のファイルが無い。コミット 79db06f0946977266d316adfebebb56d3f18ba32 の
    README.md の記載（「本サンプルのライセンスはコード含めて全て *MIT License* です。」）と MIT License の本文を書いた。使った blackWords.js は前のコミット b91e1d4 と中身が同じ
