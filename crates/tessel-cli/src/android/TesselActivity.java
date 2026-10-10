// The activity a Tessel app runs in on Android.
//
// The app itself is native code, which Android's NativeActivity loads and
// draws for. This adds what only Java can reach: the on-screen keyboard as
// a text editor would use it (words being composed, corrections, emoji),
// the system's dialogs and clipboard, and word of the system's bars and
// keyboard moving. `tessel build --android` compiles it into the package.
package dev.tessel;

import android.app.AlertDialog;
import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.DialogInterface;
import android.os.Bundle;
import android.text.InputType;
import android.view.KeyEvent;
import android.view.SurfaceHolder;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;
import java.util.concurrent.CountDownLatch;

public class TesselActivity extends NativeActivity {
    // The app's code. (NativeActivity loads it too, but not in the way
    // that lets Java find the functions declared `native` below.)
    static {
        System.loadLibrary("main");
    }

    // Text from the keyboard: typed (kind 0), or the word being composed,
    // which replaces the one before (kind 1).
    private static native void onText(String text, int kind);
    // The keyboard takes back characters before and after the caret.
    private static native void onDelete(int before, int after);
    // The system's bars or the keyboard moved.
    private static native void onInsets();

    private InputView input;
    private AlertDialog dialog;
    private Answer answer;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        // The keyboard types into a view; the app draws its own text
        // fields, so this one stands in for whichever is being edited.
        input = new InputView(this);
        addContentView(input, new ViewGroup.LayoutParams(1, 1));
        getWindow().getDecorView().setOnApplyWindowInsetsListener(new View.OnApplyWindowInsetsListener() {
            @Override
            public WindowInsets onApplyWindowInsets(View view, WindowInsets insets) {
                onInsets();
                return view.onApplyWindowInsets(insets);
            }
        });
    }

    // The app waits for a dialog's answer, and the system waits for the app
    // when it takes the screen away: a dialog still open then is cancelled,
    // or the two would wait for each other.
    @Override
    protected void onPause() {
        closeDialog();
        super.onPause();
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        closeDialog();
        super.surfaceDestroyed(holder);
    }

    private void closeDialog() {
        AlertDialog open = dialog;
        Answer waiting = answer;
        dialog = null;
        answer = null;
        if (open != null) {
            open.dismiss();
        }
        if (waiting != null) {
            waiting.give(false);
        }
    }

    // ----- called by the app (from its own thread) -----

    /** Shows the keyboard for a text field (kind 0), a secret one (1) or code (2), or hides it. */
    public void keyboard(final boolean show, final int kind) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                InputMethodManager manager = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
                if (show) {
                    input.kind = kind;
                    input.composing = "";
                    input.requestFocus();
                    manager.restartInput(input);
                    manager.showSoftInput(input, 0);
                } else {
                    manager.hideSoftInputFromWindow(input.getWindowToken(), 0);
                }
            }
        });
    }

    /** A message (cancel is null) or a question: waits for the answer. */
    public boolean ask(final String title, final String message, final String ok, final String cancel) {
        final Answer mine = new Answer();
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                closeDialog();
                AlertDialog.Builder builder = new AlertDialog.Builder(TesselActivity.this);
                builder.setTitle(title);
                if (!message.isEmpty()) {
                    builder.setMessage(message);
                }
                builder.setPositiveButton(ok, new DialogInterface.OnClickListener() {
                    @Override
                    public void onClick(DialogInterface d, int which) {
                        mine.give(true);
                    }
                });
                if (cancel != null) {
                    builder.setNegativeButton(cancel, new DialogInterface.OnClickListener() {
                        @Override
                        public void onClick(DialogInterface d, int which) {
                            mine.give(false);
                        }
                    });
                }
                // (Back, or a touch outside: for a message, as good as its button.)
                builder.setOnCancelListener(new DialogInterface.OnCancelListener() {
                    @Override
                    public void onCancel(DialogInterface d) {
                        mine.give(cancel == null);
                    }
                });
                answer = mine;
                dialog = builder.create();
                dialog.show();
            }
        });
        return mine.await();
    }

    public void copy(String text) {
        ClipboardManager clipboard = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        clipboard.setPrimaryClip(ClipData.newPlainText("", text));
    }

    /** The clipboard's text, or null. */
    public String pasted() {
        ClipboardManager clipboard = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        ClipData clip = clipboard.getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) {
            return null;
        }
        CharSequence text = clip.getItemAt(0).coerceToText(this);
        return text == null ? null : text.toString();
    }

    /** A dialog's answer, which the app's thread waits for. */
    private static final class Answer {
        private final CountDownLatch given = new CountDownLatch(1);
        private volatile boolean yes;

        void give(boolean answer) {
            if (given.getCount() > 0) {
                yes = answer;
                given.countDown();
            }
        }

        boolean await() {
            try {
                given.await();
            } catch (InterruptedException e) {
                return false;
            }
            return yes;
        }
    }

    /** What the keyboard types into. */
    private static final class InputView extends View {
        int kind;
        /** The word being composed, shown but not typed yet. */
        String composing = "";

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
            info.inputType = InputType.TYPE_CLASS_TEXT;
            if (kind == 1) {
                info.inputType |= InputType.TYPE_TEXT_VARIATION_PASSWORD;
            } else if (kind == 2) {
                // Code: no corrections, no suggestions.
                info.inputType |= InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
            }
            // (The keyboard never covers the app with a field of its own.)
            info.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI | EditorInfo.IME_FLAG_NO_FULLSCREEN;
            return new BaseInputConnection(this, false) {
                @Override
                public boolean commitText(CharSequence text, int newCursorPosition) {
                    composing = "";
                    String typed = text.toString();
                    if (typed.equals("\n")) {
                        // (Enter is a key to the app, whatever it does there.)
                        sendKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER));
                        sendKeyEvent(new KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_ENTER));
                    } else {
                        onText(typed, 0);
                    }
                    return true;
                }

                @Override
                public boolean setComposingText(CharSequence text, int newCursorPosition) {
                    composing = text.toString();
                    onText(composing, 1);
                    return true;
                }

                @Override
                public boolean finishComposingText() {
                    if (!composing.isEmpty()) {
                        String typed = composing;
                        composing = "";
                        onText(typed, 0);
                    }
                    return true;
                }

                @Override
                public boolean deleteSurroundingText(int before, int after) {
                    onDelete(before, after);
                    return true;
                }

                @Override
                public boolean deleteSurroundingTextInCodePoints(int before, int after) {
                    onDelete(before, after);
                    return true;
                }

                // (The app's text isn't the keyboard's to mark up.)
                @Override
                public boolean setComposingRegion(int start, int end) {
                    return true;
                }
            };
        }
    }
}
