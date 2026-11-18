#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Sinh toàn bộ icon cho Utility Rebuild — MỘT ngôn ngữ hình duy nhất.

Chuẩn icon (xem docs/chuan_giao_dien.md):
  - Canvas 32x32, nền ĐEN tuyền (0,0,0) = màu khoá trong suốt của
    ImageList_AddMasked phía launcher.
  - Glyph nét đơn sắc thép xanh RGB(40,93,145), bút 2px, không chữ,
    không đổ bóng, hình học primitives (line / rect / ellipse / arc).
  - 9 glyph đầu (unlock, cpu, disk, sliders, document, shield,
    refresh, trash, app-minus) giữ đúng toạ độ của draw-icons.ps1
    (bản System.Drawing cũ) để không đổi nhận diện đã quen; 23 glyph
    còn lại vẽ mới cùng ngôn ngữ, phủ đủ 32 nút launcher.
  - Ngoài ra sinh qsu.ico (16/24/32/48) cho icon cửa sổ dùng chung:
    nền bo góc thép xanh + glyph cửa sổ 4 ô trắng.

Chạy:  python3 packaging/draw-icons.py
Ra:    rust/launcher/assets/<tên>.bmp (32 file)
       rust/qsu_common/assets/qsu.ico
       /tmp/qsu-icons-contact.png (bảng đối chiếu để kiểm mắt)
"""
from PIL import Image, ImageDraw
import os

INK = (40, 93, 145, 255)
BG = (0, 0, 0, 255)
W = 2  # bề dày nét chuẩn

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSETS = os.path.join(ROOT, "rust", "launcher", "assets")
COMMON_ASSETS = os.path.join(ROOT, "rust", "qsu_common", "assets")


def canvas():
    img = Image.new("RGBA", (32, 32), BG)
    return img, ImageDraw.Draw(img)


def L(d, x1, y1, x2, y2):
    d.line([x1, y1, x2, y2], fill=INK, width=W)


def R(d, x, y, w, h):
    d.rectangle([x, y, x + w, y + h], outline=INK, width=W)


def E(d, x, y, w, h):
    d.ellipse([x, y, x + w, y + h], outline=INK, width=W)


def A(d, x, y, w, h, start, end):
    d.arc([x, y, x + w, y + h], start, end, fill=INK, width=W)


def P(d, pts):
    d.line(pts + [pts[0]], fill=INK, width=W, joint="curve")


def PL(d, pts):
    d.line(pts, fill=INK, width=W, joint="curve")


# ---------------------------------------------------------------- 9 glyph cũ
def g_unlock(d):
    R(d, 7, 14, 18, 13)
    A(d, 11, 3, 13, 17, 180, 360)
    L(d, 11, 11, 11, 14)
    L(d, 16, 19, 16, 23)


def g_cpu(d):
    R(d, 8, 8, 16, 16)
    R(d, 12, 12, 8, 8)
    for v in (11, 16, 21):
        L(d, v, 3, v, 7); L(d, v, 25, v, 29)
        L(d, 3, v, 7, v); L(d, 25, v, 29, v)


def g_disk(d):
    R(d, 5, 5, 22, 22)
    E(d, 9, 8, 14, 14)
    E(d, 14, 13, 4, 4)
    L(d, 16, 16, 24, 23)
    L(d, 8, 24, 11, 24)


def g_sliders(d):
    for v in (8, 16, 24):
        L(d, 4, v, 28, v)
    R(d, 9, 5, 4, 6); R(d, 20, 13, 4, 6); R(d, 12, 21, 4, 6)


def g_document(d):
    PL(d, [(7, 3), (20, 3), (26, 9), (26, 29), (7, 29), (7, 3)])
    L(d, 20, 3, 20, 9); L(d, 20, 9, 26, 9)
    L(d, 11, 14, 22, 14); L(d, 11, 19, 22, 19); L(d, 11, 24, 19, 24)


def g_shield(d):
    P(d, [(16, 3), (27, 7), (26, 19), (22, 25), (16, 29), (10, 25), (6, 19), (5, 7)])
    PL(d, [(10, 16), (14, 20), (23, 11)])


def g_refresh(d):
    A(d, 5, 5, 22, 22, 210, 355)
    A(d, 5, 5, 22, 22, 30, 175)
    L(d, 27, 6, 27, 14); L(d, 27, 14, 19, 14)
    L(d, 5, 26, 5, 18); L(d, 5, 18, 13, 18)


def g_trash(d):
    L(d, 5, 8, 27, 8)
    R(d, 12, 4, 8, 4)
    PL(d, [(8, 10), (10, 28), (23, 28), (25, 10)])
    L(d, 13, 12, 14, 24); L(d, 20, 12, 19, 24)


def g_app_minus(d):
    R(d, 4, 5, 24, 23)
    L(d, 4, 11, 28, 11)
    L(d, 8, 8, 9, 8); L(d, 12, 8, 13, 8)
    L(d, 10, 20, 22, 20)


# ---------------------------------------------------------------- glyph mới
def g_irst(d):  # khay ổ đĩa xếp chồng (kho driver lưu trữ)
    R(d, 5, 6, 22, 8); R(d, 5, 18, 22, 8)
    L(d, 9, 10, 10, 10); L(d, 9, 22, 10, 22)
    L(d, 14, 10, 24, 10); L(d, 14, 22, 24, 22)


def g_windows(d):  # cửa sổ 4 ô
    R(d, 6, 6, 20, 20)
    L(d, 16, 6, 16, 26); L(d, 6, 16, 26, 16)


def g_key(d):  # chìa khoá (bản quyền)
    E(d, 5, 5, 11, 11)
    L(d, 14, 14, 27, 27)
    L(d, 19, 19, 23, 15); L(d, 23, 23, 27, 19)


def g_driver(d):  # vòng sao lưu/khôi phục quanh đĩa
    A(d, 5, 5, 22, 22, 200, 340)
    A(d, 5, 5, 22, 22, 20, 160)
    L(d, 26, 8, 26, 15); L(d, 26, 15, 19, 15)
    L(d, 6, 24, 6, 17); L(d, 6, 17, 13, 17)
    E(d, 12, 12, 8, 8)


def g_usb(d):  # USB boot
    R(d, 12, 5, 8, 8)
    L(d, 16, 13, 16, 17)
    R(d, 10, 17, 12, 12)
    L(d, 16, 22, 16, 24)


def g_auto(d):  # cửa sổ + mũi tên tải xuống (cài tự động)
    R(d, 5, 5, 22, 22)
    L(d, 5, 11, 27, 11)
    L(d, 16, 15, 16, 23)
    PL(d, [(12, 19), (16, 23), (20, 19)])


def g_winpe(d):  # cửa sổ terminal trong WinPE
    R(d, 4, 6, 24, 20)
    L(d, 4, 11, 28, 11)
    PL(d, [(9, 16), (14, 20), (9, 24)])
    L(d, 17, 24, 24, 24)


def _doc_outline(d):
    PL(d, [(5, 3), (17, 3), (23, 8), (23, 24), (5, 24), (5, 3)])
    L(d, 17, 3, 17, 8); L(d, 17, 8, 23, 8)


def g_office_online(d):  # tài liệu + mũi tên xuống (tải qua mạng)
    _doc_outline(d)
    L(d, 10, 13, 18, 13)
    L(d, 14, 15, 14, 21)
    PL(d, [(11, 18), (14, 21), (17, 18)])


def g_office_offline(d):  # tài liệu + đĩa cài
    _doc_outline(d)
    L(d, 9, 13, 16, 13); L(d, 9, 17, 16, 17)
    E(d, 17, 18, 11, 11)
    E(d, 21, 22, 3, 3)


def g_activate(d):  # chìa khoá + dấu kiểm (kích hoạt)
    E(d, 4, 4, 10, 10)
    L(d, 12, 12, 21, 21)
    L(d, 17, 17, 21, 13)
    PL(d, [(17, 25), (20, 28), (28, 20)])


def g_package(d):  # hộp phần mềm
    R(d, 6, 11, 20, 17)
    PL(d, [(6, 11), (16, 5), (26, 11)])
    L(d, 16, 11, 16, 19)
    L(d, 6, 19, 26, 19)


def g_backup_data(d):  # thư mục + mũi tên lên (sao lưu)
    P(d, [(4, 10), (13, 10), (16, 13), (28, 13), (28, 27), (4, 27)])
    L(d, 16, 24, 16, 16)
    PL(d, [(12, 20), (16, 16), (20, 20)])


def g_wifi(d):  # sóng wifi
    A(d, 4, 10, 24, 24, 205, 335)
    A(d, 8, 15, 16, 16, 205, 335)
    d.ellipse([14, 23, 18, 27], fill=INK)


def g_globe(d):  # quả địa cầu (trình duyệt)
    E(d, 6, 6, 20, 20)
    E(d, 12, 6, 8, 20)
    L(d, 6, 16, 26, 16)


def g_chat(d):  # bong bóng chat
    d.rounded_rectangle([5, 5, 27, 21], radius=3, outline=INK, width=W)
    P(d, [(10, 21), (10, 27), (17, 21)])
    L(d, 10, 11, 22, 11); L(d, 10, 15, 18, 15)


def g_gauge(d):  # đồng hồ kiểm tra (check/test)
    A(d, 5, 7, 22, 22, 180, 360)
    L(d, 5, 18, 27, 18)
    L(d, 16, 18, 22, 10)
    E(d, 14, 16, 4, 4)
    L(d, 8, 12, 10, 14); L(d, 24, 12, 22, 14)


def g_keyboard(d):  # bàn phím (BIOS)
    R(d, 3, 9, 26, 14)
    for x in (7, 12, 17, 22):
        L(d, x, 13, x + 1, 13)
    for x in (9, 14, 19):
        L(d, x, 16, x + 1, 16)
    L(d, 10, 20, 22, 20)


def g_printer(d):  # máy in
    R(d, 6, 13, 20, 9)
    R(d, 11, 5, 10, 8)
    R(d, 11, 19, 10, 9)
    L(d, 23, 16, 24, 16)


def g_printer_fix(d):  # máy in + dấu chấm than (sửa lỗi in)
    R(d, 4, 13, 18, 9)
    R(d, 8, 5, 10, 8)
    R(d, 8, 19, 10, 7)
    L(d, 26, 12, 26, 20)
    L(d, 26, 24, 26, 25)


def g_share(d):  # nút mạng chia sẻ
    E(d, 13, 13, 6, 6)
    E(d, 3, 3, 6, 6); E(d, 23, 3, 6, 6); E(d, 13, 23, 6, 6)
    L(d, 8, 8, 13, 14); L(d, 24, 8, 19, 14); L(d, 16, 19, 16, 23)


def g_folder(d):  # thư mục quản lý file
    P(d, [(4, 10), (13, 10), (16, 13), (28, 13), (28, 27), (4, 27)])
    L(d, 9, 18, 23, 18); L(d, 9, 22, 19, 22)


def g_swap(d):  # hai cửa sổ + mũi tên hoán đổi (chuyển OS)
    R(d, 3, 5, 14, 12)
    R(d, 15, 15, 14, 12)
    L(d, 19, 8, 27, 8); PL(d, [(24, 5), (27, 8), (24, 11)])
    L(d, 13, 24, 5, 24); PL(d, [(8, 21), (5, 24), (8, 27)])


def g_multi_chat(d):  # hai bong bóng chat chồng nhau
    d.rounded_rectangle([10, 3, 29, 16], radius=3, outline=INK, width=W)
    P(d, [(23, 16), (23, 21), (17, 16)])
    d.rounded_rectangle([3, 14, 21, 26], radius=3, outline=INK, width=W)
    P(d, [(8, 26), (8, 30), (13, 26)])
    L(d, 7, 20, 17, 20)


GLYPHS = {
    # 9 glyph gốc (toạ độ giữ nguyên từ draw-icons.ps1)
    "unlock": g_unlock, "cpu": g_cpu, "disk": g_disk, "sliders": g_sliders,
    "document": g_document, "shield": g_shield, "refresh": g_refresh,
    "trash": g_trash, "app-minus": g_app_minus,
    # 23 glyph mới, cùng ngôn ngữ
    "irst": g_irst, "windows": g_windows, "key": g_key, "driver": g_driver,
    "usb": g_usb, "auto": g_auto, "winpe": g_winpe,
    "office-online": g_office_online, "office-offline": g_office_offline,
    "activate": g_activate, "package": g_package, "backup-data": g_backup_data,
    "wifi": g_wifi, "globe": g_globe, "chat": g_chat, "gauge": g_gauge,
    "keyboard": g_keyboard, "printer": g_printer, "printer-fix": g_printer_fix,
    "share": g_share, "folder": g_folder, "swap": g_swap, "multi-chat": g_multi_chat,
}


def build_app_icon():
    """qsu.ico: nền bo góc thép xanh, glyph cửa sổ 4 ô trắng ở giữa."""
    sizes = [16, 24, 32, 48]
    base = None
    for s in sizes:
        scale = s / 32.0
        img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        d.rounded_rectangle([0, 0, s - 1, s - 1], radius=max(2, int(6 * scale)),
                            fill=INK)
        wht = (255, 255, 255, 255)
        lw = max(1, int(2 * scale))
        m = 8 * scale
        d.rectangle([m, m, s - 1 - m, s - 1 - m], outline=wht, width=lw)
        mid = s / 2.0
        d.line([mid, m, mid, s - 1 - m], fill=wht, width=lw)
        d.line([m, mid, s - 1 - m, mid], fill=wht, width=lw)
        if base is None:
            base = img
        else:
            base = base if s == 48 else base
        img.save(f"/tmp/qsu-icon-{s}.png")
    ico_img = Image.open("/tmp/qsu-icon-48.png")
    os.makedirs(COMMON_ASSETS, exist_ok=True)
    ico_img.save(os.path.join(COMMON_ASSETS, "qsu.ico"), format="ICO",
                 sizes=[(16, 16), (24, 24), (32, 32), (48, 48)])


def main():
    os.makedirs(ASSETS, exist_ok=True)
    tiles = []
    for name, fn in GLYPHS.items():
        img, d = canvas()
        fn(d)
        img.save(os.path.join(ASSETS, f"{name}.bmp"), format="BMP")
        tiles.append((name, img))
    # Bảng đối chiếu: phóng 5x, nền xám để thấy cả glyph lẫn khoá đen.
    cols = 8
    rows = (len(tiles) + cols - 1) // cols
    sheet = Image.new("RGBA", (cols * 36, rows * 36), (90, 90, 90, 255))
    for i, (_, img) in enumerate(tiles):
        big = img.resize((32, 32), Image.NEAREST)
        sheet.paste(big, ((i % cols) * 36 + 2, (i // cols) * 36 + 2), big)
    sheet = sheet.resize((sheet.width * 3, sheet.height * 3), Image.NEAREST)
    sheet.save("/tmp/qsu-icons-contact.png")
    build_app_icon()
    print(f"Đã sinh {len(tiles)} BMP + qsu.ico")


if __name__ == "__main__":
    main()
