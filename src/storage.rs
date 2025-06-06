use anyhow::{Result, anyhow};
use chrono::NaiveDateTime;
use serde::Deserialize;
use ureq::{
    Agent, Body, SendBody,
    http::{HeaderValue, Request, Response, StatusCode, header},
    middleware::MiddlewareNext,
};

const API_KEY_HEADER: &str = "AccessKey";
const USER_AGENT: &str = "bunnysync/0.1.0";
const APPLICATION_JSON: HeaderValue = HeaderValue::from_static("application/json");
const APPLICATION_OCTET_STREAM: HeaderValue = HeaderValue::from_static("application/octet-stream");
const ALL: HeaderValue = HeaderValue::from_static("*/*");

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StorageObject {
    pub guid: String,
    pub storage_zone_name: String,
    pub path: String,
    pub object_name: String,
    pub length: u64,
    pub last_changed: NaiveDateTime,
    pub is_directory: bool,
    pub date_created: NaiveDateTime,
}

pub fn base_url(region: &str) -> Option<String> {
    match region {
        "uk" => Some("https://uk.storage.bunnycdn.com".to_string()),
        "us_ny" | "ny" => Some("https://ny.storage.bunnycdn.com".to_string()),
        "us_la" | "la" => Some("https://la.storage.bunnycdn.com".to_string()),
        "sg" => Some("https://sg.storage.bunnycdn.com".to_string()),
        "se" => Some("https://se.storage.bunnycdn.com".to_string()),
        "br" => Some("https://br.storage.bunnycdn.com".to_string()),
        "sa" => Some("https://ja.storage.bunnycdn.com".to_string()),
        "au" | "au_syd" | "syd" => Some("https://syd.storage.bunnycdn.com".to_string()),
        "" | "de" => Some("https://storage.bunnycdn.com".to_string()),
        _ => None,
    }
}

pub fn agent(api_key: &str) -> Result<Agent> {
    // Set api key.
    let mut auth_value = HeaderValue::from_str(api_key)?;
    auth_value.set_sensitive(true);

    // Create headers middleware.
    let default_headers = move |mut req: Request<SendBody>,
                                next: MiddlewareNext|
          -> Result<Response<Body>, ureq::Error> {
        req.headers_mut().insert(API_KEY_HEADER, auth_value.clone());
        next.handle(req)
    };

    let config = Agent::config_builder()
        .user_agent(USER_AGENT)
        .https_only(true)
        .middleware(default_headers)
        .build();
    let agent: Agent = config.into();
    Ok(agent)
}

/// Get the list of objects at the destination
pub fn get_objects(agent: &Agent, base_url: &str, path: &str) -> Result<Vec<StorageObject>> {
    let url = format!("{}/{}", base_url, path);
    let mut response = agent
        .get(&url)
        .header(header::ACCEPT, APPLICATION_JSON)
        .call()?;

    match response.status() {
        StatusCode::UNAUTHORIZED => Err(anyhow!("Remote unauthorized")),
        StatusCode::NOT_FOUND => Err(anyhow!("Not found: Path {} does not exist", path)),
        StatusCode::FORBIDDEN => Err(anyhow!("Forbidden: Access denied to path {}", path)),
        _ if response.status().is_success() => {
            let records = response.body_mut().read_json::<Vec<StorageObject>>()?;
            Ok(records)
        }
        _ => Err(anyhow!(
            "Failed to get objects from {}: HTTP {}",
            url,
            response.status()
        )),
    }
}

/// Get all objects in a directory and its subdirectories.
pub fn get_all_objects(agent: &Agent, base_url: &str, path: &str) -> Result<Vec<StorageObject>> {
    let mut objects = Vec::new();
    let mut paths = vec![path.to_string()];

    while let Some(next_path) = paths.pop() {
        let records = get_objects(agent, base_url, &next_path)?;
        for record in &records {
            if record.is_directory {
                paths.push(format!("{}{}/", record.path, record.object_name));
            }
        }
        objects.extend(records);
    }
    Ok(objects)
}

/// Store an object.
pub fn put_object(agent: &Agent, base_url: &str, path: &str, data: &[u8]) -> Result<()> {
    let url = format!("{}/{}", base_url, path);
    let response = agent
        .put(&url)
        .header(header::CONTENT_TYPE, APPLICATION_OCTET_STREAM)
        .send(data.to_vec())?;

    match response.status() {
        StatusCode::UNAUTHORIZED => Err(anyhow!("Remote unauthorized")),
        StatusCode::NOT_FOUND => Err(anyhow!("Not found: Path {} does not exist", path)),
        StatusCode::FORBIDDEN => Err(anyhow!("Forbidden: Access denied to path {}", path)),
        _ if response.status().is_success() => Ok(()),
        _ => Err(anyhow!(
            "Failed to put object to {}: HTTP {}",
            &url,
            response.status()
        )),
    }
}

/// Download an object.
pub fn get_object(agent: &Agent, base_url: &str, path: &str) -> Result<Vec<u8>> {
    let url = format!("{}/{}", base_url, path);
    let mut response = agent.get(&url).header(header::ACCEPT, ALL).call()?;
    match response.status() {
        StatusCode::UNAUTHORIZED => Err(anyhow!("Remote unauthorized")),
        StatusCode::NOT_FOUND => Err(anyhow!("Not found: Path {} does not exist", path)),
        StatusCode::FORBIDDEN => Err(anyhow!("Forbidden: Access denied to path {}", path)),
        _ if response.status().is_success() => Ok(response.body_mut().read_to_vec()?),
        _ => Err(anyhow!(
            "Failed to get object from {}: HTTP {}",
            &url,
            response.status()
        )),
    }
}

/// Delete an object.
pub fn delete_object(agent: &Agent, base_url: &str, path: &str) -> Result<()> {
    let url = format!("{}/{}", base_url, path);
    let response = agent.delete(&url).call()?;
    match response.status() {
        StatusCode::UNAUTHORIZED => Err(anyhow!("Remote unauthorized")),
        StatusCode::NOT_FOUND => Err(anyhow!("Not found: Path {} does not exist", path)),
        StatusCode::FORBIDDEN => Err(anyhow!("Forbidden: Access denied to path {}", path)),
        _ if response.status().is_success() => Ok(()),
        _ => Err(anyhow!(
            "Failed to delete object from {}: HTTP {}",
            &url,
            response.status()
        )),
    }
}

/// Get the zone name from the destination. It is the first part of the path.
pub fn zone_name(remote: &str) -> String {
    let parts = remote.split('/');
    for part in parts {
        if part.is_empty() {
            continue;
        }
        return part.to_string();
    }
    String::default()
}

/// Strip the zone prefix from a path.
pub fn strip_zone_prefix(path: &str) -> &str {
    path.strip_prefix("zone://").unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_url() {
        assert_eq!(
            base_url("uk"),
            Some("https://uk.storage.bunnycdn.com".to_string())
        );
        assert_eq!(
            base_url("de"),
            Some("https://storage.bunnycdn.com".to_string())
        );
        assert_eq!(base_url("invalid"), None);
    }

    // Test deserialization of StorageObject.
    #[test]
    fn test_storage_object_deserialization() {
        let json = "{\"Guid\":\"33ea1f9b-3012-4ddd-af33-24741c559ef0\",\"StorageZoneName\":\"my-storage-zone\",\"Path\":\"/my-storage-zone/\",\"ObjectName\":\"404.html\",\"Length\":11720,\"LastChanged\":\"2025-02-03T21:26:21.866\",\"ServerId\":12,\"ArrayNumber\":5,\"IsDirectory\":false,\"UserId\":\"0e64cafc-0bf2-47e1-9adc-257c80124475\",\"ContentType\":\"\",\"DateCreated\":\"2025-02-03T21:26:21.866\",\"StorageZoneId\":134123,\"Checksum\":\"312341234adfadsfasdf\",\"ReplicatedZones\":\"DE\"}";

        let record: StorageObject = serde_json::from_str(&json).unwrap();
        let expect = StorageObject {
            guid: "33ea1f9b-3012-4ddd-af33-24741c559ef0".to_string(),
            storage_zone_name: "my-storage-zone".to_string(),
            path: "/my-storage-zone/".to_string(),
            object_name: "404.html".to_string(),
            length: 11720,
            last_changed: NaiveDateTime::parse_from_str(
                "2025-02-03T21:26:21.866",
                "%Y-%m-%dT%H:%M:%S%.f",
            )
            .unwrap(),
            is_directory: false,
            date_created: NaiveDateTime::parse_from_str(
                "2025-02-03T21:26:21.866",
                "%Y-%m-%dT%H:%M:%S%.f",
            )
            .unwrap(),
        };
        assert_eq!(record, expect);
    }

    #[test]
    fn test_zone_name() {
        assert_eq!(zone_name("test/"), "test");
        assert_eq!(zone_name("test/foo/bar"), "test");
        assert_eq!(zone_name("/test/foo/bar/"), "test");
    }

    #[test]
    fn test_strip_zone_prefix() {
        assert_eq!(strip_zone_prefix("zone://test/path"), "test/path");
        assert_eq!(strip_zone_prefix("test/path"), "test/path");
    }
}
