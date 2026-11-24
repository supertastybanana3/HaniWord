// Windows に登録されている全 IME の表示属性 (下線の種類など) を一覧する
use windows::{
    core::GUID,
    Win32::{
        System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED},
        UI::TextServices::{
            CLSID_TF_DisplayAttributeMgr, ITfDisplayAttributeInfo, ITfDisplayAttributeMgr,
            TF_DISPLAYATTRIBUTE,
        },
    },
};

fn main() -> windows::core::Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mgr: ITfDisplayAttributeMgr = CoCreateInstance(&CLSID_TF_DisplayAttributeMgr, None, CLSCTX_INPROC_SERVER)?;
        let e = mgr.EnumDisplayAttributeInfo()?;
        loop {
            let mut info: [Option<ITfDisplayAttributeInfo>; 1] = [None];
            let mut fetched = 0u32;
            if e.Next(&mut info, &mut fetched).is_err() || fetched == 0 {
                break;
            }
            let Some(info) = info[0].take() else { break };
            let guid: GUID = info.GetGUID()?;
            let desc = info.GetDescription().map(|b| b.to_string()).unwrap_or_default();
            let mut owner = GUID::zeroed();
            let mut dummy: Option<ITfDisplayAttributeInfo> = None;
            let _ = mgr.GetDisplayAttributeInfo(&guid, &mut dummy, &mut owner);
            let mut a = TF_DISPLAYATTRIBUTE::default();
            info.GetAttributeInfo(&mut a)?;
            println!(
                "owner={owner:?}\t{guid:?}\t{desc}\tline={:?}\tbold={}\tlinecolor={:?}/{}\ttext={:?}/{}\tbk={:?}/{}\tattr={:?}",
                a.lsStyle,
                a.fBoldLine.as_bool(),
                a.crLine.r#type,
                a.crLine.Anonymous.cr.0,
                a.crText.r#type,
                a.crText.Anonymous.cr.0,
                a.crBk.r#type,
                a.crBk.Anonymous.cr.0,
                a.bAttr
            );
        }
    }
    Ok(())
}
