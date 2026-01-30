package app.tauri.storage

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
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
import android.util.Base64

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

@TauriPlugin
class StoragePlugin(private val activity: Activity) : Plugin(activity) {
    private val outputStreams = mutableMapOf<Long, OutputStream>()
    private var nextHandle: Long = 1
    private var pendingInvoke: Invoke? = null
    private lateinit var pickFolderLauncher: ActivityResultLauncher<Intent>

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

            val ret = JSObject()
            ret.put("handle", handle)
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
}
