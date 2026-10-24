//! Pulih core: a fast, read-only deleted-file recovery engine.
//!
//! * **Quick scan** — reads NTFS / FAT12/16/32 / exFAT metadata directly from
//!   the raw device and lists deleted files with their original names,
//!   folders, dates, and an estimate of whether their clusters were reused.
//! * **Deep scan** — streams the (unallocated) space of the device through a
//!   parallel signature carver that recognises ~30 formats and computes each
//!   file's exact size from its internal structure.

pub mod bytes;
pub mod carve;
pub mod device;
pub mod fs;
pub mod inspect;
pub mod recover;
pub mod scan;
pub mod target;
pub mod types;
pub mod volume;

pub use device::{
    list_devices, locate_folder, same_disk, Device, DeviceInfo, DeviceKind, MemSource, ReadAt,
};
pub use inspect::{inspect, Inspection};
pub use recover::{read_head, recover, RecoverOptions, RecoverProgress, RecoverReport};
pub use scan::{start_scan, Phase, ScanEvent, ScanHandle, ScanOptions, ScanSummary, Snapshot};
pub use target::{canon_ext, parse_date, Target};
pub use types::{Category, Condition, Extent, FileData, FoundFile, Origin};
pub use volume::{find_volumes, FsType, Volume};
