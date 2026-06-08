use crate::{
    add_custom_publisher_core, fetch_article_body_core, force_recluster_core,
    generate_cluster_summary_core, get_clusters_core, get_publishers_core,
    refresh_feed_core, remove_custom_publisher_core, split_cluster_core,
    translate_summary_core, wipe_all_data_core, MerillCore,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

type BridgeCallback = unsafe extern "C" fn(*const c_char, *mut c_void);

static NATIVE_CORE: OnceLock<Arc<MerillCore>> = OnceLock::new();

fn success(value: Value) -> String {
    json!({ "ok": true, "data": value }).to_string()
}

fn failure(error: impl ToString) -> String {
    json!({ "ok": false, "error": error.to_string() }).to_string()
}

fn decode<T: DeserializeOwned>(payload: &Value) -> Result<T, String> {
    serde_json::from_value(payload.clone()).map_err(|e| e.to_string())
}

async fn dispatch(core: &MerillCore, request_json: &str) -> Result<Value, String> {
    let request: Value = serde_json::from_str(request_json).map_err(|e| e.to_string())?;
    let command = request
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing command".to_string())?;
    let payload = request.get("payload").cloned().unwrap_or_else(|| json!({}));

    match command {
        "get_clusters" => {
            #[derive(serde::Deserialize)]
            struct Input {
                #[serde(default)]
                blindspots_only: bool,
            }
            let input: Input = decode(&payload)?;
            serde_json::to_value(get_clusters_core(core, input.blindspots_only).await.map_err(failure)?)
                .map_err(|e| e.to_string())
        }
        "refresh_feed" => serde_json::to_value(refresh_feed_core(core).await.map_err(failure)?)
            .map_err(|e| e.to_string()),
        "fetch_article_body" => {
            #[derive(serde::Deserialize)]
            struct Input {
                article_id: String,
                url: String,
            }
            let input: Input = decode(&payload)?;
            serde_json::to_value(fetch_article_body_core(core, input.article_id, input.url).await.map_err(failure)?)
                .map_err(|e| e.to_string())
        }
        "generate_cluster_summary" => {
            #[derive(serde::Deserialize)]
            struct Input {
                cluster_id: String,
                headlines: Vec<String>,
                snippets: Vec<String>,
            }
            let input: Input = decode(&payload)?;
            serde_json::to_value(
                generate_cluster_summary_core(core, input.cluster_id, input.headlines, input.snippets)
                    .await
                    .map_err(failure)?,
            )
            .map_err(|e| e.to_string())
        }
        "get_publishers" => serde_json::to_value(get_publishers_core(core)).map_err(|e| e.to_string()),
        "add_custom_publisher" => {
            #[derive(serde::Deserialize)]
            struct Input {
                url: String,
                #[serde(default)]
                name: String,
                is_global: bool,
            }
            let input: Input = decode(&payload)?;
            serde_json::to_value(
                add_custom_publisher_core(core, input.url, input.name, input.is_global)
                    .await
                    .map_err(failure)?,
            )
            .map_err(|e| e.to_string())
        }
        "remove_custom_publisher" => {
            #[derive(serde::Deserialize)]
            struct Input {
                id: String,
            }
            let input: Input = decode(&payload)?;
            remove_custom_publisher_core(core, input.id).map_err(failure)?;
            Ok(Value::Null)
        }
        "split_cluster" => {
            #[derive(serde::Deserialize)]
            struct Input {
                article_id: String,
                headline: String,
                published_at: String,
            }
            let input: Input = decode(&payload)?;
            serde_json::to_value(
                split_cluster_core(core, input.article_id, input.headline, input.published_at)
                    .await
                    .map_err(failure)?,
            )
            .map_err(|e| e.to_string())
        }
        "force_recluster" => serde_json::to_value(force_recluster_core(core).map_err(failure)?)
            .map_err(|e| e.to_string()),
        "wipe_all_data" => {
            wipe_all_data_core(core).map_err(failure)?;
            Ok(Value::Null)
        }
        "translate_summary" => {
            #[derive(serde::Deserialize)]
            struct Input {
                text: String,
                to: String,
            }
            let input: Input = decode(&payload)?;
            serde_json::to_value(translate_summary_core(input.text, input.to).await.map_err(failure)?)
                .map_err(|e| e.to_string())
        }
        _ => Err(format!("unsupported command: {command}")),
    }
}

fn callback_with(callback: BridgeCallback, context: *mut c_void, response: String) {
    let response = CString::new(response).unwrap_or_else(|_| CString::new(failure("invalid response")).unwrap());
    unsafe {
        callback(response.into_raw(), context);
    }
}

#[no_mangle]
pub unsafe extern "C" fn merill_initialize(data_dir: *const c_char) -> bool {
    if data_dir.is_null() {
        return false;
    }
    if NATIVE_CORE.get().is_some() {
        return true;
    }
    let Ok(data_dir) = unsafe { CStr::from_ptr(data_dir) }.to_str() else {
        return false;
    };
    let db_path = PathBuf::from(data_dir).join("merill-native.db");
    let Ok(core) = MerillCore::open(&db_path) else {
        return false;
    };
    NATIVE_CORE.set(Arc::new(core)).is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn merill_call_async(
    request_json: *const c_char,
    callback: Option<BridgeCallback>,
    context: *mut c_void,
) {
    let Some(callback) = callback else {
        return;
    };
    if request_json.is_null() {
        callback_with(callback, context, failure("request is null"));
        return;
    }
    let request = unsafe { CStr::from_ptr(request_json) }
        .to_string_lossy()
        .into_owned();
    let Some(core) = NATIVE_CORE.get().cloned() else {
        callback_with(callback, context, failure("Merill core is not initialized"));
        return;
    };
    let context = context as usize;
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        let response = match runtime {
            Ok(runtime) => match runtime.block_on(dispatch(&core, &request)) {
                Ok(value) => success(value),
                Err(error) => {
                    if let Ok(envelope) = serde_json::from_str::<Value>(&error) {
                        envelope.to_string()
                    } else {
                        failure(error)
                    }
                }
            },
            Err(error) => failure(error),
        };
        callback_with(callback, context as *mut c_void, response);
    });
}

#[no_mangle]
pub unsafe extern "C" fn merill_free_string(pointer: *mut c_char) {
    if !pointer.is_null() {
        drop(unsafe { CString::from_raw(pointer) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_core(name: &str) -> MerillCore {
        let dir = std::env::temp_dir().join(format!("merill-native-{name}-{}", uuid::Uuid::new_v4()));
        MerillCore::open(&dir.join("merill-native.db")).unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dispatches_read_only_contracts() {
        let core = test_core("read");
        let publishers = dispatch(&core, r#"{"command":"get_publishers","payload":{}}"#).await.unwrap();
        assert!(publishers.as_array().is_some_and(|items| !items.is_empty()));

        let clusters = dispatch(&core, r#"{"command":"get_clusters","payload":{"blindspots_only":false}}"#).await.unwrap();
        assert!(clusters.get("clusters").is_some_and(Value::is_array));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dispatches_maintenance_contracts() {
        let core = test_core("maintenance");
        let recluster = dispatch(&core, r#"{"command":"force_recluster","payload":{}}"#).await.unwrap();
        assert!(recluster.as_str().is_some_and(|message| message.contains("clusters created")));
        let wiped = dispatch(&core, r#"{"command":"wipe_all_data","payload":{}}"#).await.unwrap();
        assert!(wiped.is_null());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_invalid_contracts() {
        let core = test_core("invalid");
        assert!(dispatch(&core, "not json").await.is_err());
        assert!(dispatch(&core, r#"{"payload":{}}"#).await.is_err());
        assert!(dispatch(&core, r#"{"command":"missing","payload":{}}"#).await.is_err());
        assert!(dispatch(&core, r#"{"command":"split_cluster","payload":{}}"#).await.is_err());
    }
}
