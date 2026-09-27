pub mod calendar;
pub mod cluster;
pub mod db;
pub mod exif;
pub mod geo;
pub mod model;
pub mod scan;
pub mod store;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http: {0}")]
    Http(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
pub(crate) mod testutil {
    use ::exif::{experimental::Writer, Field, In, Rational, Tag, Value};

    pub fn ascii(tag: Tag, s: &str) -> Field {
        Field { tag, ifd_num: In::PRIMARY, value: Value::Ascii(vec![s.as_bytes().to_vec()]) }
    }

    pub fn dms(tag: Tag, d: u32, m: u32, s_num: u32, s_den: u32) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Rational(vec![
                Rational { num: d, denom: 1 },
                Rational { num: m, denom: 1 },
                Rational { num: s_num, denom: s_den },
            ]),
        }
    }

    pub fn tiff(fields: &[Field]) -> Vec<u8> {
        let mut w = Writer::new();
        for f in fields {
            w.push_field(f);
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        w.write(&mut buf, false).unwrap();
        buf.into_inner()
    }

    pub fn jpeg(tiff: &[u8]) -> Vec<u8> {
        let len = (2 + 6 + tiff.len()) as u16;
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE1];
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(b"Exif\0\0");
        v.extend_from_slice(tiff);
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }
}
