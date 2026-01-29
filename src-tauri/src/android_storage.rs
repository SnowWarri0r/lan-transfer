use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::Wry;
#[cfg(target_os = "android")]
use tauri::Manager;

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "app.tauri.storage";

#[derive(Clone)]
pub struct AndroidStorage(PluginHandle<Wry>);

#[derive(Serialize)]
struct EmptyPayload {}

#[derive(Deserialize)]
struct PickFolderResponse {
    uri: Option<String>,
}

#[derive(Serialize)]
struct OpenWriterPayload {
    tree_uri: String,
    file_name: String,
}

#[derive(Deserialize)]
struct OpenWriterResponse {
    handle: i64,
}

#[derive(Serialize)]
struct WriteChunkPayload {
    handle: i64,
    data_base64: String,
}

#[derive(Deserialize)]
struct WriteChunkResponse {
    ok: bool,
}

#[derive(Serialize)]
struct CloseWriterPayload {
    handle: i64,
}

#[derive(Deserialize)]
struct CloseWriterResponse {
    ok: bool,
}

pub fn init() -> TauriPlugin<Wry> {
    let mut builder = Builder::<Wry>::new("android-storage");

    builder = builder.setup(|app, api| {
        #[cfg(target_os = "android")]
        {
            let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "StoragePlugin")?;
            let storage = AndroidStorage(handle);
            app.manage(storage);
        }
        let _ = app;
        let _ = api;
        Ok(())
    });

    builder.build()
}

impl AndroidStorage {
    pub fn pick_folder(&self) -> Result<Option<String>, String> {
        #[cfg(target_os = "android")]
        {
            let res = self
                .0
                .run_mobile_plugin::<PickFolderResponse>("pickFolder", EmptyPayload {});
            return res
                .map(|r| r.uri)
                .map_err(|e| format!("pickFolder failed: {e}"));
        }
        #[allow(unreachable_code)]
        Err("pickFolder is only supported on Android".to_string())
    }

    pub fn open_writer(&self, tree_uri: String, file_name: String) -> Result<i64, String> {
        #[cfg(target_os = "android")]
        {
            let payload = OpenWriterPayload { tree_uri, file_name };
            let res = self
                .0
                .run_mobile_plugin::<OpenWriterResponse>("openWriter", payload);
            return res
                .map(|r| r.handle)
                .map_err(|e| format!("openWriter failed: {e}"));
        }
        #[allow(unreachable_code)]
        Err("openWriter is only supported on Android".to_string())
    }

    pub fn write_chunk(&self, handle: i64, data_base64: String) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            let payload = WriteChunkPayload { handle, data_base64 };
            let res = self
                .0
                .run_mobile_plugin::<WriteChunkResponse>("writeChunk", payload);
            return res
                .map(|r| {
                    let _ = r.ok;
                })
                .map_err(|e| format!("writeChunk failed: {e}"));
        }
        #[allow(unreachable_code)]
        Err("writeChunk is only supported on Android".to_string())
    }

    pub fn close_writer(&self, handle: i64) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            let payload = CloseWriterPayload { handle };
            let res = self
                .0
                .run_mobile_plugin::<CloseWriterResponse>("closeWriter", payload);
            return res
                .map(|r| {
                    let _ = r.ok;
                })
                .map_err(|e| format!("closeWriter failed: {e}"));
        }
        #[allow(unreachable_code)]
        Err("closeWriter is only supported on Android".to_string())
    }
}
