# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile

# ===========================================
# Tauri Plugin Framework
# ===========================================
# Keep Tauri plugin annotations
-keepattributes *Annotation*

# Keep all Tauri plugin classes and their members
-keep class app.tauri.** { *; }
-keep interface app.tauri.** { *; }

# ===========================================
# Custom StoragePlugin
# ===========================================
# Keep StoragePlugin and all its methods
-keep class app.tauri.storage.StoragePlugin { *; }

# Keep all @InvokeArg annotated classes and their fields
# These are used for deserialization from Rust
-keep class app.tauri.storage.PickFolderArgs { *; }
-keep class app.tauri.storage.OpenWriterArgs { *; }
-keep class app.tauri.storage.WriteChunkArgs { *; }
-keep class app.tauri.storage.CloseWriterArgs { *; }
-keep class app.tauri.storage.DeleteDocumentArgs { *; }
-keep class app.tauri.storage.UriArgs { *; }
-keep class app.tauri.storage.ReadUriChunkArgs { *; }

# Keep any class annotated with @InvokeArg
-keep @app.tauri.annotation.InvokeArg class * { *; }

# Keep any class annotated with @TauriPlugin
-keep @app.tauri.annotation.TauriPlugin class * { *; }

# Keep methods annotated with @Command
-keepclassmembers class * {
    @app.tauri.annotation.Command <methods>;
}

# ===========================================
# MainActivity (for MulticastLock)
# ===========================================
-keep class com.lantransfer.app.MainActivity { *; }