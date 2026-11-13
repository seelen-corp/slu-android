package com.seelen.slu_launcher;

import android.app.Notification;
import android.service.notification.NotificationListenerService;
import android.service.notification.StatusBarNotification;
import android.util.Log;

import java.util.HashMap;
import java.util.Map;

/**
 * Notification badges of the apps on Home. Bound by the system once the user
 * allows the launcher's notification access (Settings > Notification access,
 * see system-modules/badges.rs); it runs in the Home process. Counts the
 * notifications of each package and forwards them to Rust when they change.
 */
public class NotificationBadgeService extends NotificationListenerService {
    static {
        System.loadLibrary("slu_launcher");
    }

    private static final String TAG = "slu_launcher";

    /** Implemented in Rust (system-modules/badges.rs): notifications per package. */
    private static native void nativeOnBadgesChanged(String[] packages, int[] counts);

    /** Last counts sent to Rust, so unchanged updates (e.g. progress) are not. */
    private Map<String, Integer> sent;

    @Override
    public void onListenerConnected() {
        update();
    }

    @Override
    public void onListenerDisconnected() {
        send(new HashMap<>());
    }

    @Override
    public void onNotificationPosted(StatusBarNotification sbn, RankingMap rankingMap) {
        update();
    }

    @Override
    public void onNotificationRemoved(StatusBarNotification sbn, RankingMap rankingMap) {
        update();
    }

    @Override
    public void onNotificationRankingUpdate(RankingMap rankingMap) {
        // The user may have turned the badge of a channel or app off.
        update();
    }

    private void update() {
        StatusBarNotification[] active;
        try {
            active = getActiveNotifications();
        } catch (SecurityException e) {
            // Not connected anymore (access revoked).
            Log.w(TAG, "notification listener not connected", e);
            return;
        }
        if (active == null) {
            return;
        }
        RankingMap rankingMap = getCurrentRanking();
        Ranking ranking = new Ranking();
        Map<String, Integer> counts = new HashMap<>();
        for (StatusBarNotification sbn : active) {
            Notification notification = sbn.getNotification();
            // Like the stock launcher: no group summaries (their children are
            // counted), no ongoing ones (music, downloads...), and none the user
            // or the app turned the badge off for.
            if ((notification.flags & Notification.FLAG_GROUP_SUMMARY) != 0 || sbn.isOngoing()) {
                continue;
            }
            if (rankingMap != null && rankingMap.getRanking(sbn.getKey(), ranking)
                    && !ranking.canShowBadge()) {
                continue;
            }
            int count = notification.number > 0 ? notification.number : 1;
            counts.merge(sbn.getPackageName(), count, Integer::sum);
        }
        send(counts);
    }

    private void send(Map<String, Integer> counts) {
        if (counts.equals(sent)) {
            return;
        }
        sent = counts;
        String[] packages = new String[counts.size()];
        int[] values = new int[counts.size()];
        int i = 0;
        for (Map.Entry<String, Integer> entry : counts.entrySet()) {
            packages[i] = entry.getKey();
            values[i] = entry.getValue();
            i++;
        }
        nativeOnBadgesChanged(packages, values);
    }
}
