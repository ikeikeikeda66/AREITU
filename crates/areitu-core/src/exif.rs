use chrono::NaiveDateTime;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use ::exif::{Exif, In, Reader, Tag, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct PhotoMeta {
    pub taken_at: NaiveDateTime,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

pub fn parse_exif(exif: &Exif) -> Option<PhotoMeta> {
    let field = exif.get_field(Tag::DateTimeOriginal, In::PRIMARY)?;
    let taken_at = match &field.value {
        Value::Ascii(v) if !v.is_empty() => {
            let s = std::str::from_utf8(&v[0]).ok()?.trim_end_matches('\0');
            NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").ok()?
        }
        _ => return None,
    };
    let lat = coord(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, b'S');
    let lon = coord(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, b'W');
    let (lat, lon) = match (lat, lon) {
        (Some(a), Some(b)) if !(a == 0.0 && b == 0.0) => (Some(a), Some(b)),
        _ => (None, None),
    };
    Some(PhotoMeta { taken_at, lat, lon })
}

fn coord(exif: &Exif, tag: Tag, ref_tag: Tag, negative: u8) -> Option<f64> {
    let v = match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Rational(v) if v.len() == 3 && v.iter().all(|r| r.denom != 0) => v,
        _ => return None,
    };
    let deg = v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0;
    let is_negative = matches!(
        exif.get_field(ref_tag, In::PRIMARY).map(|f| &f.value),
        Some(Value::Ascii(a)) if a.first().and_then(|s| s.first()) == Some(&negative)
    );
    Some(if is_negative { -deg } else { deg })
}

pub fn read_photo(path: &Path) -> Option<PhotoMeta> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    let exif = Reader::new().read_from_container(&mut reader).ok()?;
    parse_exif(&exif)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{ascii, dms, tiff};
    use ::exif::{Reader, Tag};

    fn parse(fields: &[::exif::Field]) -> Option<PhotoMeta> {
        let exif = Reader::new().read_raw(tiff(fields)).unwrap();
        parse_exif(&exif)
    }

    fn dt() -> ::exif::Field {
        ascii(Tag::DateTimeOriginal, "2026:09:01 12:34:56")
    }

    #[test]
    fn reads_time_and_north_east_gps() {
        let m = parse(&[
            dt(),
            dms(Tag::GPSLatitude, 35, 40, 522, 10),
            ascii(Tag::GPSLatitudeRef, "N"),
            dms(Tag::GPSLongitude, 139, 46, 0, 1),
            ascii(Tag::GPSLongitudeRef, "E"),
        ])
        .unwrap();
        assert_eq!(m.taken_at.to_string(), "2026-09-01 12:34:56");
        assert!((m.lat.unwrap() - 35.681166).abs() < 1e-5);
        assert!((m.lon.unwrap() - 139.766666).abs() < 1e-5);
    }

    #[test]
    fn south_and_west_are_negative() {
        let m = parse(&[
            dt(),
            dms(Tag::GPSLatitude, 33, 52, 0, 1),
            ascii(Tag::GPSLatitudeRef, "S"),
            dms(Tag::GPSLongitude, 70, 40, 0, 1),
            ascii(Tag::GPSLongitudeRef, "W"),
        ])
        .unwrap();
        assert!(m.lat.unwrap() < 0.0);
        assert!(m.lon.unwrap() < 0.0);
    }

    #[test]
    fn no_gps_gives_time_only() {
        let m = parse(&[dt()]).unwrap();
        assert_eq!(m.lat, None);
        assert_eq!(m.lon, None);
    }

    #[test]
    fn zero_zero_gps_is_treated_as_missing() {
        let m = parse(&[
            dt(),
            dms(Tag::GPSLatitude, 0, 0, 0, 1),
            ascii(Tag::GPSLatitudeRef, "N"),
            dms(Tag::GPSLongitude, 0, 0, 0, 1),
            ascii(Tag::GPSLongitudeRef, "E"),
        ])
        .unwrap();
        assert_eq!(m.lat, None);
    }

    #[test]
    fn missing_datetime_returns_none() {
        assert_eq!(parse(&[ascii(Tag::Make, "Apple")]), None);
    }
}
