package de.musiker15.sshcraft;

import android.app.NativeActivity;
import android.content.Context;
import android.os.Bundle;
import android.text.InputType;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
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
 * {@link #takeText} every frame and shows or hides the keyboard with {@link #keyboard}. Built
 * into classes.dex by {@code cargo xtask android}.
 */
public class MainActivity extends NativeActivity {
    /** Text the keyboard committed, one entry per commit, until Rust takes it. */
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
        input.requestFocus();
    }

    /** The oldest text the keyboard committed, or null when there is none. */
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
                manager.showSoftInput(input, 0);
            } else {
                manager.hideSoftInputFromWindow(input.getWindowToken(), 0);
            }
        });
    }

    static void commit(CharSequence text) {
        if (text == null || text.length() == 0) {
            return;
        }
        int waiting = 0;
        for (String s : COMMITTED) {
            waiting += s.length();
        }
        if (waiting + text.length() <= MAX_WAITING) {
            COMMITTED.offer(text.toString());
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
            composing = "";
            commit(text);
            getEditable().clear();
            return true;
        }

        @Override
        public boolean setComposingText(CharSequence text, int newCursorPosition) {
            composing = text == null ? "" : text.toString();
            return true;
        }

        @Override
        public boolean finishComposingText() {
            commit(composing);
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
