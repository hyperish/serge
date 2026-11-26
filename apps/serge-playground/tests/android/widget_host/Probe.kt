package org.hyperish.widgetprobe

import android.app.*
import android.appwidget.*
import android.content.ComponentName
import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import kotlinx.coroutines.*
import org.hyperish.playground.SergeWidgetBridge
import org.hyperish.playground.ExportWidgetReceiver

class Probe : Activity() {
    private val scope = CoroutineScope(Dispatchers.Main + SupervisorJob())
    private val host by lazy { AppWidgetHost(this, 33) }
    private val manager by lazy { getSystemService(NotificationManager::class.java) }
    private var listening = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        scope.launch {
            try {
                val denied = intent.getBooleanExtra("denied", false)
                val first = """{"project_name":"First export","progress":0.25}"""
                val last = """{"project_name":"Second export","progress":0.75}"""
                check(publish(first) != denied) { "permission result" }
                if (!denied) {
                    expectNotification("First export", 25)
                    check(publish(last))
                    expectNotification("Second export", 75)
                    SergeWidgetBridge.cancel(this@Probe, "ExportProgress", 41)
                    withTimeout(5000) { while (manager.activeNotifications.isNotEmpty()) delay(50) }
                    widget(first, last)
                }
                result(if (denied) "denied" else "updated")
            } catch (e: Exception) { result("failure: " + e.stackTraceToString()) }
        }
    }

    private fun publish(json: String) = SergeWidgetBridge.notify(this, "ExportProgress", json, 41, "probe", "Exports", android.R.drawable.ic_dialog_info)

    private suspend fun expectNotification(title: String, progress: Int) = withTimeout(5000) {
        while (true) {
            val n = manager.activeNotifications.singleOrNull()?.notification
            if (n != null && n.extras.getCharSequence(Notification.EXTRA_TITLE)?.toString() == title &&
                n.extras.getInt(Notification.EXTRA_PROGRESS) == progress &&
                n.flags and Notification.FLAG_ONGOING_EVENT != 0) break
            delay(50)
        }
    }

    private suspend fun widget(first: String, last: String) {
        val widgets = AppWidgetManager.getInstance(this)
        val id = host.allocateAppWidgetId()
        try {
            check(widgets.bindAppWidgetIdIfAllowed(id, ComponentName(this, ExportWidgetReceiver::class.java)))
            host.startListening()
            listening = true
            val view = host.createView(this, id, widgets.getAppWidgetInfo(id))
            setContentView(view)
            for ((json, caption) in listOf(first to "First export", last to "Second export")) {
                val update = SergeWidgetBridge.update(this, "ExportWidget", json, id)
                check(withContext(Dispatchers.IO) { update.get(20, java.util.concurrent.TimeUnit.SECONDS) } == 1)
                withTimeout(20000) { while (!containsText(view, caption)) delay(100) }
            }
        } finally { host.deleteAppWidgetId(id) }
    }

    private fun containsText(view: View, text: String): Boolean =
        (view is TextView && view.text.contains(text)) ||
        (view is ViewGroup && (0 until view.childCount).any { containsText(view.getChildAt(it), text) })

    private fun result(text: String) {
        openFileOutput("result", MODE_PRIVATE).bufferedWriter().use { it.write(text) }
        finish()
    }
    override fun onDestroy() {
        scope.cancel()
        if (listening) host.stopListening()
        super.onDestroy()
    }
}
