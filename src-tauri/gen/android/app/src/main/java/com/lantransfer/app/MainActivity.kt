package com.lantransfer.app

import android.content.Context
import android.net.wifi.WifiManager
import android.os.Bundle

class MainActivity : TauriActivity() {
    private var multicastLock: WifiManager.MulticastLock? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // 获取 MulticastLock 以支持 UDP 组播
        val wifiManager = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        multicastLock = wifiManager.createMulticastLock("lan_transfer_multicast")
        multicastLock?.acquire()
    }

    override fun onDestroy() {
        super.onDestroy()
        multicastLock?.release()
    }
}