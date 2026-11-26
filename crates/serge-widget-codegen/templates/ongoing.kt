package {{package}}

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import org.json.JSONObject

object {{name}} {
    /** Returns false when the user has disabled notifications or this channel. */
    @JvmStatic fun publish(context: Context, id: Int, channel: String, channelName: String,
                           smallIcon: Int, json: String, onTap: PendingIntent?): Boolean {
        require(channel.isNotBlank() && channelName.isNotBlank() && smallIcon != 0)
        require(json.toByteArray(Charsets.UTF_8).size <= 65_536)
        val state = JSONObject(json)
        val manager = context.getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= 33 &&
            context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) return false
        if (Build.VERSION.SDK_INT >= 24 && !manager.areNotificationsEnabled()) return false
        @Suppress("DEPRECATION")
        val builder = if (Build.VERSION.SDK_INT >= 26) {
            manager.createNotificationChannel(NotificationChannel(channel, channelName, NotificationManager.IMPORTANCE_LOW))
            if (manager.getNotificationChannel(channel).importance == NotificationManager.IMPORTANCE_NONE) return false
            Notification.Builder(context, channel)
        } else Notification.Builder(context)
        builder.setSmallIcon(smallIcon).setOnlyAlertOnce(true).setOngoing(true)
            .setVisibility(Notification.VISIBILITY_PRIVATE).setContentTitle({{title}})
{{body}}{{progress}}        if (onTap != null) builder.setContentIntent(onTap)
        manager.notify("serge.{{name}}", id, builder.build())
        return true
    }

    @JvmStatic fun cancel(context: Context, id: Int) {
        context.getSystemService(NotificationManager::class.java).cancel("serge.{{name}}", id)
    }
}
