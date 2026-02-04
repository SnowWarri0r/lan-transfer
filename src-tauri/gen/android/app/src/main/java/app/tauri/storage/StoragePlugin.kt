package app.tauri.storage

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.OutputStream
import java.io.InputStream
import android.util.Base64
import org.json.JSONArray

@InvokeArg
internal class PickFolderArgs

@InvokeArg
internal class OpenWriterArgs {
    lateinit var tree_uri: String
    lateinit var file_name: String
}

@InvokeArg
internal class WriteChunkArgs {
    var handle: Long = 0
    lateinit var data_base64: String
}

@InvokeArg
internal class CloseWriterArgs {
    var handle: Long = 0
}

@InvokeArg
internal class DeleteDocumentArgs {
    lateinit var document_uri: String
}

@InvokeArg
internal class UriArgs {
    lateinit var uri: String
}

@InvokeArg
internal class ReadUriChunkArgs {
    lateinit var uri: String
    var offset: Long = 0
    var size: Int = 0
}

@InvokeArg
internal class SetClipboardArgs {
    lateinit var content: String
}

@TauriPlugin
class StoragePlugin(private val activity: Activity) : Plugin(activity) {
    private val outputStreams = mutableMapOf<Long, OutputStream>()
    private val documentUris = mutableMapOf<Long, Uri>()
    private var nextHandle: Long = 1
    private var pendingInvoke: Invoke? = null
    private lateinit var pickFolderLauncher: ActivityResultLauncher<Intent>
    private lateinit var pickFilesLauncher: ActivityResultLauncher<Intent>

    init {
        // 使用 activityResultRegistry.register 而不是 registerForActivityResult
        // 因为插件初始化时 Activity 可能已经 STARTED，会抛 IllegalStateException
        pickFolderLauncher = (activity as ComponentActivity).activityResultRegistry.register(
            "pickFolder",
            ActivityResultContracts.StartActivityForResult()
        ) { result ->
            val invoke = pendingInvoke ?: return@register
            pendingInvoke = null

            if (result.resultCode == Activity.RESULT_OK) {
                val uri = result.data?.data
                if (uri != null) {
                    // 持久化权限
                    activity.contentResolver.takePersistableUriPermission(
                        uri,
                        Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                    )
                    val ret = JSObject()
                    ret.put("uri", uri.toString())
                    invoke.resolve(ret)
                } else {
                    invoke.reject("No URI returned")
                }
            } else {
                invoke.reject("User cancelled")
            }
        }

        // 多文件选择器
        pickFilesLauncher = (activity as ComponentActivity).activityResultRegistry.register(
            "pickMultipleFiles",
            ActivityResultContracts.StartActivityForResult()
        ) { result ->
            val invoke = pendingInvoke ?: return@register
            pendingInvoke = null

            if (result.resultCode == Activity.RESULT_OK) {
                val data = result.data
                val uris = mutableListOf<String>()

                // 处理多选
                data?.clipData?.let { clipData ->
                    for (i in 0 until clipData.itemCount) {
                        clipData.getItemAt(i).uri?.let { uri ->
                            // ACTION_GET_CONTENT 返回的 URI 已经有临时读取权限
                            // 不需要也不能调用 takePersistableUriPermission
                            uris.add(uri.toString())
                        }
                    }
                }

                // 处理单选（fallback）
                if (uris.isEmpty()) {
                    data?.data?.let { uri ->
                        uris.add(uri.toString())
                    }
                }

                if (uris.isNotEmpty()) {
                    val jsonArray = JSONArray()
                    uris.forEach { uri -> jsonArray.put(uri) }

                    val ret = JSObject()
                    ret.put("uris", jsonArray)
                    invoke.resolve(ret)
                } else {
                    invoke.reject("No files selected")
                }
            } else {
                invoke.reject("User cancelled")
            }
        }
    }

    @Command
    fun pickFolder(invoke: Invoke) {
        pendingInvoke = invoke
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
        }
        pickFolderLauncher.launch(intent)
    }

    @Command
    fun pickMultipleFiles(invoke: Invoke) {
        pendingInvoke = invoke
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            type = "*/*"
            putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
            addCategory(Intent.CATEGORY_OPENABLE)
            // 允许跨设备访问
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        pickFilesLauncher.launch(intent)
    }

    @Command
    fun openWriter(invoke: Invoke) {
        val args = invoke.parseArgs(OpenWriterArgs::class.java)
        val treeUri = Uri.parse(args.tree_uri)
        val docUri = DocumentsContract.buildDocumentUriUsingTree(
            treeUri,
            DocumentsContract.getTreeDocumentId(treeUri)
        )

        try {
            val fileUri = DocumentsContract.createDocument(
                activity.contentResolver,
                docUri,
                "application/octet-stream",
                args.file_name
            ) ?: run {
                invoke.reject("Failed to create document")
                return
            }

            val outputStream = activity.contentResolver.openOutputStream(fileUri) ?: run {
                invoke.reject("Failed to open output stream")
                return
            }

            val handle = nextHandle++
            outputStreams[handle] = outputStream
            documentUris[handle] = fileUri

            val ret = JSObject()
            ret.put("handle", handle)
            ret.put("document_uri", fileUri.toString())
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Error: ${e.message}")
        }
    }

    @Command
    fun writeChunk(invoke: Invoke) {
        val args = invoke.parseArgs(WriteChunkArgs::class.java)
        val outputStream = outputStreams[args.handle]

        if (outputStream == null) {
            invoke.reject("Invalid handle")
            return
        }

        try {
            val bytes = Base64.decode(args.data_base64, Base64.DEFAULT)
            outputStream.write(bytes)
            val ret = JSObject()
            ret.put("ok", true)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Write error: ${e.message}")
        }
    }

    @Command
    fun closeWriter(invoke: Invoke) {
        val args = invoke.parseArgs(CloseWriterArgs::class.java)
        val outputStream = outputStreams.remove(args.handle)
        documentUris.remove(args.handle)

        if (outputStream == null) {
            invoke.reject("Invalid handle")
            return
        }

        try {
            outputStream.flush()
            outputStream.close()
            val ret = JSObject()
            ret.put("ok", true)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Close error: ${e.message}")
        }
    }

    @Command
    fun deleteDocument(invoke: Invoke) {
        val args = invoke.parseArgs(DeleteDocumentArgs::class.java)
        val uri = Uri.parse(args.document_uri)

        try {
            // Close output stream if still open for this document
            val handleToRemove = documentUris.entries.find { it.value == uri }?.key
            if (handleToRemove != null) {
                val outputStream = outputStreams.remove(handleToRemove)
                documentUris.remove(handleToRemove)
                try {
                    outputStream?.close()
                } catch (_: Exception) {}
            }

            val deleted = DocumentsContract.deleteDocument(activity.contentResolver, uri)
            val ret = JSObject()
            ret.put("ok", deleted)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Delete error: ${e.message}")
        }
    }

    @Command
    fun getFileInfo(invoke: Invoke) {
        val args = invoke.parseArgs(UriArgs::class.java)
        val uri = Uri.parse(args.uri)

        try {
            activity.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) {
                    val nameIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    val sizeIndex = cursor.getColumnIndex(OpenableColumns.SIZE)

                    val name = if (nameIndex >= 0) cursor.getString(nameIndex) else "unknown"
                    val size = if (sizeIndex >= 0) cursor.getLong(sizeIndex) else 0L

                    val ret = JSObject()
                    ret.put("name", name)
                    ret.put("size", size)
                    invoke.resolve(ret)
                } else {
                    invoke.reject("Failed to read file info")
                }
            } ?: invoke.reject("Failed to query URI")
        } catch (e: Exception) {
            invoke.reject("Error getting file info: ${e.message}")
        }
    }

    @Command
    fun readUriChunk(invoke: Invoke) {
        val args = invoke.parseArgs(ReadUriChunkArgs::class.java)
        val uri = Uri.parse(args.uri)

        try {
            activity.contentResolver.openInputStream(uri)?.use { inputStream ->
                // Skip to offset
                inputStream.skip(args.offset)

                // Read chunk
                val buffer = ByteArray(args.size)
                val bytesRead = inputStream.read(buffer, 0, args.size)

                if (bytesRead > 0) {
                    // Encode to base64
                    val actualData = if (bytesRead < args.size) {
                        buffer.copyOf(bytesRead)
                    } else {
                        buffer
                    }
                    val base64Data = Base64.encodeToString(actualData, Base64.NO_WRAP)

                    val ret = JSObject()
                    ret.put("data", base64Data)
                    ret.put("bytes_read", bytesRead)
                    invoke.resolve(ret)
                } else {
                    val ret = JSObject()
                    ret.put("data", "")
                    ret.put("bytes_read", 0)
                    invoke.resolve(ret)
                }
            } ?: invoke.reject("Failed to open input stream")
        } catch (e: Exception) {
            invoke.reject("Error reading URI chunk: ${e.message}")
        }
    }

    @Command
    fun getClipboard(invoke: Invoke) {
        try {
            val clipboardManager = activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            val clip = clipboardManager.primaryClip

            val text = if (clip != null && clip.itemCount > 0) {
                clip.getItemAt(0).coerceToText(activity).toString()
            } else {
                ""
            }

            val ret = JSObject()
            ret.put("content", text)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Failed to get clipboard: ${e.message}")
        }
    }

    @Command
    fun setClipboard(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SetClipboardArgs::class.java)
            val clipboardManager = activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            val clip = ClipData.newPlainText("text", args.content)
            clipboardManager.setPrimaryClip(clip)

            val ret = JSObject()
            ret.put("ok", true)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Failed to set clipboard: ${e.message}")
        }
    }

    @Command
    fun getDeviceName(invoke: Invoke) {
        try {
            val manufacturer = Build.MANUFACTURER
            val model = Build.MODEL
            val deviceName = if (model.startsWith(manufacturer, ignoreCase = true)) {
                model
            } else {
                "$manufacturer $model"
            }
            val ret = JSObject()
            ret.put("name", deviceName)
            invoke.resolve(ret)
        } catch (e: Exception) {
            val ret = JSObject()
            ret.put("name", "Android")
            invoke.resolve(ret)
        }
    }
}
