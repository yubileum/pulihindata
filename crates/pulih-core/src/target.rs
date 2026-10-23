//! "What am I looking for?" — file-name patterns, specific types, original
//! folder, size and date limits. Shared by the GUI, the CLI and the deep-scan
//! planner (which only looks for the types that can match).

use crate::types::{FoundFile, Origin};

/// Canonical spelling of an extension, so `jpeg`/`JPG`/`.jpg` all compare equal.
pub fn canon_ext(e: &str) -> String {
    let e = e
        .trim()
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .to_ascii_lowercase();
    match e.as_str() {
        "jpeg" | "jpe" | "jfif" => "jpg".into(),
        "tiff" => "tif".into(),
        "mpeg" | "vob" => "mpg".into(),
        "m2ts" => "mts".into(),
        "aiff" | "aifc" => "aif".into(),
        "midi" => "mid".into(),
        "db" | "sqlite3" => "sqlite".into(),
        "htm" => "html".into(),
        _ => e,
    }
}

/// Case-insensitive glob with `*` and `?` (pattern already lowercase).
pub fn glob(pat: &str, s: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = s.chars().flat_map(char::to_lowercase).collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ti;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

fn contains_ci(hay: &str, needle_lc: &str) -> bool {
    needle_lc.is_empty() || hay.to_lowercase().contains(needle_lc)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    /// Lowercase name patterns. Without `*`/`?` a pattern matches anywhere
    /// in the name *or the folder path* ("skripsi" finds "Skripsi
    /// Final.docx" and everything in a "Skripsi" folder); with wildcards it
    /// must match the whole file name.
    pub names: Vec<String>,
    /// Canonical extensions; empty = any type.
    pub exts: Vec<String>,
    /// Lowercase text the original folder must contain.
    pub folder: String,
    pub min_size: u64,
    /// 0 = no upper limit.
    pub max_size: u64,
    /// Modified-time range, Unix seconds.
    pub after: Option<i64>,
    pub before: Option<i64>,
    /// Signature results carry no original name, folder or (often) date.
    /// When true they still match if their type fits; when false they must
    /// match on what they do carry (a title read from the content, a date).
    pub include_unnamed: bool,
}

impl Default for Target {
    fn default() -> Self {
        Target {
            names: Vec::new(),
            exts: Vec::new(),
            folder: String::new(),
            min_size: 0,
            max_size: 0,
            after: None,
            before: None,
            include_unnamed: true,
        }
    }
}

impl Target {
    /// Split user input like `skripsi*; *.docx, foto liburan` into patterns.
    pub fn set_names(&mut self, input: &str) {
        self.names = input
            .split([';', ','])
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty())
            .collect();
    }

    pub fn set_exts<'a>(&mut self, exts: impl IntoIterator<Item = &'a str>) {
        self.exts = exts
            .into_iter()
            .map(canon_ext)
            .filter(|e| !e.is_empty())
            .collect();
        self.exts.sort();
        self.exts.dedup();
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
            && self.exts.is_empty()
            && self.folder.is_empty()
            && self.min_size == 0
            && self.max_size == 0
            && self.after.is_none()
            && self.before.is_none()
    }

    /// Extension a name pattern pins down (`*.docx`, `skripsi.pdf`), if any.
    fn pattern_ext(p: &str) -> Option<String> {
        let (_, e) = p.rsplit_once('.')?;
        (!e.is_empty() && e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
            .then(|| canon_ext(e))
    }

    /// Types a deep scan has to look for (`None` = all types).
    pub fn wanted_exts(&self) -> Option<Vec<String>> {
        let mut v = self.exts.clone();
        if !self.names.is_empty() {
            let from_names: Option<Vec<String>> =
                self.names.iter().map(|p| Self::pattern_ext(p)).collect();
            match from_names {
                // Every pattern names its type: those types are all we need
                // (narrowed further by explicit types, if any).
                Some(n) if v.is_empty() => v = n,
                Some(n) => v.retain(|e| n.contains(e)),
                None => {}
            }
        }
        if v.is_empty() && self.exts.is_empty() {
            return None;
        }
        v.sort();
        v.dedup();
        Some(v)
    }

    fn name_matches(&self, name: &str, dir: &str) -> bool {
        self.names.iter().any(|p| {
            if p.contains(['*', '?']) {
                glob(p, name)
            } else {
                contains_ci(name, p) || contains_ci(dir, p)
            }
        })
    }

    /// The user narrowed the search to types that include `ext`.
    fn type_narrowed(&self, ext: &str) -> bool {
        !self.exts.is_empty()
            || self
                .names
                .iter()
                .any(|p| Self::pattern_ext(p).as_deref() == Some(ext))
    }

    pub fn matches(&self, f: &FoundFile) -> bool {
        if f.size < self.min_size || (self.max_size > 0 && f.size > self.max_size) {
            return false;
        }
        let ext = canon_ext(&f.ext);
        if !self.exts.is_empty() && !self.exts.contains(&ext) {
            return false;
        }
        // Signature results have no original name/folder and often no date.
        // A criterion they cannot be checked against lets them through only
        // when the user also picked their type (then it *could* be the file).
        let carved = f.origin == Origin::Carved;
        let unknown_ok = carved && self.include_unnamed && self.type_narrowed(&ext);
        match f.modified {
            Some(t) => {
                if self.after.is_some_and(|a| t < a) || self.before.is_some_and(|b| t >= b) {
                    return false;
                }
            }
            None if (self.after.is_some() || self.before.is_some()) && !unknown_ok => return false,
            None => {}
        }
        if !self.folder.is_empty() {
            let ok = if carved {
                unknown_ok
            } else {
                contains_ci(&f.dir, &self.folder)
            };
            if !ok {
                return false;
            }
        }
        if !self.names.is_empty() {
            // A carved file may still carry a name read from its content
            // (document title, song tag): that is checked first.
            let named = self.name_matches(&f.name, &f.dir)
                || f.info.as_deref().is_some_and(|i| self.name_matches(i, ""));
            if !named && !unknown_ok {
                return false;
            }
        }
        true
    }
}

/// Parse `YYYY-MM-DD` (also `DD-MM-YYYY` / `DD/MM/YYYY`) as Unix seconds at 00:00.
pub fn parse_date(s: &str) -> Option<i64> {
    let parts: Vec<i64> = s
        .trim()
        .split(['-', '/', '.'])
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    let [a, b, c] = parts[..] else { return None };
    let (y, m, d) = if a > 31 { (a, b, c) } else { (c, b, a) };
    if !(1970..=2100).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(crate::bytes::days_from_civil(y, m, d) * 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use std::sync::Arc;

    fn file(name: &str, dir: &str, origin: Origin, info: Option<&str>) -> FoundFile {
        FoundFile {
            id: 0,
            name: name.into(),
            dir: Arc::from(dir),
            ext: ext_of(name),
            category: Category::from_ext(&ext_of(name)),
            size: 1000,
            modified: Some(1_700_000_000),
            origin,
            condition: Condition::Good,
            volume: 0,
            data: FileData::Extents(vec![]),
            info: info.map(String::from),
        }
    }

    #[test]
    fn globbing() {
        assert!(glob("skripsi*.docx", "Skripsi Final.DOCX"));
        assert!(glob("img_????.jpg", "IMG_2024.jpg"));
        assert!(!glob("*.docx", "a.doc"));
        assert!(glob("*", ""));
    }

    #[test]
    fn matching() {
        let mut t = Target::default();
        t.set_names("skripsi");
        assert!(t.matches(&file("Skripsi Final.docx", "Dokumen", Origin::Ntfs, None)));
        assert!(!t.matches(&file("laporan.docx", "Dokumen", Origin::Ntfs, None)));
        assert!(t.matches(&file("Bab 1.docx", "Kuliah/Skripsi", Origin::Ntfs, None)));
        // Unnamed carved document: only kept once the type is narrowed down.
        let carved = file("f000001234.docx", "", Origin::Carved, None);
        assert!(!t.matches(&carved));
        t.set_names("skripsi*.docx");
        assert!(t.matches(&carved));
        t.include_unnamed = false;
        assert!(!t.matches(&carved));
        t.set_names("skripsi");
        // Carved file whose content title matches.
        let titled = file(
            "Skripsi Bab 1.docx",
            "",
            Origin::Carved,
            Some("Skripsi Bab 1"),
        );
        assert!(t.matches(&titled));

        let mut t = Target::default();
        t.set_names("*.pdf");
        assert_eq!(t.wanted_exts(), Some(vec!["pdf".to_string()]));
        assert!(!t.matches(&file("a.docx", "", Origin::Fat, None)));
        t.set_exts(["JPEG", "png"]);
        assert_eq!(t.exts, vec!["jpg", "png"]);

        let mut t = Target {
            folder: "kuliah".into(),
            ..Default::default()
        };
        assert!(t.matches(&file(
            "x.pdf",
            "Documents/Kuliah/Semester 5",
            Origin::Ntfs,
            None
        )));
        assert!(!t.matches(&file("x.pdf", "Downloads", Origin::Ntfs, None)));
        t.after = parse_date("2024-01-01");
        assert!(!t.matches(&file("x.pdf", "Kuliah", Origin::Ntfs, None)));
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date("2023-11-14"), Some(1_699_920_000));
        assert_eq!(parse_date("14/11/2023"), Some(1_699_920_000));
        assert_eq!(parse_date("kemarin"), None);
    }
}
