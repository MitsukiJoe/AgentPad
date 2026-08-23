package app.agentpad

import android.content.Context
import android.media.AudioManager
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import android.view.MotionEvent
import android.view.WindowManager
import android.view.inputmethod.InputMethodManager
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONObject
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket
import java.net.SocketAddress
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import javax.net.SocketFactory

class MainActivity : FlutterActivity() {
    private lateinit var voiceRecording: VoiceRecordingTracker

    override fun onCreate(savedInstanceState: android.os.Bundle?) {
        super.onCreate(savedInstanceState)
        preferPeakRefreshRate()
    }

    override fun onResume() {
        super.onResume()
        preferPeakRefreshRate()
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        preferPeakRefreshRate()
        voiceRecording = VoiceRecordingTracker(this).also { it.start() }
        WifiWs(this, flutterEngine, voiceRecording)
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        if (::voiceRecording.isInitialized) voiceRecording.stop()
        super.cleanUpFlutterEngine(flutterEngine)
    }

    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        window.decorView.requestUnbufferedDispatch(event)
        return super.dispatchTouchEvent(event)
    }

    // Flutter often stays on 60Hz until the window asks for a higher mode.
    private fun preferPeakRefreshRate() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return
        val display =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                display
            } else {
                @Suppress("DEPRECATION")
                windowManager.defaultDisplay
            } ?: return
        val current = display.mode
        val best =
            display.supportedModes
                .filter {
                    it.physicalWidth == current.physicalWidth &&
                        it.physicalHeight == current.physicalHeight
                }
                .maxByOrNull { it.refreshRate }
                ?: display.supportedModes.maxByOrNull { it.refreshRate }
                ?: return
        val attrs = window.attributes
        attrs.preferredDisplayModeId = best.modeId
        @Suppress("DEPRECATION")
        attrs.preferredRefreshRate = best.refreshRate
        window.attributes = attrs
    }
}

class VoiceRecordingTracker(ctx: Context) {
    private val tracker = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
        VoiceRecordingTracker24(ctx)
    } else {
        null
    }

    fun start() = tracker?.start() ?: Unit

    fun stop() = tracker?.stop() ?: Unit

    fun reset() = tracker?.reset() ?: Unit

    fun hasRecentEvidence() = tracker?.hasRecentEvidence() == true
}

@android.annotation.TargetApi(Build.VERSION_CODES.N)
private class VoiceRecordingTracker24(ctx: Context) {
    private val audio = ctx.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private val main = Handler(Looper.getMainLooper())
    private var active = false
    private var observedAfterReset = false
    private var stoppedAt = Long.MIN_VALUE
    private val callback = object : AudioManager.AudioRecordingCallback() {
        override fun onRecordingConfigChanged(configs: List<android.media.AudioRecordingConfiguration>) {
            val next = configs.isNotEmpty()
            if (!active && next) observedAfterReset = true
            if (active && !next && observedAfterReset) stoppedAt = SystemClock.elapsedRealtime()
            active = next
        }
    }

    fun start() {
        active = audio.activeRecordingConfigurations.isNotEmpty()
        audio.registerAudioRecordingCallback(callback, main)
    }

    fun stop() = audio.unregisterAudioRecordingCallback(callback)

    fun reset() {
        observedAfterReset = false
        stoppedAt = Long.MIN_VALUE
    }

    fun hasRecentEvidence(): Boolean {
        val recent = observedAfterReset &&
            (active || SystemClock.elapsedRealtime() - stoppedAt <= RECENT_RECORDING_MS)
        return recent
    }

    private companion object {
        const val RECENT_RECORDING_MS = 3000L
    }
}

private class RoutedSocket(
    private val wifi: Network? = null,
    private val reaches: ((InetAddress) -> Boolean)? = null,
) : Socket() {
    override fun connect(endpoint: SocketAddress?) {
        maybeBind(endpoint)
        super.connect(endpoint)
        lowLatency()
    }

    override fun connect(endpoint: SocketAddress?, timeout: Int) {
        maybeBind(endpoint)
        super.connect(endpoint, timeout)
        lowLatency()
    }

    private fun maybeBind(endpoint: SocketAddress?) {
        val net = wifi ?: return
        val check = reaches ?: return
        val inet = endpoint as? InetSocketAddress ?: return
        val host = inet.address ?: return
        if (check(host)) runCatching { net.bindSocket(this) }
    }

    fun lowLatency() {
        tcpNoDelay = true
        keepAlive = true
        trafficClass = 0x10
    }
}

class WifiWs(
    private val ctx: Context,
    engine: FlutterEngine,
    private val voiceRecording: VoiceRecordingTracker,
) : EventChannel.StreamHandler {
    private val sockets = ConcurrentHashMap<String, WebSocket>()
    private val pointer = PointerPump(sockets)
    private var events: EventChannel.EventSink? = null
    private val main = Handler(Looper.getMainLooper())
    private val wifiLock: WifiManager.WifiLock? = runCatching { newWifiLock() }.getOrNull()
    private val cpuLock: PowerManager.WakeLock? =
        runCatching {
            (ctx.getSystemService(Context.POWER_SERVICE) as PowerManager)
                .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "agentpad:cpu")
                .apply { setReferenceCounted(false) }
        }.getOrNull()

    init {
        MethodChannel(engine.dartExecutor.binaryMessenger, "agentpad/ws")
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "connect" -> {
                        val id = call.argument<String>("id") ?: ""
                        val host = call.argument<String>("host") ?: ""
                        val port = call.argument<Int>("port") ?: 9618
                        connect(id, host, port, result)
                    }
                    "pointer" -> {
                        val id = call.argument<String>("id") ?: ""
                        pointer.add(
                            id,
                            num(call, "dx"),
                            num(call, "dy"),
                            intNum(call, "buttons"),
                            intNum(call, "wheel"),
                            call.argument<Boolean>("immediate") == true,
                        )
                        result.success(true)
                    }
                    "displayRefreshHz" -> {
                        val hz = displayRefreshHz()
                        result.success(hz)
                    }
                    "send" -> {
                        val id = call.argument<String>("id") ?: ""
                        val text = call.argument<String>("text") ?: ""
                        result.success(sockets[id]?.send(text) == true)
                    }
                    "close" -> {
                        val id = call.argument<String>("id") ?: ""
                        sockets.remove(id)?.close(1000, null)
                        pointer.drop(id)
                        releaseWifiIfIdle()
                        result.success(true)
                    }
                    "setAppIcon" -> {
                        val icon = call.argument<String>("icon") ?: "white"
                        try {
                            val pm = ctx.packageManager
                            val pkg = ctx.packageName
                            val defaultComponent = android.content.ComponentName(pkg, "$pkg.MainActivity")
                            val blackComponent = android.content.ComponentName(pkg, "$pkg.MainActivityBlack")

                            // 刚安装时组件为 DEFAULT 态（非显式 ENABLED/DISABLED），
                            // 若按查询结果跳过写入，会漏禁用另一入口导致桌面出现双图标；
                            // 这里无条件把两个组件写成目标状态，幂等且可自愈历史脏数据。
                            if (icon == "black") {
                                pm.setComponentEnabledSetting(
                                    blackComponent,
                                    android.content.pm.PackageManager.COMPONENT_ENABLED_STATE_ENABLED,
                                    android.content.pm.PackageManager.DONT_KILL_APP
                                )
                                pm.setComponentEnabledSetting(
                                    defaultComponent,
                                    android.content.pm.PackageManager.COMPONENT_ENABLED_STATE_DISABLED,
                                    android.content.pm.PackageManager.DONT_KILL_APP
                                )
                            } else {
                                pm.setComponentEnabledSetting(
                                    defaultComponent,
                                    android.content.pm.PackageManager.COMPONENT_ENABLED_STATE_ENABLED,
                                    android.content.pm.PackageManager.DONT_KILL_APP
                                )
                                pm.setComponentEnabledSetting(
                                    blackComponent,
                                    android.content.pm.PackageManager.COMPONENT_ENABLED_STATE_DISABLED,
                                    android.content.pm.PackageManager.DONT_KILL_APP
                                )
                            }
                            result.success(true)
                        } catch (e: Exception) {
                            result.error("ICON_CHANGE_FAILED", e.message, null)
                        }
                    }
                    "openUrl" -> {
                        val url = call.argument<String>("url") ?: ""
                        try {
                            val intent = android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(url))
                            intent.addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
                            ctx.startActivity(intent)
                            result.success(true)
                        } catch (e: Exception) {
                            result.error("OPEN_FAILED", e.message, null)
                        }
                    }
                    "voiceEvidence" -> {
                        val manager = ctx.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
                        result.success(
                            manager.currentInputMethodSubtype?.mode == "voice" ||
                                voiceRecording.hasRecentEvidence()
                        )
                    }
                    "resetVoiceEvidence" -> {
                        voiceRecording.reset()
                        result.success(null)
                    }
                    else -> result.notImplemented()
                }
            }
        EventChannel(engine.dartExecutor.binaryMessenger, "agentpad/ws_events")
            .setStreamHandler(this)
    }

    override fun onListen(arguments: Any?, events: EventChannel.EventSink?) {
        this.events = events
    }

    override fun onCancel(arguments: Any?) {
        events = null
    }

    private fun emit(id: String, event: String) {
        main.post { events?.success(mapOf("id" to id, "event" to event)) }
    }

    private fun connect(id: String, host: String, port: Int, result: MethodChannel.Result) {
        sockets.remove(id)?.close(1000, null)
        val settled = AtomicBoolean()
        fun settle(ok: Boolean) {
            if (settled.compareAndSet(false, true)) main.post { result.success(ok) }
        }
        val client = OkHttpClient.Builder()
            .socketFactory(pointerSocketFactory())
            .connectTimeout(4, TimeUnit.SECONDS)
            .build()
        val req = try {
            Request.Builder().url("ws://$host:$port").build()
        } catch (_: IllegalArgumentException) {
            settle(false)
            return
        }
        val ws = client.newWebSocket(
            req,
            object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    if (sockets[id] === webSocket) holdWifi()
                    settle(sockets[id] === webSocket)
                }

                override fun onMessage(webSocket: WebSocket, text: String) {
                    if (sockets[id] !== webSocket) return
                    main.post {
                        events?.success(mapOf("id" to id, "event" to "text", "data" to text))
                    }
                }

                override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                    if (sockets.remove(id, webSocket)) {
                        pointer.drop(id)
                        releaseWifiIfIdle()
                        emit(id, "close")
                    }
                    settle(false)
                }

                override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                    if (sockets.remove(id, webSocket)) {
                        pointer.drop(id)
                        releaseWifiIfIdle()
                        emit(id, "close")
                    }
                    settle(false)
                }
            },
        )
        sockets[id] = ws
    }

    private fun pointerSocketFactory(): SocketFactory {
        val wifi = wifiNetwork()
        return object : SocketFactory() {
            override fun createSocket(): Socket = RoutedSocket(wifi, ::wifiReaches)

            override fun createSocket(host: String?, port: Int): Socket =
                open(InetSocketAddress(host, port))

            override fun createSocket(
                host: String?,
                port: Int,
                localHost: InetAddress?,
                localPort: Int,
            ): Socket = createSocket(host, port)

            override fun createSocket(host: InetAddress?, port: Int): Socket =
                open(InetSocketAddress(host, port))

            override fun createSocket(
                address: InetAddress?,
                port: Int,
                localAddress: InetAddress?,
                localPort: Int,
            ): Socket = createSocket(address, port)

            private fun open(endpoint: InetSocketAddress): Socket {
                val s = RoutedSocket(wifi, ::wifiReaches)
                s.connect(endpoint)
                return s
            }
        }
    }

    /** True when [host] shares a prefix with the physical Wi‑Fi NIC. */
    private fun wifiReaches(host: InetAddress): Boolean {
        if (host.isLoopbackAddress) return false
        val wifi = wifiNetwork() ?: return false
        val cm = ctx.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val lp = cm.getLinkProperties(wifi) ?: return false
        for (la in lp.linkAddresses) {
            val local = la.address
            if (local is Inet4Address && host is Inet4Address) {
                if (sameV4Prefix(local, host, la.prefixLength)) return true
            }
        }
        return false
    }

    private fun sameV4Prefix(a: Inet4Address, b: Inet4Address, prefix: Int): Boolean {
        if (prefix <= 0) return false
        val bits = prefix.coerceAtMost(32)
        val mask = if (bits == 0) 0 else -1 shl (32 - bits)
        fun toInt(x: Inet4Address): Int {
            val p = x.address
            return (p[0].toInt() and 0xff shl 24) or
                (p[1].toInt() and 0xff shl 16) or
                (p[2].toInt() and 0xff shl 8) or
                (p[3].toInt() and 0xff)
        }
        return (toInt(a) and mask) == (toInt(b) and mask)
    }

    // Peak supported rate — current getRefreshRate() is often 60 while Flutter is mode-locked.
    private fun displayRefreshHz(): Double {
        val display =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                ctx.display
            } else {
                @Suppress("DEPRECATION")
                (ctx.getSystemService(Context.WINDOW_SERVICE) as WindowManager).defaultDisplay
            } ?: return 0.0
        var peak = display.refreshRate.toDouble()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            for (mode in display.supportedModes) {
                peak = maxOf(peak, mode.refreshRate.toDouble())
            }
        }
        return peak
    }

    private fun holdWifi() {
        wifiLock?.takeUnless { it.isHeld }?.let { runCatching { it.acquire() } }
        cpuLock?.takeUnless { it.isHeld }?.let { runCatching { it.acquire() } }
    }

    private fun releaseWifiIfIdle() {
        if (!sockets.isEmpty()) return
        runCatching { wifiLock?.takeIf { it.isHeld }?.release() }
        runCatching { cpuLock?.takeIf { it.isHeld }?.release() }
    }

    @Suppress("DEPRECATION")
    private fun newWifiLock(): WifiManager.WifiLock {
        val wifi = ctx.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        val mode =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                WifiManager.WIFI_MODE_FULL_LOW_LATENCY
            } else {
                WifiManager.WIFI_MODE_FULL_HIGH_PERF
            }
        return wifi.createWifiLock(mode, "agentpad").apply { setReferenceCounted(false) }
    }

    private fun wifiNetwork(): Network? {
        val cm = ctx.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        return cm.allNetworks.firstOrNull { n ->
            val caps = cm.getNetworkCapabilities(n) ?: return@firstOrNull false
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) &&
                caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
        }
    }
}

private class PointerPump(
    private val sockets: ConcurrentHashMap<String, WebSocket>,
) : Runnable {
    private val lock = Any()
    private val dx = HashMap<String, Double>()
    private val dy = HashMap<String, Double>()
    private val buttons = HashMap<String, Int>()
    private val wheel = HashMap<String, Int>()
    private val pending = HashSet<String>()
    private val thread = HandlerThread("agentpad-pointer").apply { start() }
    private val handler = Handler(thread.looper)
    private var draining = false

    fun add(id: String, ddx: Double, ddy: Double, btn: Int, wh: Int, immediate: Boolean) {
        var schedule = false
        synchronized(lock) {
            dx[id] = (dx[id] ?: 0.0) + ddx
            dy[id] = (dy[id] ?: 0.0) + ddy
            buttons[id] = btn
            wheel[id] = (wheel[id] ?: 0) + wh
            pending.add(id)
            if (!draining) {
                draining = true
                schedule = true
            }
        }
        // ponytail: immediate only forces a wake; coalescing still happens on the writer thread.
        if (schedule || immediate) handler.post(this)
    }

    fun drop(id: String) {
        synchronized(lock) {
            pending.remove(id)
            dx.remove(id)
            dy.remove(id)
            buttons.remove(id)
            wheel.remove(id)
        }
    }

    // Single-flight: coalesce only while the previous WebSocket write is busy.
    override fun run() {
        while (true) {
            val batch = synchronized(lock) {
                if (pending.isEmpty()) {
                    draining = false
                    return
                }
                val ids = pending.toList()
                pending.clear()
                ids.mapNotNull { id -> takeLocked(id)?.let { id to it } }
            }
            for (packet in batch) sockets[packet.first]?.send(packet.second)
        }
    }

    private fun takeLocked(id: String): String? {
        if (!dx.containsKey(id) &&
            !dy.containsKey(id) &&
            !buttons.containsKey(id) &&
            !wheel.containsKey(id)
        ) {
            return null
        }
        val x = dx.remove(id) ?: 0.0
        val y = dy.remove(id) ?: 0.0
        val b = buttons.remove(id) ?: 0
        val w = wheel.remove(id) ?: 0
        return JSONObject()
            .put("type", "pointer")
            .put("dx", x)
            .put("dy", y)
            .put("buttons", b)
            .put("wheel", w)
            .toString()
    }
}

private fun num(call: MethodCall, key: String): Double {
    val v = call.argument<Any>(key) ?: return 0.0
    return if (v is Number) v.toDouble() else 0.0
}

private fun intNum(call: MethodCall, key: String): Int {
    val v = call.argument<Any>(key) ?: return 0
    return if (v is Number) v.toInt() else 0
}
