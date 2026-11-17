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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use qsu_common::cmd;
use qsu_common::disk;
use qsu_common::log::Logger;
use qsu_common::stage::StageConfig;
use qsu_common::sys::{self, DriveInfo};

const DATA_MARKER: &str = ".qsu_data_drive";

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

/// Tìm phân vùng EFI (FAT32, có thư mục EFI) hoặc gán ký tự bằng mountvol /s
fn find_efi_letter(drives: &[DriveInfo]) -> Option<char> {
    for d in drives {
        if Path::new(&format!("{}:\\EFI", d.letter)).exists() {
            return Some(d.letter);
        }
    }
    // Chưa có ký tự: gán S: (hoặc Z:) cho phân vùng hệ thống EFI
    for cand in ['S', 'Z', 'R'] {
        if drives.iter().any(|d| d.letter == cand) {
            continue;
        }
        let target = format!("{}:", cand);
        let out = cmd::run_capture("mountvol", &[&target, "/s"]).ok()?;
        if out.code == Some(0) && Path::new(&format!("{}:\\EFI", cand)).exists() {
            return Some(cand);
        }
    }
    None
}

fn main() {
    let staging_name = std::env::args().nth(1).unwrap_or_else(|| "QSU-Stage".to_string());

    // Tìm staging trước khi có log (log nằm trên ổ Data)
    let (data_letter, stage_dir, cfg) = match find_stage(&staging_name) {
        Some(v) => v,
        None => {
            eprintln!("QSU WinPEInstall: không tìm thấy stage.json trên ổ Data có marker. Dừng.");
            std::process::exit(2);
        }
    };
    let data_root = format!("{}:\\", data_letter);
    let logs_dir = Path::new(&data_root).join(&cfg.logs_folder);
    let log_path = logs_dir.join(format!("winpe-{}.log", qsu_common::log::log_stamp()));
    let log = Arc::new(Logger::new(log_path, None));
    log.line("=== WinPEInstall bắt đầu ===");
    log.line(&format!("Ổ Data: {} (serial stage ghi: {})", data_letter, cfg.data_volume_serial));
    log.line(&format!(
        "Image: {} index {} ({})",
        cfg.image_file, cfg.image_index, cfg.image_name
    ));

    let drives = sys::list_fixed_drives();
    // Đối chiếu serial ổ Data
    if cfg.data_volume_serial != 0 {
        let actual = volume_serial(data_letter).unwrap_or(0) as u64;
        if actual != cfg.data_volume_serial {
            log.line(&format!(
                "LỖI AN TOÀN: serial ổ Data thực tế {} != stage {}. Dừng.",
                actual, cfg.data_volume_serial
            ));
            std::process::exit(3);
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
            std::process::exit(10);
        }
        let disks = disk::list_physical_disks();
        let target = match disk::find_disk_by_serial(&disks, &cfg.target_disk_serial) {
            Some(d) => d.clone(),
            None => {
                log.line(&format!(
                    "LỖI AN TOÀN: không tìm thấy (hoặc trùng) đĩa vật lý có serial '{}' như stage.json. Dừng, không đụng đĩa nào.",
                    cfg.target_disk_serial
                ));
                std::process::exit(10);
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
                std::process::exit(11);
            }
            Some(_) => {}
            None => {
                log.line("LỖI AN TOÀN: không xác định được ổ Data nằm trên đĩa vật lý nào — không thể chứng minh đĩa đích an toàn. Dừng.");
                std::process::exit(11);
            }
        }
        log.line(&format!("Đĩa đích wipe: {}", target.summary()));
        log.line(&format!(
            "Các phân vùng trên đĩa đích sắp bị XOÁ TOÀN BỘ:\n{}",
            target.describe_partitions()
        ));
        match disk::execute_wipe(&log, target.number, &target.serial, target.size, uefi) {
            Ok(r) => {
                layout_boot_letter = Some(r.boot_letter);
                layout_recovery_letter = r.recovery_letter;
                r.windows_letter
            }
            Err(e) => {
                log.line(&format!("LỖI: wipe đĩa đích thất bại: {}. Dừng.", e));
                std::process::exit(12);
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
            std::process::exit(13);
        }
        if cfg.rebuild_partitions.is_empty() {
            log.line("LỖI AN TOÀN: stage.json chọn rebuild nhưng rebuildPartitions rỗng — không có căn cứ nào để xoá. Dừng (exit 13).");
            std::process::exit(13);
        }
        let disks = disk::list_physical_disks();
        let target = match disk::find_disk_by_serial(&disks, &cfg.target_disk_serial) {
            Some(d) => d.clone(),
            None => {
                log.line(&format!(
                    "LỖI AN TOÀN: không tìm thấy (hoặc trùng) đĩa vật lý có serial '{}' như stage.json. Dừng, không đụng đĩa nào (exit 13).",
                    cfg.target_disk_serial
                ));
                std::process::exit(13);
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
                        std::process::exit(13);
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
                std::process::exit(13);
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
                std::process::exit(13);
            }
        }
    } else {
        match find_os_drive(&cfg, data_letter, &drives) {
            Some(l) => l,
            None => {
                log.line("LỖI: không xác định được phân vùng OS theo serial trong stage.json. Dừng, không format gì.");
                std::process::exit(4);
            }
        }
    };
    if os_letter == data_letter
        || Path::new(&format!("{}:\\{}", os_letter, DATA_MARKER)).exists()
    {
        log.line("LỖI AN TOÀN: đích cài trùng ổ Data. Dừng.");
        std::process::exit(5);
    }
    let os_root = format!("{}:\\", os_letter);
    log.line(&format!("Ổ OS đích: {}", os_letter));

    // --- Kiểm tra image ---
    let image_path = stage_dir.join(&cfg.image_file);
    if !image_path.exists() {
        log.line(&format!("LỖI: không thấy file image {}", image_path.display()));
        std::process::exit(6);
    }

    // --- Format phân vùng OS (wipeDisk/rebuild đã format khi chia lại) ---
    if wipe_mode || rebuild_mode {
        log.line("Chế độ chia lại phân vùng: phân vùng Windows đã được format trong script diskpart — bỏ qua bước format riêng.");
    } else {
        log.line("Đang format phân vùng OS (NTFS, quick)...");
        if !format_os_partition(&log, os_letter) {
            log.line("LỖI: format thất bại. Dừng.");
            std::process::exit(7);
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
    let mut last_pct = String::new();
    let apply_res = cmd::run_streaming(&dism, &args, |line| {
        let t = line.trim();
        if t.contains('%') {
            last_pct = t.to_string();
        }
        if !t.is_empty() {
            log.line(&format!("  | {}", t));
        }
    });
    match apply_res {
        Ok(Some(0)) => log.line("Apply-Image hoàn tất."),
        other => {
            log.line(&format!("LỖI: Apply-Image thất bại ({:?}), tiến độ cuối: {}", other, last_pct));
            std::process::exit(8);
        }
    }

    // --- Inject driver offline (IRST/NVMe...) nếu staging có ---
    let drivers_dir = stage_dir.join("Drivers");
    if drivers_dir.exists()
        && std::fs::read_dir(&drivers_dir).map(|mut d| d.next().is_some()).unwrap_or(false)
    {
        let img_arg = format!("/Image:{}", os_root);
        let drv_arg = format!("/Driver:{}", drivers_dir.to_string_lossy());
        log.line("Đang inject driver offline...");
        let _ = cmd::run_logged(&log, &dism, &[&img_arg, "/Add-Driver", &drv_arg, "/Recurse"]);
    } else {
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
    let backup_safe = !backup_rel.is_empty()
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
    let panther = Path::new(&os_root).join("Windows").join("Panther");
    let _ = std::fs::create_dir_all(&panther);
    let unattend_src = stage_dir.join("autounattend.xml");
    if unattend_src.exists() {
        for name in ["autounattend.xml", "unattend.xml"] {
            let _ = std::fs::copy(&unattend_src, panther.join(name));
        }
        log.line("Đã copy autounattend.xml vào Windows\\Panther.");
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
    if post_src.exists() {
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
        match layout_recovery_letter {
            Some(r) => {
                disk::copy_winre(&log, os_letter, r);
            }
            None => log.line(
                "CẢNH BÁO: chế độ chia lại phân vùng nhưng phân vùng Recovery không có ký tự ổ — bỏ qua bước WinRE.",
            ),
        }
    }

    // --- bcdboot ---
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
        let s_arg = format!("{}:", bl);
        let fw = if uefi { "UEFI" } else { "BIOS" };
        cmd::run_logged(&log, &bcdboot_s, &[&win_arg, "/s", &s_arg, "/f", fw])
            .map(|o| o.code == Some(0))
            .unwrap_or(false)
    } else if cfg.boot_mode.eq_ignore_ascii_case("UEFI") {
        match find_efi_letter(&sys::list_fixed_drives()) {
            Some(efi) => {
                let s_arg = format!("{}:", efi);
                cmd::run_logged(&log, &bcdboot_s, &[&win_arg, "/s", &s_arg, "/f", "UEFI"])
                    .map(|o| o.code == Some(0))
                    .unwrap_or(false)
            }
            None => {
                log.line("LỖI: không tìm được phân vùng EFI cho bcdboot.");
                false
            }
        }
    } else {
        let s_arg = format!("{}:", os_letter);
        cmd::run_logged(&log, &bcdboot_s, &[&win_arg, "/s", &s_arg, "/f", "BIOS"])
            .map(|o| o.code == Some(0))
            .unwrap_or(false)
    };
    if !boot_ok {
        log.line("LỖI: bcdboot thất bại. KHÔNG reboot — cần kỹ thuật viên kiểm tra.");
        std::process::exit(9);
    }

    log.line("=== WinPEInstall hoàn tất — reboot vào Windows mới ===");
    if cmd::run_capture("wpeutil", &["reboot"]).is_err() {
        // Dự phòng chỉ khi wpeutil không chạy được
        let _ = cmd::run_capture("shutdown", &["/r", "/t", "10"]);
    }
}
