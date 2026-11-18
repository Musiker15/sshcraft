package de.musiker15.sshcraft;

import android.Manifest;
import android.app.NativeActivity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.Context;
import android.os.Bundle;
import android.graphics.Insets;
import android.text.InputType;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;
import java.util.concurrent.ConcurrentLinkedQueue;

/**
 * SSHCraft's activity: the NativeActivity the Rust app runs in, plus a text connection to the
 * on-screen keyboard. NativeActivity's own view has none, so a keyboard can only send it single
 * key presses, and text it commits at once (a paste from its clipboard, a swiped word, an
 * umlaut) is lost. Here the keyboard writes into {@link InputView}; Rust takes the text with
 * {@link #takeText} every frame (and the word the keyboard is still composing, so it shows) and shows or hides the keyboard with {@link #keyboard}. Built
 * into classes.dex by {@code cargo xtask android}.
 */
public class MainActivity extends NativeActivity {
    /**
     * What the keyboard sent, in order, until Rust takes it: "C" and text it committed, or "P"
     * and the word it is composing (empty: the word went away). Rust reads it in
     * {@code Composer} (src/lib.rs).
     */
    private static final ConcurrentLinkedQueue<String> COMMITTED = new ConcurrentLinkedQueue<>();

    /** At most this many characters wait; a keyboard cannot flood the app. */
    private static final int MAX_WAITING = 1 << 20;

    private InputView input;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        input = new InputView(this);
        addContentView(input, new ViewGroup.LayoutParams(1, 1));
        // NativeActivity's view would take the keyboard back on a tap, without a text connection.
        View root = findViewById(android.R.id.content);
        if (root instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) root;
            for (int i = 0; i < group.getChildCount(); i++) {
                View child = group.getChildAt(i);
                if (child != input) {
                    child.setFocusable(false);
                    child.setFocusableInTouchMode(false);
                }
            }
        }
        // NativeActivity sets its own keyboard mode in onCreate, which opens the keyboard for a
        // focused editor at start. It opens when the app asks for it (keyboard()), not before.
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_STATE_HIDDEN | WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        // Notifications (open sessions, messages) need the user's yes since Android 13.
        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[] {Manifest.permission.POST_NOTIFICATIONS}, 1);
        }
    }

    @Override
    protected void onDestroy() {
        // The app ends: nothing is left to keep open.
        stopService(new Intent(this, SessionService.class));
        super.onDestroy();
    }

    /**
     * Starts, updates or stops the service that keeps sessions open in the background: Rust
     * calls this when the number of open sessions changes, with the notification's text and the
     * channel's name in the app's language. Zero stops it.
     */
    public void sessions(int count, String text, String channel) {
        Intent intent = new Intent(this, SessionService.class);
        if (count <= 0) {
            stopService(intent);
            return;
        }
        intent.putExtra(SessionService.TEXT, text);
        intent.putExtra(SessionService.CHANNEL_NAME, channel);
        startForegroundService(intent);
    }

    /** Shows one of the app's messages as a notification; Rust says what and in which channel. */
    public void notify(String title, String body, String channel, int id) {
        NotificationManager manager = getSystemService(NotificationManager.class);
        manager.createNotificationChannel(new NotificationChannel(SessionService.MESSAGES, channel, NotificationManager.IMPORTANCE_DEFAULT));
        Notification notification = new Notification.Builder(this, SessionService.MESSAGES)
                .setContentTitle(title)
                .setContentText(body)
                .setStyle(new Notification.BigTextStyle().bigText(body))
                .setSmallIcon(SessionService.icon(this))
                .setAutoCancel(true)
                .build();
        manager.notify(id, notification);
    }

    private static long pack(int pixels) {
        return Math.max(0, Math.min(pixels, 0xFFFF));
    }

    /**
     * What the system covers of the window now, in pixels (keyboard, status and navigation bars,
     * camera cutout), packed as four 16-bit values: left, top, right, bottom. Android 15 draws
     * apps edge to edge and no longer shrinks the window for the keyboard, so the app keeps
     * these parts free itself. Read from the window each time, so no callback can be missed.
     */
    public long covered() {
        View decor = getWindow().getDecorView();
        WindowInsets insets = decor == null ? null : decor.getRootWindowInsets();
        if (insets == null) {
            return 0;
        }
        Insets used = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.ime() | WindowInsets.Type.displayCutout());
        return pack(used.left) | pack(used.top) << 16 | pack(used.right) << 32 | pack(used.bottom) << 48;
    }

    /** The oldest entry of {@link #COMMITTED}, or null when there is none. */
    public static String takeText() {
        return COMMITTED.poll();
    }

    /** Shows the on-screen keyboard for the input view, or hides it. */
    public void keyboard(final boolean show) {
        runOnUiThread(() -> {
            InputMethodManager manager = getSystemService(InputMethodManager.class);
            if (manager == null || input == null) {
                return;
            }
            if (show) {
                input.requestFocus();
                // On the next turn of the loop: the keyboard binds to a newly focused view only
                // after the focus change went through.
                input.post(() -> {
                    manager.restartInput(input);
                    manager.showSoftInput(input, 0);
                    // The way for an app drawn edge to edge (Android 11 and later).
                    if (getWindow().getInsetsController() != null) {
                        getWindow().getInsetsController().show(WindowInsets.Type.ime());
                    }
                });
            } else {
                manager.hideSoftInputFromWindow(input.getWindowToken(), 0);
            }
        });
    }

    static void commit(CharSequence text) {
        if (text == null || text.length() == 0) {
            return;
        }
        send("C" + text);
    }

    /** The word being composed, shown where it will go; an empty one takes it away. */
    static void composing(CharSequence text) {
        send("P" + (text == null ? "" : text));
    }

    private static void send(String entry) {
        int waiting = 0;
        for (String s : COMMITTED) {
            waiting += s.length();
        }
        if (waiting + entry.length() <= MAX_WAITING) {
            COMMITTED.offer(entry);
        }
    }

    /** A view of one pixel that only holds the keyboard's connection. */
    static final class InputView extends View {
        InputView(Context context) {
            super(context);
            setFocusable(true);
            setFocusableInTouchMode(true);
        }

        @Override
        public boolean onCheckIsTextEditor() {
            return true;
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo info) {
            // Visible password: no suggestions and no auto-correct, as a terminal needs, and the
            // keyboard still offers its clipboard.
            info.inputType = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
            info.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN | EditorInfo.IME_FLAG_NO_EXTRACT_UI | EditorInfo.IME_FLAG_NO_ENTER_ACTION;
            return new Connection(this);
        }
    }

    /** Commits go to Rust; deleting and Enter become the keys a terminal knows. */
    static final class Connection extends BaseInputConnection {
        private String composing = "";

        Connection(View view) {
            super(view, true);
        }

        @Override
        public boolean commitText(CharSequence text, int newCursorPosition) {
            boolean was = !composing.isEmpty();
            composing = "";
            commit(text);
            if (was && (text == null || text.length() == 0)) {
                composing("");
            }
            getEditable().clear();
            return true;
        }

        @Override
        public boolean setComposingText(CharSequence text, int newCursorPosition) {
            String now = text == null ? "" : text.toString();
            if (!now.equals(composing)) {
                composing = now;
                composing(now);
            }
            return true;
        }

        @Override
        public boolean finishComposingText() {
            if (!composing.isEmpty()) {
                commit(composing);
            }
            composing = "";
            getEditable().clear();
            return true;
        }

        @Override
        public boolean deleteSurroundingText(int beforeLength, int afterLength) {
            for (int i = 0; i < Math.min(beforeLength, 256); i++) {
                key(KeyEvent.KEYCODE_DEL);
            }
            return true;
        }

        @Override
        public boolean performEditorAction(int actionCode) {
            key(KeyEvent.KEYCODE_ENTER);
            return true;
        }

        private void key(int code) {
            sendKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, code));
            sendKeyEvent(new KeyEvent(KeyEvent.ACTION_UP, code));
        }
    }
}
