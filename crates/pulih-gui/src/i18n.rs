//! UI strings. Indonesian is the default; English is one click away.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Id,
    En,
}

impl Lang {
    pub fn t(self) -> &'static T {
        match self {
            Lang::Id => &ID,
            Lang::En => &EN,
        }
    }
}

/// `name: type = "Indonesian", "English";` → one struct field per string.
macro_rules! strings {
    ($($name:ident: $ty:ty = $id:expr, $en:expr;)*) => {
        pub struct T { $(pub $name: $ty,)* }
        pub static ID: T = T { $($name: $id,)* };
        pub static EN: T = T { $($name: $en,)* };
    };
}

strings! {
    refresh: &'static str = "⟳ Muat ulang", "⟳ Refresh";
    image_hint: &'static str = "atau ketik path perangkat / file image (.img, .dd, .bin)", "or type a device path / image file (.img, .dd, .bin)";
    no_devices: &'static str = "Tidak ada drive yang terdeteksi. Coba jalankan sebagai Administrator / root, atau buka file image.", "No drives detected. Try running as Administrator / root, or open an image file.";
    step_options: &'static str = "3. Metode pemindaian", "3. Scan method";
    mode_both: &'static str = "Lengkap (disarankan)", "Complete (recommended)";
    mode_both_hint: &'static str = "Baca metadata filesystem (nama & folder asli), lalu cari sisa file lewat signature.", "Read filesystem metadata (original names & folders), then find the rest by signature.";
    mode_quick: &'static str = "Cepat", "Quick";
    mode_quick_hint: &'static str = "Hanya metadata NTFS / FAT / exFAT. Selesai dalam hitungan detik.", "NTFS / FAT / exFAT metadata only. Finishes in seconds.";
    mode_deep: &'static str = "Mendalam", "Deep";
    mode_deep_hint: &'static str = "Hanya pencarian signature — untuk drive terformat, rusak, atau filesystem lain (ext4, APFS…).", "Signature search only — for formatted or damaged drives, or other filesystems (ext4, APFS…).";
    free_only: &'static str = "Hanya ruang kosong", "Free space only";
    free_only_hint: &'static str = "Lewati area yang sedang dipakai file aktif. Jauh lebih cepat dan tanpa duplikat.", "Skip areas used by live files. Much faster and no duplicates.";
    byte_level: &'static str = "Pencarian per byte (lambat)", "Byte-level search (slow)";
    byte_level_hint: &'static str = "Periksa setiap offset, bukan hanya awal sektor. Jarang diperlukan.", "Test every offset instead of sector starts. Rarely needed.";
    cats: [&'static str; 6] = ["Foto", "Video", "Audio", "Dokumen", "Arsip", "Lainnya"], ["Photos", "Videos", "Audio", "Documents", "Archives", "Other"];
    advanced: &'static str = "Lanjutan", "Advanced";
    threads: &'static str = "Thread pemindai", "Scanner threads";
    chunk: &'static str = "Ukuran blok baca (MB)", "Read block size (MB)";
    pick_source_first: &'static str = "Pilih drive atau file image terlebih dulu.", "Choose a drive or image file first.";
    warn_write: &'static str = "⚠ Jangan menyimpan, mengunduh, atau menginstal apa pun ke drive yang ingin dipulihkan — data baru bisa menimpa file yang terhapus. Pulih hanya membaca drive tersebut.", "⚠ Don't save, download or install anything onto the drive you want to recover — new data can overwrite deleted files. Pulih only reads that drive.";
    need_admin: &'static str = "Akses ditolak. Akses disk mentah memerlukan hak Administrator (Windows) atau root (Linux: jalankan dengan sudo).", "Access denied. Raw disk access needs Administrator rights (Windows) or root (Linux: run with sudo).";
    open_failed: &'static str = "Tidak bisa membuka", "Could not open";
    phase_analyzing: &'static str = "Menganalisis partisi…", "Analysing partitions…";
    phase_fs: &'static str = "Membaca metadata filesystem", "Reading filesystem metadata";
    phase_carving: &'static str = "Pencarian mendalam (signature)", "Deep scan (signatures)";
    phase_done: &'static str = "Selesai", "Finished";
    phase_cancelled: &'static str = "Dihentikan", "Stopped";
    speed: &'static str = "Kecepatan", "Speed";
    eta: &'static str = "Sisa waktu", "Remaining";
    elapsed: &'static str = "Waktu", "Elapsed";
    found: &'static str = "Ditemukan", "Found";
    bad_sectors: &'static str = "Sektor rusak", "Bad sectors";
    stop: &'static str = "⏹ Hentikan", "⏹ Stop";
    new_scan: &'static str = "↺ Pindai baru", "↺ New scan";
    only_good: &'static str = "Hanya kondisi baik", "Good condition only";
    origin_all: &'static str = "Semua sumber", "All sources";
    origin_fs: &'static str = "Filesystem", "Filesystem";
    origin_carved: &'static str = "Signature", "Signature";
    select_all: &'static str = "Pilih semua", "Select all";
    select_none: &'static str = "Batal pilih", "Select none";
    col_name: &'static str = "Nama", "Name";
    col_folder: &'static str = "Folder asal", "Original folder";
    col_type: &'static str = "Tipe", "Type";
    col_size: &'static str = "Ukuran", "Size";
    col_modified: &'static str = "Diubah", "Modified";
    col_condition: &'static str = "Kondisi", "Condition";
    col_source: &'static str = "Sumber", "Source";
    cond: [&'static str; 4] = ["Baik", "Tak pasti", "Sebagian", "Tertimpa"], ["Good", "Uncertain", "Partial", "Overwritten"];
    cond_hint: [&'static str; 4] = ["Data utuh — peluang pulih sangat tinggi.", "Ukuran diperkirakan; file mungkin berisi data tambahan di akhir.", "Sebagian data sudah ditimpa atau terpotong.", "Area data sudah dipakai file lain — kemungkinan besar rusak."], ["Data intact — excellent chance of recovery.", "Size is estimated; the file may carry extra data at the end.", "Part of the data has been overwritten or is cut short.", "The data area is used by another file — most likely damaged."];
    preview: &'static str = "Pratinjau", "Preview";
    no_preview: &'static str = "Pratinjau tidak tersedia untuk tipe ini.", "No preview for this type.";
    loading: &'static str = "Memuat…", "Loading…";
    offset: &'static str = "Offset", "Offset";
    fragments: &'static str = "Fragmen", "Fragments";
    volume: &'static str = "Volume", "Volume";
    whole_device: &'static str = "Seluruh perangkat", "Whole device";
    carved_name_note: &'static str = "Nama asli tidak tersimpan; file ditemukan dari isinya.", "The original name was not stored; the file was found by its content.";
    selected: &'static str = "dipilih", "selected";
    recover: &'static str = "Pulihkan", "Recover";
    recover_one: &'static str = "Pulihkan file ini", "Recover this file";
    keep_folders: &'static str = "Pertahankan struktur folder", "Keep folder structure";
    choose_out: &'static str = "Pilih folder tujuan", "Choose destination folder";
    save_to_hint: &'static str = "folder di drive LAIN", "a folder on ANOTHER drive";
    continue_anyway: &'static str = "Tetap lanjutkan (berisiko)", "Continue anyway (risky)";
    same_disk: &'static str = "Folder tujuan berada di disk yang sama dengan sumber! Simpan ke drive lain agar file terhapus tidak tertimpa.", "The destination is on the same disk as the source! Save to another drive so deleted files are not overwritten.";
    recovering: &'static str = "Memulihkan file…", "Recovering files…";
    recovered: &'static str = "file berhasil dipulihkan", "files recovered";
    failed: &'static str = "gagal", "failed";
    cancel: &'static str = "Batal", "Cancel";
    empty_results: &'static str = "Tidak ada file yang cocok dengan filter.", "No files match the filters.";
    empty_scanning: &'static str = "Memindai… hasil akan muncul di sini.", "Scanning… results will appear here.";
    log: &'static str = "Catatan", "Log";
    drop_hint: &'static str = "Tip: seret file image disk ke jendela ini.", "Tip: drop a disk image file onto this window.";
    of: &'static str = "dari", "of";
    find_name_hint: &'static str = "mis. skripsi; IMG_2024*; *.pdf", "e.g. thesis; IMG_2024*; *.pdf";
    find_name_help: &'static str = "Pisahkan beberapa nama dengan ;   * = karakter apa saja, ? = satu karakter. Tanpa * berarti \"mengandung\" — nama folder juga ikut dicocokkan, begitu pula judul di dalam dokumen/lagu.", "Separate several names with ;   * = any characters, ? = one character. Without * it means \"contains\" — folder names match too, as do titles inside documents/songs.";
    find_types: &'static str = "Jenis file (kosong = semua jenis)", "File types (none = all types)";
    find_folder: &'static str = "Folder asal", "Original folder";
    find_folder_hint: &'static str = "mengandung, mis. Documents/Kuliah", "contains, e.g. Documents/Thesis";
    find_date: &'static str = "Terakhir diubah", "Last modified";
    find_size: &'static str = "Ukuran", "Size";
    find_min: &'static str = "min", "min";
    find_max: &'static str = "maks", "max";
    find_from: &'static str = "dari", "from";
    find_to: &'static str = "sampai", "to";
    date_hint: &'static str = "TTTT-BB-HH", "YYYY-MM-DD";
    include_unnamed: &'static str = "Sertakan hasil signature yang jenisnya cocok", "Include signature results of matching type";
    include_unnamed_hint: &'static str = "File dari pencarian mendalam tidak menyimpan nama, folder, atau (sering) tanggal aslinya. Bila dicentang, file seperti itu tetap ditampilkan asalkan jenisnya cocok dengan jenis yang Anda pilih — siapa tahu itu file yang Anda cari.", "Deep-scan results don't keep their original name, folder or (often) date. When ticked they are still shown if their type matches the types you picked — one of them may be the file you're after.";
    deep_limited: &'static str = "Pencarian mendalam hanya mencari", "Deep scan only looks for";
    search_active: &'static str = "Pencarian aktif", "Search active";
    reset_search: &'static str = "Reset pencarian", "Clear search";
    info: &'static str = "Info", "Info";
    named_from_content: &'static str = "Nama diambil dari isi file (judul dokumen / tag lagu).", "Name taken from the file's content (document title / song tag).";
    nothing_matches: &'static str = "Tidak ada file yang cocok dengan pencarian. Coba longgarkan kriteria, atau klik Reset pencarian.", "Nothing matches the search. Loosen the criteria or click Clear search.";
    content_title: &'static str = "Judul (isi)", "Title (content)";
    content_date: &'static str = "Tanggal (isi)", "Date (content)";
    doc_text: &'static str = "Isi dokumen", "Document text";
    date_presets: [&'static str; 5] = ["Kapan saja", "7 hari terakhir", "30 hari terakhir", "1 tahun terakhir", "Rentang tanggal…"], ["Any time", "Last 7 days", "Last 30 days", "Last year", "Date range…"];
    tagline: &'static str = "Pemulihan Data", "Data Recovery";
    nav_home: &'static str = "Pilih lokasi", "Choose location";
    nav_results: &'static str = "Hasil pindaian", "Scan results";
    nav_tips: &'static str = "Tips pemulihan", "Recovery tips";
    language: &'static str = "Bahasa", "Language";
    appearance: &'static str = "Tampilan", "Appearance";
    theme_names: [&'static str; 3] = ["Terang", "Gelap", "Sistem"], ["Light", "Dark", "System"];
    home_title: &'static str = "Pilih lokasi untuk memulihkan data", "Select a location to recover data";
    home_sub: &'static str = "Pilih drive, partisi, atau folder tempat file Anda dulu tersimpan — Pulih hanya membacanya, tidak pernah menulis ke sana.", "Pick the drive, partition or folder where your files used to be — Pulih only reads it, never writes to it.";
    find_title: &'static str = "Mencari file tertentu?", "Looking for a specific file?";
    find_sub: &'static str = "Opsional — ketik nama file, lalu persempit dengan jenis, folder, tanggal, atau ukuran.", "Optional — type a file name, then narrow it down by type, folder, date or size.";
    more_filters: &'static str = "Filter lanjutan", "More filters";
    sec_drives: &'static str = "Drive & partisi", "Drives & partitions";
    sec_disks: &'static str = "Disk fisik", "Physical disks";
    sec_places: &'static str = "Lokasi umum", "Common locations";
    sec_other: &'static str = "Sumber lain", "Other sources";
    local_disk: &'static str = "Disk Lokal", "Local Disk";
    removable_disk: &'static str = "Disk lepas-pasang", "Removable disk";
    free_of: &'static str = "bebas dari", "free of";
    whole_disk_hint: &'static str = "Seluruh disk, termasuk partisi yang hilang", "Whole disk, including lost partitions";
    places: [&'static str; 6] = ["Desktop", "Dokumen", "Gambar", "Unduhan", "Video", "Musik"], ["Desktop", "Documents", "Pictures", "Downloads", "Videos", "Music"];
    pick_folder: &'static str = "Pilih folder…", "Choose folder…";
    pick_folder_sub: &'static str = "Pindai folder tertentu", "Scan one specific folder";
    image_title: &'static str = "File image disk", "Disk image file";
    image_sub: &'static str = "Buka .img / .dd / .bin", "Open .img / .dd / .bin";
    manual_path: &'static str = "Path perangkat atau image", "Device or image path";
    location: &'static str = "Lokasi", "Location";
    no_location: &'static str = "Belum ada lokasi dipilih", "No location selected";
    scan_btn: &'static str = "Pindai", "Scan";
    options: &'static str = "Opsi", "Options";
    modes: [&'static str; 3] = ["Lengkap", "Cepat", "Mendalam"], ["Complete", "Quick", "Deep"];
    scanning_title: &'static str = "Memindai", "Scanning";
    done_title: &'static str = "Pemindaian selesai", "Scan complete";
    files_found: &'static str = "file ditemukan", "files found";
    view_list: &'static str = "Daftar", "List";
    view_grid: &'static str = "Thumbnail", "Thumbnails";
    tab_path: &'static str = "Jalur", "Path";
    tab_type: &'static str = "Jenis", "Type";
    all_files: &'static str = "Semua file", "All files";
    deleted_named: &'static str = "File terhapus", "Deleted files";
    found_by_sig: &'static str = "Ditemukan lewat signature", "Found by signature";
    filter_type: &'static str = "Jenis", "Type";
    filter_date: &'static str = "Tanggal", "Date";
    filter_size: &'static str = "Ukuran", "Size";
    filter_cond: &'static str = "Kondisi", "Condition";
    clear_filters: &'static str = "Hapus filter", "Clear filters";
    search_files: &'static str = "Cari file atau folder…", "Search files or folders…";
    dlg_title: &'static str = "Pulihkan file", "Recover files";
    dlg_sub: &'static str = "Simpan ke folder di drive yang BERBEDA dari sumber, agar file terhapus lainnya tidak tertimpa.", "Save to a folder on a DIFFERENT drive than the source so other deleted files are not overwritten.";
    browse: &'static str = "Telusuri…", "Browse…";
    dest: &'static str = "Folder tujuan", "Destination folder";
    rec_done: &'static str = "Pemulihan selesai", "Recovery complete";
    view_recovered: &'static str = "Lihat file yang dipulihkan", "View recovered files";
    finish: &'static str = "Selesai", "Done";
    tips_title: &'static str = "Tips agar pemulihan berhasil", "Tips for a successful recovery";
    tips_sub: &'static str = "Beberapa langkah sederhana yang paling menentukan peluang file Anda kembali.", "A few simple steps that matter most for getting your files back.";
    tip_titles: [&'static str; 6] = ["Berhenti memakai drive tersebut", "Simpan hasil ke drive lain", "Mulai dari Pindai Cepat", "Kartu SD & flashdisk", "SSD dan TRIM", "Perhatikan kondisi file"], ["Stop using that drive", "Save results to another drive", "Start with a Quick scan", "SD cards & USB sticks", "SSDs and TRIM", "Mind the condition label"];
    tip_bodies: [&'static str; 6] = ["Jangan menyimpan, menginstal, atau mengunduh apa pun ke drive yang berisi file terhapus. Setiap data baru bisa menimpa file yang ingin Anda selamatkan.", "Pulihkan ke flashdisk, HDD eksternal, atau partisi lain — jangan ke drive yang sedang dipindai.", "Pindai Cepat membaca metadata filesystem dan selesai dalam hitungan detik, lengkap dengan nama dan folder asli. Pakai Pindai Mendalam bila file tidak muncul atau drive sudah diformat.", "Foto dan video kamera biasanya tersimpan berurutan, jadi peluang pulihnya tinggi — bahkan setelah kartu diformat.", "Di SSD, sistem operasi biasanya menghapus blok data secara permanen (TRIM) tak lama setelah file dihapus. Bertindaklah secepatnya; data yang sudah di-TRIM tidak bisa dipulihkan aplikasi apa pun.", "Baik berarti datanya utuh. Sebagian atau Tertimpa berarti sebagian data sudah dipakai file lain, sehingga file mungkin rusak."], ["Don't save, install or download anything onto the drive that held the deleted files. Any new data can overwrite what you want back.", "Recover to a USB stick, an external disk or another partition — never to the drive being scanned.", "A Quick scan reads filesystem metadata and finishes in seconds, with original names and folders. Use a Deep scan when files don't show up or the drive was formatted.", "Camera photos and videos are usually stored contiguously, so they come back well — even after the card was formatted.", "On SSDs the OS usually wipes freed blocks for good (TRIM) shortly after deletion. Act fast; trimmed data cannot be recovered by any tool.", "Good means the data is intact. Partial or Overwritten means another file reused some of its space, so it may be damaged."];
    preview_hint: &'static str = "Pilih file untuk melihat pratinjau dan detailnya.", "Select a file to see its preview and details.";
    hex: &'static str = "Data mentah (hex)", "Raw data (hex)";
    nothing_found: &'static str = "Tidak ada file terhapus yang ditemukan di lokasi ini.", "No deleted files were found at this location.";
    try_deep: &'static str = "Coba metode Mendalam bila drive pernah diformat.", "Try the Deep method if the drive was formatted.";
    details: &'static str = "Detail", "Details";
    whole_disk: &'static str = "Seluruh disk", "Whole disk";
}
