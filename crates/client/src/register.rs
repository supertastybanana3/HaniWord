use windows::{
    core::{w, GUID},
    Win32::{
        Globalization::LocaleNameToLCID,
        System::{
            Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
            Registry::HKEY_CLASSES_ROOT,
        },
        UI::{
            Input::KeyboardAndMouse::HKL,
            TextServices::{
                CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, ITfCategoryMgr,
                ITfInputProcessorProfileMgr, GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
                GUID_TFCAT_TIPCAP_COMLESS, GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
                GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT, GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
                GUID_TFCAT_TIPCAP_UIELEMENTENABLED, GUID_TFCAT_TIP_KEYBOARD,
            },
        },
    },
};

use crate::{
    extension::{GUIDExt as _, RegKey as _, StringExt as _},
    globals::{
        CLSID_PREFIX, GUID_PROFILE, GUID_PROFILE_LIVE, GUID_TEXT_SERVICE, INPROC_SUFFIX,
        SERVICE_NAME, SERVICE_NAME_LIVE,
    },
};

/// 2026-10-30: 言語の一覧 (Win+Space・言語バー) に出すアイコンのファイルと番号。インストールした IME 本体
/// (Program Files\HaniWord\Releases\<版>-<ビルド>\azookey.dll) は、版によらない Program Files\HaniWord\haniword.ico を指す
/// (DLL のパスは更新のたびに変わり、前のフォルダを片付けたあと、一覧が覚えていた前のパスが無くなって既定の字に戻った)。
/// アイコンが無い (前のインストーラー・開発や試験のビルド) ときは、今までどおり DLL の中の 1 つ目のアイコン
fn icon_file(dll_path: &str, exists: impl Fn(&std::path::Path) -> bool) -> (String, u32) {
    let root = std::path::Path::new(dll_path).parent().and_then(|folder| folder.parent())
        .filter(|releases| releases.file_name().is_some_and(|name| name.eq_ignore_ascii_case("Releases")))
        .and_then(|releases| releases.parent());
    match root.map(|root| root.join("haniword.ico")) {
        Some(icon) if exists(&icon) => (icon.to_string_lossy().into_owned(), 0),
        _ => (dll_path.to_string(), 0),
    }
}

// register the information (lang, icon, name, path_of_dll) of the TextService
pub struct ProfileMgr;

impl ProfileMgr {
    pub fn register(dll_path: &str) -> anyhow::Result<()> {
        unsafe {
            let profiles: ITfInputProcessorProfileMgr =
                CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;

            let langid: u16 = LocaleNameToLCID(w!("ja-JP"), 0).try_into()?;
            let (icon, icon_index) = icon_file(dll_path, |path| path.is_file());

            // 2026-10-20: 登録するのはライブ変換のプロファイルだけ (Space で変換する通常版は登録しない)
            for (profile, name) in [(&GUID_PROFILE_LIVE, SERVICE_NAME_LIVE)] {
                profiles.RegisterProfile(
                    &GUID_TEXT_SERVICE,
                    langid,
                    profile,
                    name.to_wide_16().as_slice(),
                    icon.as_str().to_wide_16().as_slice(),
                    icon_index,
                    HKL::default(),
                    0,
                    true,
                    0,
                )?;
            }
            Ok(())
        }
    }

    pub fn unregister() -> anyhow::Result<()> {
        unsafe {
            let profiles: ITfInputProcessorProfileMgr =
                CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;

            let langid: u16 = LocaleNameToLCID(w!("ja-JP"), 0).try_into()?;

            let _ = profiles.UnregisterProfile(&GUID_TEXT_SERVICE, langid, &GUID_PROFILE_LIVE, 0);
            Ok(profiles.UnregisterProfile(&GUID_TEXT_SERVICE, langid, &GUID_PROFILE, 0)?)
        }
    }
}

// register the CLSID(unique id for textservice) of the TextService
pub struct CLSIDMgr;
impl CLSIDMgr {
    pub fn register(dll_path: &str) -> anyhow::Result<()> {
        let clsid_key = CLSID_PREFIX.to_owned() + &GUID_TEXT_SERVICE.to_string();
        let inproc_key = clsid_key.clone() + INPROC_SUFFIX;

        let hkey = HKEY_CLASSES_ROOT.create_subkey(&clsid_key)?;
        hkey.set_string("", SERVICE_NAME)?;
        hkey.close()?;

        let inproc_hkey = HKEY_CLASSES_ROOT.create_subkey(&inproc_key)?;
        inproc_hkey.set_string("", dll_path)?;
        inproc_hkey.set_string("ThreadingModel", "Apartment")?;
        inproc_hkey.close()?;

        Ok(())
    }

    pub fn unregister() -> anyhow::Result<()> {
        let clsid_key = CLSID_PREFIX.to_owned() + &GUID_TEXT_SERVICE.to_string();
        let inproc_key = clsid_key.clone() + INPROC_SUFFIX;

        HKEY_CLASSES_ROOT.delete_tree(&clsid_key)?;
        HKEY_CLASSES_ROOT.delete_tree(&inproc_key)?;

        Ok(())
    }
}

// register the category (entitlements of textservice) of the TextService
pub struct CategoryMgr;
impl CategoryMgr {
    const CATEGORIES: [GUID; 7] = [
        GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
        GUID_TFCAT_TIPCAP_COMLESS,
        GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
        GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
        GUID_TFCAT_TIP_KEYBOARD,
        GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
        GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    ];

    pub fn register() -> anyhow::Result<()> {
        unsafe {
            let catmgr: ITfCategoryMgr =
                CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;

            for cat in Self::CATEGORIES.iter() {
                catmgr.RegisterCategory(&GUID_TEXT_SERVICE, cat, &GUID_TEXT_SERVICE)?;
            }

            Ok(())
        }
    }

    pub fn unregister() -> anyhow::Result<()> {
        unsafe {
            let catmgr: ITfCategoryMgr =
                CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;

            for cat in Self::CATEGORIES.iter() {
                catmgr.UnregisterCategory(&GUID_TEXT_SERVICE, cat, &GUID_TEXT_SERVICE)?;
            }

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::icon_file;

    /// 2026-10-30: インストールした IME 本体は版によらないアイコンを指し、それ以外は DLL の中のアイコン
    #[test]
    fn installed_profiles_use_the_icon_outside_the_versioned_folder() {
        let installed = r"C:\Program Files\HaniWord\Releases\0.1.0-202610300000\azookey32.dll";
        let icon = r"C:\Program Files\HaniWord\haniword.ico";
        assert_eq!(icon_file(installed, |path| path == std::path::Path::new(icon)), (icon.to_string(), 0));
        assert_eq!(icon_file(r"C:\Program Files\HaniWord\Releases\0.1.0\azookey.dll", |_| true), (icon.to_string(), 0));
        // アイコンを置いていない前のインストーラー
        assert_eq!(icon_file(installed, |_| false), (installed.to_string(), 0));
        // 開発・試験のビルド (Releases の下でない)
        for dll in [r"C:\work\target\release\azookey_windows.dll", r"C:\Program Files\HaniWord\azookey.dll", "azookey.dll"] {
            assert_eq!(icon_file(dll, |_| true), (dll.to_string(), 0), "{dll}");
        }
    }
}
