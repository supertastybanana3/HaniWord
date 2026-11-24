// 測りにだけ使う例。配る物と表づくりには使わない
// Microsoft IME (Windows 標準) に IFELanguage で変換させる。入力: 1 行 1 文のひらがな、出力: 変換結果
use std::io::Write;
use windows::{
    core::{BSTR, GUID, HRESULT, PCWSTR, Interface, IUnknown},
    Win32::System::Com::{CLSIDFromProgID, CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED},
};

#[repr(C)]
struct IFELanguageVtbl {
    query_interface: usize,
    add_ref: usize,
    release: usize,
    open: unsafe extern "system" fn(*mut std::ffi::c_void) -> HRESULT,
    close: unsafe extern "system" fn(*mut std::ffi::c_void) -> HRESULT,
    get_jmorph_result: usize,
    get_conversion_mode_caps: usize,
    get_phonetic: usize,
    get_conversion: unsafe extern "system" fn(*mut std::ffi::c_void, *mut u16, i32, i32, *mut *mut u16) -> HRESULT,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let input = std::fs::read_to_string(&args[1])?;
    let mut out = std::fs::File::create(&args[2])?;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let clsid = CLSIDFromProgID(PCWSTR(windows::core::w!("MSIME.Japan").as_ptr()))?;
        let unknown: IUnknown = CoCreateInstance(&clsid, None, CLSCTX_ALL)?;
        let iid = GUID::from_u128(0x019F7152_E6DB_11d0_83C3_00C04FDDB82E);
        let mut fe: *mut std::ffi::c_void = std::ptr::null_mut();
        unknown.query(&iid, &mut fe).ok()?;
        let vtbl = &**(fe as *mut *mut IFELanguageVtbl);
        (vtbl.open)(fe).ok()?;
        for line in input.lines() {
            let b = BSTR::from(line);
            let mut result: *mut u16 = std::ptr::null_mut();
            let len = line.encode_utf16().count() as i32;
            let hr = (vtbl.get_conversion)(fe, b.as_ptr() as *mut u16, 1, len, &mut result);
            let text = if hr.is_ok() && !result.is_null() {
                let r = BSTR::from_raw(result);
                r.to_string()
            } else {
                format!("ERROR {hr:?}")
            };
            writeln!(out, "{text}")?;
        }
        (vtbl.close)(fe).ok()?;
    }
    Ok(())
}
