use reqwest::Client;
use serde::Deserialize;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Deserialize)]
pub struct ModelInfo {
    pub id: String,
}

pub struct CacheState {
    cache: RwLock<Option<Vec<ModelInfo>>>,
    seeded: Vec<ModelInfo>,
    consecutive_failures: AtomicU32,
}

pub type Cache = Arc<CacheState>;

pub fn new_cache(seeded_models: &[String]) -> Cache {
    let seeded = dedupe_models(
        seeded_models
            .iter()
            .filter(|model| !model.trim().is_empty())
            .map(|model| ModelInfo { id: model.clone() })
            .collect(),
    );
    Arc::new(CacheState {
        cache: RwLock::new(if seeded.is_empty() {
            None
        } else {
            Some(seeded.clone())
        }),
        seeded,
        consecutive_failures: AtomicU32::new(0),
    })
}

fn dedupe_models(models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    let mut out = Vec::new();
    for model in models {
        if !out
            .iter()
            .any(|existing: &ModelInfo| existing.id == model.id)
        {
            out.push(model);
        }
    }
    out
}

pub async fn refresh(client: &Client, models_url: &str, api_key: Option<&str>, state: &Cache) {
    let result: Result<Vec<ModelInfo>, String> = async {
        let mut req = client
            .get(models_url)
            .timeout(std::time::Duration::from_secs(30));
        if let Some(api_key) = api_key.filter(|value| !value.is_empty()) {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }
        let resp = req.send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("models endpoint returned {}", resp.status()));
        }
        let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let models = data["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| serde_json::from_value(m.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        Ok(models)
    }
    .await;

    match result {
        Ok(models) => {
            let merged = dedupe_models(
                state
                    .seeded
                    .iter()
                    .cloned()
                    .chain(models.into_iter())
                    .collect(),
            );
            tracing::debug!("Model cache refreshed: {} models", merged.len());
            *state.cache.write().await = Some(merged);
            state.consecutive_failures.store(0, Ordering::Relaxed);
        }
        Err(e) => {
            let failures = state.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
            if failures >= 5 {
                tracing::error!(
                    "Model cache refresh failed {} consecutive times: {}",
                    failures,
                    e
                );
                state.consecutive_failures.store(0, Ordering::Relaxed);
            } else {
                tracing::warn!("Model cache refresh failed (attempt {}): {}", failures, e);
            }
        }
    }
}

pub async fn snapshot(state: &Cache) -> Vec<ModelInfo> {
    state
        .cache
        .read()
        .await
        .as_ref()
        .cloned()
        .unwrap_or_else(|| state.seeded.clone())
}

pub async fn normalize_model(model: &str, state: &Cache) -> String {
    let model_lower = model.to_lowercase();
    let guard = state.cache.read().await;
    if let Some(models) = guard.as_ref() {
        if models.iter().any(|m| m.id == model) {
            return model.to_string();
        }
        if let Some(matched) = models.iter().find(|m| m.id.to_lowercase() == model_lower) {
            tracing::info!("Model: {} → {} (case-corrected)", model, matched.id);
            return matched.id.clone();
        }
    }
    model.to_string()
}
