; HaniWord の配布用インストーラー (scripts\release\package.py で ..\release を作り、installer\compile.ps1 でコンパイルする)
; 変換の AI モデル (zenz-v3.2-small) は同梱せず、インストール中に Hugging Face の作者の公開場所から取得し、SHA-256 で確かめる
#define MyAppName "HaniWord"
#ifndef VerifiedSecurityPayload
  #error Compile with installer\compile.ps1, which checks ..\release against its SHA256SUMS.txt first.
#endif
#ifndef MyAppVersion
  #define MyAppVersion "0.1.1"
#endif
#ifndef MyAppBuild
  #error Compile with installer\compile.ps1, which passes the build number (MyAppBuild).
#endif
; 2026-10-29: 版とビルドごとに別のフォルダ。更新で使用中の azookey.dll・azookey32.dll を入れ替えない (再起動を待たない)。
; 新しい DLL を登録し直すので、開いているアプリは古い DLL のまま、新しく開いたアプリから新しい DLL。古いフォルダは setup_tasks.ps1 が片付ける
#define BinDir "{commonpf64}\HaniWord\Releases\" + MyAppVersion + "-" + MyAppBuild
#include "generated_stop_command.iss"
#define ModelUrl "https://huggingface.co/Miwa-Keita/zenz-v3.2-small-gguf/resolve/c67e03e07d215c869f591b274c1631170d3e11fe/ggml-model-Q5_K_M.gguf"
#define ModelSha256 "29c223d4c23327b80fd13ebb5ab2555057a46317997d5da391584ffbef0db673"
; 2026-11-10: モデルの大きさ (バイト)。手元の同じモデルを探すとき、大きさが違うファイルは写さずに飛ばす (ModelSha256 と一緒に替える)
#define ModelSize 73871936
; 2026-10-31: 配る一式の分野辞書の名前 (..\release\files\domain\domains.txt。変換サーバーが読むのと同じ表) を ",law,medicine," の形で持つ。
; 専門分野のページには、ここにある分野だけを出す (分野辞書を一式に入れる・外すだけで、ページも付いてくる)。# を含む行 (説明) は読まない
#define DomainsFile AddBackslash(SourcePath) + "..\release\files\domain\domains.txt"
#if !FileExists(DomainsFile)
  #error ..\release\files\domain\domains.txt is missing
#endif
#define PublishedDomains ","
#define DomainLine ""
#sub ReadDomainLine
  #expr DomainLine = Trim(FileRead(DomainHandle))
  #if DomainLine != "" && Pos("#", DomainLine) == 0
    #expr PublishedDomains = PublishedDomains + DomainLine + ","
  #endif
#endsub
#define DomainHandle FileOpen(DomainsFile)
#for {0; !FileEof(DomainHandle); 0} ReadDomainLine
#expr FileClose(DomainHandle)

[Setup]
AppId={{246849B1-9E94-4C78-A64C-8C61CCD616BD}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher=supertastybanana3
; 2026-10-19 (security review #4): {app} is the protected folder in Program Files. The elevated installer writes nothing into
; the user's profile; install_user_files.ps1 copies the server, dictionaries and model into %APPDATA%\HaniWord as the original user
DefaultDirName={commonpf64}\HaniWord
DisableDirPage=yes
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
OutputDir=.
OutputBaseFilename=haniword-setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\crates\client\res\azookey.ico
UninstallDisplayIcon={#BinDir}\azookey-settings.exe
UninstallDisplayName=HaniWord
CloseApplications=no
; 2026-10-29: HKCU に触るのは新しい版の知らせの差出人の名前を消す所だけ ([Registry])。インストーラーを始めた人と管理者が同じ人のときだけ
; 入れられる (SameUserAsOriginal) ので、管理者の HKCU は利用者の HKCU
UsedUserAreasWarning=no
LicenseFile=..\release\files\licenses\HaniWord_LICENSE.txt

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"

[Files]
; Program Files\HaniWord: 管理者権限で動きうるもの (IME 本体は管理者のアプリにも読み込まれる・起動用と候補窓はログオン時に一瞬管理者で動く・
; 設定画面は言語バーから開く)。ユーザーが書き換えられない場所に置く。変換サーバーと辞書・モデルも、まずここ (user-files) に置き、
; 利用者の権限で動く install_user_files.ps1 が %APPDATA%\HaniWord へ写す
; 2026-10-30: 言語の一覧 (Win+Space・言語バー) のアイコンは、版によらない場所に置く (IME 本体が登録のときにここを指す。DLL より先に置く)。
; 版ごとのフォルダの DLL を指していたときは、更新で前のフォルダを片付けると、一覧が覚えていた前のパスが無くなって既定の字に戻った
Source: "..\release\bin\haniword.ico"; DestDir: "{commonpf64}\HaniWord"; Flags: ignoreversion
Source: "..\release\bin\azookey.dll"; DestDir: "{#BinDir}"; Flags: ignoreversion regserver 64bit restartreplace uninsrestartdelete
Source: "..\release\bin\azookey32.dll"; DestDir: "{#BinDir}"; Flags: ignoreversion regserver 32bit restartreplace uninsrestartdelete
Source: "..\release\bin\*"; Excludes: "azookey.dll,azookey32.dll,haniword.ico"; DestDir: "{#BinDir}"; Flags: ignoreversion restartreplace uninsrestartdelete
Source: "..\release\bin\latin_words\*"; DestDir: "{#BinDir}\latin_words"; Flags: ignoreversion recursesubdirs createallsubdirs
; 2026-11-06: 次の語の候補の表 (next_words、13 言語で約 135MB) は英語だけ必ず入れ、ほかの言語は言語のページで選んだものと、
; 前の版の設定で選んであるものだけ入れる (LanguageChosen。言語と表の対応は IME 本体の next_word.rs の LANGUAGE_FILES と同じ)
Source: "..\release\bin\next_words\next_en.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion
Source: "..\release\bin\next_words\next_fr.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('fr')
Source: "..\release\bin\next_words\next_de.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('de')
Source: "..\release\bin\next_words\next_es.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('es')
Source: "..\release\bin\next_words\next_it.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('it')
Source: "..\release\bin\next_words\next_pt.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('pt')
Source: "..\release\bin\next_words\next_pl.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('pl')
Source: "..\release\bin\next_words\next_cs.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('cs')
Source: "..\release\bin\next_words\next_tr.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('tr')
Source: "..\release\bin\next_words\next_da.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('nordic')
Source: "..\release\bin\next_words\next_sv.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('nordic')
Source: "..\release\bin\next_words\next_nb.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('nordic')
Source: "..\release\bin\next_words\next_is.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('nordic')
Source: "..\release\bin\next_words\next_fi.hwn"; DestDir: "{#BinDir}\next_words"; Flags: ignoreversion; Check: LanguageChosen('nordic')
Source: "..\release\files\*"; DestDir: "{#BinDir}\user-files"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "setup_tasks.ps1"; DestDir: "{commonpf64}\HaniWord"; Flags: ignoreversion
Source: "remove_tasks.ps1"; DestDir: "{commonpf64}\HaniWord"; Flags: ignoreversion
Source: "protect_user_data.ps1"; DestDir: "{commonpf64}\HaniWord"; Flags: ignoreversion
Source: "install_user_files.ps1"; DestDir: "{commonpf64}\HaniWord"; Flags: ignoreversion
Source: "README.txt"; DestDir: "{#BinDir}"; Flags: ignoreversion isreadme

[Tasks]
Name: "desktopicon"; Description: "デスクトップに「HaniWord の設定」を置く"; Flags: unchecked

[Icons]
; 設定アプリはいつでも開けるように、スタートメニューには必ず置く。デスクトップは選んだときだけ
Name: "{autoprograms}\HaniWord の設定"; Filename: "{#BinDir}\azookey-settings.exe"; WorkingDir: "{#BinDir}"
Name: "{autodesktop}\HaniWord の設定"; Filename: "{#BinDir}\azookey-settings.exe"; WorkingDir: "{#BinDir}"; Tasks: desktopicon

[Registry]
; 2026-10-29: 新しい版の知らせの差出人の名前 (launcher が利用者の HKCU に登録する)。入れるときは作らず、アンインストールで消す
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\HaniWord.Updates"; Flags: uninsdeletekey dontcreatekey

[UninstallRun]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ""{commonpf64}\HaniWord\remove_tasks.ps1"""; Flags: runhidden waituntilterminated; RunOnceId: "RemoveHaniWordTasks"

[InstallDelete]
; 以前の版が %APPDATA%\HaniWord に置いていた起動用・候補窓・IME 本体は、利用者の権限で動く install_user_files.ps1 が消す
; (管理者権限のインストーラーは利用者のフォルダの中を消さない。2026-10-19 security review #4)

[UninstallDelete]
; 2026-10-19 (security review #4): アンインストーラーは管理者権限で動くので、利用者のフォルダ (%APPDATA%\HaniWord の学習・設定・
; 変換サーバー・モデル) は消さない。消し方は README に書く
Type: files; Name: "{commonpf64}\HaniWord\launch.vbs"
; 2026-10-29: 片付けきれていない前の版のフォルダ (Releases\<版>-<ビルド>) も消す
Type: filesandordirs; Name: "{commonpf64}\HaniWord\Releases"
Type: dirifempty; Name: "{commonpf64}\HaniWord"

[Code]
var
  DownloadPage: TDownloadWizardPage;
  SetupModePage: TInputOptionWizardPage;
  DomainPage: TInputOptionWizardPage;
  SymbolPage: TInputOptionWizardPage;
  LanguagePage: TInputOptionWizardPage;
  // 専門分野のページに出した分野の名前 (DomainPage の並びと対応)
  ShownDomains: TStringList;

// 長押しのアクセント付きの文字の言語 (shared の LATIN_LANGUAGES と同じ順。LanguagePage の並びと対応)
// 専門分野の辞書の名前と、ページに出す名前 (shared の DOMAIN_NAMES と同じ順)。配る一式の domains.txt に無い分野は出さない。
// domains.txt に新しい分野を足したときは、ここにも名前を足す (無ければページに出ない)
const
  LanguageCodes = 'fr,de,es,it,pt,nordic,pl,cs,tr,pinyin';
  DomainNames = 'law,medicine,biology,chemistry,it,geography,history,earth,architecture,religion,math';
  DomainLabels = '法律,医学,生物,化学,情報,地理,歴史,地学,建築,仏教・神道,数学';
  PublishedDomains = '{#PublishedDomains}';

procedure InitializeWizard;
var
  Names, Labels: TStringList;
  I: Integer;
begin
  DownloadPage := CreateDownloadPage('変換の AI モデルを取得しています', 'Hugging Face の作者の公開場所から zenz-v3.2-small (約 74MB) を取得します', nil);
  // 2026-10-20: 急ぐ人は「おすすめの設定ですぐ使う」で設定のページを全部飛ばせる
  SetupModePage := CreateInputOptionPage(wpWelcome, 'セットアップの方法', 'セットアップの方法を選んでください',
    '設定はあとから変更できます。', True, False);
  SetupModePage.Add('おすすめの設定ですぐに使う');
  SetupModePage.Add('設定を選んでから使う');
  SetupModePage.Values[0] := True;
  // 2026-10-31: よく打つ専門分野 (input.domains_focus)。名前だけを並べ、初期値はどれも選ばない。おすすめの設定でも飛ばさない
  DomainPage := CreateInputOptionPage(SetupModePage.ID, '専門分野', 'よく打つ分野があればチェックを入れてください',
    'チェックを入れた分野は、その分野の話でない文でも用語が出やすくなります。選ばなくても使えます。あとから設定で変更できます。' + #13#10 +
    '分野の用語の出方の感想（良かった・悪かった）は、GitHub の Issues で教えてください。', False, True);
  ShownDomains := TStringList.Create;
  Names := TStringList.Create;
  Labels := TStringList.Create;
  try
    Names.CommaText := DomainNames;
    Labels.CommaText := DomainLabels;
    for I := 0 to Names.Count - 1 do
      if Pos(',' + Names[I] + ',', PublishedDomains) > 0 then begin
        DomainPage.Add(Labels[I]);
        ShownDomains.Add(Names[I]);
      end;
  finally
    Names.Free;
    Labels.Free;
  end;
  SymbolPage := CreateInputOptionPage(DomainPage.ID, '記号の入力', '記号の全角・半角を選んでください',
    '記号ごとの設定は、あとから「詳細設定」で変更できます。', True, False);
  SymbolPage.Add('全角 (！？（）：＃)');
  SymbolPage.Add('半角 (!?():#)');
  SymbolPage.Add('！？～ だけ全角、ほかは半角');
  SymbolPage.Values[0] := True;
  // 2026-10-20: 既定は英語だけ (長押しで思いがけず候補が出ないように)。ほかの言語は選んだときだけ
  LanguagePage := CreateInputOptionPage(SymbolPage.ID, '外国語の入力', '使う言語にチェックを入れてください',
    'キーの長押しで é ñ ü などを入力できます。あとから設定で変更できます。', False, True);
  LanguagePage.Add('フランス語 (é è ç œ)');
  LanguagePage.Add('ドイツ語 (ä ö ü ß)');
  LanguagePage.Add('スペイン語 (ñ á é)');
  LanguagePage.Add('イタリア語 (à è ì)');
  LanguagePage.Add('ポルトガル語 (ã õ ç)');
  LanguagePage.Add('北欧の言語 (å æ ø)');
  LanguagePage.Add('ポーランド語 (ą ł ż)');
  LanguagePage.Add('チェコ語 (č ř ů)');
  LanguagePage.Add('トルコ語 (ğ ı ş)');
  LanguagePage.Add('ピンイン (声調 ā á ǎ à)');
end;

function QuickSetup: Boolean;
begin
  Result := SetupModePage.Values[0];
end;

// おすすめの設定なら、記号・言語・ショートカットのページを飛ばす (ショートカットは既定のとおりスタートメニューだけ、言語は英語だけ)。
// 専門分野のページは飛ばさない (初期値はどれも選ばないので、そのまま次へ進めばおすすめの設定と同じ)。配る分野辞書が無ければ飛ばす
function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (QuickSetup and ((PageID = SymbolPage.ID) or (PageID = LanguagePage.ID) or (PageID = wpSelectTasks)))
    or ((PageID = DomainPage.ID) and (ShownDomains.Count = 0));
end;

// 記号のページで選んだもの (install_user_files.ps1 の -Symbols)。おすすめの設定なら空 (初期値のまま)
function SymbolPreset: String;
begin
  Result := '';
  if not QuickSetup then begin
    if SymbolPage.Values[1] then Result := 'half'
    else if SymbolPage.Values[2] then Result := 'marks'
    else Result := 'full';
  end;
end;

// 言語のページで選んだもの (install_user_files.ps1 の -Languages、コンマ区切り)。おすすめの設定か、選ばなければ空
function LanguageList: String;
var
  Codes: TStringList;
  I: Integer;
begin
  Result := '';
  if QuickSetup then exit;
  Codes := TStringList.Create;
  try
    Codes.CommaText := LanguageCodes;
    for I := 0 to Codes.Count - 1 do
      if LanguagePage.Values[I] then begin
        if Result <> '' then Result := Result + ',';
        Result := Result + Codes[I];
      end;
  finally
    Codes.Free;
  end;
end;

// 2026-11-06: 次の語の候補の表 (next_words) を入れる言語か。言語のページで選んだものと、前の版の設定 (%APPDATA%\HaniWord\settings.json の
// input.latin_languages) で選んであるもの (更新でおすすめの設定を選んでも、前に選んだ言語の表を消さない)。入れていない言語を設定アプリで
// あとから選ぶと、その言語の次の語の候補は出ない (表が無ければ IME 本体は読まないだけで落ちない)。入れ直しで足せる
function LanguageChosen(Code: String): Boolean;
var
  Settings: AnsiString;
  S: String;
  P: Integer;
begin
  Result := Pos(',' + Code + ',', ',' + LanguageList + ',') > 0;
  if Result then exit;
  if LoadStringFromFile(ExpandConstant('{userappdata}\HaniWord\settings.json'), Settings) then begin
    S := String(Settings);
    P := Pos('"latin_languages"', S);
    if P > 0 then begin
      S := Copy(S, P, Length(S));
      P := Pos(']', S);
      if P > 0 then S := Copy(S, 1, P);
      Result := Pos('"' + Code + '"', S) > 0;
    end;
  end;
end;

// 専門分野のページで選んだもの (install_user_files.ps1 の -Domains、コンマ区切り)。おすすめの設定でも選べる。選ばなければ空
function DomainList: String;
var
  I: Integer;
begin
  Result := '';
  for I := 0 to ShownDomains.Count - 1 do
    if DomainPage.Values[I] then begin
      if Result <> '' then Result := Result + ',';
      Result := Result + ShownDomains[I];
    end;
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if CurPageID = wpFinished then
    WizardForm.FinishedLabel.Caption := 'HaniWord を入れました。' + #13#10#13#10 +
      'Win + Space で「HaniWord」を選び、半角/全角 キー (US 配列なら Alt + `) で「あ」にして打ちます。' + #13#10 +
      '入れた直後と PC を起動した直後は、最初の変換に数秒かかります。' + #13#10#13#10 +
      '設定はスタートメニューの「HaniWord の設定」から変更できます。';
end;

// 2026-10-20 (Codex の再調査 L1): 標準ユーザーが別の管理者のパスワードで昇格したときは、%APPDATA%・ログオンのタスク・
// キーボードの登録が別の人のものになって食い違うので、始める前に止める (インストーラーを始めた人の %APPDATA% と比べる)
function SameUserAsOriginal: Boolean;
var
  ExitCode: Integer;
  Expected: String;
begin
  Expected := ExpandConstant('{userappdata}');
  StringChangeEx(Expected, '''', '''''', True);
  Result := ExecAsOriginalUser(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -Command "if ([Environment]::GetFolderPath(''ApplicationData'') -ieq ''' + Expected + ''') { exit 0 } else { exit 3 }"',
    '', SW_HIDE, ewWaitUntilTerminated, ExitCode) and (ExitCode = 0);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ExitCode: Integer;
begin
  Result := '';
  if not SameUserAsOriginal then begin
    Result := 'インストーラーを始めたアカウントと、管理者として許可したアカウントが違うため、入れられません。' + #13#10 +
      'HaniWord を使う管理者のアカウントでサインインしてから、もう一度インストーラーを実行してください。';
    exit;
  end;
  if not Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
      '-NoProfile -EncodedCommand {#StopCommandEncoded}', '', SW_HIDE, ewWaitUntilTerminated, ExitCode) then
    Result := 'HaniWord の更新準備を開始できませんでした。'
  else if ExitCode <> 0 then
    Result := 'HaniWord のサーバーを停止できませんでした。入力を終えてからもう一度試してください。';
end;

// 2026-11-10 (Fable の諮問 perf 3): 更新のたびに同じモデル (約 74MB) を取り直さない。Source のモデルを {tmp} に写し、写した方の SHA-256 が
// 取得のときと同じ値 (ModelSha256) なら、それを使う (照らしたあとで元のファイルが替えられても、使うのは照らした写し)。合わなければ写しを消す
function TryLocalModel(const Source: String): Boolean;
var
  Target: String;
  Size: Int64;
begin
  Result := False;
  Target := ExpandConstant('{tmp}\zenz-v3.2-small.gguf');
  if not FileExists(Source) then exit;
  // 大きさが違えば写さない (モデルの版が変わったときに、置き場の数だけ 74MB を写して消さないように。まとめ役 15:58)
  if not FileSize64(Source, Size) then exit;
  if Size <> {#ModelSize} then exit;
  if not CopyFile(Source, Target, False) then exit;
  try
    Result := CompareText(GetSHA256OfFile(Target), '{#ModelSha256}') = 0;
  except
    Result := False;
  end;
  if Result then
    Log('Reusing the conversion model from ' + Source)
  else
    DeleteFile(Target);
end;

// 前の版の置き場 (Releases\<版>-<ビルド>\user-files。管理者だけが書ける) を先に、なければ %APPDATA%\HaniWord のモデルを見る。
// 古い版のフォルダは、入れ終わったあとで setup_tasks.ps1 が片付けるので、ここではまだ残っている
function ReuseLocalModel: Boolean;
var
  FindRec: TFindRec;
  Releases: String;
begin
  Result := False;
  Releases := ExpandConstant('{commonpf64}\HaniWord\Releases');
  if FindFirst(Releases + '\*', FindRec) then begin
    try
      repeat
        if ((FindRec.Attributes and FILE_ATTRIBUTE_DIRECTORY) <> 0) and (FindRec.Name <> '.') and (FindRec.Name <> '..') then
          Result := TryLocalModel(Releases + '\' + FindRec.Name + '\user-files\zenz-v3.2-small.gguf');
      until Result or not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;
  if not Result then
    Result := TryLocalModel(ExpandConstant('{userappdata}\HaniWord\zenz-v3.2-small.gguf'));
end;

// 準備ができたら (ファイルを入れる前に) モデルを取得する。前の版と同じモデルが手元にあれば取得しない。取得できなければインストールを止める
function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if (CurPageID = wpReady) and not ReuseLocalModel then begin
    DownloadPage.Clear;
    DownloadPage.Add('{#ModelUrl}', 'zenz-v3.2-small.gguf', '{#ModelSha256}');
    DownloadPage.Show;
    try
      try
        DownloadPage.Download;
      except
        SuppressibleMsgBox('モデルを取得できませんでした。インターネットにつながっているか確かめて、もう一度インストールしてください。' + #13#10 + GetExceptionMessage, mbCriticalError, MB_OK, IDOK);
        Result := False;
      end;
    finally
      DownloadPage.Hide;
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ExitCode: Integer;
  ChoiceArguments: String;
begin
  ChoiceArguments := '';
  if SymbolPreset <> '' then ChoiceArguments := ' -Symbols ' + SymbolPreset;
  if LanguageList <> '' then ChoiceArguments := ChoiceArguments + ' -Languages ' + LanguageList;
  if DomainList <> '' then ChoiceArguments := ChoiceArguments + ' -Domains ' + DomainList;
  if CurStep = ssPostInstall then begin
    // モデルはまず保護された user-files に置き、利用者の権限で %APPDATA% へ写す
    WizardForm.StatusLabel.Caption := '変換サーバーと辞書を置いています...';
    if not CopyFile(ExpandConstant('{tmp}\zenz-v3.2-small.gguf'), ExpandConstant('{#BinDir}\user-files\zenz-v3.2-small.gguf'), False) then
      RaiseException('Could not stage the conversion model.');
    if not ExecAsOriginalUser(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
        '-NoProfile -ExecutionPolicy Bypass -File "' + ExpandConstant('{commonpf64}\HaniWord\install_user_files.ps1') +
        '" -Payload "' + ExpandConstant('{#BinDir}\user-files') + '"' + ChoiceArguments, '', SW_HIDE, ewWaitUntilTerminated, ExitCode) then
      RaiseException('Could not install the conversion server for this user.');
    if ExitCode <> 0 then
      RaiseException('Installing the conversion server for this user failed. Run Setup from the normal desktop, not an elevated terminal.');
    if not ExecAsOriginalUser(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
        '-NoProfile -ExecutionPolicy Bypass -File "' + ExpandConstant('{commonpf64}\HaniWord\protect_user_data.ps1') +
        '" -App "' + ExpandConstant('{userappdata}\HaniWord') + '" -Launcher "' + ExpandConstant('{#BinDir}\launcher.exe') + '"', '', SW_HIDE, ewWaitUntilTerminated, ExitCode) then
      RaiseException('Could not protect private learning data.');
    if ExitCode <> 0 then
      RaiseException('Private-data protection failed. Run Setup from the normal desktop, not an elevated terminal.');
    // 2026-10-20: IME の登録と変換サーバーの起動は、%APPDATA% へ全部写したあと ([Run] にすると写す前に起動し、llama.dll が無いと落ちた)
    WizardForm.StatusLabel.Caption := 'IME を登録しています...';
    if not Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
        '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + ExpandConstant('{commonpf64}\HaniWord\setup_tasks.ps1') +
        '" -App "' + ExpandConstant('{app}') + '" -Bin "' + ExpandConstant('{#BinDir}') + '" -JapaneseAddedMarker "' + ExpandConstant('{tmp}\ja_added.txt') + '"',
        '', SW_HIDE, ewWaitUntilTerminated, ExitCode) then
      RaiseException('Could not register the IME.');
    if ExitCode <> 0 then
      RaiseException('Registering the IME failed.');
    // 2026-11-08 (仕事 53): 日本語を新しく足したとき (英語版の Windows など) だけ、US 配列の人へ「ハードウェア キーボード レイアウト」の案内。
    // HaniWord は設定を書き換えない (再起動が要る・システム全体の設定のため)。はい なら利用者の権限で言語の設定を開く
    if FileExists(ExpandConstant('{tmp}\ja_added.txt')) then begin
      if SuppressibleMsgBox('Windows に日本語の入力を足しました。' + #13#10 + #13#10 +
          'US 配列 (英語キーボード) をお使いなら、Windows の言語の設定で、日本語の「ハードウェア キーボード レイアウト」を' +
          '「英語キーボード (101/102 キー)」に変えてください。変えたあとは再起動が要ります。' + #13#10 +
          'あ／A の切り替えは Alt + ` (1 の左のキー) です。' + #13#10 + #13#10 +
          '今、言語の設定を開きますか？', mbInformation, MB_YESNO, IDNO) = IDYES then
        if not ShellExecAsOriginalUser('open', 'ms-settings:regionlanguage', '', '', SW_SHOWNORMAL, ewNoWait, ExitCode) then
          Log('Could not open the language settings: ' + IntToStr(ExitCode));
    end;
  end;
end;
