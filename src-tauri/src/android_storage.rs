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

#[derive(Deserialize)]
struct PickMultipleFilesResponse {
    uris: Vec<String>,
}

#[derive(Serialize)]
struct UriPayload {
    uri: String,
}

#[derive(Deserialize)]
struct FileInfoResponse {
    name: String,
    size: u64,
}

#[derive(Serialize)]
struct ReadUriChunkPayload {
    uri: String,
    offset: u64,
    size: i32,
}

#[derive(Deserialize)]
struct ReadUriChunkResponse {
    data: String,
    bytes_read: i32,
}

#[derive(Serialize)]
struct OpenWriterPayload {
    tree_uri: String,
    file_name: String,
}

#[derive(Deserialize)]
struct OpenWriterResponse {
    handle: i64,
    document_uri: String,
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

#[derive(Serialize)]
struct DeleteDocumentPayload {
    document_uri: String,
}

#[derive(Deserialize)]
struct DeleteDocumentResponse {
    ok: bool,
}

#[derive(Deserialize)]
struct GetClipboardResponse {
    content: String,
}

#[derive(Deserialize)]
struct GetDeviceNameResponse {
    name: String,
}

#[derive(Serialize)]
struct SetClipboardPayload {
    content: String,
}

#[derive(Deserialize)]
struct SetClipboardResponse {
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

    pub fn pick_multiple_files(&self) -> Result<Vec<String>, String> {
        #[cfg(target_os = "android")]
        {
            let res = self
                .0
                .run_mobile_plugin::<PickMultipleFilesResponse>("pickMultipleFiles", EmptyPayload {});
            return res
                .map(|r| r.uris)
                .map_err(|e| format!("pickMultipleFiles failed: {e}"));
        }
        #[allow(unreachable_code)]
        Err("pickMultipleFiles is only supported on Android".to_string())
    }

    pub fn get_file_info(&self, uri: String) -> Result<(String, u64), String> {
        #[cfg(target_os = "android")]
        {
            let payload = UriPayload { uri };
            let res = self
                .0
                .run_mobile_plugin::<FileInfoResponse>("getFileInfo", payload);
            return res
                .map(|r| (r.name, r.size))
                .map_err(|e| format!("getFileInfo failed: {e}"));
        }
        #[allow(unreachable_code)]
        Err("getFileInfo is only supported on Android".to_string())
    }

    pub fn read_uri_chunk(&self, uri: String, offset: u64, size: i32) -> Result<(String, i32), String> {
        #[cfg(target_os = "android")]
        {
            let payload = ReadUriChunkPayload { uri, offset, size };
            let res = self
                .0
                .run_mobile_plugin::<ReadUriChunkResponse>("readUriChunk", payload);
            return res
                .map(|r| (r.data, r.bytes_read))
                .map_err(|e| format!("readUriChunk failed: {e}"));
        }
        #[allow(unreachable_code)]
        Err("readUriChunk is only supported on Android".to_string())
    }

    pub fn open_writer(&self, tree_uri: String, file_name: String) -> Result<(i64, String), String> {
        #[cfg(target_os = "android")]
        {
            let payload = OpenWriterPayload { tree_uri, file_name };
            let res = self
                .0
                .run_mobile_plugin::<OpenWriterResponse>("openWriter", payload);
            return res
                .map(|r| (r.handle, r.document_uri))
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

    pub fn delete_document(&self, document_uri: String) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            let payload = DeleteDocumentPayload { document_uri };
            let res = self
                .0
                .run_mobile_plugin::<DeleteDocumentResponse>("deleteDocument", payload);
            return res
                .map(|r| {
                    let _ = r.ok;
                })
                .map_err(|e| format!("deleteDocument failed: {e}"));
        }
        #[allow(unreachable_code)]
        Err("deleteDocument is only supported on Android".to_string())
    }

    pub fn get_clipboard(&self) -> Result<String, String> {
        #[cfg(target_os = "android")]
        {
            let res = self
                .0
                .run_mobile_plugin::<GetClipboardResponse>("getClipboard", EmptyPayload {});
            return res
                .map(|r| r.content)
                .map_err(|e| format!("getClipboard failed: {e}"));
        }
        #[allow(unreachable_code)]
        Err("getClipboard is only supported on Android".to_string())
    }

    pub fn set_clipboard(&self, content: String) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            let payload = SetClipboardPayload { content };
            let res = self
                .0
                .run_mobile_plugin::<SetClipboardResponse>("setClipboard", payload);
            return res
                .map(|r| {
                    let _ = r.ok;
                })
                .map_err(|e| format!("setClipboard failed: {e}"));
        }
        #[allow(unreachable_code)]
        Err("setClipboard is only supported on Android".to_string())
    }

    pub fn get_device_name(&self) -> Result<String, String> {
        #[cfg(target_os = "android")]
        {
            let res = self
                .0
                .run_mobile_plugin::<GetDeviceNameResponse>("getDeviceName", EmptyPayload {});
            return res
                .map(|r| r.name)
                .map_err(|e| format!("getDeviceName failed: {e}"));
        }
        #[allow(unreachable_code)]
        Err("getDeviceName is only supported on Android".to_string())
    }
}
