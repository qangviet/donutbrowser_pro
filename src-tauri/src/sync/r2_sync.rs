use crate::group_manager::GroupManager;
use crate::profile::manager::ProfileManager;
use crate::proxy_manager::PROXY_MANAGER;
use chrono::Utc;
use reqwest::Client;
use ring::hmac;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::sleep;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct R2SyncPublicSettings {
  pub enabled: bool,
  pub account_id: String,
  pub bucket_name: String,
  pub interval_minutes: u32,
  pub last_sync: Option<u64>,
  pub last_sync_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R2SyncSecrets {
  pub access_key_id: String,
  pub secret_access_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R2SyncSettings {
  #[serde(flatten)]
  pub public: R2SyncPublicSettings,
  pub access_key_id: Option<String>,
  pub secret_access_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R2SyncResult {
  pub success: bool,
  pub synced_at: u64,
  pub profiles_count: usize,
  pub proxies_count: usize,
  pub groups_count: usize,
}

fn bytes_to_hex(bytes: &[u8]) -> String {
  bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(data: &[u8]) -> String {
  let hash = Sha256::digest(data);
  bytes_to_hex(hash.as_slice())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
  let key = hmac::Key::new(hmac::HMAC_SHA256, key);
  hmac::sign(&key, data).as_ref().to_vec()
}

fn derive_signing_key(secret_key: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
  let k_date_input = format!("AWS4{secret_key}");
  let k_date = hmac_sha256(k_date_input.as_bytes(), date.as_bytes());
  let k_region = hmac_sha256(&k_date, region.as_bytes());
  let k_service = hmac_sha256(&k_region, service.as_bytes());
  hmac_sha256(&k_service, b"aws4_request")
}

struct SignedRequest {
  url: String,
  authorization: String,
  x_amz_date: String,
  x_amz_content_sha256: String,
}

struct R2RequestParams<'a> {
  method: &'a str,
  account_id: &'a str,
  bucket: &'a str,
  key: &'a str,
  access_key_id: &'a str,
  secret_access_key: &'a str,
  body: &'a [u8],
  extra_query: Option<&'a str>,
}

fn build_signed_request(params: R2RequestParams<'_>) -> SignedRequest {
  let R2RequestParams {
    method,
    account_id,
    bucket,
    key,
    access_key_id,
    secret_access_key,
    body,
    extra_query,
  } = params;
  let now = Utc::now();
  let date_str = now.format("%Y%m%d").to_string();
  let datetime_str = now.format("%Y%m%dT%H%M%SZ").to_string();

  let region = "auto";
  let service = "s3";
  let host = format!("{account_id}.r2.cloudflarestorage.com");
  let url_path = if key.is_empty() {
    format!("/{bucket}")
  } else {
    format!("/{bucket}/{key}")
  };

  let body_hash = sha256_hex(body);
  let query_str = extra_query.unwrap_or("");

  let canonical_headers =
    format!("host:{host}\nx-amz-content-sha256:{body_hash}\nx-amz-date:{datetime_str}\n");
  let signed_headers = "host;x-amz-content-sha256;x-amz-date";

  let canonical_request = format!(
    "{method}\n{url_path}\n{query_str}\n{canonical_headers}\n{signed_headers}\n{body_hash}"
  );

  let credential_scope = format!("{date_str}/{region}/{service}/aws4_request");
  let canonical_request_hash = sha256_hex(canonical_request.as_bytes());
  let string_to_sign =
    format!("AWS4-HMAC-SHA256\n{datetime_str}\n{credential_scope}\n{canonical_request_hash}");

  let signing_key = derive_signing_key(secret_access_key, &date_str, region, service);
  let signature = bytes_to_hex(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));

  let authorization = format!(
    "AWS4-HMAC-SHA256 Credential={access_key_id}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"
  );

  let full_url = if query_str.is_empty() {
    format!("https://{host}{url_path}")
  } else {
    format!("https://{host}{url_path}?{query_str}")
  };

  SignedRequest {
    url: full_url,
    authorization,
    x_amz_date: datetime_str,
    x_amz_content_sha256: body_hash,
  }
}

pub struct R2SyncEngine {
  client: Client,
  account_id: String,
  bucket_name: String,
  access_key_id: String,
  secret_access_key: String,
}

impl R2SyncEngine {
  pub fn new(
    account_id: String,
    bucket_name: String,
    access_key_id: String,
    secret_access_key: String,
  ) -> Self {
    Self {
      client: Client::new(),
      account_id,
      bucket_name,
      access_key_id,
      secret_access_key,
    }
  }

  async fn put_object(&self, key: &str, data: &[u8]) -> Result<(), String> {
    let signed = build_signed_request(R2RequestParams {
      method: "PUT",
      account_id: &self.account_id,
      bucket: &self.bucket_name,
      key,
      access_key_id: &self.access_key_id,
      secret_access_key: &self.secret_access_key,
      body: data,
      extra_query: None,
    });

    let resp = self
      .client
      .put(&signed.url)
      .header("Authorization", &signed.authorization)
      .header("x-amz-date", &signed.x_amz_date)
      .header("x-amz-content-sha256", &signed.x_amz_content_sha256)
      .header("Content-Type", "application/json")
      .header("Content-Length", data.len().to_string())
      .body(data.to_vec())
      .send()
      .await
      .map_err(|e| format!("R2 PUT request failed: {e}"))?;

    if !resp.status().is_success() {
      let status = resp.status();
      let body = resp.text().await.unwrap_or_default();
      return Err(format!("R2 PUT failed with status {status}: {body}"));
    }

    Ok(())
  }

  pub async fn test_connection(&self) -> Result<(), String> {
    let signed = build_signed_request(R2RequestParams {
      method: "GET",
      account_id: &self.account_id,
      bucket: &self.bucket_name,
      key: "",
      access_key_id: &self.access_key_id,
      secret_access_key: &self.secret_access_key,
      body: b"",
      extra_query: Some("list-type=2&max-keys=1"),
    });

    let resp = self
      .client
      .get(&signed.url)
      .header("Authorization", &signed.authorization)
      .header("x-amz-date", &signed.x_amz_date)
      .header("x-amz-content-sha256", &signed.x_amz_content_sha256)
      .send()
      .await
      .map_err(|e| format!("Connection test failed: {e}"))?;

    if resp.status().is_success() {
      return Ok(());
    }

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if status == 403 {
      return Err(
        "Authentication failed: check your Access Key ID and Secret Access Key".to_string(),
      );
    }
    if status == 404 {
      return Err("Bucket not found: check your Account ID and Bucket Name".to_string());
    }

    Err(format!(
      "Connection test failed with status {status}: {body}"
    ))
  }

  pub async fn sync_all(&self) -> Result<R2SyncResult, String> {
    let synced_at = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap_or_default()
      .as_secs();

    let profiles = ProfileManager::instance()
      .list_profiles()
      .map_err(|e| format!("Failed to list profiles: {e}"))?;
    let profiles_count = profiles.len();
    let profiles_json = serde_json::to_vec_pretty(&profiles)
      .map_err(|e| format!("Failed to serialize profiles: {e}"))?;

    let proxies = PROXY_MANAGER.get_stored_proxies();
    let proxies_count = proxies.len();
    let proxies_json = serde_json::to_vec_pretty(&proxies)
      .map_err(|e| format!("Failed to serialize proxies: {e}"))?;

    let group_manager = GroupManager::new();
    let groups = group_manager
      .get_all_groups()
      .map_err(|e| format!("Failed to list groups: {e}"))?;
    let groups_count = groups.len();
    let groups_json =
      serde_json::to_vec_pretty(&groups).map_err(|e| format!("Failed to serialize groups: {e}"))?;

    self
      .put_object("donut-sync/profiles.json", &profiles_json)
      .await?;
    self
      .put_object("donut-sync/proxies.json", &proxies_json)
      .await?;
    self
      .put_object("donut-sync/groups.json", &groups_json)
      .await?;

    log::info!(
      "[r2_sync] Synced {profiles_count} profiles, {proxies_count} proxies, {groups_count} groups"
    );

    Ok(R2SyncResult {
      success: true,
      synced_at,
      profiles_count,
      proxies_count,
      groups_count,
    })
  }
}

struct R2SyncScheduler {
  stop_tx: watch::Sender<bool>,
  handle: Option<JoinHandle<()>>,
}

impl R2SyncScheduler {
  fn start(
    account_id: String,
    bucket_name: String,
    access_key_id: String,
    secret_access_key: String,
    interval_minutes: u32,
    settings_dir: PathBuf,
  ) -> Self {
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let interval_secs = (interval_minutes.max(1) as u64) * 60;

    let handle = tokio::spawn(async move {
      loop {
        tokio::select! {
          _ = sleep(Duration::from_secs(interval_secs)) => {
            let engine = R2SyncEngine::new(
              account_id.clone(),
              bucket_name.clone(),
              access_key_id.clone(),
              secret_access_key.clone(),
            );
            match engine.sync_all().await {
              Ok(result) => {
                log::info!(
                  "[r2_sync] Scheduled sync: {} profiles, {} proxies, {} groups",
                  result.profiles_count, result.proxies_count, result.groups_count
                );
                let _ = update_last_sync(&settings_dir, result.synced_at, None);
              }
              Err(e) => {
                log::error!("[r2_sync] Scheduled sync failed: {e}");
                let _ = update_last_sync(
                  &settings_dir,
                  SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
                  Some(e),
                );
              }
            }
          }
          _ = stop_rx.changed() => {
            if *stop_rx.borrow() {
              log::info!("[r2_sync] Scheduler stopped");
              break;
            }
          }
        }
      }
    });

    Self {
      stop_tx,
      handle: Some(handle),
    }
  }

  fn stop(&self) {
    let _ = self.stop_tx.send(true);
  }
}

impl Drop for R2SyncScheduler {
  fn drop(&mut self) {
    self.stop();
    if let Some(handle) = self.handle.take() {
      handle.abort();
    }
  }
}

fn update_last_sync(
  settings_dir: &Path,
  synced_at: u64,
  error: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
  let file = settings_dir.join("r2_sync_settings.json");
  if !file.exists() {
    return Ok(());
  }
  let content = std::fs::read_to_string(&file)?;
  let mut settings: R2SyncPublicSettings = serde_json::from_str(&content)?;
  settings.last_sync = Some(synced_at);
  settings.last_sync_error = error;
  let json = serde_json::to_string_pretty(&settings)?;
  std::fs::write(&file, json)?;
  Ok(())
}

lazy_static::lazy_static! {
  static ref GLOBAL_R2_SCHEDULER: Mutex<Option<R2SyncScheduler>> = Mutex::new(None);
}

pub fn start_r2_scheduler(
  account_id: String,
  bucket_name: String,
  access_key_id: String,
  secret_access_key: String,
  interval_minutes: u32,
  settings_dir: PathBuf,
) {
  let mut scheduler = GLOBAL_R2_SCHEDULER.lock().unwrap();
  if let Some(s) = scheduler.take() {
    s.stop();
  }
  *scheduler = Some(R2SyncScheduler::start(
    account_id,
    bucket_name,
    access_key_id,
    secret_access_key,
    interval_minutes,
    settings_dir,
  ));
}

pub fn stop_r2_scheduler() {
  let mut scheduler = GLOBAL_R2_SCHEDULER.lock().unwrap();
  if let Some(s) = scheduler.take() {
    s.stop();
  }
}
