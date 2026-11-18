//! Icon cho 32 nút chức năng của launcher — MỘT ngôn ngữ hình duy
//! nhất (docs/chuan_giao_dien.md): glyph nét 2px, mực thép xanh
//! RGB(40,93,145) trên nền đen-khoá-trong-suốt, canvas 32x32, sinh
//! bởi packaging/draw-icons.py. Mỗi file module có đúng một glyph;
//! gắn vào nút bằng ImageList + BCM_SETIMAGELIST (nút vẫn là nút
//! native, chỉ thêm hình ở cạnh trái).

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

/// Bảng ánh xạ file module -> file glyph trong assets/ (đủ 32/32).
fn glyph_bytes(file: &str) -> Option<&'static [u8]> {
    Some(match file {
        "KhoDriverIRST.exe" => include_bytes!("../assets/irst.bmp"),
        "KhoWindows.exe" => include_bytes!("../assets/windows.bmp"),
        "BackupLicenseMS.exe" => include_bytes!("../assets/key.bmp"),
        "BackupRestoreDriver.exe" => include_bytes!("../assets/driver.bmp"),
        "TatBitlocker.exe" => include_bytes!("../assets/unlock.bmp"),
        "TaoBootWinPE.exe" => include_bytes!("../assets/usb.bmp"),
        "CaiDatWinTuDong.exe" => include_bytes!("../assets/auto.bmp"),
        "CaiDatWinWinPE.exe" => include_bytes!("../assets/winpe.bmp"),
        "CaiDatOfficeOnline.exe" => include_bytes!("../assets/office-online.bmp"),
        "CaiDatOfficeOffline.exe" => include_bytes!("../assets/office-offline.bmp"),
        "KichHoatWinOffice.exe" => include_bytes!("../assets/activate.bmp"),
        "CaiDatPhanMem.exe" => include_bytes!("../assets/package.bmp"),
        "SaoLuuDuLieu.exe" => include_bytes!("../assets/backup-data.bmp"),
        "SaoLuuWifi.exe" => include_bytes!("../assets/wifi.bmp"),
        "SaoLuuChromeEdge.exe" => include_bytes!("../assets/globe.bmp"),
        "BackupRestoreZalo.exe" => include_bytes!("../assets/chat.bmp"),
        "CheckCauHinh.exe" => include_bytes!("../assets/cpu.bmp"),
        "CheckTest.exe" => include_bytes!("../assets/gauge.bmp"),
        "PhimTatBIOS.exe" => include_bytes!("../assets/keyboard.bmp"),
        "PhanVungOCung.exe" => include_bytes!("../assets/disk.bmp"),
        "TuyChinhWindows.exe" => include_bytes!("../assets/sliders.bmp"),
        "TuyChinhOffice.exe" => include_bytes!("../assets/document.bmp"),
        "WindowsSecurity.exe" => include_bytes!("../assets/shield.bmp"),
        "QuanLyUpdateWindows.exe" => include_bytes!("../assets/refresh.bmp"),
        "DonRacMayTinh.exe" => include_bytes!("../assets/trash.bmp"),
        "GoBoPhanMem.exe" => include_bytes!("../assets/app-minus.bmp"),
        "FixLoiMayIn.exe" => include_bytes!("../assets/printer-fix.bmp"),
        "QuanLyMayIn.exe" => include_bytes!("../assets/printer.bmp"),
        "FixShareMangLAN.exe" => include_bytes!("../assets/share.bmp"),
        "QuanLyFileFolder.exe" => include_bytes!("../assets/folder.bmp"),
        "ChuyenWindowsOS.exe" => include_bytes!("../assets/swap.bmp"),
        "MultiZaloPC.exe" => include_bytes!("../assets/multi-chat.bmp"),
        _ => return None,
    })
}

pub fn attach(button: &nwg::Button, file: &str) -> Option<ButtonIcon> {
    let bytes: &[u8] = glyph_bytes(file)?;
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
