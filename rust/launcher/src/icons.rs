use native_windows_gui as nwg;
use winapi::shared::windef::RECT;
use winapi::um::commctrl::{ImageList_AddMasked, ImageList_Create, ImageList_Destroy, HIMAGELIST, ILC_COLOR32, ILC_MASK};
use winapi::um::winuser::SendMessageW;

pub struct ButtonIcon(HIMAGELIST);
impl Drop for ButtonIcon {
    fn drop(&mut self) { unsafe { ImageList_Destroy(self.0); } }
}

#[repr(C)]
struct ButtonImageList { images: HIMAGELIST, margin: RECT, align: u32 }

pub fn attach(button: &nwg::Button, file: &str) -> Option<ButtonIcon> {
    let bytes: &[u8] = match file {
        "TatBitlocker.exe" => include_bytes!("../assets/unlock.bmp"),
        "CheckCauHinh.exe" => include_bytes!("../assets/cpu.bmp"),
        "PhanVungOCung.exe" => include_bytes!("../assets/disk.bmp"),
        "TuyChinhWindows.exe" => include_bytes!("../assets/sliders.bmp"),
        "TuyChinhOffice.exe" => include_bytes!("../assets/document.bmp"),
        "WindowsSecurity.exe" => include_bytes!("../assets/shield.bmp"),
        "QuanLyUpdateWindows.exe" => include_bytes!("../assets/refresh.bmp"),
        "DonRacMayTinh.exe" => include_bytes!("../assets/trash.bmp"),
        "GoBoPhanMem.exe" => include_bytes!("../assets/app-minus.bmp"),
        _ => return None,
    };
    let bitmap = nwg::Bitmap::from_bin(bytes).expect("embedded module icon");
    unsafe {
        let images = ImageList_Create(32, 32, ILC_COLOR32 | ILC_MASK, 1, 0);
        if images.is_null() { return None; }
        let icon = ButtonIcon(images);
        if ImageList_AddMasked(images, bitmap.handle as _, 0) < 0 { return None; }
        let config = ButtonImageList {
            images,
            margin: RECT { left: 8, top: 0, right: 4, bottom: 0 },
            align: 0, // BUTTON_IMAGELIST_ALIGN_LEFT
        };
        if SendMessageW(button.handle.hwnd()?, 0x1602, 0, &config as *const _ as isize) == 0 { return None; }
        Some(icon)
    }
}
