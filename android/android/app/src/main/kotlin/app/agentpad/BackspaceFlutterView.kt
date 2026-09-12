package app.agentpad

import android.content.Context
import android.text.Selection
import android.view.KeyEvent
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputConnectionWrapper
import io.flutter.embedding.android.FlutterView

@android.annotation.SuppressLint("ViewConstructor")
class BackspaceFlutterView(context: Context, internal var onEmptyBackspace: () -> Unit) :
    FlutterView(context) {
    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection? {
        val connection = super.onCreateInputConnection(outAttrs) ?: return null
        return EmptyBackspaceConnection(connection) { onEmptyBackspace() }
    }
}

internal class EmptyBackspaceConnection(
    private val connection: InputConnection,
    private val onEmptyBackspace: () -> Unit,
) : InputConnectionWrapper(connection, false) {
    private var consumedBackspace = false

    private fun isEmpty(): Boolean {
        val editable = (connection as? BaseInputConnection)?.editable ?: return false
        return editable.isEmpty() &&
            Selection.getSelectionStart(editable) == 0 &&
            Selection.getSelectionEnd(editable) == 0
    }

    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        if (beforeLength > 0 && afterLength == 0 && isEmpty()) {
            onEmptyBackspace()
            return true
        }
        return super.deleteSurroundingText(beforeLength, afterLength)
    }

    override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
        if (beforeLength > 0 && afterLength == 0 && isEmpty()) {
            onEmptyBackspace()
            return true
        }
        return super.deleteSurroundingTextInCodePoints(beforeLength, afterLength)
    }

    override fun sendKeyEvent(event: KeyEvent): Boolean {
        if (event.keyCode == KeyEvent.KEYCODE_DEL) {
            if (event.action == KeyEvent.ACTION_DOWN) {
                consumedBackspace = isEmpty()
                if (consumedBackspace) {
                    onEmptyBackspace()
                    return true
                }
            }
            if (event.action == KeyEvent.ACTION_UP && consumedBackspace) {
                consumedBackspace = false
                return true
            }
        }
        return super.sendKeyEvent(event)
    }
}
