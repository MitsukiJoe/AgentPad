package app.agentpad

import android.os.Handler
import android.os.HandlerThread
import okhttp3.WebSocket
import org.json.JSONObject
import java.util.ArrayDeque
import java.util.concurrent.ConcurrentHashMap

internal class PointerPump(
    private val sockets: ConcurrentHashMap<String, WebSocket>,
    scheduler: ((Runnable) -> Unit)? = null,
    private val sendPacket: ((String, String) -> Boolean)? = null,
) : Runnable {
    private data class Segment(
        var dx: Double,
        var dy: Double,
        val buttons: Int,
        var wheel: Int,
        val buttonEdge: Boolean,
    )

    private data class Packet(
        val id: String,
        val json: String,
        val socket: WebSocket?,
    )

    private val lock = Any()
    private val queues = HashMap<String, ArrayDeque<Segment>>()
    private val buttonState = HashMap<String, Int>()
    private val pending = LinkedHashSet<String>()
    private val thread: HandlerThread?
    private val handler: Handler?
    private val wake: (Runnable) -> Unit
    private var draining = false

    init {
        if (scheduler == null) {
            thread = HandlerThread("agentpad-pointer").apply { start() }
            handler = Handler(thread.looper)
            wake = { runnable -> handler.post(runnable) }
        } else {
            thread = null
            handler = null
            wake = scheduler
        }
    }

    fun add(id: String, ddx: Double, ddy: Double, btn: Int, wh: Int, immediate: Boolean) {
        var schedule = false
        synchronized(lock) {
            val edge = (buttonState.put(id, btn) ?: 0) != btn
            val queue = queues.getOrPut(id) { ArrayDeque() }
            val last = queue.peekLast()
            if (last != null && !last.buttonEdge && !edge && last.buttons == btn) {
                last.dx += ddx
                last.dy += ddy
                last.wheel += wh
            } else {
                queue.addLast(Segment(ddx, ddy, btn, wh, edge))
            }
            pending.add(id)
            if (!draining) {
                draining = true
                schedule = true
            }
        }
        if (schedule) wake(this)
    }

    fun drop(id: String) {
        synchronized(lock) {
            pending.remove(id)
            queues.remove(id)
            buttonState.remove(id)
        }
    }

    override fun run() {
        while (true) {
            val batch = synchronized(lock) {
                if (pending.isEmpty()) {
                    draining = false
                    return
                }
                val ids = pending.toList()
                pending.clear()
                ids.mapNotNull(::takeLocked)
            }
            for (packet in batch) {
                sendPacket?.invoke(packet.id, packet.json) ?: packet.socket?.send(packet.json)
            }
        }
    }

    private fun takeLocked(id: String): Packet? {
        val queue = queues[id] ?: return null
        val segment = queue.removeFirst()
        if (queue.isEmpty()) queues.remove(id) else pending.add(id)
        return Packet(
            id,
            JSONObject()
                .put("type", "pointer")
                .put("dx", segment.dx)
                .put("dy", segment.dy)
                .put("buttons", segment.buttons)
                .put("wheel", segment.wheel)
                .toString(),
            sockets[id],
        )
    }
}
