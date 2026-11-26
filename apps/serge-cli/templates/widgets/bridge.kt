package {{package}}

import android.content.Context
import androidx.glance.appwidget.GlanceAppWidgetManager
import java.util.concurrent.*
import kotlinx.coroutines.runBlocking

/** Bridge used by the native activity. Only declarations in this application can be selected. */
object SergeWidgetBridge {
    private val workers = ThreadPoolExecutor(2, 2, 15, TimeUnit.SECONDS, ArrayBlockingQueue(32)).apply {
        allowCoreThreadTimeOut(true)
    }
    @JvmStatic fun update(context: Context, kind: String, json: String, id: Int): Future<Int> {
        require(json.toByteArray(Charsets.UTF_8).size <= 65536)
        val app = context.applicationContext
        return workers.submit<Int> {
            runBlocking {
                val manager = GlanceAppWidgetManager(app)
                when (kind) {
{{updates}}                    else -> throw IllegalArgumentException("Unknown home widget: $kind")
                }
            }
        }
    }

    @JvmStatic fun notify(context: Context, kind: String, json: String, id: Int, channel: String, label: String, smallIcon: Int): Boolean =
        when (kind) {
{{notifications}}            else -> throw IllegalArgumentException("Unknown notification: $kind")
        }

    @JvmStatic fun cancel(context: Context, kind: String, id: Int) {
        when (kind) {
{{cancels}}            else -> throw IllegalArgumentException("Unknown notification: $kind")
        }
    }
}
