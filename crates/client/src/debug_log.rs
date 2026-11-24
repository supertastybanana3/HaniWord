// 動作確認用のログ。%APPDATA%\HaniWord\debug_log_path.txt があるときだけ、そこに書いたパスへ追記する。
// 打ったキーが記録されるので、ふつうの利用者の環境 (このファイルが無い) では何も書かない。
// 置き場は shared::app_data_dir (2026-11-10: 試験のビルドでは一時フォルダなので、この PC の本物のログには書かない)
use std::io::Write as _;
use std::sync::OnceLock;

fn log_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = shared::app_data_dir()?;
        let text = std::fs::read_to_string(dir.join("debug_log_path.txt")).ok()?;
        let path = text.trim().trim_start_matches('\u{feff}').to_string();
        allowed_log_path(&dir, &path).then_some(path)
    })
    .as_deref()
}

/// 2026-11-23 (公開前の点検 10): 書いてよい保存先か。打ったキーと窓の題が残るので、保存先は %APPDATA%\HaniWord (app_dir) の下だけにする。
/// 次は書かない: 空・相対パス・.. を含むパス・ネットワークやデバイスのパス (\\ か // で始まる)・app_dir の外・
/// app_dir から先のフォルダかファイルそのものがリンクやジャンクション (リパースポイント。先が app_dir の外でありうる)
pub(crate) fn allowed_log_path(app_dir: &std::path::Path, path: &str) -> bool {
    use std::path::{Component, Path};
    if path.is_empty() || path.starts_with("\\\\") || path.starts_with("//") {
        return false;
    }
    let target = Path::new(path);
    if !target.is_absolute() || target.components().any(|c| matches!(c, Component::ParentDir)) {
        return false;
    }
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let app = norm(app_dir);
    let full = norm(target);
    let Some(rest) = full.strip_prefix(&format!("{app}\\")) else { return false };
    if rest.is_empty() {
        return false;
    }
    // app_dir の下の各段 (あるものだけ) とファイルそのものが、リパースポイントでない (Windows の名前は大文字小文字を問わないので、小文字にした残りでたどる)
    let mut current = app_dir.to_path_buf();
    for part in rest.split('\\').filter(|part| !part.is_empty()) {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if is_reparse_point(&meta) => return false,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    true
}

#[cfg(windows)]
fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || meta.file_type().is_symlink()
}

#[cfg(not(windows))]
fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

/// 2026-11-06: 書く設定になっているか (書く前に TSF に聞くなど、ログのためだけの調べをするときに見る)
pub fn enabled() -> bool {
    !crate::engine::direct_field::private_field()
        && shared::winsec::private_data_allowed()
        && shared::app_data_dir().is_some_and(|dir| dir.join("debug_log_path.txt").is_file())
        && log_path().is_some()
}

pub fn write(message: &str) {
    if !shared::winsec::private_data_allowed() { return; }
    // 2026-11-23 (Astra の点検 37): 覚えない欄 (IS_PRIVATE・パスワード・PIN) でのキーの間は書かない (打った字・欄の様子が残るため)
    if crate::engine::direct_field::private_field() { return; }
    // A cached path must not keep logging after the user disables the setting.
    let Some(dir) = shared::app_data_dir() else { return };
    if !dir.join("debug_log_path.txt").is_file() { return; }
    let Some(path) = log_path() else { return };
    // 2026-11-23: 書くたびに、保存先が app_dir の下でリンクを通らないかを見直す (読んだあとでフォルダがジャンクションに替えられても書かない)
    if !allowed_log_path(&dir, path) { return; }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            file,
            "{} pid={} {}",
            chrono::Local::now().format("%H:%M:%S%.3f"),
            std::process::id(),
            message
        );
    }
}

#[cfg(test)]
mod tests {
    use super::allowed_log_path;

    /// 2026-11-23 (公開前の点検 10): 保存先は %APPDATA%\HaniWord の下だけ。外・相対・..・ネットワーク・リンクの先は書かない
    #[test]
    fn log_path_must_stay_under_the_app_data() {
        let base = std::env::temp_dir().join(format!("haniword_logpath_{}", std::process::id()));
        let app = base.join("HaniWord");
        let outside = base.join("Other");
        std::fs::create_dir_all(app.join("logs")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let s = |p: &std::path::Path| p.to_string_lossy().to_string();
        assert!(allowed_log_path(&app, &s(&app.join("debug.log"))));
        assert!(allowed_log_path(&app, &s(&app.join("logs").join("debug.log"))));
        assert!(allowed_log_path(&app, &s(&app.join("new").join("debug.log"))), "まだ無いフォルダの下は、作られたときに見直す");
        assert!(allowed_log_path(&app, &s(&app.join("debug.log")).to_uppercase()), "大文字小文字は問わない");
        assert!(!allowed_log_path(&app, ""));
        assert!(!allowed_log_path(&app, "debug.log"), "相対パス");
        assert!(!allowed_log_path(&app, &s(&outside.join("debug.log"))), "外");
        assert!(!allowed_log_path(&app, &s(&app)), "フォルダそのもの");
        assert!(!allowed_log_path(&app, &format!("{}\\..\\Other\\debug.log", s(&app))), "..");
        assert!(!allowed_log_path(&app, "\\\\server\\share\\debug.log"), "ネットワーク");
        assert!(!allowed_log_path(&app, "//server/share/debug.log"), "ネットワーク");
        assert!(!allowed_log_path(&app, &format!("{}x\\debug.log", s(&app))), "名前の頭だけが同じ別のフォルダ (HaniWordx)");
        // app の下のジャンクション (先は外) は書かない
        #[cfg(windows)]
        {
            let link = app.join("link");
            let made = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(&link).arg(&outside).output().map(|o| o.status.success()).unwrap_or(false);
            assert!(made, "mklink /J");
            assert!(!allowed_log_path(&app, &s(&link.join("debug.log"))), "ジャンクションの先");
            assert!(!allowed_log_path(&app, &s(&link)), "ジャンクションそのもの");
            let _ = std::fs::remove_dir(&link);
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 2026-11-10: IME 本体の試験のビルドでは、利用者のデータの置き場 (設定・学習・予測の履歴・このログ) が本物の %APPDATA% にならない
    /// (dev-dependencies で shared の機能 test-data-root を入れている)。学習の置き場 (auto_english::data_dir) も同じ所
    #[test]
    fn tests_keep_away_from_the_real_app_data() {
        let dir = shared::app_data_dir().expect("APPDATA");
        assert!(dir.starts_with(std::env::temp_dir()), "{}", dir.display());
        if std::env::var_os("AZK_WORDS_DIR").is_none() {
            assert_eq!(crate::engine::auto_english::data_dir(), Some(dir));
        }
    }
}
