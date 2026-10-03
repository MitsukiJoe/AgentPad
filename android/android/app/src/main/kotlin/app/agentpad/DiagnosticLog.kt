package app.agentpad

import java.io.File
import java.util.Timer
import java.util.TimerTask

object DiagnosticLog {
    private const val LIMIT = 2 * 1024 * 1024
    private var directory: File? = null
    private var timer: Timer? = null
    private var pointerTask: TimerTask? = null
    private val pointerCounts = mutableMapOf<String, Long>()
    @Volatile var enabled = false
        private set
    private val kinds = setOf("text", "key", "pointer", "undo", "ping", "hello", "connected", "unknown", "connection", "lifecycle", "update")
    private val stages = setOf("send", "receive", "start", "stop", "check")
    private val results = setOf("ok", "failed", "active", "inactive")

    @Synchronized fun setEnabled(root: File, value: Boolean): Boolean {
        if (enabled == value) return enabled
        timer?.cancel()
        timer = null
        pointerTask = null
        pointerCounts.clear()
        enabled = false
        if (!value) return false
        return runCatching {
            directory = File(root, "diagnostics").apply { mkdirs() }
            clear()
            enabled = true
            event("lifecycle", "start", "active")
            timer = Timer("agentpad-diagnostics", true)
            true
        }.getOrElse { enabled = false; false }
    }

    @Synchronized fun event(kind: String?, stage: String?, result: String?, count: Int = 1) {
        if (!enabled || count !in 1..1000000 || kind !in kinds || stage !in stages || result !in results) return
        if (kind == "pointer") {
            val key = "$stage $result"
            pointerCounts[key] = (pointerCounts[key] ?: 0) + count
            if (pointerTask == null) {
                pointerTask = object : TimerTask() {
                    override fun run() = flushPointers(this)
                }.also { timer?.schedule(it, 1000) }
            }
            return
        }
        append("$kind $stage $result data=[redacted]")
    }

    @Synchronized private fun flushPointers(task: TimerTask) {
        if (!enabled || pointerTask !== task) return
        pointerTask = null
        for ((key, count) in pointerCounts) append("pointer $key count=$count data=[redacted]")
        pointerCounts.clear()
    }

    private fun append(event: String) {
        runCatching {
            val dir = directory ?: return
            val current = File(dir, "current.log")
            val line = "${System.currentTimeMillis()} $event\n"
            if (current.length() + line.toByteArray().size > LIMIT) {
                val previous = File(dir, "previous.log")
                previous.delete()
                check(current.renameTo(previous))
            }
            current.appendText(line)
        }
    }

    @Synchronized fun read(root: File): String = runCatching {
        val dir = File(root, "diagnostics")
        listOf("previous.log", "current.log").joinToString("") { name ->
            val file = File(dir, name)
            if (file.isFile && file.length() <= LIMIT) file.readText() else ""
        }
    }.getOrDefault("读取失败")

    @Synchronized fun clear(root: File? = null) {
        val dir = root?.let { File(it, "diagnostics") } ?: directory ?: return
        pointerTask?.cancel()
        pointerTask = null
        pointerCounts.clear()
        for (name in listOf("current.log", "previous.log")) {
            val file = File(dir, name)
            check(!file.exists() || file.delete())
        }
    }
}
