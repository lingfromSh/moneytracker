//! Object storage the sync engine talks to.
//!
//! Everything goes through [`ObjectStore`] so the engine can be tested without
//! a network, and so a different backend (WebDAV, SFTP, plain files) is a trait
//! implementation away.

use std::collections::BTreeMap;

use crate::sigv4::{signed_headers, Credentials};
use crate::SyncError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// Overwrite whatever is there.
    Any,
    /// Fail with 412 unless the object does not exist yet.
    Create,
    /// Fail with 412 unless the ETag still matches.
    Match(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub body: Vec<u8>,
    pub etag: Option<String>,
}

impl Fetched {
    pub fn text(&self) -> Result<String, SyncError> {
        String::from_utf8(self.body.clone())
            .map_err(|_| SyncError::Parse("object is not UTF-8 text".to_string()))
    }
}

pub trait ObjectStore: Send + Sync {
    /// `None` when the object does not exist.
    fn get(&self, key: &str) -> Result<Option<Fetched>, SyncError>;
    fn head(&self, key: &str) -> Result<bool, SyncError>;
    fn put(&self, key: &str, body: &[u8], condition: Condition) -> Result<String, SyncError>;
    fn delete(&self, key: &str) -> Result<(), SyncError>;
}

// ---------------------------------------------------------------------------
// S3 / MinIO
// ---------------------------------------------------------------------------

pub struct S3Store {
    endpoint: String,
    bucket: String,
    path_style: bool,
    creds: Credentials,
    agent: ureq::Agent,
}

impl S3Store {
    pub fn new(config: &crate::SyncConfig) -> Result<Self, SyncError> {
        let endpoint = config.endpoint.trim().trim_end_matches('/').to_string();
        if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
            return Err(SyncError::Config(format!(
                "endpoint 必须以 http:// 或 https:// 开头：{endpoint}"
            )));
        }
        if config.bucket.trim().is_empty() {
            return Err(SyncError::Config("bucket 不能为空".to_string()));
        }
        if config.access_key.trim().is_empty() || config.secret_key.trim().is_empty() {
            return Err(SyncError::Config(
                "Access Key / Secret Key 不能为空".to_string(),
            ));
        }
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(10))
            .timeout_read(std::time::Duration::from_secs(60))
            .timeout_write(std::time::Duration::from_secs(60))
            .build();
        Ok(Self {
            endpoint,
            bucket: config.bucket.trim().to_string(),
            path_style: config.path_style,
            creds: Credentials {
                access_key: config.access_key.trim().to_string(),
                secret_key: config.secret_key.trim().to_string(),
                region: if config.region.trim().is_empty() {
                    "us-east-1".to_string()
                } else {
                    config.region.trim().to_string()
                },
            },
            agent,
        })
    }

    fn url_for(&self, key: &str) -> String {
        if self.path_style {
            return format!("{}/{}/{}", self.endpoint, self.bucket, key);
        }
        let (scheme, rest) = self
            .endpoint
            .split_once("://")
            .unwrap_or(("https", self.endpoint.as_str()));
        format!("{scheme}://{}.{rest}/{key}", self.bucket)
    }

    fn send(
        &self,
        method: &str,
        key: &str,
        body: &[u8],
        extra: &[(&str, &str)],
    ) -> Result<Option<Fetched>, SyncError> {
        let url = self.url_for(key);
        let headers = signed_headers(method, &url, body, extra, &self.creds, chrono::Utc::now())
            .map_err(SyncError::Config)?;
        let mut request = self.agent.request(method, &url);
        for (name, value) in &headers {
            request = request.set(name, value);
        }
        let result = if method == "PUT" || method == "POST" {
            request.send_bytes(body)
        } else {
            request.call()
        };
        match result {
            Ok(response) => {
                let etag = response
                    .header("etag")
                    .map(|value| value.trim_matches('"').to_string());
                let mut bytes = Vec::new();
                use std::io::Read;
                response
                    .into_reader()
                    .take(64 * 1024 * 1024)
                    .read_to_end(&mut bytes)
                    .map_err(|e| SyncError::Io(e.to_string()))?;
                Ok(Some(Fetched { body: bytes, etag }))
            }
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(ureq::Error::Status(status, response)) => {
                let mut detail = String::new();
                if let Ok(text) = response.into_string() {
                    detail = text.chars().take(400).collect();
                }
                Err(SyncError::Http(status, detail))
            }
            Err(ureq::Error::Transport(err)) => Err(SyncError::Io(err.to_string())),
        }
    }
}

impl ObjectStore for S3Store {
    fn get(&self, key: &str) -> Result<Option<Fetched>, SyncError> {
        self.send("GET", key, b"", &[])
    }

    fn head(&self, key: &str) -> Result<bool, SyncError> {
        Ok(self.send("HEAD", key, b"", &[])?.is_some())
    }

    fn put(&self, key: &str, body: &[u8], condition: Condition) -> Result<String, SyncError> {
        let extra: Vec<(&str, &str)> = match &condition {
            Condition::Any => Vec::new(),
            Condition::Create => vec![("if-none-match", "*")],
            Condition::Match(etag) => vec![("if-match", etag.as_str())],
        };
        let found = self.send("PUT", key, body, &extra)?;
        Ok(found
            .and_then(|response| response.etag)
            .unwrap_or_else(|| "*".to_string()))
    }

    fn delete(&self, key: &str) -> Result<(), SyncError> {
        self.send("DELETE", key, b"", &[]).map(|_| ())
    }
}

// ---------------------------------------------------------------------------
// In-memory store, for tests
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MemoryStore {
    objects: std::sync::Mutex<BTreeMap<String, (Vec<u8>, String)>>,
    sequence: std::sync::atomic::AtomicU64,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn keys(&self) -> Vec<String> {
        self.objects
            .lock()
            .expect("memory store poisoned")
            .keys()
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.objects.lock().expect("memory store poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ObjectStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<Fetched>, SyncError> {
        Ok(self
            .objects
            .lock()
            .expect("memory store poisoned")
            .get(key)
            .map(|(body, etag)| Fetched {
                body: body.clone(),
                etag: Some(etag.clone()),
            }))
    }

    fn head(&self, key: &str) -> Result<bool, SyncError> {
        Ok(self
            .objects
            .lock()
            .expect("memory store poisoned")
            .contains_key(key))
    }

    fn put(&self, key: &str, body: &[u8], condition: Condition) -> Result<String, SyncError> {
        let mut objects = self.objects.lock().expect("memory store poisoned");
        let existing = objects.get(key).map(|(_, etag)| etag.clone());
        match (&condition, &existing) {
            (Condition::Create, Some(_)) => {
                return Err(SyncError::Http(412, "if-none-match: *".to_string()))
            }
            (Condition::Match(want), Some(have)) if want != have => {
                return Err(SyncError::Http(412, format!("if-match {want} != {have}")))
            }
            (Condition::Match(_), None) => {
                return Err(SyncError::Http(
                    412,
                    "if-match on a missing object".to_string(),
                ))
            }
            _ => {}
        }
        let seq = self
            .sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let etag = format!("mem-{seq}");
        objects.insert(key.to_string(), (body.to_vec(), etag.clone()));
        Ok(etag)
    }

    fn delete(&self, key: &str) -> Result<(), SyncError> {
        self.objects
            .lock()
            .expect("memory store poisoned")
            .remove(key);
        Ok(())
    }
}
