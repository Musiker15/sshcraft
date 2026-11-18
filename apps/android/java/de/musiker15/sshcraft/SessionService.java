package de.musiker15.sshcraft;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;

/**
 * Keeps the app, and with it its open SSH sessions and their network, running while it is in
 * the background: Android stops a backgrounded app's network otherwise. A foreground service
 * must show a notification; it names the open sessions. Rust decides when it runs and what it
 * says ({@link MainActivity#sessions}); this class only hands that to Android.
 */
public class SessionService extends Service {
    /** The extras Rust's text arrives in. */
    static final String TEXT = "text";
    static final String CHANNEL_NAME = "channel";

    /** The channel of the service's notification. */
    static final String SESSIONS = "sessions";

    /** The channel of the app's messages (a long command ended, a server's message). */
    static final String MESSAGES = "messages";

    private static final int ID = 1;

    /** The notifications' small icon: the app's own. */
    static int icon(Context context) {
        int icon = context.getApplicationInfo().icon;
        return icon != 0 ? icon : android.R.drawable.ic_dialog_info;
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        String text = intent == null ? null : intent.getStringExtra(TEXT);
        String channel = intent == null ? null : intent.getStringExtra(CHANNEL_NAME);
        NotificationManager manager = getSystemService(NotificationManager.class);
        manager.createNotificationChannel(new NotificationChannel(SESSIONS, channel == null ? "SSHCraft" : channel, NotificationManager.IMPORTANCE_LOW));
        Intent open = new Intent(this, MainActivity.class).setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP | Intent.FLAG_ACTIVITY_REORDER_TO_FRONT);
        Notification notification = new Notification.Builder(this, SESSIONS)
                .setContentTitle("SSHCraft")
                .setContentText(text == null ? "" : text)
                .setSmallIcon(icon(this))
                .setOngoing(true)
                .setContentIntent(PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_IMMUTABLE))
                .build();
        if (Build.VERSION.SDK_INT >= 34) {
            startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE);
        } else {
            startForeground(ID, notification);
        }
        return START_NOT_STICKY;
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
