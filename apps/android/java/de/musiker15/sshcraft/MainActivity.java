package de.musiker15.sshcraft;

import android.Manifest;
import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.database.Cursor;
import android.provider.OpenableColumns;
import java.util.ArrayDeque;
import java.util.HashMap;
import java.util.Map;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
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

    /** Picked files as "id<TAB>path<TAB>name" ("id<TAB>": cancelled), until Rust takes them. */
    private static final ConcurrentLinkedQueue<String> PICKED = new ConcurrentLinkedQueue<>();

    /** The open requests: the request code Android hands back, Rust's id and whether to write. */
    private final Map<Integer, long[]> picking = new HashMap<>();

    /** At most this many picked files are kept in the cache; the oldest goes first. */
    private static final int KEEP_OPEN = 8;

    private int nextCode = 1000;

    /**
     * Opens the system's file picker for Rust's request {@code id}: a document to read, or one to
     * create and write ({@code save}, named {@code name}). The answer comes through
     * {@link #takePicked}.
     */
    public void pick(final long id, final boolean save, final String name, final String mime) {
        runOnUiThread(() -> {
            Intent intent = new Intent(save ? Intent.ACTION_CREATE_DOCUMENT : Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType(mime == null || mime.isEmpty() ? "*/*" : mime);
            if (save && name != null && !name.isEmpty()) {
                intent.putExtra(Intent.EXTRA_TITLE, name);
            }
            int code = nextCode++;
            picking.put(code, new long[] {id, save ? 1 : 0});
            try {
                startActivityForResult(intent, code);
            } catch (ActivityNotFoundException e) {
                picking.remove(code);
                PICKED.add(id + "\t");
            }
        });
    }

    @Override
    protected void onActivityResult(int code, int result, Intent data) {
        super.onActivityResult(code, result, data);
        long[] request = picking.remove(code);
        if (request == null) {
            return;
        }
        String path = "";
        String name = "";
        if (result == RESULT_OK && data != null && data.getData() != null) {
            // The document's name as the picker shows it; the path below is only a number.
            // Rust checks it like any name from outside.
            try (Cursor cursor = getContentResolver().query(data.getData(), new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
                if (cursor != null && cursor.moveToFirst() && !cursor.isNull(0)) {
                    name = cursor.getString(0);
                }
            } catch (Exception e) {
                name = "";
            }
            if (request[1] == 0) {
                // A file to read: Android lets the app open the document, but not open it again
                // by its descriptor's path when it belongs to another app. A copy in the app's
                // cache is a file Rust can read; it is made off the main thread.
                final long id = request[0];
                final String shown = name;
                final android.net.Uri uri = data.getData();
                new Thread(() -> PICKED.add(id + "\t" + copyToCache(code, uri) + "\t" + shown)).start();
                return;
            }
            // A file to write: the app may not open the created document again by its
            // descriptor's path either. Rust writes a file in the cache; each time it closes it,
            // the file goes into the document (see SaveWatch).
            path = saveWatch().add(code, data.getData());
        }
        PICKED.add(request[0] + "\t" + path + (path.isEmpty() ? "" : "\t" + name));
    }

    private SaveWatch saves;

    private synchronized SaveWatch saveWatch() {
        if (saves == null) {
            saves = new SaveWatch(new java.io.File(getCacheDir(), "saving"));
            saves.startWatching();
        }
        return saves;
    }

    /**
     * Files Rust writes for created documents: when one is closed after writing, its content
     * goes into its document. At most {@link #KEEP_OPEN} stay; the oldest goes first.
     */
    private final class SaveWatch extends android.os.FileObserver {
        private final java.io.File dir;
        private final Map<String, android.net.Uri> targets = new java.util.concurrent.ConcurrentHashMap<>();
        private final ArrayDeque<String> order = new ArrayDeque<>();

        SaveWatch(java.io.File dir) {
            super(dir, CLOSE_WRITE);
            this.dir = dir;
            dir.mkdirs();
        }

        /** A file for the document {@code uri}; its path, or "" when it could not be made. */
        synchronized String add(int code, android.net.Uri uri) {
            java.io.File file = new java.io.File(dir, Integer.toString(code));
            try {
                if (!dir.isDirectory() || (!file.exists() && !file.createNewFile())) {
                    return "";
                }
            } catch (java.io.IOException e) {
                return "";
            }
            targets.put(file.getName(), uri);
            order.addLast(file.getName());
            while (order.size() > KEEP_OPEN) {
                String old = order.removeFirst();
                targets.remove(old);
                new java.io.File(dir, old).delete();
            }
            return file.getAbsolutePath();
        }

        @Override
        public void onEvent(int event, String name) {
            android.net.Uri uri = name == null ? null : targets.get(name);
            if (uri == null) {
                return;
            }
            try (java.io.InputStream in = new java.io.FileInputStream(new java.io.File(dir, name));
                    java.io.OutputStream out = getContentResolver().openOutputStream(uri, "wt")) {
                if (out != null) {
                    in.transferTo(out);
                }
            } catch (Exception e) {
                // The document went away or cannot be written; Rust's file stays until it is old.
            }
        }
    }

    /** Copies made of picked files, oldest first; at most {@link #KEEP_OPEN} stay. */
    private final ArrayDeque<java.io.File> copies = new ArrayDeque<>();

    /** Copies the document {@code uri} into the app's cache; its path, or "" when it failed. */
    private String copyToCache(int code, android.net.Uri uri) {
        java.io.File dir = new java.io.File(getCacheDir(), "picked");
        java.io.File copy = new java.io.File(dir, Integer.toString(code));
        try (java.io.InputStream in = getContentResolver().openInputStream(uri)) {
            if (in == null || (!dir.isDirectory() && !dir.mkdirs())) {
                return "";
            }
            java.nio.file.Files.copy(in, copy.toPath(), java.nio.file.StandardCopyOption.REPLACE_EXISTING);
        } catch (Exception e) {
            copy.delete();
            return "";
        }
        synchronized (copies) {
            copies.addLast(copy);
            while (copies.size() > KEEP_OPEN) {
                copies.removeFirst().delete();
            }
        }
        return copy.getAbsolutePath();
    }

    /** The oldest answer of the file picker as "id<TAB>path<TAB>name", or null when there is none. */
    public static String takePicked() {
        return PICKED.poll();
    }

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
