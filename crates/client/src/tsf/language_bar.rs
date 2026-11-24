use windows::{
    core::{IUnknown, Interface as _, BSTR, GUID, PCWSTR},
    Win32::{
        Foundation::{BOOL, E_INVALIDARG, POINT, RECT},
        System::Ole::CONNECT_E_CANNOTCONNECT,
        UI::{
            TextServices::{
                ITfLangBarItemButton_Impl, ITfLangBarItemSink, ITfLangBarItem_Impl, ITfMenu,
                ITfSource_Impl, TfLBIClick, GUID_LBI_INPUTMODE, TF_LANGBARITEMINFO,
                TF_LBI_CLK_RIGHT, TF_LBI_STYLE_BTN_BUTTON, TF_LBMENUF_GRAYED, TF_LBMENUF_RADIOCHECKED,
                TF_LBMENUF_SEPARATOR,
            },
            WindowsAndMessaging::{LoadImageW, HICON, IMAGE_ICON, LR_DEFAULTCOLOR},
        },
    },
};

use crate::{
    engine::{
        client_action::ClientAction, composition::CompositionState, input_mode::InputMode,
        state::IMEState, theme::get_theme,
    },
    globals::{DllModule, GUID_TEXT_SERVICE, TEXTSERVICE_LANGBARITEMSINK_COOKIE},
};

use anyhow::{Context as _, Result};

use super::factory::TextServiceFactory_Impl;

const MENU_TEX_AUTO: u32 = 1;
const MENU_TEX_ON: u32 = 2;
const MENU_TEX_OFF: u32 = 3;
const MENU_SETTINGS: u32 = 10;
/// 2026-11-04: 設定アプリを単語登録のページで開く
const MENU_USER_DICTIONARY: u32 = 11;

/// 右クリックのメニューを出して、選ばれた項目の ID を返す (選ばなければ None)
fn show_popup_menu(pt: &POINT) -> Option<u32> {
    use crate::engine::tex::{installed, mode, TexMode};
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, TrackPopupMenu, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
        TPM_LEFTALIGN, TPM_LEFTBUTTON, TPM_NONOTIFY, TPM_RETURNCMD, TPM_TOPALIGN,
    };
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let add = |id: u32, flags, text: &str| {
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = AppendMenuW(menu, MF_STRING | flags, id as usize, PCWSTR(wide.as_ptr()));
        };
        if installed() {
            let current = mode();
            let check = |m: TexMode| if current == m { MF_CHECKED } else { Default::default() };
            add(MENU_TEX_AUTO, check(TexMode::Auto), "TeX: 自動 (.tex を開いているとき)");
            add(MENU_TEX_ON, check(TexMode::On), "TeX: オン");
            add(MENU_TEX_OFF, check(TexMode::Off), "TeX: オフ");
        } else {
            add(0, MF_GRAYED, "TeX 版は入っていません");
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        add(MENU_SETTINGS, Default::default(), "設定を開く");
        add(MENU_USER_DICTIONARY, Default::default(), "単語登録を開く");
        // Mozc と同じ出し方: 持ち主はこのスレッドで入力を受けているウィンドウ、左ボタンで選ぶ (右ボタンにすると、
        // 右クリックを離した瞬間に閉じる)、TPM_NONOTIFY (アプリにメニューの通知を送らない)
        let owner = windows::Win32::UI::Input::KeyboardAndMouse::GetFocus();
        let id = TrackPopupMenu(
            menu,
            TPM_NONOTIFY | TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN | TPM_LEFTBUTTON,
            pt.x,
            pt.y,
            0,
            owner,
            None,
        );
        let _ = DestroyMenu(menu);
        crate::debug_log::write(&format!("langbar menu -> {}", id.0));
        (id.0 > 0).then_some(id.0 as u32)
    }
}

const INFO: TF_LANGBARITEMINFO = TF_LANGBARITEMINFO {
    clsidService: GUID_TEXT_SERVICE,
    guidItem: GUID_LBI_INPUTMODE,
    // 左クリックで あ/A。右クリックは OnClick に TF_LBI_CLK_RIGHT で届くので、メニューは自分で出す (Windows 8 以降のタスクバー)
    dwStyle: TF_LBI_STYLE_BTN_BUTTON,
    ulSort: 0,
    szDescription: [0; 32],
};

// you need to implement these three interfaces to create a language bar item
// if not, you will get E_FAIL error in ITfLangBarItemMgr::AddItem

impl ITfLangBarItem_Impl for TextServiceFactory_Impl {
    #[macros::anyhow]
    fn GetInfo(&self, p_info: *mut TF_LANGBARITEMINFO) -> Result<()> {
        unsafe {
            *p_info = INFO;
        }
        Ok(())
    }

    #[macros::anyhow]
    fn GetStatus(&self) -> Result<u32> {
        Ok(0)
    }

    #[macros::anyhow]
    fn Show(&self, _f_show: BOOL) -> Result<()> {
        Ok(())
    }

    // this will be shown as a tooltip when you hover the language bar item
    #[macros::anyhow]
    fn GetTooltipString(&self) -> Result<BSTR> {
        Ok(BSTR::default())
    }
}

impl ITfLangBarItemButton_Impl for TextServiceFactory_Impl {
    #[macros::anyhow]
    fn OnClick(&self, click: TfLBIClick, pt: &POINT, _prcarea: *const RECT) -> Result<()> {
        crate::debug_log::write(&format!("langbar click {:?} at ({}, {})", click.0, pt.x, pt.y));
        if click == TF_LBI_CLK_RIGHT {
            if let Some(id) = show_popup_menu(pt) {
                self.OnMenuSelect(id)?;
            }
            return Ok(());
        }
        let mode = {
            let ime_mode = &IMEState::get()?.input_mode;
            match ime_mode {
                InputMode::Latin => InputMode::Kana,
                InputMode::Kana => InputMode::Latin,
            }
        };

        let actions = vec![ClientAction::SetIMEMode(mode)];
        self.handle_action(&actions, CompositionState::None)?;

        Ok(())
    }

    // 右クリックのメニュー
    #[macros::anyhow]
    fn InitMenu(&self, pmenu: Option<&ITfMenu>) -> Result<()> {
        use crate::engine::tex::{installed, mode, TexMode};
        let Some(menu) = pmenu else { return Ok(()) };
        let add = |id: u32, flags: u32, text: &str| -> Result<()> {
            let wide: Vec<u16> = text.encode_utf16().collect();
            unsafe { menu.AddMenuItem(id, flags, None, None, &wide, std::ptr::null_mut())? };
            Ok(())
        };
        if installed() {
            let current = mode();
            let check = |m: TexMode| if current == m { TF_LBMENUF_RADIOCHECKED } else { 0 };
            add(MENU_TEX_AUTO, check(TexMode::Auto), "TeX: 自動 (.tex を開いているとき)")?;
            add(MENU_TEX_ON, check(TexMode::On), "TeX: オン")?;
            add(MENU_TEX_OFF, check(TexMode::Off), "TeX: オフ")?;
        } else {
            add(0, TF_LBMENUF_GRAYED, "TeX 版は入っていません")?;
        }
        add(0, TF_LBMENUF_SEPARATOR, "")?;
        add(MENU_SETTINGS, 0, "設定を開く")?;
        add(MENU_USER_DICTIONARY, 0, "単語登録を開く")?;
        Ok(())
    }

    #[macros::anyhow]
    fn OnMenuSelect(&self, w_id: u32) -> Result<()> {
        use crate::engine::tex::{set_mode, TexMode};
        match w_id {
            MENU_TEX_AUTO => set_mode(TexMode::Auto),
            MENU_TEX_ON => set_mode(TexMode::On),
            MENU_TEX_OFF => set_mode(TexMode::Off),
            // 2026-10-14 (脆弱性の再調査 #7): 管理者権限のアプリの言語バーからは起動しない (設定アプリが管理者権限で動くため)
            MENU_SETTINGS | MENU_USER_DICTIONARY if shared::winsec::is_elevated() => {}
            MENU_SETTINGS | MENU_USER_DICTIONARY => {
                if let Some(dir) = crate::engine::auto_english::dll_dir() {
                    let mut settings = std::process::Command::new(dir.join("azookey-settings.exe"));
                    if w_id == MENU_USER_DICTIONARY {
                        settings.arg("--user-dictionary");
                    }
                    let _ = settings.spawn();
                }
            }
            _ => {}
        }
        Ok(())
    }

    #[macros::anyhow]
    fn GetIcon(&self) -> Result<HICON> {
        let dll_module = DllModule::get()?;
        let state = &IMEState::get()?;
        let input_mode = &state.input_mode;
        let theme = get_theme()?;

        // 2026-11-06 (仕事 25): そのまま渡す欄 (パスワード欄など) にフォーカスがある間は、あ のモードでも A の絵 (direct_field::shown_latin)
        let shown_latin = crate::engine::direct_field::shown_latin();
        let icon_id = match input_mode {
            InputMode::Kana if !shown_latin => {
                if theme {
                    102
                } else {
                    104
                }
            }
            InputMode::Latin | InputMode::Kana => {
                if theme {
                    103
                } else {
                    105
                }
            }
        };

        unsafe {
            let handle = LoadImageW(
                dll_module.hinst.context("Dll instance not found")?,
                PCWSTR(icon_id as *mut u16),
                IMAGE_ICON,
                0,
                0,
                LR_DEFAULTCOLOR,
            )?;

            Ok(HICON(handle.0))
        }
    }

    #[macros::anyhow]
    fn GetText(&self) -> Result<BSTR> {
        Ok(BSTR::default())
    }
}

impl ITfSource_Impl for TextServiceFactory_Impl {
    #[macros::anyhow]
    fn AdviseSink(&self, riid: *const GUID, punk: Option<&IUnknown>) -> Result<u32> {
        let riid = unsafe { *riid };

        if riid != ITfLangBarItemSink::IID {
            return Err(anyhow::Error::new(windows_core::Error::from_hresult(
                E_INVALIDARG,
            )));
        }

        if punk.is_none() {
            return Err(anyhow::Error::new(windows_core::Error::from_hresult(
                E_INVALIDARG,
            )));
        }

        Ok(TEXTSERVICE_LANGBARITEMSINK_COOKIE)
    }

    #[macros::anyhow]
    fn UnadviseSink(&self, dw_cookie: u32) -> Result<()> {
        if dw_cookie != TEXTSERVICE_LANGBARITEMSINK_COOKIE {
            return Err(anyhow::Error::new(windows_core::Error::from_hresult(
                CONNECT_E_CANNOTCONNECT,
            )));
        }

        Ok(())
    }
}
