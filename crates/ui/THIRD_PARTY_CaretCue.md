# CaretCue 由来の部分（2026-10-13）

入力モードの表示（`src/indicator.html`）と、テキストカーソルの位置探し（`src/caret.rs`）は、CaretCue を元にしている。

2026-11-06: 入力モードの表示は設定「入力モードの切り替えの表示」で 3 つから選ぶ形になった。CaretCue を元にした札は「カーソルのそばに札」（初期値）。
「カーソルのそばに小さく」は Mozc の表示を移したもの（`THIRD_PARTY_Mozc_indicator.md`）、「出さない」は何も出さない（Caps Lock の表示も）。

- 元: https://github.com/GoldAnd07/CaretCue （コミット `9f7ae1a381e52152e64bf3d13a71a110488587b1`、2026-09-14）
- ライセンス: Apache License 2.0（下に全文）。元のリポジトリに NOTICE ファイルはない
- 元にしたファイル: `src/CaretCue/IndicatorWindow.xaml`・`IndicatorWindow.xaml.cs`・`Models.cs`（AnimationSettings・IndicatorTheme・WindowsAccentColorProvider）・`CaretLocator.cs`

## 変えたところ

- WPF（XAML・C#）を、WebView2 の HTML/CSS/JavaScript（`indicator.html`）と Rust（`indicator.rs`・`caret.rs`・`main.rs`）に書き直した。大きさ・色・影・動きの長さと形・カーソルとの間・Caps Lock の矢印の形は元の値のまま
- 並べるものを、読み込まれている入力言語から「あ」「A」（日本語入力のかな・英数）に変えた
- Caps Lock の表示を、0.47 秒で消える代わりにオンの間ずっと出すようにした（macOS と同じ）。モードを切り替えたときは一時的にモードの表示に替わり、止まったあと Caps Lock の表示に戻る
- 入力モードの表示の位置は、IME（TSF）から届くカーソルの矩形を使う。CaretLocator の移植は Caps Lock の表示が付いていくためだけに使い、見つからないときはマウス位置ではなく IME から最後に届いた位置を使う
- 候補ウィンドウが出ている間は表示を隠す
- トレイ・設定画面・インストーラーは移していない

## Apache License 2.0（CaretCue の LICENSE そのまま）

```
Apache License
Version 2.0, January 2004
http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

"License" shall mean the terms and conditions for use, reproduction, and distribution as defined by Sections 1 through 9 of this document.

"Licensor" shall mean the copyright owner or entity authorized by the copyright owner that is granting the License.

"Legal Entity" shall mean the union of the acting entity and all other entities that control, are controlled by, or are under common control with that entity. For the purposes of this definition, "control" means (i) the power, direct or indirect, to cause the direction or management of such entity, whether by contract or otherwise, or (ii) ownership of fifty percent (50%) or more of the outstanding shares, or (iii) beneficial ownership of such entity.

"You" (or "Your") shall mean an individual or Legal Entity exercising permissions granted by this License.

"Source" form shall mean the preferred form for making modifications, including but not limited to software source code, documentation source, and configuration files.

"Object" form shall mean any form resulting from mechanical transformation or translation of a Source form, including but not limited to compiled object code, generated documentation, and conversions to other media types.

"Work" shall mean the work of authorship, whether in Source or Object form, made available under the License, as indicated by a copyright notice that is included in or attached to the work.

"Derivative Works" shall mean any work, whether in Source or Object form, that is based on the Work and for which the editorial revisions, annotations, elaborations, or other modifications represent, as a whole, an original work of authorship. Derivative Works shall not include works that remain separable from, or merely link to the interfaces of, the Work.

"Contribution" shall mean any work of authorship, including the original version of the Work and any modifications or additions intentionally submitted to Licensor for inclusion in the Work by the copyright owner or by an authorized individual or Legal Entity. "Submitted" means any form of electronic, verbal, or written communication sent to the Licensor or its representatives for discussing and improving the Work, excluding communication conspicuously marked as "Not a Contribution."

"Contributor" shall mean Licensor and any individual or Legal Entity on behalf of whom a Contribution has been received and incorporated within the Work.

2. Grant of Copyright License. Subject to the terms and conditions of this License, each Contributor grants to You a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable copyright license to reproduce, prepare Derivative Works of, publicly display, publicly perform, sublicense, and distribute the Work and such Derivative Works in Source or Object form.

3. Grant of Patent License. Subject to the terms and conditions of this License, each Contributor grants to You a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable patent license to make, have made, use, offer to sell, sell, import, and otherwise transfer the Work, where such license applies only to patent claims licensable by such Contributor that are necessarily infringed by their Contribution alone or by combination of their Contribution with the Work. If You institute patent litigation alleging that the Work or a Contribution constitutes patent infringement, then any patent licenses granted to You under this License shall terminate as of the date such litigation is filed.

4. Redistribution. You may reproduce and distribute copies of the Work or Derivative Works in any medium, with or without modifications, and in Source or Object form, provided that You meet the following conditions:

(a) You must give any other recipients of the Work or Derivative Works a copy of this License; and

(b) You must cause any modified files to carry prominent notices stating that You changed the files; and

(c) You must retain, in the Source form of any Derivative Works that You distribute, all copyright, patent, trademark, and attribution notices from the Source form of the Work, excluding notices that do not pertain to any part of the Derivative Works; and

(d) If the Work includes a NOTICE file, any Derivative Works that You distribute must include a readable copy of the attribution notices contained within the NOTICE file, excluding notices that do not pertain to any part of the Derivative Works, in at least one of the following places: within a NOTICE file distributed as part of the Derivative Works; within the Source form or documentation; or within a display generated by the Derivative Works, wherever such notices normally appear. The contents of the NOTICE file are for informational purposes only and do not modify the License. You may add Your own attribution notices, provided that they cannot be construed as modifying the License.

You may add Your own copyright statement to Your modifications and may provide additional or different license terms for Your modifications or Derivative Works, provided Your use, reproduction, and distribution of the Work otherwise complies with this License.

5. Submission of Contributions. Unless You explicitly state otherwise, any Contribution intentionally submitted for inclusion in the Work shall be under the terms of this License, without additional terms or conditions.

6. Trademarks. This License does not grant permission to use the trade names, trademarks, service marks, or product names of the Licensor, except as required for reasonable and customary use in describing the origin of the Work and reproducing the NOTICE file.

7. Disclaimer of Warranty. Unless required by applicable law or agreed to in writing, Licensor provides the Work on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied, including warranties or conditions of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A PARTICULAR PURPOSE. You are solely responsible for determining the appropriateness of using or redistributing the Work and assume any risks associated with Your exercise of permissions under this License.

8. Limitation of Liability. In no event and under no legal theory, whether in tort, contract, or otherwise, unless required by applicable law or agreed to in writing, shall any Contributor be liable to You for damages, including any direct, indirect, special, incidental, or consequential damages arising as a result of this License or out of the use or inability to use the Work, even if advised of the possibility of such damages.

9. Accepting Warranty or Additional Liability. While redistributing the Work or Derivative Works, You may choose to offer support, warranty, indemnity, or other liability obligations and charge a fee, but only on Your own behalf and responsibility. You agree to indemnify, defend, and hold each Contributor harmless for any liability incurred by, or claims asserted against, such Contributor by reason of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS

Copyright 2026 Andy Scarlett

Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with the License. You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the License for the specific language governing permissions and limitations under the License.
```
