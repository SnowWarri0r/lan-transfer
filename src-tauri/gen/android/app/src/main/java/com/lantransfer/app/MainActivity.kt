package com.lantransfer.app

import android.content.Context
import android.net.wifi.WifiManager
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback

class MainActivity : TauriActivity() {
    private var multicastLock: WifiManager.MulticastLock? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // 获取 MulticastLock 以支持 UDP 组播
        val wifiManager = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        multicastLock = wifiManager.createMulticastLock("lan_transfer_multicast")
        multicastLock?.acquire()

        // 使用新的 OnBackPressedDispatcher API 处理返回手势和返回键
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                val webView = findWebView(findViewById(android.R.id.content))
                if (webView != null) {
                    webView.evaluateJavascript(
                        "(function() { if (typeof window.__TAURI_BACK_HANDLER__ === 'function') { return window.__TAURI_BACK_HANDLER__(); } return true; })()"
                    ) { result ->
                        if (result == "true") {
                            // 允许退出：禁用此回调，然后再次触发返回
                            isEnabled = false
                            onBackPressedDispatcher.onBackPressed()
                        }
                        // result == "false" 时不做任何事，阻止退出
                    }
                } else {
                    // 没有找到 WebView，允许默认行为
                    isEnabled = false
                    onBackPressedDispatcher.onBackPressed()
                }
            }
        })
    }

    override fun onDestroy() {
        super.onDestroy()
        multicastLock?.release()
    }

    private fun findWebView(view: android.view.View?): WebView? {
        if (view == null) return null
        if (view is WebView) return view
        if (view is android.view.ViewGroup) {
            for (i in 0 until view.childCount) {
                val found = findWebView(view.getChildAt(i))
                if (found != null) return found
            }
        }
        return null
    }
}