use rusqlite::Connection;
use std::path::Path;
use walkdir::WalkDir;

use crate::exif::read_photo;
use crate::model::{RawLog, Source};
use crate::store::upsert_raw_log;
use crate::{Error, Result};

const PHOTO_EXTENSIONS: &[&str] = &["jpg", "jpeg", "heic", "heif", "png", "tif", "tiff"];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub seen: usize,
    pub inserted: usize,
    pub skipped: usize,
}

pub fn scan_photos(conn: &Connection, dir: &Path) -> Result<ScanReport> {
    if !dir.is_dir() {
        return Err(Error::Invalid(format!("not a directory: {}", dir.display())));
    }
    let mut report = ScanReport::default();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let is_photo = entry.file_type().is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| PHOTO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false);
        if !is_photo {
            continue;
        }
        report.seen += 1;
        let Some(meta) = read_photo(path) else {
            report.skipped += 1;
            continue;
        };
        upsert_raw_log(
            conn,
            &RawLog {
                source: Source::Photo,
                source_id: path.to_string_lossy().into_owned(),
                occurred_at: meta.taken_at,
                ended_at: None,
                lat: meta.lat,
                lon: meta.lon,
                text: None,
            },
        )?;
        report.inserted += 1;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::store::unassigned_raw_logs;
    use crate::testutil::{ascii, dms, jpeg, tiff};
    use ::exif::Tag;

    fn photo_bytes() -> Vec<u8> {
        jpeg(&tiff(&[
            ascii(Tag::DateTimeOriginal, "2026:09:01 12:00:00"),
            dms(Tag::GPSLatitude, 35, 40, 0, 1),
            ascii(Tag::GPSLatitudeRef, "N"),
            dms(Tag::GPSLongitude, 139, 46, 0, 1),
            ascii(Tag::GPSLongitudeRef, "E"),
        ]))
    }

    #[test]
    fn scans_nested_photos_and_skips_junk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/IMG_0001.JPG"), photo_bytes()).unwrap();
        std::fs::write(dir.path().join("broken.jpg"), b"not a jpeg").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"hello").unwrap();

        let c = open_in_memory().unwrap();
        let r = scan_photos(&c, dir.path()).unwrap();

        assert_eq!(r, ScanReport { seen: 2, inserted: 1, skipped: 1 });
        let logs = unassigned_raw_logs(&c).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].1.source, Source::Photo);
        assert!(logs[0].1.lat.is_some());
    }

    #[test]
    fn rescan_does_not_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), photo_bytes()).unwrap();
        let c = open_in_memory().unwrap();
        scan_photos(&c, dir.path()).unwrap();
        scan_photos(&c, dir.path()).unwrap();
        assert_eq!(unassigned_raw_logs(&c).unwrap().len(), 1);
    }

    #[test]
    fn missing_dir_is_an_error() {
        let c = open_in_memory().unwrap();
        assert!(scan_photos(&c, Path::new("/no/such/dir/areitu")).is_err());
    }
}
