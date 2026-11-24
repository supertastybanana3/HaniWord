use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc::Sender,
    Arc, Mutex, MutexGuard, OnceLock,
};

use anyhow::{Context, Result};

use windows::{
    core::GUID,
    Win32::{
        Foundation::{FALSE, HMODULE, MAX_PATH, TRUE},
        System::LibraryLoader::GetModuleFileNameW,
        UI::TextServices::{
            TF_ATTR_CONVERTED, TF_ATTR_INPUT, TF_ATTR_TARGET_CONVERTED, TF_CT_NONE, TF_DA_ATTR_INFO,
            TF_DA_COLOR, TF_DA_COLOR_0, TF_DA_LINESTYLE, TF_DISPLAYATTRIBUTE, TF_LS_DOT, TF_LS_SOLID,
            TF_LS_SQUIGGLE,
        },
    },
};

pub const CLSID_PREFIX: &str = "CLSID\\";
pub const INPROC_SUFFIX: &str = "\\InProcServer32";

pub const SERVICE_NAME: &str = "HaniWord";

// ab8a96ca-a49b-41f3-aecc-a2458bd4cd3d
pub const GUID_TEXT_SERVICE: GUID = GUID::from_u128(0xab8a96ca_a49b_41f3_aecc_a2458bd4cd3d);
// b596bee4-5830-46ce-a0b3-61bee69fa2b5
pub const GUID_PROFILE: GUID = GUID::from_u128(0xb596bee4_5830_46ce_a0b3_61bee69fa2b5);
// ライブ変換版のプロファイル (同じ IME 本体を、入力中も自動で漢字にする動きで使う)
pub const GUID_PROFILE_LIVE: GUID = GUID::from_u128(0xfbf851f6_e974_4d5c_937c_1b2988a9b1ec);
pub const SERVICE_NAME_LIVE: &str = "HaniWord";

// DisplayAttribute用のGUID
pub const GUID_DISPLAY_ATTRIBUTE: GUID = GUID::from_u128(0x866d3181_3065_4554_b75e_f0750e2423cd);

pub const DISPLAY_ATTRIBUTE: TF_DISPLAYATTRIBUTE = TF_DISPLAYATTRIBUTE {
    crText: TF_DA_COLOR {
        r#type: TF_CT_NONE,
        Anonymous: TF_DA_COLOR_0 { nIndex: 0 },
    },
    crBk: TF_DA_COLOR {
        r#type: TF_CT_NONE,
        Anonymous: TF_DA_COLOR_0 { nIndex: 0 },
    },
    lsStyle: TF_LS_SOLID,
    fBoldLine: TRUE,
    crLine: TF_DA_COLOR {
        r#type: TF_CT_NONE,
        Anonymous: TF_DA_COLOR_0 { nIndex: 0 },
    },
    bAttr: TF_ATTR_TARGET_CONVERTED,
};

fn display_attribute(line: TF_DA_LINESTYLE, bold: bool, attr: TF_DA_ATTR_INFO) -> TF_DISPLAYATTRIBUTE {
    TF_DISPLAYATTRIBUTE {
        crText: TF_DA_COLOR {
            r#type: TF_CT_NONE,
            Anonymous: TF_DA_COLOR_0 { nIndex: 0 },
        },
        crBk: TF_DA_COLOR {
            r#type: TF_CT_NONE,
            Anonymous: TF_DA_COLOR_0 { nIndex: 0 },
        },
        lsStyle: line,
        fBoldLine: if bold { TRUE } else { FALSE },
        crLine: TF_DA_COLOR {
            r#type: TF_CT_NONE,
            Anonymous: TF_DA_COLOR_0 { nIndex: 0 },
        },
        bAttr: attr,
    }
}

// 入力中 (ひらがなのまま): 波線。Microsoft IME と同じ
pub const GUID_DISPLAY_ATTRIBUTE_INPUT: GUID =
    GUID::from_u128(0x5b8d41c5_a08e_4e96_b5ba_67a27f5e02a0);
// ライブ変換の仮表示 (辞書だけで変換したもの): 点線
pub const GUID_DISPLAY_ATTRIBUTE_LIVE_DRAFT: GUID =
    GUID::from_u128(0x54a67e8e_9717_47cb_bbef_b72a6a93a031);
// 変換済み (変換中のほかの文節、ライブ変換の清書後): 細い実線
pub const GUID_DISPLAY_ATTRIBUTE_CONVERTED: GUID =
    GUID::from_u128(0x96f8091e_ee96_4459_965f_917c967403bd);

// 2026-11-08 (仕事 53): 入力モード (あ／A) を切り替える保ったキー (TSF の PreserveKey)。Alt を押したキーは key event sink では受けないので
pub const GUID_PRESERVED_KEY_BACKQUOTE: GUID = GUID::from_u128(0x47c980d7_2cc6_4ab6_a0fb_34d5fe9beaeb);
pub const GUID_PRESERVED_KEY_KANJI: GUID = GUID::from_u128(0x06db3fb1_0fe2_4fc1_8bfa_e0033da1b142);

/// 保ったキーの一覧: Alt+` (VK_OEM_3。切り替えるのは配列で 1 の左の位置にあるときだけ。mode_keys.rs) と VK_KANJI (修飾キーによらない)
pub fn preserved_mode_keys() -> [(GUID, windows::Win32::UI::TextServices::TF_PRESERVEDKEY); 2] {
    use windows::Win32::UI::TextServices::{TF_MOD_ALT, TF_MOD_IGNORE_ALL_MODIFIER, TF_PRESERVEDKEY};
    [
        (GUID_PRESERVED_KEY_BACKQUOTE, TF_PRESERVEDKEY { uVKey: crate::engine::mode_keys::VK_OEM_3 as u32, uModifiers: TF_MOD_ALT }),
        (GUID_PRESERVED_KEY_KANJI, TF_PRESERVEDKEY { uVKey: crate::engine::mode_keys::VK_KANJI as u32, uModifiers: TF_MOD_IGNORE_ALL_MODIFIER }),
    ]
}

pub fn display_attribute_input() -> TF_DISPLAYATTRIBUTE {
    display_attribute(TF_LS_SQUIGGLE, false, TF_ATTR_INPUT)
}
pub fn display_attribute_live_draft() -> TF_DISPLAYATTRIBUTE {
    display_attribute(TF_LS_DOT, false, TF_ATTR_INPUT)
}
pub fn display_attribute_converted() -> TF_DISPLAYATTRIBUTE {
    display_attribute(TF_LS_SOLID, false, TF_ATTR_CONVERTED)
}

// You can use any value for this cookie.
pub const TEXTSERVICE_LANGBARITEMSINK_COOKIE: u32 = 0;

pub static DLL_INSTANCE: OnceLock<Mutex<DllModule>> = OnceLock::new();

unsafe impl Sync for DllModule {}
unsafe impl Send for DllModule {}

#[derive(Debug)]
pub struct DllModule {
    pub ref_count: Arc<AtomicUsize>,
    pub hinst: Option<HMODULE>,
    pub sender: Option<Sender<bool>>,
}

impl DllModule {
    pub fn new() -> Self {
        Self {
            ref_count: Arc::new(AtomicUsize::new(0)),
            hinst: None,
            sender: None,
        }
    }

    pub fn get() -> Result<MutexGuard<'static, DllModule>> {
        DLL_INSTANCE
            .get()
            .ok_or_else(|| anyhow::anyhow!("DllModule is not initialized"))?
            .lock()
            // 持っている途中でパニックしたロックも、印を外して使い続ける (state.rs の try_lock_recovering と同じ考え)
            .or_else(|poisoned| {
                if let Some(mutex) = DLL_INSTANCE.get() {
                    mutex.clear_poison();
                }
                Ok::<_, anyhow::Error>(poisoned.into_inner())
            })
    }

    pub fn get_path() -> anyhow::Result<String> {
        let path = {
            let dll_instance = DllModule::get()?.hinst;

            let mut buffer: [u16; MAX_PATH as usize] = [0; MAX_PATH as usize];
            let length = unsafe {
                GetModuleFileNameW(dll_instance.context("Dll instance not found")?, &mut buffer)
            };

            String::from_utf16_lossy(&buffer[..length as usize])
        };
        Ok(path)
    }

    pub fn add_ref(&mut self) -> usize {
        self.ref_count.fetch_add(1, Ordering::SeqCst)
    }

    pub fn release(&mut self) -> usize {
        self.ref_count.fetch_sub(1, Ordering::SeqCst)
    }

    pub fn can_unload(&self) -> bool {
        self.ref_count.load(Ordering::SeqCst) <= 0
    }
}
