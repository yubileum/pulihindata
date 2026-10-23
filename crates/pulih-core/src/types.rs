use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Image,
    Video,
    Audio,
    Document,
    Archive,
    Other,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Image,
        Category::Video,
        Category::Audio,
        Category::Document,
        Category::Archive,
        Category::Other,
    ];

    #[inline]
    pub fn bit(self) -> u8 {
        1 << self as u8
    }

    pub fn from_ext(ext: &str) -> Category {
        let e = ext.to_ascii_lowercase();
        match e.as_str() {
            "jpg" | "jpeg" | "jpe" | "jfif" | "png" | "gif" | "bmp" | "tif" | "tiff" | "webp"
            | "heic" | "heif" | "avif" | "psd" | "psb" | "cr2" | "cr3" | "nef" | "nrw" | "arw"
            | "dng" | "orf" | "rw2" | "raf" | "pef" | "srw" | "ico" | "svg" => Category::Image,
            "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" | "f4v" | "wmv" | "asf" | "3gp"
            | "3g2" | "m4v" | "mpg" | "mpeg" | "vob" | "ts" | "mts" | "m2ts" | "ogv" => {
                Category::Video
            }
            "mp3" | "wav" | "flac" | "ogg" | "oga" | "opus" | "spx" | "m4a" | "m4b" | "wma"
            | "aac" | "mid" | "midi" | "aif" | "aiff" | "aifc" | "amr" => Category::Audio,
            "pdf" | "doc" | "docx" | "docm" | "dot" | "dotx" | "xls" | "xlsx" | "xlsm" | "ppt"
            | "pptx" | "pps" | "ppsx" | "odt" | "ods" | "odp" | "odg" | "rtf" | "txt" | "csv"
            | "epub" | "msg" | "vsd" | "vsdx" | "md" | "xml" | "html" | "htm" | "ole" => {
                Category::Document
            }
            "zip" | "rar" | "7z" | "gz" | "tgz" | "tar" | "bz2" | "xz" | "zst" | "apk" | "jar"
            | "iso" | "cab" => Category::Archive,
            _ => Category::Other,
        }
    }
}

/// Where a result came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Origin {
    Ntfs,
    Fat,
    ExFat,
    Carved,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::Ntfs => "NTFS",
            Origin::Fat => "FAT",
            Origin::ExFat => "exFAT",
            Origin::Carved => "Signature",
        }
    }
}

/// How likely a result is to come back intact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Condition {
    /// Filesystem: none of the file's clusters have been reused.
    /// Carved: the file's internal structure was walked end to end.
    Good,
    /// Size could not be verified (estimated end of file).
    Unknown,
    /// Some clusters were reused / the carved structure ends early.
    Partial,
    /// Every cluster has been reused by another file.
    Overwritten,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    /// Absolute byte offset on the scanned device.
    pub offset: u64,
    pub len: u64,
    /// Sparse run: reads back as zeros.
    pub sparse: bool,
}

#[derive(Clone, Debug)]
pub enum FileData {
    Extents(Vec<Extent>),
    /// Small NTFS files stored inside their MFT record.
    Resident(Arc<[u8]>),
}

#[derive(Clone, Debug)]
pub struct FoundFile {
    pub id: u64,
    pub name: String,
    /// Original folder (for filesystem results) — shared between siblings.
    pub dir: Arc<str>,
    pub ext: String,
    pub category: Category,
    pub size: u64,
    /// Unix seconds.
    pub modified: Option<i64>,
    pub origin: Origin,
    pub condition: Condition,
    /// Index into `ScanSummary::volumes`, or `u16::MAX` for whole-device carving.
    pub volume: u16,
    pub data: FileData,
    /// Extra identification read from the file's content (camera model,
    /// song artist, document title…), mostly for signature results.
    pub info: Option<String>,
}

impl FoundFile {
    pub fn first_offset(&self) -> u64 {
        match &self.data {
            FileData::Extents(e) => e.iter().find(|x| !x.sparse).map_or(u64::MAX, |x| x.offset),
            FileData::Resident(_) => u64::MAX,
        }
    }

    pub fn full_path(&self) -> String {
        if self.dir.is_empty() {
            self.name.clone()
        } else {
            format!("{}/{}", self.dir, self.name)
        }
    }
}

/// Lowercase extension of a file name ("" when none).
pub fn ext_of(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 && name.len() - i <= 8 => name[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}
