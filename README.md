# Pulih — Pemulihan File Terhapus

**Pulih** adalah aplikasi desktop native untuk memulihkan file yang terhapus dari
hard disk, SSD, flashdisk, dan kartu SD. Ditulis dalam Rust, tanpa runtime atau
browser tertanam: satu file `.exe` sekitar 9 MB yang langsung jalan.

- **Cepat**: pencarian mendalam hanya dibatasi kecepatan disk. Engine-nya
  memproses **~5,8 GB/s** (data dari cache RAM), jauh di atas kecepatan SSD
  NVMe. Pindai cepat NTFS/FAT/exFAT selesai dalam hitungan detik.
- **Aman**: drive sumber hanya **dibaca**, tidak pernah ditulisi. Pulih menolak
  menyimpan hasil ke disk yang sama dan meminta konfirmasi bila Anda tetap
  mencobanya.
- **Akurat**: ukuran setiap file dihitung dari struktur internalnya (chunk PNG,
  box MP4, central directory ZIP, dsb.), jadi file hasil pulih tidak terpotong
  dan tidak berisi sampah di akhir.

![Hasil pindaian: navigasi Jalur/Jenis, filter, thumbnail foto, dan pratinjau](docs/screenshot.png)


## Fitur

| | |
|---|---|
| **Pindai cepat (metadata)** | NTFS (seluruh MFT, termasuk file terfragmentasi, file kecil *resident*, dan folder yang ikut terhapus), FAT12/16/32 (nama panjang/LFN dipulihkan), exFAT. Nama, folder asal, dan tanggal asli tetap terjaga. |
| **Pindai mendalam (signature)** | Mencari file dari isinya. Bisa dipakai pada drive yang sudah diformat, rusak, atau ber-filesystem lain (ext4, APFS, HFS+). Secara default hanya **ruang kosong** yang dipindai. |
| **Cari file tertentu** | Sebelum memindai, isi nama file (wildcard `*`/`?` didukung; tanpa wildcard juga mencocokkan nama folder), pilih jenis spesifik (JPG, HEIC, RAW, MP4, DOCX, PDF, …), folder asal, rentang tanggal, dan ukuran. Hasil langsung tersaring, dan pencarian mendalam **hanya mencari jenis yang dipilih**, jadi lebih cepat. Di layar hasil, kriteria yang sama bisa diubah kapan saja, ditambah pohon folder asal. |
| **Identitas dari isi file** | Hasil pencarian mendalam diberi nama dari isinya: judul dokumen Word/Excel/PowerPoint/ODF/PDF (atau kalimat pertamanya), "Artis - Judul" untuk MP3/FLAC. Foto diberi tanggal pengambilan (EXIF) dan model kamera; video diberi tanggal rekam. Jadi file tetap bisa dicari dengan nama dan tanggal walau metadata filesystem-nya sudah hilang. |
| **Penilaian kondisi** | Setiap hasil diberi label *Baik / Tak pasti / Sebagian / Tertimpa*, dihitung dari bitmap alokasi dan tumpang-tindih antar file terhapus. |
| **Partisi** | MBR (termasuk partisi extended), GPT (dengan nama partisi), atau volume langsung. Ruang di luar partisi (misalnya partisi yang terhapus) ikut dipindai. |
| **Pratinjau** | Foto ditampilkan langsung (JPG, PNG, GIF, BMP, WEBP, TIFF) beserta tanggal EXIF dan kamera. Dokumen Office/ODF menampilkan **isi teksnya**, judul, dan tanggal simpan, untuk memastikan "ini file yang saya cari". File lain ditampilkan sebagai teks atau hex. |
| **Sektor rusak** | Pembacaan tetap berjalan; sektor yang tidak terbaca diisi nol dan dihitung. |
| **Tampilan modern** | Navigasi samping, kartu drive dengan bar kapasitas, pintasan Desktop/Dokumen/Gambar/Unduhan, hasil dalam **daftar** atau **thumbnail foto**, panel kiri *Jalur* (pohon folder asal) dan *Jenis* (per kategori & ekstensi), filter Jenis/Tanggal/Ukuran/Kondisi, dan dialog pemulihan yang menuntun. Menyesuaikan diri dengan ukuran jendela. |
| **Bahasa & tema** | Indonesia (default) dan Inggris. Tema terang (default), gelap, atau mengikuti sistem. |

### Format yang dikenali pencarian mendalam

| Kategori | Format |
|---|---|
| Foto | JPEG (termasuk progresif), PNG, GIF, BMP, TIFF, WEBP, HEIC/HEIF, AVIF, PSD/PSB, RAW kamera: CR2, CR3, NEF, ARW, DNG, ORF, RW2, RAF, PEF, SRW |
| Video | MP4, MOV, M4V, 3GP, MKV, WEBM, AVI (termasuk OpenDML >1 GB), FLV, WMV/ASF, MPEG-PS (MPG/VOB), MPEG-TS, AVCHD (MTS/M2TS) |
| Audio | MP3 (ID3 maupun tanpa tag), WAV, FLAC (ukuran pasti lewat CRC-16 per frame), OGG, OPUS, M4A, WMA, AIFF, MIDI |
| Dokumen | PDF (termasuk revisi inkremental), DOCX, XLSX, PPTX, VSDX, ODT/ODS/ODP, EPUB, DOC/XLS/PPT/MSG (OLE2), RTF |
| Arsip | ZIP (termasuk ZIP64 dan data descriptor), RAR 4/5, 7Z, APK, JAR |
| Lainnya | SQLite, EXE/DLL/SYS (termasuk tabel simbol & sertifikat), ELF |

## Instalasi Langkah demi Langkah (Step-by-Step Installation)

> ⚠ **Penting:** Jangan menginstal atau mengunduh Pulih ke drive yang berisi file yang ingin Anda pulihkan (misalnya jika file hilang di `D:`, instal Pulih di `C:`). Data baru yang ditulis ke drive tersebut dapat menimpa file yang terhapus.

### Opsi 1: Menggunakan Installer Windows (`Pulih-Setup-1.2.0.exe`) — *Disarankan*

1. **Unduh atau jalankan file installer:**
   Buka `dist\Pulih-Setup-1.2.0.exe` (atau unduh dari halaman **Releases** di GitHub).
2. **Setujui izin Administrator (UAC):**
   Klik **Yes** saat Windows menanyakan izin Administrator (atau pilih mode instalasi pengguna saat diminta).
3. **Setujui Lisensi & Pilih Lokasi Instalasi:**
   - Baca lisensi MIT, pilih *I accept the agreement*, lalu klik **Next**.
   - Pilih folder instalasi (default: `C:\Program Files\Pulih`), lalu klik **Next**.
4. **Pilih Tugas Tambahan (Opsional):**
   - Centang **Create a desktop shortcut** jika ingin ikon pintasan **Pulih** di Desktop.
   - Centang **Add pulih-cli to PATH** jika ingin menjalankan perintah `pulih-cli` langsung dari PowerShell atau Command Prompt mana pun.
5. **Selesaikan Instalasi:**
   Klik **Install**, tunggu proses ekstraksi selesai, lalu klik **Finish** untuk langsung menjalankan aplikasi **Pulih**.
6. **Cara Menghapus Instalasi (Uninstall):**
   Buka **Settings → Apps → Installed apps** di Windows, cari **Pulih — Recover Deleted Files**, lalu klik **Uninstall** (atau melalui menu **Start → Pulih → Uninstall Pulih**).

### Opsi 2: Menggunakan File Portabel (`pulih.exe` & `pulih-cli.exe`)

Jika Anda ingin menjalankan Pulih langsung dari flashdisk tanpa instalasi:
1. Salin `target\release\pulih.exe` (dan `target\release\pulih-cli.exe` bila perlu) ke flashdisk atau drive lain.
2. Klik ganda `pulih.exe` dan setujui permintaan UAC (**Run as Administrator**).

---

## Panduan Pengguna (User Guide)

### A. Aplikasi Desktop (`pulih`) — Windows & Linux

#### Langkah 1: Buka Aplikasi sebagai Administrator
- **Windows:** Buka **Pulih** dari Start Menu, pintasan Desktop, atau jalankan `pulih.exe`. Klik **Yes** pada dialog UAC karena pembacaan sektor disk mentah memerlukan hak Administrator.
- **Linux:** Jalankan dengan `sudo ./pulih` agar aplikasi memiliki akses baca ke `/dev/sdX` atau `/dev/nvmeXnY`.
- **Mengubah Bahasa & Tema:** Gunakan tombol di pojok kiri bawah bilah samping untuk beralih antara bahasa **Indonesia** / **English** dan tema **Terang** / **Gelap** / **Sistem**.

#### Langkah 2: Pilih Lokasi atau Perangkat yang Akan Dipindai
Pada halaman **Beranda**, pilih sumber tempat file Anda sebelumnya berada:
- **Drive & Partisi** (mis. `C:`, `D:`, kartu SD, flashdisk): Pilihan tercepat; memulihkan nama file, struktur folder, dan tanggal asli.
- **Lokasi Umum** (*Desktop*, *Dokumen*, *Gambar*, *Unduhan*, *Video*, *Musik*): Memindai partisi tempat folder tersebut berada lalu otomatis menyaring hasilnya ke folder itu.
- **Disk Fisik** (`Disk 0`, `Disk 1`, dst.): Gunakan bila partisi terhapus, rusak (*RAW*), atau telah diformat ulang.
- **Sumber Lain**: Klik **Pilih folder…** untuk fokus pada satu folder tertentu, atau **File image disk** untuk membuka file `.img` / `.dd` (Anda juga bisa menyeret file `.img` langsung ke jendela aplikasi).

![Halaman awal: memilih lokasi](docs/beranda.png)

#### Langkah 3: Tentukan File yang Dicari (Opsional)
Jika Anda mencari file spesifik, isi kolom pencarian sebelum memindai agar proses pemindaian mendalam jauh lebih cepat:
- **Kata kunci nama / judul:** Ketik misalnya `skripsi` — mencocokkan nama file, nama folder, maupun judul di dalam isi dokumen (Word/PDF) yang ditemukan lewat signature.
- **Pola wildcard (`*` dan `?`):** Ketik misalnya `IMG_2024*.jpg` atau `Laporan_??.docx` untuk mencocokkan pola nama file secara presisi.
- **Filter Lanjutan:** Klik **Filter lanjutan** untuk membatasi berdasarkan:
  - **Jenis file spesifik** (mis. hanya `DOCX` + `PDF`, atau `JPG` + `HEIC` + `CR3`)
  - **Folder asal** (mis. `Kuliah/Semester5`)
  - **Rentang tanggal modifikasi** (*Dari tanggal* – *Sampai tanggal*)
  - **Ukuran file** (* ukuran minimum / maksimum)

> Kosongkan semua filter jika Anda ingin menampilkan seluruh file yang dapat dipulihkan di drive tersebut.

![Mencari "liburan", hanya JPG dan PNG](docs/cari-file.png)

#### Langkah 4: Pilih Mode Pemindaian & Mulai Pindai
1. Klik tombol **Opsi** di sebelah tombol Pindai bila ingin mengatur metode:
   - **Lengkap (Disarankan):** Menjalankan pindai cepat metadata filesystem (NTFS/FAT/exFAT) terlebih dahulu, lalu melanjutkan pindai mendalam (*signature carving*) pada ruang kosong.
   - **Cepat (Metadata saja):** Selesai dalam hitungan detik; cocok untuk file yang baru saja dihapus.
   - **Mendalam (Signature saja):** Memindai isi sektor demi sektor; cocok untuk drive yang sudah diformat.
   - **Pindai seluruh ruang disk:** Centang jika ingin memindai seluruh disk termasuk area yang sudah teralokasi.
2. Klik **Pindai**. Hasil akan langsung muncul secara *real-time* beserta persentase progres, kecepatan baca (`MB/s` atau `GB/s`), dan estimasi waktu selesai (ETA). Anda dapat menjeda atau menghentikan pemindaian kapan saja.

#### Langkah 5: Telusuri, Saring, dan Pratinjau Hasil
Setelah (atau selama) pemindaian berjalan:
1. **Navigasi Panel Kiri:**
   - Tab **Jalur**: Menjelajahi pohon folder asli tempat file dihapus, serta kelompok *Ditemukan lewat signature*.
   - Tab **Jenis**: Mengelompokkan temuan berdasarkan kategori (*Foto*, *Video*, *Audio*, *Dokumen*, *Arsip*, *Lainnya*) dan per ekstensi.
2. **Bilah Filter Atas:** Saring hasil kapan saja menurut kata kunci, **Jenis**, **Tanggal**, **Ukuran**, atau **Kondisi** (*Baik*, *Tak pasti*, *Sebagian*, *Tertimpa*). Klik **Hapus filter** untuk mengatur ulang.
3. **Mode Tampilan & Pratinjau (Panel Kanan):**
   - Beralih antara tampilan **Daftar** (detail lengkap) dan **Thumbnail** (galeri foto).
   - Klik sebuah file untuk melihat pratinjaunya di panel kanan: foto ditampilkan penuh beserta info kamera/EXIF, dokumen Office/ODF/PDF menampilkan kutipan isi teks dan judulnya, sedangkan file lain menampilkan pratinjau teks atau heksadesimal.
   - **Pintasan Keyboard:** Gunakan <kbd>↑</kbd>/<kbd>↓</kbd> (serta <kbd>←</kbd>/<kbd>→</kbd> pada mode Thumbnail) untuk berpindah file, tekan <kbd>Spasi</kbd> atau klik ganda untuk mencentang file, dan klik kanan untuk memilih *Pulihkan file ini*.

#### Langkah 6: Pulihkan File ke Drive Lain
1. Centang kotak di samping file atau folder yang ingin diselamatkan.
2. Klik tombol **Pulihkan (N)** di kanan bawah.
3. Klik **Telusuri…** dan pilih folder tujuan di **drive yang berbeda** dari drive yang sedang dipindai (misalnya memindai `D:`, simpan ke `C:\HasilPulih`).
4. Klik **Pulihkan**. Setelah selesai, klik **Lihat file yang dipulihkan** untuk membuka folder hasil di File Explorer.

![Tema gelap: hasil pencarian mendalam dan isi dokumen di pratinjau](docs/gelap.png)

> **Catatan tentang hasil pencarian mendalam (*signature*):** File yang ditemukan murni dari signature tidak lagi memiliki metadata nama atau folder asli dari filesystem. Bila file tersebut memiliki judul internal (seperti dokumen Office/PDF atau tag lagu MP3/FLAC), Pulih otomatis menamainya dari judul tersebut; selebihnya diberi nama nomor sektor seperti `f000012345.jpg`.

---

### B. Antarmuka Baris Perintah (`pulih-cli`)

Untuk penggunaan di server, skrip otomatis, atau terminal (jalankan terminal sebagai **Administrator** di Windows atau dengan `sudo` di Linux):

#### 1. Menampilkan Daftar Drive & Partisi
```powershell
pulih-cli list
```
Contoh keluaran di Windows:
```text
\\.\PhysicalDrive0               954 GB  Disk      Disk 0 — NVMe KBG60ZNS1T02
\\.\PhysicalDrive1               238 GB  Disk      Disk 1 — Card Reader
\\.\C:                           933 GB  Partition NTFS       C: [Windows]  [C:\]
\\.\D:                           238 GB  Partition exFAT      D: [Ace Pro]  [D:\]
```

#### 2. Memindai Drive atau Image Disk
```powershell
# Pindai partisi D: dan tampilkan daftar semua file yang ditemukan
pulih-cli scan \\.\D: --list

# Pindai cepat (hanya metadata NTFS/FAT/exFAT) dan simpan hasilnya ke C:\Pulih
pulih-cli scan \\.\D: --quick-only --recover C:\Pulih

# Pindai image disk secara mendalam di seluruh ruang
pulih-cli scan disk.img --deep-only --all-space --list
```

#### 3. Mencari & Memulihkan File Tertentu
```powershell
# Cari file yang namanya mengandung "skripsi" di D: dan pulihkan ke E:\Hasil
pulih-cli scan \\.\D: --name "skripsi" --recover E:\Hasil

# Cari foto JPG/HEIC dari bulan Agustus 2023 yang kondisinya Baik
pulih-cli scan \\.\D: --name "IMG_2023*" --ext jpg,heic --after 2023-08-01 --before 2023-08-31 --good-only --recover C:\FotoPulih

# Cari dokumen DOCX dan PDF yang dulunya berada di folder "Kuliah"
pulih-cli scan disk.img --ext docx,pdf --folder Kuliah --list
```

#### 4. Daftar Lengkap Opsi `pulih-cli scan`

| Opsi | Keterangan |
|---|---|
| `--quick-only` | Hanya membaca metadata filesystem (NTFS/FAT/exFAT) |
| `--deep-only` | Hanya pemindaian mendalam berbasis signature |
| `--all-space` | Pindai seluruh kapasitas perangkat, bukan hanya ruang kosong |
| `--byte-level` | Periksa setiap offset byte (lebih lambat; menemukan file yang tertanam di dalam file lain) |
| `--types DAFTAR` | Filter kategori, dipisahkan koma: `image,video,audio,document,archive,other` |
| `--ext DAFTAR` | Filter ekstensi spesifik, mis. `docx,pdf` atau `jpg,heic,cr3` |
| `--name POLA` | Pola nama file (dipisahkan `;`, mendukung wildcard `*` dan `?`) |
| `--folder TEKS` | Jalur folder asal mengandung `TEKS` |
| `--after TTTT-BB-HH` | Hanya file yang dimodifikasi pada atau setelah tanggal tersebut |
| `--before TTTT-BB-HH` | Hanya file yang dimodifikasi sebelum tanggal tersebut |
| `--min-size BYTES` | Abaikan file yang lebih kecil dari ukuran ini |
| `--max-size BYTES` | Abaikan file yang lebih besar dari ukuran ini |
| `--good-only` | Hanya sertakan file yang kondisinya dinilai *Baik* (`Good`) |
| `--strict` | Buang hasil signature yang tidak memiliki nama/folder/tanggal untuk dicocokkan |
| `--threads N` | Jumlah thread worker untuk pemindaian mendalam (default: jumlah inti CPU, maks. 8) |
| `--list` | Cetak setiap file yang ditemukan ke layar (`stdout`) |
| `--recover DIR` | Pulihkan semua file yang cocok ke direktori `DIR` (harus berada di disk berbeda) |

## Mengapa cepat

```
            ┌──────────── 1 thread pembaca ────────────┐
 disk ──►   │ baca berurutan, blok 16 MB, buffer daur  │
            │ ulang (4 KB-aligned, tanpa alokasi baru) │
            └───────────────┬──────────────────────────┘
                            │ antrean terbatas (backpressure)
          ┌─────────────────┼─────────────────┐
          ▼                 ▼                 ▼
     worker 1          worker 2   …      worker N
  cek header hanya di awal sektor (1 dari 512 posisi)
  → resolver format menghitung ukuran pasti dari struktur
  → melompati payload (box MP4, chunk RIFF) tanpa membacanya
```

- **Satu aliran baca berurutan.** Pola akses tercepat untuk HDD, SSD, maupun
  USB. Thread pembaca tidak pernah menunggu CPU.
- **Hanya ruang kosong.** Dengan bitmap alokasi NTFS/FAT/exFAT, area yang
  dipakai file aktif dilewati. Pada disk yang 80% terisi, I/O-nya 5× lebih
  sedikit, dan file yang sudah ditemukan lewat metadata tidak dipindai ulang.
- **Cek per sektor, bukan per byte.** Di semua filesystem, file dimulai di
  batas cluster, jadi cukup memeriksa awal sektor. Mode per byte (Aho-Corasick
  SIMD, ~4,4 GB/s) tetap tersedia.
- **Resolver yang melompat.** Video 4 GB dianalisis dengan beberapa pembacaan
  kecil (header box), bukan 4 GB I/O tambahan.
- **Deduplikasi seketika.** Data di dalam file yang sudah tervalidasi (thumbnail
  EXIF, frame MP3, paket TS) tidak dianalisis ulang.

### Benchmark

Diukur pada image disk 4 GB berisi 1.356 file campuran yang sudah ada di cache
RAM, sehingga yang terukur adalah kecepatan engine, bukan kecepatan disk:

| Mode | 1 thread | 4 thread |
|---|---|---|
| Per sektor (default) | 5,85 GB/s | 4,47 GB/s |
| Per byte | 1,80 GB/s | 4,37 GB/s |

Pada disk sungguhan, pemindaian berjalan pada kecepatan baca disk itu sendiri:
±150 MB/s untuk HDD, ±500 MB/s untuk SSD SATA, dan beberapa GB/s untuk NVMe.

## Keterbatasan (jujur)

- **SSD dengan TRIM.** Setelah file dihapus, SSD biasanya mengosongkan
  blok-bloknya dalam hitungan detik hingga menit. Tidak ada perangkat lunak yang
  bisa memulihkan data yang sudah di-TRIM. Flashdisk dan kartu SD umumnya tidak
  terkena TRIM.
- **File terfragmentasi di FAT/exFAT.** Rantai cluster dihapus bersama filenya,
  jadi Pulih mengasumsikan cluster berurutan (benar untuk sebagian besar foto
  dan video kamera). Bila asumsi itu tidak berlaku, file ditandai *Sebagian*.
  Di NTFS, file terfragmentasi pulih utuh.
- **NTFS terkompresi atau terenkripsi (EFS).** File seperti ini dilewati, dan
  jumlahnya dicatat di log.
- **Hasil pencarian mendalam tidak punya nama asli.** Bila isinya punya judul
  (dokumen, lagu), judul itu dipakai sebagai nama; selain itu file diberi nama
  seperti `f000012345.jpg` (nomor sektor), dikelompokkan per tipe.
- **ext4/APFS** hanya didukung lewat pencarian mendalam.

## Membangun dari sumber & Membuat Installer

Perlu [Rust](https://rustup.rs) 1.95 atau lebih baru (`rustup update`).

### 1. Membangun Binary Release (Windows / Linux)

```sh
cargo build --release
# hasil: target/release/pulih (GUI) dan target/release/pulih-cli (CLI)
```

### 2. Membuat Installer Windows (`Pulih-Setup-1.2.0.exe`)

Pastikan [Inno Setup 6](https://jrsoftware.org/isinfo.php) sudah terpasang, lalu jalankan skrip pembangun installer dari PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-installer.ps1
# hasil: dist\Pulih-Setup-1.2.0.exe
```

Skrip ini secara otomatis membuat ikon aplikasi (`installer\pulih.ico`), mengompilasi binary release (`pulih.exe` dan `pulih-cli.exe`), lalu membungkusnya menjadi installer Windows menggunakan konfigurasi `installer\pulih.iss`.

### 3. Kompilasi Silang ke Windows dari Linux

```sh
rustup target add x86_64-pc-windows-gnu
sudo apt install gcc-mingw-w64-x86-64
cargo build --release --target x86_64-pc-windows-gnu
# hasil: target/x86_64-pc-windows-gnu/release/pulih.exe (manifest Administrator tertanam)
```

Di Linux, GUI membutuhkan pustaka X11/Wayland dan OpenGL yang umumnya sudah
tersedia di desktop.

## Struktur kode

```
crates/
  pulih-core/        engine (tanpa dependensi GUI)
    device.rs        akses disk mentah read-only, aligned I/O, toleran sektor rusak
    volume.rs        MBR / GPT / deteksi filesystem
    fs/ntfs.rs       pembaca MFT, runlist, $Bitmap, attribute list
    fs/fat.rs        FAT12/16/32, LFN terhapus, heuristik high-word FAT32
    fs/exfat.rs      entry set terhapus, bitmap alokasi
    carve/mod.rs     pipeline pembaca + worker paralel
    carve/formats.rs signature dan resolver ukuran untuk ±30 format
    carve/window.rs  akses byte untuk resolver (memori dulu, baru disk)
    carve/meta.rs    identitas dari isi: judul dokumen/PDF, EXIF, ID3/Vorbis, tanggal video
    target.rs        kriteria "file yang dicari": pola nama, jenis, folder, tanggal, ukuran
    inspect.rs       pratinjau isi dokumen & metadata untuk hasil mana pun
    scan.rs          orkestrasi, progres lock-free, aliran event
    recover.rs       penulisan hasil (berurutan sesuai posisi di disk)
  pulih-gui/         aplikasi desktop (egui)
    app.rs           state aplikasi, navigasi samping, halaman Tips
    home.rs          halaman awal: kartu drive/lokasi, pencarian, bar Pindai
    results.rs       hasil: progres, panel Jalur/Jenis, filter, daftar & thumbnail, pratinjau
    dialogs.rs       dialog pemulihan (tujuan, progres, selesai)
    theme.rs         sistem desain: palet terang/gelap, font, widget & ikon vektor
    find.rs          kriteria pencarian, pemilih jenis/tanggal/ukuran, pohon folder
    thumbs.rs        thumbnail foto di latar (pratinjau EXIF untuk JPEG)
  pulih-cli/         antarmuka baris perintah
installer/           konfigurasi Inno Setup (pulih.iss) dan ikon aplikasi (pulih.ico)
scripts/             skrip pembuatan installer (build-installer.ps1) & pengujian end-to-end
```

## Pengujian

```sh
cargo test --workspace            # unit test + uji carving sintetis
./scripts/gen_corpus.sh           # buat korpus file uji (ImageMagick, ffmpeg, …)
python3 scripts/carve_test.py     # 36 format diletakkan di antara data acak
./scripts/fs_test.sh ntfs         # buat FS, salin file, hapus, pulihkan, cocokkan MD5
./scripts/fs_test.sh fat32        # juga: fat16, exfat
```

Hasil pada versi ini:

- Carving: 36/36 file identik byte-per-byte, tanpa false positive.
- NTFS, FAT32, FAT16, dan exFAT: semua file yang dihapus pulih identik,
  termasuk folder yang ikut terhapus dan file NTFS terfragmentasi.
- Setelah quick format: 16/16 file pulih identik lewat pencarian mendalam.

## Lisensi

MIT. Antarmuka memakai font [Plus Jakarta Sans](https://github.com/tokotype/PlusJakartaSans)
(SIL Open Font License 1.1, lihat `crates/pulih-gui/assets/OFL-PlusJakartaSans.txt`).
