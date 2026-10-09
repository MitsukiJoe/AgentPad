package app.agentspads

import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.os.Bundle
import android.os.SystemClock
import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.ExtractedTextRequest
import android.view.inputmethod.InputConnection
import io.flutter.embedding.android.FlutterActivity

class InputConnectionChecks : Instrumentation() {
    private var checkIconRestart = false
    private var lastLaunch = ""

    override fun onCreate(arguments: Bundle?) {
        super.onCreate(arguments)
        checkIconRestart = arguments?.getString("check") == "icon-restart"
        start()
    }

    override fun onStart() {
        val result = Bundle()
        try {
            if (checkIconRestart) {
                targetContext.getSystemService(android.app.ActivityManager::class.java).appTasks
                    .forEach { it.finishAndRemoveTask() }
            }
            val activity = startActivitySync(
                checkNotNull(targetContext.packageManager.getLaunchIntentForPackage(targetContext.packageName))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            ) as MainActivity
            if (checkIconRestart) {
                val original = LauncherIcon.state(activity).getValue("current")!!
                var previous = activity
                for (target in listOf(if (original == "white") "black" else "white", original)) {
                    val oldTask = previous.taskId
                    val observer = object : ActivityMonitor() {
                        override fun onStartActivity(intent: Intent): ActivityResult? {
                            lastLaunch = "${intent.component}, flags=${intent.flags}"
                            return null
                        }
                    }
                    addMonitor(observer)
                    val monitor = addMonitor(MainActivity::class.java.name, null, false)
                    try {
                        runOnMainSync {
                            LauncherIcon.setPending(previous, target)
                            LauncherIcon.restart(previous)
                        }
                        val restarted = checkNotNull(waitForMonitorWithTimeout(monitor, 5000)) as MainActivity
                        check(restarted.taskId != oldTask) { "Restart reused the disabled launcher's task" }
                        SystemClock.sleep(1500)
                        check(!restarted.isFinishing && !restarted.isDestroyed) { "Restarted activity was closed" }
                        check(uiAutomation.rootInActiveWindow?.packageName == targetContext.packageName) {
                            "Restart did not retain the foreground window"
                        }
                        val tasks = targetContext.getSystemService(android.app.ActivityManager::class.java).appTasks
                        check(tasks.size == 1) { "Restart left ${tasks.size} app tasks" }
                        check(tasks.single().taskInfo.baseIntent.component?.className == MainActivity::class.java.name)
                        check(LauncherIcon.state(restarted).getValue("current") == target)
                        previous = restarted
                    } finally {
                        removeMonitor(monitor)
                        removeMonitor(observer)
                    }
                }
                result.putString("stream", "OK: icon restart creates one visible task and retires the disabled alias (2 directions)\n")
                finish(Activity.RESULT_OK, result)
                return
            }
            val view = activity.findViewById<BackspaceFlutterView>(FlutterActivity.FLUTTER_VIEW_ID)
            uiAutomation
            var focusedEditor = false
            repeat(50) {
                if (!focusedEditor) {
                    checkOnMain {
                        val provider = view.accessibilityNodeProvider
                        // ponytail: this small screen fits 256 semantics IDs; use integration tests if it grows.
                        for (id in 0 until 256) {
                            if (provider?.createAccessibilityNodeInfo(id)?.isEditable == true) {
                                focusedEditor = provider.performAction(id, AccessibilityNodeInfo.ACTION_CLICK, null)
                                break
                            }
                        }
                    }
                    if (!focusedEditor) SystemClock.sleep(100)
                }
            }
            check(focusedEditor) { "Flutter text field did not become accessible" }
            var connection: InputConnection? = null
            repeat(50) {
                if (connection == null) {
                    runOnMainSync { connection = view.onCreateInputConnection(EditorInfo()) }
                    if (connection == null) SystemClock.sleep(100)
                }
            }
            val input = checkNotNull(connection) { "Flutter input connection was not created" }
            var forwarded = 0
            val original = view.onEmptyBackspace
            try {
                checkOnMain {
                    view.onEmptyBackspace = { forwarded++ }
                    val text = { input.getExtractedText(ExtractedTextRequest(), 0).text.toString() }
                    input.finishComposingText()
                    input.setSelection(0, text().length)
                    input.commitText("", 1)
                    check(text().isEmpty())
                    input.deleteSurroundingText(1, 0)
                    check(forwarded == 1) { "Empty IME deletion must forward once; got $forwarded" }
                    input.commitText("a", 1)
                    input.deleteSurroundingText(1, 0)
                    check(text().isEmpty() && forwarded == 1)
                    input.deleteSurroundingText(1, 0)
                    check(forwarded == 2)
                    input.commitText("😀", 1)
                    input.deleteSurroundingTextInCodePoints(1, 0)
                    check(text().isEmpty() && forwarded == 2)
                    input.deleteSurroundingTextInCodePoints(1, 0)
                    check(forwarded == 3)
                    input.deleteSurroundingText(0, 0)
                    input.deleteSurroundingText(0, 1)
                    check(forwarded == 3)
                    input.setComposingText("ni", 1)
                    check(text() == "ni")
                    input.deleteSurroundingText(1, 0)
                    check(forwarded == 3)
                    input.commitText("你", 1)
                    check(text() == "你")
                    input.finishComposingText()
                    input.setSelection(0, 1)
                    input.commitText("paste", 1)
                    check(text() == "paste" && forwarded == 3)
                    input.setSelection(0, 5)
                    input.commitText("", 1)
                    val now = SystemClock.uptimeMillis()
                    for (repeat in 0..2) {
                        input.sendKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL, repeat))
                    }
                    input.sendKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_UP, KeyEvent.KEYCODE_DEL, 0))
                    check(forwarded == 6 && text().isEmpty())
                }
            } finally {
                runOnMainSync { view.onEmptyBackspace = original }
            }
            result.putString("stream", "OK: real Flutter InputConnection; empty/last-char/codepoint/no-op/composing/selection/paste/repeat (8 checks)\n")
            finish(Activity.RESULT_OK, result)
        } catch (error: Throwable) {
            result.putString("stream", "FAILURE: ${error.stackTraceToString()}\nLast launch: $lastLaunch\n")
            finish(Activity.RESULT_CANCELED, result)
        }
    }

    private fun checkOnMain(block: () -> Unit) {
        var failure: Throwable? = null
        runOnMainSync {
            try {
                block()
            } catch (error: Throwable) {
                failure = error
            }
        }
        failure?.let { throw it }
    }
}
