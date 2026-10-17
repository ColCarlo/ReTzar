//! Windows' own folder picker (`IFileOpenDialog` asked for folders), used
//! by the portable copy to ask where the original game is installed
//! ([`crate::original`]). Elsewhere there is none.
use std::path::{Path, PathBuf};

/// A folder chosen by the player, starting from `start`; none when the
/// dialog is closed or cannot be shown.
pub fn folder(title: &str, start: Option<&Path>) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows::folder(title, start)
    }
    #[cfg(not(windows))]
    {
        let _ = (title, start);
        None
    }
}
/// A question with «OK» and «Annulla» (Windows' message box); true for
/// «OK». Without Windows, false.
pub fn confirm(text: &str) -> bool {
    #[cfg(windows)]
    {
        windows::confirm(text)
    }
    #[cfg(not(windows))]
    {
        let _ = text;
        false
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};

    type Hresult = i32;
    #[repr(C)]
    pub(super) struct Guid(u32, u16, u16, [u8; 8]);
    const CLSID_FILE_OPEN_DIALOG: Guid = Guid(
        0xdc1c_5a9c,
        0xe88a,
        0x4dde,
        [0xa5, 0xa1, 0x60, 0xf8, 0x2a, 0x20, 0xae, 0xf7],
    );
    const IID_FILE_OPEN_DIALOG: Guid = Guid(
        0xd57c_7288,
        0xd4ad,
        0x4768,
        [0xbe, 0x02, 0x9d, 0x96, 0x95, 0x32, 0xd9, 0x60],
    );
    pub(super) const IID_SHELL_ITEM: Guid = Guid(
        0x4382_6d1e,
        0xe718,
        0x42ee,
        [0xbc, 0x55, 0xa1, 0xe2, 0x61, 0xc3, 0x7b, 0xfe],
    );
    const CLSCTX_INPROC_SERVER: u32 = 1;
    const COINIT_APARTMENTTHREADED: u32 = 2;
    const COINIT_DISABLE_OLE1DDE: u32 = 4;
    const FOS_PICKFOLDERS: u32 = 0x20;
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    const FOS_PATHMUSTEXIST: u32 = 0x800;
    const SIGDN_FILESYSPATH: u32 = 0x8005_8000;

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> Hresult;
        fn CoUninitialize();
        fn CoCreateInstance(
            class: *const Guid,
            outer: *mut c_void,
            context: u32,
            interface: *const Guid,
            out: *mut *mut c_void,
        ) -> Hresult;
        fn CoTaskMemFree(memory: *mut c_void);
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        pub(super) fn SHCreateItemFromParsingName(
            path: *const u16,
            context: *mut c_void,
            interface: *const Guid,
            out: *mut *mut c_void,
        ) -> Hresult;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            window: *mut c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }

    pub(super) fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }
    /// The function at `slot` of a COM object's table (the slots of
    /// ShObjIdl.h: IUnknown 0–2, IModalWindow::Show 3, IFileDialog from 4).
    pub(super) unsafe fn slot(object: *mut c_void, slot: usize) -> *const c_void {
        unsafe { *(*(object as *const *const *const c_void)).add(slot) }
    }
    pub(super) unsafe fn release(object: *mut c_void) {
        let f: unsafe extern "system" fn(*mut c_void) -> u32 =
            unsafe { std::mem::transmute(slot(object, 2)) };
        unsafe { f(object) };
    }
    /// An `IShellItem`'s file system path (`GetDisplayName`, slot 5).
    pub(super) unsafe fn item_path(item: *mut c_void) -> Option<PathBuf> {
        let f: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16) -> Hresult =
            unsafe { std::mem::transmute(slot(item, 5)) };
        let mut name: *mut u16 = std::ptr::null_mut();
        if unsafe { f(item, SIGDN_FILESYSPATH, &mut name) } < 0 || name.is_null() {
            return None;
        }
        let length = (0..).take_while(|&i| unsafe { *name.add(i) } != 0).count();
        let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(name, length) });
        unsafe { CoTaskMemFree(name.cast()) };
        Some(PathBuf::from(text))
    }
    /// A folder dialog made ready: folders only, its title and first
    /// folder. The caller releases it.
    pub(super) unsafe fn dialog(title: &str, start: Option<&Path>) -> Option<*mut c_void> {
        let mut object: *mut c_void = std::ptr::null_mut();
        let made = unsafe {
            CoCreateInstance(
                &CLSID_FILE_OPEN_DIALOG,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_FILE_OPEN_DIALOG,
                &mut object,
            )
        };
        if made < 0 || object.is_null() {
            return None;
        }
        let get_options: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult =
            unsafe { std::mem::transmute(slot(object, 10)) };
        let set_options: unsafe extern "system" fn(*mut c_void, u32) -> Hresult =
            unsafe { std::mem::transmute(slot(object, 9)) };
        let set_title: unsafe extern "system" fn(*mut c_void, *const u16) -> Hresult =
            unsafe { std::mem::transmute(slot(object, 17)) };
        let set_folder: unsafe extern "system" fn(*mut c_void, *mut c_void) -> Hresult =
            unsafe { std::mem::transmute(slot(object, 12)) };
        let mut options = 0;
        unsafe {
            get_options(object, &mut options);
            set_options(
                object,
                options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST,
            );
            set_title(object, wide(title).as_ptr());
        }
        if let Some(start) = start {
            let mut item: *mut c_void = std::ptr::null_mut();
            let path = wide(&start.to_string_lossy());
            let found = unsafe {
                SHCreateItemFromParsingName(
                    path.as_ptr(),
                    std::ptr::null_mut(),
                    &IID_SHELL_ITEM,
                    &mut item,
                )
            };
            if found >= 0 && !item.is_null() {
                unsafe {
                    set_folder(object, item);
                    release(item);
                }
            }
        }
        Some(object)
    }

    pub(super) fn folder(title: &str, start: Option<&Path>) -> Option<PathBuf> {
        let started = unsafe {
            CoInitializeEx(
                std::ptr::null_mut(),
                COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE,
            )
        };
        let chosen = unsafe {
            dialog(title, start).and_then(|object| {
                let show: unsafe extern "system" fn(*mut c_void, *mut c_void) -> Hresult =
                    std::mem::transmute(slot(object, 3));
                let get_result: unsafe extern "system" fn(
                    *mut c_void,
                    *mut *mut c_void,
                ) -> Hresult = std::mem::transmute(slot(object, 20));
                let mut item: *mut c_void = std::ptr::null_mut();
                // A closed dialog answers ERROR_CANCELLED.
                let path = (show(object, std::ptr::null_mut()) >= 0
                    && get_result(object, &mut item) >= 0
                    && !item.is_null())
                .then(|| {
                    let path = item_path(item);
                    release(item);
                    path
                })
                .flatten();
                release(object);
                path
            })
        };
        // S_OK or S_FALSE: this thread started COM here.
        if started >= 0 {
            unsafe { CoUninitialize() };
        }
        chosen
    }

    pub(super) fn confirm(text: &str) -> bool {
        let (text, caption) = (wide(text), wide("ReTzar"));
        // MB_OKCANCEL | MB_ICONWARNING; IDOK.
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), 0x31) == 1 }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::windows::*;
    use std::ffi::c_void;

    /// The dialog's slots answer as their methods, without showing it:
    /// its options keep the folders, a result before showing is refused,
    /// and a folder read back through a shell item is the same.
    #[test]
    fn the_folder_dialog_is_made_without_being_shown() {
        #[link(name = "ole32")]
        unsafe extern "system" {
            fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
        }
        unsafe { CoInitializeEx(std::ptr::null_mut(), 2) };
        let start = std::env::temp_dir();
        let object = unsafe { dialog("ReTzar", Some(&start)) }.expect("dialogo");
        unsafe {
            let get_options: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32 =
                std::mem::transmute(slot(object, 10));
            let mut options = 0;
            assert!(get_options(object, &mut options) >= 0);
            assert_eq!(options & 0x20, 0x20, "FOS_PICKFOLDERS");
            let get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 =
                std::mem::transmute(slot(object, 20));
            let mut item: *mut c_void = std::ptr::null_mut();
            assert!(get_result(object, &mut item) < 0);
            release(object);
            let path = wide(&start.to_string_lossy());
            let mut item: *mut c_void = std::ptr::null_mut();
            assert!(
                SHCreateItemFromParsingName(
                    path.as_ptr(),
                    std::ptr::null_mut(),
                    &IID_SHELL_ITEM,
                    &mut item
                ) >= 0
            );
            let back = item_path(item).expect("percorso");
            release(item);
            assert_eq!(back.canonicalize().unwrap(), start.canonicalize().unwrap());
        }
    }
}
