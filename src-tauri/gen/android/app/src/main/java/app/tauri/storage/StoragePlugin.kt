package app.tauri.storage

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import android.util.Base64
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject
import java.io.OutputStream
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

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

@TauriPlugin
class StoragePlugin(private val activity: Activity) : Plugin(activity) {

    private var pendingInvoke: Invoke? = null
    private val writers = ConcurrentHashMap<Long, OutputStream>()
    private val nextHandle = AtomicLong(1)

    private val folderPickerLauncher: ActivityResultLauncher<Uri?> =
        (activity as ComponentActivity).activityResultRegistry.register(
            "storage_pick_folder",
            ActivityResultContracts.OpenDocumentTree()
        ) { uri: Uri? ->
            val invoke = pendingInvoke ?: return@register
            pendingInvoke = null

            val result = JSObject()
            if (uri != null) {
                // Persist read/write permission across device reboots
                val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or
                        Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                activity.contentResolver.takePersistableUriPermission(uri, flags)
                result.put("uri", uri.toString())
            } else {
                result.put("uri", JSONObject.NULL)
            }
            invoke.resolve(result)
        }

    @Command
    fun pickFolder(invoke: Invoke) {
        pendingInvoke = invoke
        folderPickerLauncher.launch(null)
    }

    @Command
    fun openWriter(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(OpenWriterArgs::class.java)

            val treeUri = Uri.parse(args.tree_uri)
            val docId = DocumentsContract.getTreeDocumentId(treeUri)
            val dirUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, docId)

            val fileUri = DocumentsContract.createDocument(
                activity.contentResolver, dirUri, "application/octet-stream", args.file_name
            )

            if (fileUri == null) {
                invoke.reject("Failed to create file")
                return
            }

            val outputStream = activity.contentResolver.openOutputStream(fileUri)
            if (outputStream == null) {
                invoke.reject("Failed to open output stream")
                return
            }

            val handle = nextHandle.getAndIncrement()
            writers[handle] = outputStream

            val result = JSObject()
            result.put("handle", handle)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("openWriter failed: ${e.message}")
        }
    }

    @Command
    fun writeChunk(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(WriteChunkArgs::class.java)

            val outputStream = writers[args.handle]
            if (outputStream == null) {
                invoke.reject("Invalid writer handle: ${args.handle}")
                return
            }

            val bytes: ByteArray = Base64.decode(args.data_base64, Base64.NO_WRAP)
            outputStream.write(bytes)

            val result = JSObject()
            result.put("ok", true)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("writeChunk failed: ${e.message}")
        }
    }

    @Command
    fun closeWriter(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseWriterArgs::class.java)
            val outputStream = writers.remove(args.handle)
            if (outputStream == null) {
                invoke.reject("Invalid writer handle: ${args.handle}")
                return
            }

            outputStream.flush()
            outputStream.close()

            val result = JSObject()
            result.put("ok", true)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("closeWriter failed: ${e.message}")
        }
    }
}
