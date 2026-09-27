#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    #[serde(default, rename = "modifiedTime")]
    pub modified_time: Option<String>,
    #[serde(default, rename = "md5Checksum")]
    pub md5_checksum: Option<String>,
}

#[derive(serde::Deserialize)]
struct FilesListResponse {
    #[serde(default)]
    files: Vec<DriveFile>,
}

pub trait DriveApi {
    fn find_db_file(&self, access_token: &str, name: &str) -> crate::Result<Option<DriveFile>>;
    fn upload_create(&self, access_token: &str, name: &str, content: &[u8]) -> crate::Result<DriveFile>;
    fn upload_update(&self, access_token: &str, file_id: &str, content: &[u8]) -> crate::Result<DriveFile>;
    fn download(&self, access_token: &str, file_id: &str) -> crate::Result<Vec<u8>>;
}

pub struct DriveClient {
    client: reqwest::blocking::Client,
    base_url: String,
    upload_base_url: String,
}

impl DriveClient {
    pub fn new() -> crate::Result<DriveClient> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| crate::Error::Http(e.to_string()))?;
        Ok(DriveClient {
            client,
            base_url: "https://www.googleapis.com".to_owned(),
            upload_base_url: "https://www.googleapis.com/upload".to_owned(),
        })
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.trim_end_matches('/').to_owned();
        self
    }

    pub fn with_upload_base_url(mut self, url: &str) -> Self {
        self.upload_base_url = url.trim_end_matches('/').to_owned();
        self
    }
}

/// Builds a `multipart/related` body: a JSON metadata part followed by an
/// `application/octet-stream` part carrying `content` verbatim. `boundary`
/// must not occur anywhere in `metadata` or `content` (see `make_boundary`).
pub fn build_multipart_related_body(boundary: &str, metadata: &serde_json::Value, content: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n").as_bytes());
    body.extend_from_slice(metadata.to_string().as_bytes());
    body.extend_from_slice(format!("\r\n--{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

/// Generates a random boundary token and verifies it cannot appear in the
/// multipart body: neither in the JSON metadata nor in the raw binary
/// content. Retries (astronomically unlikely) on collision.
fn make_boundary(metadata: &serde_json::Value, content: &[u8]) -> String {
    let metadata_bytes = metadata.to_string().into_bytes();
    loop {
        let random_bytes: [u8; 16] = std::array::from_fn(|_| rand::random::<u8>());
        let candidate = format!(
            "areitu-sync-boundary-{}",
            random_bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        let candidate_bytes = candidate.as_bytes();
        if !bytes_contain(&metadata_bytes, candidate_bytes) && !bytes_contain(content, candidate_bytes) {
            return candidate;
        }
    }
}

impl DriveClient {
    fn multipart_request(
        &self,
        method: reqwest::Method,
        url: &str,
        access_token: &str,
        metadata: &serde_json::Value,
        content: &[u8],
    ) -> crate::Result<DriveFile> {
        let boundary = make_boundary(metadata, content);
        let body = build_multipart_related_body(&boundary, metadata, content);
        let resp = self
            .client
            .request(method, url)
            .bearer_auth(access_token)
            .header("Content-Type", format!("multipart/related; boundary={boundary}"))
            .body(body)
            .send()
            .map_err(|e| crate::Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(crate::Error::Http(format!("drive upload status {}", resp.status())));
        }
        resp.json().map_err(|e| crate::Error::Http(e.to_string()))
    }
}

impl DriveApi for DriveClient {
    fn find_db_file(&self, access_token: &str, name: &str) -> crate::Result<Option<DriveFile>> {
        let resp = self
            .client
            .get(format!("{}/drive/v3/files", self.base_url))
            .bearer_auth(access_token)
            .query(&[
                ("spaces", "appDataFolder"),
                ("fields", "files(id,name,modifiedTime,md5Checksum)"),
                ("q", &format!("name = '{name}'")),
            ])
            .send()
            .map_err(|e| crate::Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(crate::Error::Http(format!("drive files.list status {}", resp.status())));
        }
        let parsed: FilesListResponse = resp.json().map_err(|e| crate::Error::Http(e.to_string()))?;
        Ok(parsed.files.into_iter().next())
    }

    fn upload_create(&self, access_token: &str, name: &str, content: &[u8]) -> crate::Result<DriveFile> {
        let metadata = serde_json::json!({"name": name, "parents": ["appDataFolder"]});
        let url = format!("{}/drive/v3/files?uploadType=multipart", self.upload_base_url);
        self.multipart_request(reqwest::Method::POST, &url, access_token, &metadata, content)
    }

    fn upload_update(&self, access_token: &str, file_id: &str, content: &[u8]) -> crate::Result<DriveFile> {
        let metadata = serde_json::json!({});
        let url = format!("{}/drive/v3/files/{file_id}?uploadType=multipart", self.upload_base_url);
        self.multipart_request(reqwest::Method::PATCH, &url, access_token, &metadata, content)
    }

    fn download(&self, access_token: &str, file_id: &str) -> crate::Result<Vec<u8>> {
        let resp = self
            .client
            .get(format!("{}/drive/v3/files/{file_id}", self.base_url))
            .bearer_auth(access_token)
            .query(&[("alt", "media")])
            .send()
            .map_err(|e| crate::Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(crate::Error::Http(format!("drive download status {}", resp.status())));
        }
        Ok(resp.bytes().map_err(|e| crate::Error::Http(e.to_string()))?.to_vec())
    }
}

#[cfg(test)]
mod find_db_file_tests {
    use super::*;
    use httpmock::MockServer;

    #[test]
    fn returns_none_when_no_file_matches() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/drive/v3/files");
            then.status(200).json_body(serde_json::json!({"files": []}));
        });
        let drive = DriveClient::new().unwrap().with_base_url(&server.base_url());
        assert_eq!(drive.find_db_file("token", "areitu.db").unwrap(), None);
    }

    #[test]
    fn returns_first_match_when_duplicates_exist() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/drive/v3/files");
            then.status(200).json_body(serde_json::json!({"files": [
                {"id": "file-1", "name": "areitu.db", "modifiedTime": "2026-09-27T00:00:00Z", "md5Checksum": "abc"},
                {"id": "file-2", "name": "areitu.db", "modifiedTime": "2026-09-26T00:00:00Z", "md5Checksum": "def"}
            ]}));
        });
        let drive = DriveClient::new().unwrap().with_base_url(&server.base_url());
        let found = drive.find_db_file("token", "areitu.db").unwrap().unwrap();
        assert_eq!(found.id, "file-1");
    }

    #[test]
    fn non_success_status_is_an_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/drive/v3/files");
            then.status(401);
        });
        let drive = DriveClient::new().unwrap().with_base_url(&server.base_url());
        assert!(drive.find_db_file("token", "areitu.db").is_err());
    }
}

#[cfg(test)]
mod upload_tests {
    use super::*;
    use httpmock::MockServer;

    #[test]
    fn multipart_body_contains_boundaries_metadata_and_content() {
        let metadata = serde_json::json!({"name": "areitu.db", "parents": ["appDataFolder"]});
        let body = build_multipart_related_body("BOUNDARY", &metadata, b"binary-db-bytes");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--BOUNDARY\r\n"));
        assert!(text.contains("Content-Type: application/json"));
        assert!(text.contains("\"name\":\"areitu.db\""));
        assert!(text.contains("Content-Type: application/octet-stream"));
        assert!(text.contains("binary-db-bytes"));
        assert!(text.ends_with("--BOUNDARY--\r\n"));
    }

    #[test]
    fn upload_create_posts_to_upload_endpoint_with_parents() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/upload/drive/v3/files")
                .query_param("uploadType", "multipart")
                .body_includes("appDataFolder");
            then.status(200).json_body(serde_json::json!({"id": "new-file-1", "name": "areitu.db"}));
        });
        let drive = DriveClient::new().unwrap().with_upload_base_url(&format!("{}/upload", server.base_url()));
        let created = drive.upload_create("token", "areitu.db", b"db-bytes").unwrap();
        mock.assert();
        assert_eq!(created.id, "new-file-1");
    }

    #[test]
    fn upload_update_patches_existing_file_id() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::PATCH)
                .path("/upload/drive/v3/files/existing-file-1")
                .query_param("uploadType", "multipart");
            then.status(200).json_body(serde_json::json!({"id": "existing-file-1", "name": "areitu.db"}));
        });
        let drive = DriveClient::new().unwrap().with_upload_base_url(&format!("{}/upload", server.base_url()));
        let updated = drive.upload_update("token", "existing-file-1", b"new-db-bytes").unwrap();
        mock.assert();
        assert_eq!(updated.id, "existing-file-1");
    }
}

#[cfg(test)]
mod download_tests {
    use super::*;
    use httpmock::MockServer;

    #[test]
    fn downloads_raw_bytes_with_alt_media() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/drive/v3/files/remote-file-1")
                .query_param("alt", "media");
            then.status(200).body(b"sqlite-db-content");
        });
        let drive = DriveClient::new().unwrap().with_base_url(&server.base_url());
        let bytes = drive.download("token", "remote-file-1").unwrap();
        mock.assert();
        assert_eq!(bytes, b"sqlite-db-content".to_vec());
    }

    #[test]
    fn missing_file_is_an_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/drive/v3/files/missing");
            then.status(404);
        });
        let drive = DriveClient::new().unwrap().with_base_url(&server.base_url());
        assert!(drive.download("token", "missing").is_err());
    }
}
