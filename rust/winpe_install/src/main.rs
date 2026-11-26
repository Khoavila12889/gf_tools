//! WinPEInstall — engine cài mới chạy trong WinPE (đặc tả §6 bước 4).
//!
//! Được startnet.cmd trong WinPE gọi tự động. Toàn bộ nhận diện ổ đĩa
//! dựa trên **số serial của phân vùng + file marker**, tuyệt đối không
//! dựa vào ký tự ổ đĩa (trong WinPE ký tự thay đổi mỗi lần boot).
//!
//! An toàn (chặn ở tầng code, không chỉ giao diện):
//! - Chỉ chạy khi tìm thấy ổ Data có marker `.qsu_data_drive` kèm
//!   stage.json hợp lệ.
//! - Ổ OS đích phải khớp serial wizard đã ghi; ổ chứa marker Data bị
//!   từ chối tuyệt đối ở mọi thao tác ghi/format.
//! - Chế độ "wipeDisk" (§6b): đích là cả ĐĨA VẬT LÝ, nhận diện bằng
//!   serial phần cứng trong stage.json; engine đối chiếu lại serial
//!   thật + xác minh đĩa đích không chứa ổ Data ngay trước khi chạy
//!   diskpart chia lại đĩa. stage.json cũ không có `partitionMode`
//!   mặc định là "preserve" (hành vi cũ).
//! - Chế độ "rebuild" (v0.7.13): chỉ xoá các phân vùng hệ thống liệt
//!   kê trong `rebuildPartitions` của stage.json trên đĩa đích — đĩa
//!   đích ĐƯỢC PHÉP trùng đĩa chứa ổ Data/staging (máy 1 đĩa), phân
//!   vùng staging được bảo vệ bằng đối chiếu extent; mọi đối chiếu
//!   (GUID/offset, liên tục, đủ chỗ) chạy trước khi xoá, lệch là
//!   dừng cứng với exit code 13, không đụng vào đĩa.
//! - Mọi bước ghi log ra ổ Data; lỗi ở bất kỳ bước nào cũng dừng lại,
//!   KHÔNG reboot vào trạng thái dở dang (trừ khi đã bcdboot xong).
//! - Hiển thị tiến trình (v0.7.18, xem `progress.rs`): console của
//!   launcher là bề mặt chính (mọi dòng log in thẳng ra console,
//!   phần trăm DISM cập nhật tại chỗ bằng `\r`), cộng thêm màn
//!   hình tiến trình toàn màn hình tông xanh mở trước mọi công
//!   việc. Khi lỗi, engine NEO LẠI trong WinPE với lỗi trên cả
//!   hai bề mặt thay vì thoát để launcher reboot lặng lẽ.

mod progress;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use progress::ProgressUi;
use qsu_common::cmd;
use qsu_common::disk;
use qsu_common::log::Logger;
use qsu_common::stage::StageConfig;
use qsu_common::sys::{self, DriveInfo};

const DATA_MARKER: &str = ".qsu_data_drive";

/// Ngữ cảnh thông báo kết thúc ca cài (v0.7.27): `run()` nạp
/// vào đây ngay sau khi đọc được stage.json + tạo logger (chỉ
/// khi stage.json mang token dùng một lần của relay). Mọi lối
/// ra LỖI của `run()` đều dồn về `main()`, nên `main()` dùng
/// ngữ cảnh này để gửi tin LỖI trước khi neo màn hình; tin CÀI
/// XONG do `run()` tự gửi ngay trước lệnh reboot (sau reboot
/// thì không gửi kịp nữa). Token không bao giờ vào log.
#[derive(Clone)]
struct NotifyCtx {
    relay_url: String,
    once_token: String,
    machine: String,
    log_path: PathBuf,
}

static NOTIFY_CTX: Mutex<Option<NotifyCtx>> = Mutex::new(None);

/// Gửi tin kết thúc (CÀI XONG / LỖI) qua relay Zalo bằng token
/// dùng một lần trong stage.json. Best-effort tuyệt đối: chưa
/// cấu hình thì bỏ qua lặng lẽ; gửi lỗi chỉ ghi log, không đổi
/// mã thoát, không chặn reboot hay màn hình giữ lỗi.
fn notify_engine_end(
    log: &Arc<Logger>,
    relay_url: &str,
    once_token: &str,
    machine: &str,
    ok: bool,
    step: &str,
) {
    if relay_url.is_empty() || once_token.is_empty() {
        return;
    }
    let text = if ok {
        qsu_common::notify::text_done(machine)
    } else {
        qsu_common::notify::text_error(machine, step)
    };
    match qsu_common::notify::relay_send_once(relay_url, once_token, &text) {
        Ok(()) => log.line("Đã gửi thông báo trạng thái qua relay Zalo."),
        Err(e) => log.line(&format!(
            "CẢNH BÁO: gửi thông báo qua relay Zalo thất bại (bỏ qua): {}",
            e
        )),
    }
}

fn volume_serial(letter: char) -> Option<u32> {
    sys::list_fixed_drives()
        .into_iter()
        .find(|d| d.letter == letter)
        .map(|d| d.serial)
}

/// Quét mọi ký tự ổ đĩa tìm thư mục staging chứa stage.json trên ổ có marker
fn find_stage(staging_name: &str) -> Option<(char, PathBuf, StageConfig)> {
    for letter in "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars() {
        let root = format!("{}:\\", letter);
        let marker = Path::new(&root).join(DATA_MARKER);
        let stage_file = Path::new(&root).join(staging_name).join("stage.json");
        if marker.exists() && stage_file.exists() {
            if let Ok(cfg) = StageConfig::load(&stage_file) {
                return Some((letter, Path::new(&root).join(staging_name), cfg));
            }
        }
    }
    None
}

fn find_os_drive(cfg: &StageConfig, data_letter: char, drives: &[DriveInfo]) -> Option<char> {
    // Ưu tiên khớp serial tuyệt đối
    if cfg.os_volume_serial != 0 {
        let hit = drives.iter().find(|d| d.serial as u64 == cfg.os_volume_serial);
        if let Some(d) = hit {
            if d.letter != data_letter {
                return Some(d.letter);
            }
        }
        return None; // serial không khớp -> không đoán bừa (an toàn)
    }
    // Không có dự phòng theo ký tự ổ/thư mục Windows: serial là căn cứ
    // duy nhất được phép (đặc tả §6). stage.json thiếu serial (bằng 0)
    // hoặc serial không khớp bất kỳ phân vùng nào -> dừng an toàn.
    None
}
/// Copy đệ quy một thư mục (dùng cho payload `custom` của người dùng).
/// Trả số file đã copy. Nội dung chỉ được copy nguyên trạng — không
/// bao giờ được diễn giải hay thực thi bởi bộ công cụ.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<u64> {
    std::fs::create_dir_all(dst)?;
    let mut count = 0u64;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_dir() {
            count += copy_dir_recursive(&from, &to)?;
        } else if ft.is_file() {
            std::fs::copy(&from, &to)?;
            count += 1;
        }
    }
    Ok(count)
}

fn format_os_partition(log: &Arc<Logger>, letter: char) -> bool {
    // Cách 1: format.com (có sẵn trong boot.wim của bộ cài Windows)
    let target = format!("{}:", letter);
    match cmd::run_logged(log, "format", &[&target, "/fs:NTFS", "/q", "/y", "/v:Windows"]) {
        Ok(out) if out.code == Some(0) => return true,
        Ok(out) => log.line(&format!("format.com trả mã {:?} — thử diskpart", out.code)),
        Err(e) => log.line(&format!("không chạy được format.com: {} — thử diskpart", e)),
    }
    // Cách 2: diskpart theo ký tự ổ đĩa
    let script = format!("select volume {}\nformat fs=ntfs quick label=\"Windows\"\n", letter);
    let script_path = std::env::temp_dir().join("qsu_diskpart.txt");
    if std::fs::write(&script_path, &script).is_err() {
        return false;
    }
    let sp = script_path.to_string_lossy().into_owned();
    match cmd::run_logged(log, "diskpart", &["/s", &sp]) {
        Ok(out) => out.code == Some(0),
        Err(_) => false,
    }
}

/// (Đã gỡ ở v0.7.19) Hàm find_efi_letter() cũ — quét các ổ ĐÃ CÓ
/// ký tự để tìm thư mục \EFI ở gốc, dự phòng mountvol /s — không
/// thấy được phân vùng EFI chưa có ký tự ổ và có thể trỏ nhầm
/// phân vùng EFI của đĩa khác (rủi ro R1; ca thực địa
/// 2026-11-18 ở module WinPE). Bước chốt boot của đường
/// preserve nay dùng chung `disk::finalize_boot()` trong
/// qsu_common: tìm phân vùng boot TRÊN CÙNG ĐĨA với phân vùng
/// Windows theo loại phân vùng, tự gán ký tự nếu thiếu, kiểm
/// chứng file boot thật sau bcdboot.

/// Trích phần trăm từ một segment output của DISM (dạng
/// "[==== 82.0% ====]" hoặc "82.0%") — None nếu segment không mang
/// phần trăm hợp lệ 0..=100.
fn parse_dism_percent(seg: &str) -> Option<u32> {
    let pct_pos = seg.rfind('%')?;
    let before = &seg[..pct_pos];
    let mut num: Vec<char> = Vec::new();
    for c in before.chars().rev() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
        } else if !num.is_empty() {
            break;
        }
    }
    if num.is_empty() {
        return None;
    }
    let num: String = num.into_iter().rev().collect();
    let v: f32 = num.parse().ok()?;
    if (0.0..=100.0).contains(&v) {
        Some(v as u32)
    } else {
        None
    }
}

fn main() {
    // CONSOLE LÀ BỀ MẶT TIẾN TRÌNH CHÍNH (v0.7.18): engine được
    // qsu_start.cmd chạy thẳng trong console của launcher — bảo
    // đảm stdout bám đúng console đó trước khi in bất cứ chữ nào.
    progress::ensure_console();
    // Màn hình tiến trình toàn màn hình mở TRƯỚC mọi công việc (kể
    // cả trước khi quét stage) để màn hình không bao giờ trống;
    // đây là bề mặt phụ — mọi lỗi GUI đều bị nuốt trong luồng giao
    // diện, không ảnh hưởng luồng cài đặt hay dòng console.
    let ui = ProgressUi::start();
    ui.set_phase("Đang khởi động engine cài đặt", 0);
    progress::mirror_line("=== QSU WinPEInstall v0.7.28 — engine cài đặt trong WinPE ===");
    let code = run(&ui);
    if code == 0 {
        std::process::exit(0);
    }
    // Tin LỖI qua relay Zalo (v0.7.27): mọi lối ra lỗi của run()
    // đều dồn về đây. Nếu run() đã nạp ngữ cảnh thông báo (stage
    // có token dùng một lần), gửi một tin LỖI — bước dừng lấy từ
    // pha hiện tại của màn hình tiến trình — bằng một Logger phụ
    // mở nối tiếp cùng file log. Best-effort: gửi lỗi không đổi
    // hành vi neo màn hình ngay dưới đây.
    if let Some(ctx) = NOTIFY_CTX.lock().ok().and_then(|g| g.clone()) {
        let log2 = Arc::new(Logger::new(ctx.log_path, None));
        notify_engine_end(
            &log2,
            &ctx.relay_url,
            &ctx.once_token,
            &ctx.machine,
            false,
            &ui.phase_text(),
        );
    }
    // LỖI: giữ nguyên trạng thái cuối trên CẢ màn hình lẫn console
    // và KHÔNG thoát — tiến trình neo lại tại đây nên qsu_start.cmd
    // không chạy tiếp tới `wpeutil reboot`; máy đứng nguyên trong
    // WinPE với lỗi hiển thị cho kỹ thuật viên đọc/chụp, thay vì
    // reboot lặng lẽ như các bản trước.
    ui.mark_failed();
    progress::mirror_line("============================================================");
    progress::mirror_line(&format!("ĐÃ DỪNG tại bước: {}", ui.phase_text()));
    if let Some(err) = ui.error_text() {
        progress::mirror_line(&format!("Lỗi: {}", err));
    }
    if let Some(p) = ui.log_path_text() {
        progress::mirror_line(&format!("Nhật ký chi tiết: {}", p));
    }
    progress::mirror_line("Máy KHÔNG tự khởi động lại. Đọc lỗi ở trên rồi tắt máy bằng nút nguồn.");
    loop {
        std::thread::park();
    }
}

/// Toàn bộ luồng cài đặt cũ, trả mã thoát thay vì gọi
/// `std::process::exit` tại chỗ — để `main` chốt trạng thái hiển
/// thị cuối khi lỗi. Thứ tự/bước đi không đổi so với trước.
fn run(ui: &ProgressUi) -> i32 {
    let staging_name = std::env::args().nth(1).unwrap_or_else(|| "QSU-Stage".to_string());

    ui.set_phase("Đang quét đĩa & đọc thông tin cài đặt", 2);
    // Tìm staging trước khi có log (log nằm trên ổ Data)
    let (data_letter, stage_dir, cfg) = match find_stage(&staging_name) {
        Some(v) => v,
        None => {
            let msg = "Không tìm thấy stage.json trên ổ Data có marker .qsu_data_drive — wizard chưa staging hoặc ổ Data không gắn. Dừng.";
            ui.note_error(msg);
            progress::mirror_line(&format!("LỖI: {}", msg));
            eprintln!("QSU WinPEInstall: {}", msg);
            return 2;
        }
    };
    let data_root = format!("{}:\\", data_letter);
    let logs_dir = Path::new(&data_root).join(&cfg.logs_folder);
    let log_path = logs_dir.join(format!("winpe-{}.log", qsu_common::log::log_stamp()));
    ui.set_log_path(&log_path.to_string_lossy());
    // Gương console + bắt lỗi cho màn hình tiến trình (v0.7.18):
    // MỌI dòng log đồng thời in thẳng ra console của launcher (bề
    // mặt tiến trình chính — trước đây engine chỉ ghi file nên
    // console trống trơn dù engine đang chạy khoẻ), và dòng
    // "LỖI..." gần nhất được giữ lại để trạng thái kết thúc hiển
    // thị đúng lỗi đã dừng ca cài.
    let ui_sink = ui.clone();
    let log = Arc::new(Logger::new(
        log_path,
        Some(Box::new(move |full: &str| {
            progress::mirror_line(full);
            let msg = full.splitn(2, "] ").nth(1).unwrap_or(full);
            if msg.starts_with("LỖI") {
                ui_sink.note_error(msg);
            }
        })),
    ));
    log.line("=== WinPEInstall bắt đầu ===");
    log.line(&format!("Ổ Data: {} (serial stage ghi: {})", data_letter, cfg.data_volume_serial));
    log.line(&format!(
        "Image: {} index {} ({})",
        cfg.image_file, cfg.image_index, cfg.image_name
    ));
    // Nạp ngữ cảnh thông báo kết thúc cho main() (v0.7.27): chỉ
    // khi stage.json mang token dùng một lần (wizard đã xin từ
    // relay lúc staging). Từ đây, bất kỳ lối ra lỗi nào của run()
    // cũng được main() gửi tin LỖI qua relay trước khi neo màn
    // hình; lối ra thành công tự gửi tin CÀI XONG ở cuối run().
    if !cfg.notify_relay_url.is_empty() && !cfg.notify_once_token.is_empty() {
        if let Ok(mut g) = NOTIFY_CTX.lock() {
            *g = Some(NotifyCtx {
                relay_url: cfg.notify_relay_url.clone(),
                once_token: cfg.notify_once_token.clone(),
                machine: cfg.computer_name.clone(),
                log_path: log.path().clone(),
            });
        }
        log.line("Thông báo trạng thái: ĐÃ BẬT qua relay Zalo (token dùng một lần trong stage.json).");
    }

    let drives = sys::list_fixed_drives();
    // Đối chiếu serial ổ Data
    if cfg.data_volume_serial != 0 {
        let actual = volume_serial(data_letter).unwrap_or(0) as u64;
        if actual != cfg.data_volume_serial {
            log.line(&format!(
                "LỖI AN TOÀN: serial ổ Data thực tế {} != stage {}. Dừng.",
                actual, cfg.data_volume_serial
            ));
            return 3;
        }
    }

    // --- Chế độ phân vùng: preserve (mặc định) / wipeDisk (§6b) /
    // rebuild (v0.7.13) ---
    let uefi = cfg.boot_mode.eq_ignore_ascii_case("UEFI");
    let wipe_mode = cfg.partition_mode.eq_ignore_ascii_case("wipeDisk");
    let rebuild_mode = cfg.partition_mode.eq_ignore_ascii_case("rebuild");
    log.line(&format!(
        "Chế độ phân vùng: {}",
        if wipe_mode {
            "wipeDisk — xoá toàn bộ ổ đĩa đích, chia lại từ đầu"
        } else if rebuild_mode {
            "rebuild — chỉ xoá các phân vùng hệ thống đã chọn trên đĩa đích, giữ nguyên phân vùng data"
        } else {
            "preserve — giữ cấu trúc phân vùng, chỉ format phân vùng OS"
        }
    ));
    // Ký tự ổ của layout MỚI (wipeDisk/rebuild vừa chia): bcdboot và
    // WinRE phải trỏ vào các phân vùng này, không dùng phân vùng cũ.
    let mut layout_boot_letter: Option<char> = None;
    let mut layout_recovery_letter: Option<char> = None;

    // --- Nhận diện đích cài ---
    let os_letter = if wipe_mode {
        // WIPE: đích là cả đĩa vật lý — nhận diện bằng serial phần cứng
        if cfg.target_disk_serial.trim().is_empty() {
            log.line("LỖI AN TOÀN: stage.json chọn wipeDisk nhưng thiếu targetDiskSerial. Dừng.");
            return 10;
        }
        let disks = disk::list_physical_disks();
        let target = match disk::find_disk_by_serial(&disks, &cfg.target_disk_serial) {
            Some(d) => d.clone(),
            None => {
                log.line(&format!(
                    "LỖI AN TOÀN: không tìm thấy (hoặc trùng) đĩa vật lý có serial '{}' như stage.json. Dừng, không đụng đĩa nào.",
                    cfg.target_disk_serial
                ));
                return 10;
            }
        };
        // Chốt an toàn tuyệt đối: đĩa đích KHÔNG được chứa ổ Data
        // (nơi có marker + staging + log). Không xác định được ổ Data
        // nằm trên đĩa nào cũng dừng — thà không cài còn hơn wipe nhầm.
        match disk::volume_disk_number(data_letter) {
            Some(n) if n == target.number => {
                log.line(&format!(
                    "LỖI AN TOÀN: đĩa đích wipe (đĩa {}) CHỨA chính ổ Data {}:. Từ chối wipe.",
                    target.number, data_letter
                ));
                return 11;
            }
            Some(_) => {}
            None => {
                log.line("LỖI AN TOÀN: không xác định được ổ Data nằm trên đĩa vật lý nào — không thể chứng minh đĩa đích an toàn. Dừng.");
                return 11;
            }
        }
        log.line(&format!("Đĩa đích wipe: {}", target.summary()));
        log.line(&format!(
            "Các phân vùng trên đĩa đích sắp bị XOÁ TOÀN BỘ:\n{}",
            target.describe_partitions()
        ));
        ui.set_phase("Đang xoá và chia lại phân vùng", 5);
        match disk::execute_wipe(&log, target.number, &target.serial, target.size, uefi) {
            Ok(r) => {
                layout_boot_letter = Some(r.boot_letter);
                layout_recovery_letter = r.recovery_letter;
                r.windows_letter
            }
            Err(e) => {
                log.line(&format!("LỖI: wipe đĩa đích thất bại: {}. Dừng.", e));
                return 12;
            }
        }
    } else if rebuild_mode {
        // REBUILD: đích là đĩa vật lý chứa các phân vùng hệ thống cũ —
        // nhận diện bằng serial phần cứng như wipe, NHƯNG đĩa đích
        // được phép chứa chính ổ Data/staging (đây là ca máy 1 đĩa).
        // An toàn nằm ở execute_rebuild: chỉ xoá đúng các phân vùng
        // đối chiếu được trong rebuildPartitions, sau khi chứng minh
        // phân vùng staging không nằm trong danh sách xoá. Mọi lỗi
        // ở nhánh này đều là exit 13 (dừng trước khi đụng vào đĩa,
        // hoặc execute_rebuild tự dừng trước diskpart).
        if cfg.target_disk_serial.trim().is_empty() {
            log.line("LỖI AN TOÀN: stage.json chọn rebuild nhưng thiếu targetDiskSerial. Dừng (exit 13).");
            return 13;
        }
        if cfg.rebuild_partitions.is_empty() {
            log.line("LỖI AN TOÀN: stage.json chọn rebuild nhưng rebuildPartitions rỗng — không có căn cứ nào để xoá. Dừng (exit 13).");
            return 13;
        }
        let disks = disk::list_physical_disks();
        let target = match disk::find_disk_by_serial(&disks, &cfg.target_disk_serial) {
            Some(d) => d.clone(),
            None => {
                log.line(&format!(
                    "LỖI AN TOÀN: không tìm thấy (hoặc trùng) đĩa vật lý có serial '{}' như stage.json. Dừng, không đụng đĩa nào (exit 13).",
                    cfg.target_disk_serial
                ));
                return 13;
            }
        };
        // Extent của volume Data: nếu nó nằm trên CHÍNH đĩa đích thì
        // bắt buộc phải xác định được vị trí của nó để execute_rebuild
        // chứng minh nó không bị xoá; không xác định được = dừng.
        // Nằm trên đĩa khác thì không cần (hiển nhiên an toàn).
        let staging_extent = match disk::volume_disk_number(data_letter) {
            Some(n) if n == target.number => {
                match disk::volume_disk_extent(data_letter) {
                    Some((_, off, size)) => {
                        log.line(&format!(
                            "Ổ Data {}: nằm trên CHÍNH đĩa đích (đĩa {}) tại offset {} — chế độ rebuild cho phép, phân vùng này sẽ được bảo vệ.",
                            data_letter, target.number, off
                        ));
                        Some((off, size))
                    }
                    None => {
                        log.line("LỖI AN TOÀN: ổ Data nằm trên đĩa đích nhưng không đọc được extent của nó — không chứng minh được an toàn. Dừng (exit 13).");
                        return 13;
                    }
                }
            }
            Some(n) => {
                log.line(&format!(
                    "Ổ Data {}: nằm trên đĩa {} (khác đĩa đích {}) — không thuộc vùng rebuild.",
                    data_letter, n, target.number
                ));
                None
            }
            None => {
                log.line("LỖI AN TOÀN: không xác định được ổ Data nằm trên đĩa vật lý nào. Dừng (exit 13).");
                return 13;
            }
        };
        log.line(&format!("Đĩa đích rebuild: {}", target.summary()));
        log.line("Các phân vùng hệ thống sắp bị XOÁ (theo stage.json):");
        for e in &cfg.rebuild_partitions {
            log.line(&format!(
                "  - {} — {} — offset {}",
                e.kind,
                sys::format_gb(e.size_bytes),
                e.offset_bytes
            ));
        }
        log.line(&format!(
            "Các phân vùng KHÁC trên đĩa đích được GIỮ NGUYÊN:\n{}",
            target
                .partitions
                .iter()
                .filter(|p| !cfg
                    .rebuild_partitions
                    .iter()
                    .any(|e| p.offset.abs_diff(e.offset_bytes) <= 1_048_576
                        && p.size.abs_diff(e.size_bytes) <= 1_048_576))
                .map(|p| format!("  - {}", p.describe()))
                .collect::<Vec<_>>()
                .join("\n")
        ));
        ui.set_phase("Đang xoá và chia lại phân vùng hệ thống", 5);
        match disk::execute_rebuild(
            &log,
            target.number,
            &target.serial,
            uefi,
            &cfg.rebuild_partitions,
            staging_extent,
        ) {
            Ok(r) => {
                layout_boot_letter = Some(r.boot_letter);
                layout_recovery_letter = r.recovery_letter;
                r.windows_letter
            }
            Err(e) => {
                log.line(&format!(
                    "LỖI: rebuild đĩa đích bị chặn: {}. Dừng (exit 13) — nếu lỗi xảy ra ở bước đối chiếu/kiểm tra thì đĩa chưa bị đụng vào; xem chi tiết phía trên.",
                    e
                ));
                return 13;
            }
        }
    } else {
        match find_os_drive(&cfg, data_letter, &drives) {
            Some(l) => l,
            None => {
                log.line("LỖI: không xác định được phân vùng OS theo serial trong stage.json. Dừng, không format gì.");
                return 4;
            }
        }
    };
    if os_letter == data_letter
        || Path::new(&format!("{}:\\{}", os_letter, DATA_MARKER)).exists()
    {
        log.line("LỖI AN TOÀN: đích cài trùng ổ Data. Dừng.");
        return 5;
    }
    let os_root = format!("{}:\\", os_letter);
    log.line(&format!("Ổ OS đích: {}", os_letter));
    ui.set_detail(&format!(
        "Đích cài: ổ {}: — ảnh {} ({})",
        os_letter, cfg.image_file, cfg.image_name
    ));

    // --- Kiểm tra image ---
    let image_path = stage_dir.join(&cfg.image_file);
    if !image_path.exists() {
        log.line(&format!("LỖI: không thấy file image {}", image_path.display()));
        return 6;
    }

    // --- Format phân vùng OS (wipeDisk/rebuild đã format khi chia lại) ---
    if wipe_mode || rebuild_mode {
        log.line("Chế độ chia lại phân vùng: phân vùng Windows đã được format trong script diskpart — bỏ qua bước format riêng.");
    } else {
        ui.set_phase("Đang format phân vùng hệ điều hành", 8);
        log.line("Đang format phân vùng OS (NTFS, quick)...");
        if !format_os_partition(&log, os_letter) {
            log.line("LỖI: format thất bại. Dừng.");
            return 7;
        }
    }

    // --- Apply image bằng DISM (theo dõi tiến độ theo dòng) ---
    let dism = {
        let sr = std::env::var("SystemRoot").unwrap_or_else(|_| "X:\\Windows".to_string());
        let p = Path::new(&sr).join("System32").join("dism.exe");
        if p.exists() {
            p.to_string_lossy().into_owned()
        } else {
            "dism".to_string()
        }
    };
    let image_arg = format!("/ImageFile:{}", image_path.to_string_lossy());
    let index_arg = format!("/Index:{}", cfg.image_index);
    let apply_arg = format!("/ApplyDir:{}", os_root);
    let mut args: Vec<&str> = vec!["/Apply-Image", &image_arg, &index_arg, &apply_arg];
    let swm_arg;
    if !cfg.image_swm.is_empty() {
        let swm_path = stage_dir.join(&cfg.image_swm);
        swm_arg = format!("/SWMFile:{}", swm_path.to_string_lossy());
        args.push(&swm_arg);
    }
    log.line(&format!("$ {} {}", dism, args.join(" ")));
    ui.set_phase(
        &format!("Đang bung ảnh: {}", cfg.image_name),
        progress::APPLY_BASE_PCT,
    );
    let mut last_pct = String::new();
    let mut last_shown_pct: i32 = -1;
    let mut last_logged_pct: i32 = -1;
    // run_streaming_progress cắt output theo cả \r — bắt buộc với
    // DISM vì thanh tiến độ của nó kết thúc bằng \r chứ không phải
    // \n; bộ đọc theo dòng cũ giữ mọi phần trăm trong buffer tới
    // khi DISM xong mới nhả (v0.7.18).
    let apply_res = cmd::run_streaming_progress(&dism, &args, |seg| {
        let t = seg.trim();
        if t.is_empty() {
            return;
        }
        match parse_dism_percent(t) {
            Some(p) => {
                last_pct = t.to_string();
                let pi = p as i32;
                if pi != last_shown_pct {
                    last_shown_pct = pi;
                    ui.set_apply_progress(p, &cfg.image_name);
                    progress::mirror_progress(&format!(
                        "Đang bung ảnh: {} — {}%",
                        cfg.image_name, pi
                    ));
                }
                // File log chỉ ghi mốc 10% để khỏi ngập (console và
                // màn hình vẫn nhận mọi phần trăm qua mirror ở trên).
                if pi >= last_logged_pct + 10 || pi == 100 {
                    last_logged_pct = pi;
                    log.line(&format!("  | {}", t));
                }
            }
            None => {
                log.line(&format!("  | {}", t));
            }
        }
    });
    match apply_res {
        Ok(Some(0)) => log.line("Apply-Image hoàn tất."),
        other => {
            log.line(&format!("LỖI: Apply-Image thất bại ({:?}), tiến độ cuối: {}", other, last_pct));
            return 8;
        }
    }

    // --- Inject driver offline (IRST/NVMe...) nếu staging có ---
    ui.set_phase("Đang cài driver", 78);
    // Hồ sơ CORE (v0.7.22): bỏ qua CẢ HAI đường inject driver —
    // ảnh rút gọn giữ đúng bộ driver có sẵn trong ảnh như đường
    // cài tay WinNTSetup. Nghi vấn thực địa 2026-11-20: nạp trọn
    // bộ driver của bản Windows đầy đủ (bản sao lưu pnputil) vào
    // ảnh đã cắt gọt làm một service/driver boot-start chết ở lần
    // boot đầu → CRITICAL_PROCESS_DIED (0xEF).
    let core_profile = cfg.image_profile.eq_ignore_ascii_case("core");
    // [Cơ chế] (v0.7.24): ghi rõ hồ sơ engine đang thực thi theo
    // stage.json — log phải tự chứng minh ca chạy đi hồ sơ nào
    // (hai ca test v0.7.21/v0.7.22 chạy nhầm STANDARD vì nút
    // Core ở wizard chưa BẬT).
    log.line(&format!("[Cơ chế] HỒ SƠ TRIỂN KHAI: stage.json imageProfile='{}' → {}.", cfg.image_profile, if core_profile { "CORE (ảnh rút gọn)" } else { "STANDARD (đầy đủ)" }));
    if core_profile {
        log.line("[Cơ chế] Hồ sơ CORE (v0.7.22): BỎ QUA inject driver offline (cả driver staging lẫn driver đã sao lưu) — giữ đúng bộ driver trong ảnh như đường cài WinNTSetup.");
    }
    let drivers_dir = stage_dir.join("Drivers");
    if !core_profile
        && drivers_dir.exists()
        && std::fs::read_dir(&drivers_dir).map(|mut d| d.next().is_some()).unwrap_or(false)
    {
        let img_arg = format!("/Image:{}", os_root);
        let drv_arg = format!("/Driver:{}", drivers_dir.to_string_lossy());
        log.line("Đang inject driver offline...");
        let _ = cmd::run_logged(&log, &dism, &[&img_arg, "/Add-Driver", &drv_arg, "/Recurse"]);
    } else if !core_profile {
        log.line("Staging không có thư mục Drivers — bỏ qua inject driver.");
    }

    // --- Inject driver đã sao lưu (QSU-DriverBackup) nếu có (v0.7.14) ---
    // Wizard sao lưu driver của Windows hiện tại bằng pnputil
    // /export-driver vào thư mục này trên ổ Data và ghi tên tương đối
    // vào stage.json (driverBackupRel), nhưng các bản <= v0.7.13 engine
    // chỉ inject từ staging\Drivers nên driver của chính máy đó không
    // bao giờ được nạp lại. Chỉ inject khi tên tương đối an toàn
    // (không tuyệt đối, không "..", không ký tự ổ đĩa) và thư mục tồn
    // tại, khác staging\Drivers, và không rỗng — tránh DISM /Recurse
    // quét nhầm cả ổ Data.
    let backup_rel = cfg.driver_backup_rel.trim();
    let backup_safe = !core_profile
        && !backup_rel.is_empty()
        && !backup_rel.contains("..")
        && !backup_rel.contains(':')
        && !backup_rel.starts_with('\\')
        && !backup_rel.starts_with('/');
    if backup_safe {
        let backup_dir = Path::new(&data_root).join(backup_rel);
        if backup_dir != drivers_dir
            && backup_dir.is_dir()
            && std::fs::read_dir(&backup_dir).map(|mut d| d.next().is_some()).unwrap_or(false)
        {
            let img_arg = format!("/Image:{}", os_root);
            let drv_arg = format!("/Driver:{}", backup_dir.to_string_lossy());
            log.line(&format!(
                "Đang inject driver đã sao lưu từ {}...",
                backup_dir.to_string_lossy()
            ));
            let _ = cmd::run_logged(&log, &dism, &[&img_arg, "/Add-Driver", &drv_arg, "/Recurse"]);
        } else {
            log.line("Không có thư mục driver đã sao lưu để inject — bỏ qua.");
        }
    }

    // --- Unattend + agent hậu cài vào bản Windows mới ---
    ui.set_phase("Đang chuẩn bị cấu hình khởi động đầu tiên", 84);
    let panther = Path::new(&os_root).join("Windows").join("Panther");
    if let Err(e) = std::fs::create_dir_all(&panther) {
        log.line(&format!("LỖI: Không tạo được thư mục Panther: {}", e));
        return 12;
    }
    if core_profile {
        // Hồ sơ CORE (v0.7.23 → v0.7.26): chép autounattend
        // CHỈ-pha-OOBE của wizard (nếu có trong staging) vào
        // Panther để ca cài vẫn TỰ ĐỘNG qua màn hình thiết lập +
        // tạo tài khoản + AutoLogon; ngoài ra giữ đúng công
        // thức WinNTSetup — KHÔNG agent hậu cài, KHÔNG inject
        // driver, KHÔNG chép WinRE. Staging của v0.7.22 (không
        // có unattend) vẫn chạy được: khi đó cài thuần như
        // WinNTSetup, vào OOBE gốc của ảnh.
        let unattend_src = stage_dir.join("autounattend.xml");
        if unattend_src.exists() {
            for name in ["autounattend.xml", "unattend.xml"] {
                if let Err(e) = std::fs::copy(&unattend_src, panther.join(name)) {
                    log.line(&format!("LỖI: Không chép được autounattend.xml: {}", e));
                    return 12;
                }
            }
            log.line("[Cơ chế] Hồ sơ CORE (v0.7.23): đã chép autounattend (chỉ pha oobeSystem) vào Windows\\Panther — ngoài unattend này: KHÔNG agent hậu cài, KHÔNG inject driver, KHÔNG chép WinRE (như WinNTSetup).");
        } else {
            log.line("[Cơ chế] Hồ sơ CORE (v0.7.23): staging không có autounattend.xml — cài thuần như WinNTSetup (OOBE gốc của ảnh).");
        }
        // SetupComplete.cmd TỐI THIỂU (v0.7.26 — khôi phục cơ
        // chế của v0.7.21 mà v0.7.22 đã bỏ): một lệnh net user
        // dọn cờ "phải đổi mật khẩu ở lần đăng nhập đầu" của
        // tài khoản admin. Windows chạy file này bằng quyền
        // SYSTEM ở cuối pha setup, TRƯỚC lần đăng nhập đầu —
        // nên AutoLogon vào thẳng desktop. Ca thực địa
        // v0.7.25 (2026-11-20) boot qua 0xEF nhưng dừng ở màn
        // hình đổi mật khẩu; chẩn đoán cùng ngày đã chốt 0xEF
        // do entry boot của bcdboot — SetupComplete vô tội.
        // Ảnh đã có sẵn SetupComplete.cmd thì nối thêm dòng,
        // không ghi đè.
        let scripts = Path::new(&os_root).join("Windows").join("Setup").join("Scripts");
        let sc_line = format!("net user {} /logonpasswordchg:no", cfg.admin_user);
        let sc_res = std::fs::create_dir_all(&scripts).and_then(|_| {
            let sc = scripts.join("SetupComplete.cmd");
            if sc.exists() {
                let mut cur = std::fs::read_to_string(&sc).unwrap_or_default();
                if !cur.ends_with('\n') {
                    cur.push_str("\r\n");
                }
                cur.push_str(&sc_line);
                cur.push_str("\r\n");
                std::fs::write(&sc, cur)
            } else {
                std::fs::write(&sc, format!("@echo off\r\n{}\r\n", sc_line))
            }
        });
        if let Err(e) = sc_res {
            log.line(&format!("LỖI: Không ghi được SetupComplete.cmd: {}", e));
            return 12;
        }
        log.line("[Cơ chế] Hồ sơ CORE (v0.7.26): bỏ qua agent hậu cài, đã ghi SetupComplete.cmd tối thiểu (net user <tên> /logonpasswordchg:no) vào Windows\\Setup\\Scripts để dọn cờ 'phải đổi mật khẩu ở lần đăng nhập đầu' trước khi AutoLogon chạy — khôi phục cơ chế v0.7.21 (đã minh oan: 0xEF là do entry boot của bcdboot, không phải SetupComplete).");
    } else {
        let unattend_src = stage_dir.join("autounattend.xml");
        if unattend_src.exists() {
            for name in ["autounattend.xml", "unattend.xml"] {
                if let Err(e) = std::fs::copy(&unattend_src, panther.join(name)) {
                    log.line(&format!("LỖI: Không chép được cấu hình đăng nhập tự động: {}", e));
                    return 12;
                }
            }
            if let Err(e) = qsu_common::post_install::write_to_windows(Path::new(&os_root)) {
                log.line(&format!("LỖI: Không chuẩn bị được tự cài ứng dụng: {}", e));
                return 12;
            }
            log.line("Đã chuẩn bị tự cài UltraViewer, Chrome và 7-Zip sau đăng nhập.");
            let notify_dir = Path::new(&os_root).join("ProgramData").join("QSU");
            if let Err(e) = qsu_common::post_install::copy_telegram_config(&stage_dir, &notify_dir) {
                log.line(&format!("LỖI: Không chép được cấu hình Telegram: {}", e));
                return 12;
            }
            log.line("Đã copy autounattend.xml vào Windows\\Panther.");
        }
    }
    let qsu_dir = Path::new(&os_root).join("ProgramData").join("QSU");
    let _ = std::fs::create_dir_all(&qsu_dir);
    // Payload `custom` do người dùng tự chuẩn bị (wizard chép vào
    // staging\custom): copy nguyên trạng vào ProgramData\QSU\custom của
    // bản Windows mới. Bộ công cụ không diễn giải/không chạy nội dung này;
    // việc kích hoạt script của họ (RunOnce, logon script...) do chính
    // người dùng tự bố trí.
    let custom_src = stage_dir.join("custom");
    if custom_src.is_dir() {
        match copy_dir_recursive(&custom_src, &qsu_dir.join("custom")) {
            Ok(n) => log.line(&format!(
                "Đã copy thư mục custom ({} file) vào ProgramData\\QSU\\custom.",
                n
            )),
            Err(e) => log.line(&format!("CẢNH BÁO: copy thư mục custom thất bại: {}", e)),
        }
    }
    // Agent hậu cài là TUỲ CHỌN: bản rebuild không kèm agent (xem
    // rust/post_install/README.md). Chỉ khi người dùng tự đặt
    // PostInstall.exe vào staging mới copy + đăng ký RunOnce; không có
    // thì bỏ qua lặng lẽ.
    let post_src = stage_dir.join("PostInstall.exe");
    let post_dst = qsu_dir.join("PostInstall.exe");
    if core_profile && post_src.exists() {
        log.line("[Cơ chế] Hồ sơ CORE (v0.7.22): bỏ qua PostInstall.exe trong staging — hồ sơ CORE không đăng ký agent hậu cài.");
    } else if post_src.exists() {
        let _ = std::fs::copy(&post_src, &post_dst);
        log.line("Đã copy PostInstall.exe vào ProgramData\\QSU.");
        // Đăng ký RunOnce trong SOFTWARE hive offline của bản Windows mới
        let hive = Path::new(&os_root).join("Windows").join("System32").join("config").join("SOFTWARE");
        let hive_s = hive.to_string_lossy().into_owned();
        if cmd::run_logged(&log, "reg", &["load", "HKLM\\QSU_OFFLINE", &hive_s])
            .map(|o| o.code == Some(0))
            .unwrap_or(false)
        {
            let _ = cmd::run_logged(
                &log,
                "reg",
                &[
                    "add",
                    "HKLM\\QSU_OFFLINE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon",
                    "/v",
                    "QSUFirstLogon",
                    "/t",
                    "REG_SZ",
                    "/d",
                    "1",
                    "/f",
                ],
            );
            let runonce_key =
                "HKLM\\QSU_OFFLINE\\Microsoft\\Windows\\CurrentVersion\\RunOnce";
            let _ = cmd::run_logged(
                &log,
                "reg",
                &[
                    "add",
                    runonce_key,
                    "/v",
                    "QSU-PostInstall",
                    "/t",
                    "REG_SZ",
                    "/d",
                    "C:\\ProgramData\\QSU\\PostInstall.exe",
                    "/f",
                ],
            );
            let _ = cmd::run_logged(&log, "reg", &["unload", "HKLM\\QSU_OFFLINE"]);
            log.line("Đã đăng ký RunOnce cho PostInstall.");
        } else {
            log.line("CẢNH BÁO: không load được SOFTWARE hive offline — PostInstall sẽ không tự chạy ở first logon.");
        }
    } else {
        log.line("Staging không có PostInstall.exe — bỏ qua bước agent hậu cài (no post-install agent staged).");
    }

    // --- WinRE vào phân vùng Recovery mới (wipeDisk §6b / rebuild) ---
    if wipe_mode || rebuild_mode {
        if core_profile {
            // Hồ sơ CORE (v0.7.22): không chép WinRE — phân vùng
            // Recovery để trống như layout cài tay WinNTSetup.
            log.line("[Cơ chế] Hồ sơ CORE (v0.7.22): bỏ qua chép WinRE vào phân vùng Recovery — để trống như layout cài tay WinNTSetup.");
        } else {
            match layout_recovery_letter {
                Some(r) => {
                    ui.set_phase("Đang chép WinRE", 88);
                    disk::copy_winre(&log, os_letter, r);
                }
                None => log.line(
                    "CẢNH BÁO: chế độ chia lại phân vùng nhưng phân vùng Recovery không có ký tự ổ — bỏ qua bước WinRE.",
                ),
            }
        }
    }

    // --- bcdboot ---
    ui.set_phase("Đang gắn thông tin khởi động", 93);
    let bcdboot = Path::new(&os_root).join("Windows").join("System32").join("bcdboot.exe");
    let bcdboot_s = if bcdboot.exists() {
        bcdboot.to_string_lossy().into_owned()
    } else {
        "bcdboot".to_string()
    };
    let win_arg = format!("{}\\Windows", os_root);
    let boot_ok = if let Some(bl) = layout_boot_letter {
        // wipeDisk/rebuild: bcdboot PHẢI trỏ vào phân vùng boot MỚI
        // vừa chia (EFI mới ở GPT / System Reserved mới ở MBR),
        // không dùng lại phân vùng EFI cũ như đường preserve.
        // (Giữ nguyên như các bản trước — đường này đã chạy đúng
        // ở thực địa v0.7.18.)
        let s_arg = format!("{}:", bl);
        let fw = if uefi { "UEFI" } else { "BIOS" };
        let ok = cmd::run_logged(&log, &bcdboot_s, &[&win_arg, "/s", &s_arg, "/f", fw])
            .map(|o| o.code == Some(0))
            .unwrap_or(false);
        if ok {
            // v0.7.25: chuẩn hoá entry boot (path/systemroot) —
            // bcdboot dựng entry theo BCD-Template của ảnh; ảnh
            // rút gọn có template sai khiến boot đầu lỗi 0xEF
            // (xem disk::normalize_boot_entry).
            disk::normalize_boot_entry(&log, bl, uefi);
        }
        ok
    } else {
        // preserve (v0.7.19): chốt boot bằng helper DÙNG CHUNG
        // disk::finalize_boot — tìm phân vùng boot TRÊN CÙNG ĐĨA
        // với phân vùng Windows theo loại phân vùng (GPT: EFI
        // System; MBR: System Reserved active, không có thì
        // chính phân vùng Windows), tự gán ký tự nếu thiếu,
        // bcdboot + kiểm chứng file boot thật. Thay hẳn
        // find_efi_letter() cũ (quét thư mục \EFI theo ký tự ổ +
        // mountvol /s — rủi ro R1, xem chú thích ở disk.rs).
        match disk::finalize_boot(&log, os_letter, uefi) {
            Ok(f) => {
                log.line(&format!(
                    "Chốt boot preserve xong: phân vùng boot = {}: (đĩa {}, phân vùng #{}{}).",
                    f.boot_letter,
                    f.disk_number,
                    f.boot_partition_number,
                    if f.self_boot { ", chính là phân vùng Windows" } else { "" }
                ));
                true
            }
            Err(e) => {
                log.line(&format!("LỖI: chốt boot (bcdboot) thất bại: {}", e));
                false
            }
        }
    };
    if !boot_ok {
        log.line("LỖI: bcdboot thất bại. KHÔNG reboot — cần kỹ thuật viên kiểm tra.");
        return 9;
    }

    ui.set_phase("Hoàn tất — sắp khởi động lại", 100);
    ui.mark_complete();
    // Tin CÀI XONG (v0.7.27): gửi NGAY TRƯỚC lệnh reboot — sau
    // khi reboot bắt đầu thì không gửi kịp nữa. Best-effort:
    // gửi lỗi không đổi kết quả ca cài, không chặn reboot.
    notify_engine_end(
        &log,
        &cfg.notify_relay_url,
        &cfg.notify_once_token,
        &cfg.computer_name,
        true,
        "",
    );
    log.line("=== WinPEInstall hoàn tất — reboot vào Windows mới ===");
    if cmd::run_capture("wpeutil", &["reboot"]).is_err() {
        // Dự phòng chỉ khi wpeutil không chạy được
        let _ = cmd::run_capture("shutdown", &["/r", "/t", "10"]);
    }
    0
}
