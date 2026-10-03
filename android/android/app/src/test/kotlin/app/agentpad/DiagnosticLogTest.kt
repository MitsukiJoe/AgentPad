package app.agentpad

import java.io.File
import java.nio.file.Files
import org.junit.After
import org.junit.Assert.*
import org.junit.Test

class DiagnosticLogTest {
    private val root = Files.createTempDirectory("agentpad-diagnostic-test").toFile()

    @After fun cleanup() {
        DiagnosticLog.setEnabled(root, false)
        root.deleteRecursively()
    }

    @Test fun sessionWhitelistAndClear() {
        DiagnosticLog.setEnabled(root, false)
        DiagnosticLog.event("text", "send", "ok")
        assertFalse(File(root, "diagnostics").exists())
        assertTrue(DiagnosticLog.setEnabled(root, true))
        DiagnosticLog.event("SECRET /private 192.168.1.12", "send", "ok")
        DiagnosticLog.event("text", "SECRET", "ok")
        DiagnosticLog.event("key", "send", "SECRET")
        DiagnosticLog.event("text", "send", "ok")
        DiagnosticLog.event("key", "send", "ok")
        val log = DiagnosticLog.read(root)
        assertTrue(log.contains("text send ok data=[redacted]"))
        assertTrue(log.contains("key send ok data=[redacted]"))
        assertFalse(log.contains("SECRET"))
        assertTrue(DiagnosticLog.setEnabled(root, true))
        assertEquals(log, DiagnosticLog.read(root))
        DiagnosticLog.setEnabled(root, false)
        DiagnosticLog.event("undo", "send", "ok")
        assertEquals(log, DiagnosticLog.read(root))
        DiagnosticLog.setEnabled(root, true)
        assertFalse(DiagnosticLog.read(root).contains("text send"))
        DiagnosticLog.clear(root)
        assertEquals("", DiagnosticLog.read(root))
        assertTrue(DiagnosticLog.enabled)
    }

    @Test fun boundedRotationAndPointerAggregation() {
        DiagnosticLog.setEnabled(root, true)
        DiagnosticLog.clear(root)
        val current = File(root, "diagnostics/current.log")
        current.writeText("x".repeat(2 * 1024 * 1024))
        DiagnosticLog.event("text", "send", "ok")
        val previous = File(root, "diagnostics/previous.log")
        assertEquals(2 * 1024 * 1024L, previous.length())
        current.writeText("y".repeat(2 * 1024 * 1024))
        DiagnosticLog.event("key", "send", "ok")
        assertTrue(previous.readText().startsWith("y"))
        assertEquals(2, File(root, "diagnostics").listFiles()!!.size)
        assertTrue(current.length() <= 2 * 1024 * 1024L)
        DiagnosticLog.clear(root)
        repeat(240) { DiagnosticLog.event("pointer", "send", "ok") }
        assertEquals("", DiagnosticLog.read(root))
        Thread.sleep(1200)
        val log = DiagnosticLog.read(root)
        assertTrue(log.contains("pointer send ok count=240 data=[redacted]"))
        assertEquals(1, log.lines().count { it.isNotEmpty() })
        DiagnosticLog.event("pointer", "send", "ok")
        DiagnosticLog.clear(root)
        Thread.sleep(1100)
        assertEquals("", DiagnosticLog.read(root))
        DiagnosticLog.event("pointer", "send", "ok")
        DiagnosticLog.setEnabled(root, false)
        Thread.sleep(1100)
        assertEquals("", DiagnosticLog.read(root))
    }
}
