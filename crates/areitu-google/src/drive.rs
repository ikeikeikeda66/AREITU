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
