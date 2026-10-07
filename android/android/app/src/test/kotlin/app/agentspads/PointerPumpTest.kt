package app.agentspads

import okhttp3.Request
import okhttp3.WebSocket
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import okio.ByteString
import java.util.concurrent.ConcurrentHashMap

class PointerPumpTest {
    @Test
    fun preserves_button_edges_and_motion_boundaries_when_writer_is_backlogged() {
        val h = Harness()

        h.pump.add("a", 0.0, 0.0, 1, 0, immediate = true)
        h.pump.add("a", 4.0, 0.0, 1, 0, immediate = false)
        h.pump.add("a", 2.0, 0.0, 1, 0, immediate = false)
        h.pump.add("a", 0.0, 0.0, 0, 0, immediate = true)
        h.pump.add("a", 0.0, 0.0, 0, 2, immediate = false)
        h.pump.add("a", 3.0, 0.0, 0, 0, immediate = false)
        h.drain()

        assertEquals(
            listOf(
                Packet("a", 0.0, 0.0, 1, 0),
                Packet("a", 6.0, 0.0, 1, 0),
                Packet("a", 0.0, 0.0, 0, 0),
                Packet("a", 3.0, 0.0, 0, 2),
            ),
            h.sent,
        )
    }

    @Test
    fun keeps_right_click_and_release_edges_across_drains() {
        val h = Harness()

        h.pump.add("a", 0.0, 0.0, 2, 0, immediate = true)
        h.pump.add("a", 0.0, 0.0, 0, 0, immediate = true)
        h.drain()
        h.pump.add("a", 0.0, 0.0, 2, 0, immediate = true)
        h.drain()
        h.pump.add("a", 0.0, 0.0, 0, 0, immediate = true)
        h.pump.add("a", 5.0, 0.0, 0, 0, immediate = false)
        h.drain()

        assertEquals(
            listOf(
                Packet("a", 0.0, 0.0, 2, 0),
                Packet("a", 0.0, 0.0, 0, 0),
                Packet("a", 0.0, 0.0, 2, 0),
                Packet("a", 0.0, 0.0, 0, 0),
                Packet("a", 5.0, 0.0, 0, 0),
            ),
            h.sent,
        )
    }

    @Test
    fun writer_can_accept_new_input_without_reordering_or_extra_tasks() {
        val h = Harness()
        var added = false
        h.onSend = {
            if (!added) {
                added = true
                h.pump.add("a", 3.0, 0.0, 1, 0, immediate = false)
            }
        }

        h.pump.add("a", 0.0, 0.0, 1, 0, immediate = true)
        h.pump.add("a", 1.0, 0.0, 1, 0, immediate = false)
        h.drain()

        assertEquals(
            listOf(
                Packet("a", 0.0, 0.0, 1, 0),
                Packet("a", 4.0, 0.0, 1, 0),
            ),
            h.sent,
        )
        assertEquals(0, h.tasks.size)
    }

    @Test
    fun reconnect_during_a_batch_cannot_redirect_old_packet_to_new_socket() {
        val sockets = ConcurrentHashMap<String, WebSocket>()
        val old = RecordingSocket()
        val replacement = RecordingSocket()
        lateinit var pump: PointerPump
        val other = RecordingSocket {
            pump.drop("a")
            sockets["a"] = replacement
        }
        sockets["b"] = other
        sockets["a"] = old
        val tasks = ArrayDeque<Runnable>()
        pump = PointerPump(sockets, scheduler = { tasks.addLast(it) })

        pump.add("b", 1.0, 0.0, 0, 0, immediate = false)
        pump.add("a", 0.0, 0.0, 1, 0, immediate = true)
        tasks.removeFirst().run()

        assertEquals(1, old.packets.size)
        assertEquals(0, replacement.packets.size)
    }

    @Test
    fun merges_non_boundary_motion_per_device_without_losing_totals() {
        val h = Harness()

        h.pump.add("a", 1.25, 2.5, 0, 3, immediate = false)
        h.pump.add("a", 0.75, -0.5, 0, -1, immediate = false)
        h.pump.add("b", 9.0, 8.0, 0, 4, immediate = false)
        h.drain()

        assertEquals(
            listOf(
                Packet("a", 2.0, 2.0, 0, 2),
                Packet("b", 9.0, 8.0, 0, 4),
            ),
            h.sent,
        )
    }

    @Test
    fun drop_clears_queued_segments_and_button_state() {
        val h = Harness()

        h.pump.add("a", 0.0, 0.0, 1, 0, immediate = true)
        h.pump.add("a", 7.0, 0.0, 1, 0, immediate = false)
        h.pump.drop("a")
        h.pump.add("a", 0.0, 0.0, 1, 0, immediate = false)
        h.pump.add("a", 3.0, 0.0, 1, 0, immediate = false)
        h.drain()

        assertEquals(
            listOf(
                Packet("a", 0.0, 0.0, 1, 0),
                Packet("a", 3.0, 0.0, 1, 0),
            ),
            h.sent,
        )
    }

    @Test
    fun long_motion_burst_stays_coalesced() {
        val h = Harness()
        repeat(240 * 60) {
            h.pump.add("a", 0.25, -0.25, 0, 1, immediate = false)
        }
        assertEquals(1, h.tasks.size)
        h.drain()
        assertEquals(listOf(Packet("a", 3600.0, -3600.0, 0, 14400)), h.sent)
    }

    @Test
    fun immediate_samples_do_not_enqueue_extra_writer_tasks() {
        val h = Harness()

        h.pump.add("a", 0.0, 0.0, 1, 0, immediate = true)
        h.pump.add("a", 1.0, 0.0, 1, 0, immediate = false)
        h.pump.add("a", 0.0, 0.0, 0, 0, immediate = true)

        assertEquals(1, h.tasks.size)
        h.drain()
        assertTrue(h.sent.isNotEmpty())
    }

    private class Harness {
        val tasks = ArrayDeque<Runnable>()
        val sent = mutableListOf<Packet>()
        var onSend: (() -> Unit)? = null
        val pump = PointerPump(
            ConcurrentHashMap(),
            scheduler = { tasks.addLast(it) },
            sendPacket = { id, json ->
                val packet = JSONObject(json)
                sent += Packet(
                    id,
                    packet.getDouble("dx"),
                    packet.getDouble("dy"),
                    packet.getInt("buttons"),
                    packet.getInt("wheel"),
                )
                onSend?.invoke()
                true
            },
        )

        fun drain() {
            while (tasks.isNotEmpty()) tasks.removeFirst().run()
        }
    }

    private data class Packet(
        val id: String,
        val dx: Double,
        val dy: Double,
        val buttons: Int,
        val wheel: Int,
    )

    private class RecordingSocket(private val afterSend: (() -> Unit)? = null) : WebSocket {
        val packets = mutableListOf<String>()

        override fun request() = Request.Builder().url("ws://localhost").build()

        override fun queueSize() = 0L

        override fun send(text: String): Boolean {
            packets += text
            afterSend?.invoke()
            return true
        }

        override fun send(bytes: ByteString) = true

        override fun close(code: Int, reason: String?) = true

        override fun cancel() = Unit
    }
}
