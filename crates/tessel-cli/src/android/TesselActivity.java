// The activity a Tessel app runs in on Android.
//
// The app itself is native code, which Android's NativeActivity loads and
// draws for. This adds what only Java can reach: the on-screen keyboard as
// a text editor would use it (words being composed, corrections, emoji),
// the system's dialogs, file chooser and clipboard, and word of the
// system's bars and keyboard moving. `tessel build --android` compiles it into the package.
package dev.tessel;

import android.app.AlertDialog;
import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.DialogInterface;
import android.content.Intent;
import android.content.SharedPreferences;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.text.Editable;
import android.text.InputType;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.view.KeyEvent;
import android.view.SurfaceHolder;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;
import android.webkit.MimeTypeMap;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
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
    // The keyboard changed the text it was given (see `InputView`): what was
    // from `start` to `end` is now `text`, and the selection is from
    // `selectStart` to `selectEnd` (all counted in Java's characters).
    // `field` and `serial` say which field, and which change of it.
    private static native void onEdit(int field, int serial, int start, int end, String text, int selectStart, int selectEnd);
    // The system's bars or the keyboard moved.
    private static native void onInsets();
    // The files chosen in the system's chooser (none if it was cancelled).
    private static native void onChosen(String[] paths);

    /** The request the chooser is started with. */
    private static final int CHOOSE = 7001;
    /** Where the app's copies of chosen documents are, in its cache. */
    private static final String DOCUMENTS = "documents";

    private InputView input;
    private AlertDialog dialog;
    private Answer answer;
    /** What the open chooser is for (see `choose`), or -1. */
    private int choosing = -1;
    private String[] choosingTypes = new String[0];

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

    @Override
    protected void onDestroy() {
        if (choosing >= 0) {
            choosing = -1;
            onChosen(new String[0]);
        }
        super.onDestroy();
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

    /**
     * Shows the keyboard for a text field (kind 0), a secret one (1) or code
     * (2), or hides it. `text` is what's in the field, with its selection,
     * for the keyboard to read and correct; null if it's only to type.
     * `field` numbers the field, for `onEdit`.
     */
    public void keyboard(final boolean show, final int kind, final int field, final String text, final int selectStart, final int selectEnd) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                InputMethodManager manager = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
                if (show) {
                    input.kind = kind;
                    input.composing = "";
                    input.field = field;
                    input.serial = 0;
                    input.give(text, selectStart, selectEnd);
                    input.requestFocus();
                    manager.restartInput(input);
                    manager.showSoftInput(input, 0);
                } else {
                    input.give(null, 0, 0);
                    manager.hideSoftInputFromWindow(input.getWindowToken(), 0);
                }
            }
        });
    }

    /**
     * What's in the field now, as the app has it, after the keyboard's
     * change `serial`. If that isn't what the keyboard has (the app changed
     * it, or the caret was moved), the keyboard starts over from it.
     */
    public void text(final String text, final int selectStart, final int selectEnd, final int serial) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                // (More of the keyboard's changes are on their way to the app.)
                if (input.text == null || serial < input.serial) {
                    return;
                }
                InputMethodManager manager = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
                int start = Math.max(0, Math.min(selectStart, text.length()));
                int end = Math.max(0, Math.min(selectEnd, text.length()));
                if (!text.equals(input.told)) {
                    input.give(text, start, end);
                    manager.restartInput(input);
                } else if (start != input.toldStart || end != input.toldEnd) {
                    BaseInputConnection.removeComposingSpans(input.text);
                    Selection.setSelection(input.text, start, end);
                    input.toldStart = start;
                    input.toldEnd = end;
                    manager.updateSelection(input, start, end, -1, -1);
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

    // ----- choosing files -----
    //
    // Android's files aren't paths: the system's chooser gives a document,
    // which only the system can read and write. The app works with paths,
    // so each chosen document gets a copy in the app's cache, and the app
    // is given the copy's path. What the app writes there is written to
    // the document too (`store`).

    /**
     * Opens the system's chooser: for a file to open (kind 0), several (1),
     * or a place to save one as `name` (2). The answer comes to `onChosen`.
     * `types` are extensions without the dot.
     */
    public void choose(final int kind, final String name, final String[] types) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    Intent intent;
                    if (kind == 2) {
                        String named = withType(name.isEmpty() ? "Untitled" : name, types);
                        intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
                        intent.putExtra(Intent.EXTRA_TITLE, named);
                        String mime = mimeOf(named);
                        intent.setType(mime == null ? "application/octet-stream" : mime);
                    } else {
                        intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                        intent.setType("*/*");
                        // Only the types asked for, when the system knows
                        // them all (it can't filter by an extension of the
                        // app's own).
                        String[] mimes = new String[types.length];
                        boolean known = types.length > 0;
                        for (int i = 0; i < types.length; i++) {
                            mimes[i] = mimeOf("." + types[i]);
                            known &= mimes[i] != null;
                        }
                        if (known) {
                            intent.putExtra(Intent.EXTRA_MIME_TYPES, mimes);
                        }
                        intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, kind == 1);
                    }
                    intent.addCategory(Intent.CATEGORY_OPENABLE);
                    intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                            | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
                    choosing = kind;
                    choosingTypes = types;
                    startActivityForResult(intent, CHOOSE);
                } catch (RuntimeException e) {
                    // (No chooser on this device.)
                    choosing = -1;
                    onChosen(new String[0]);
                }
            }
        });
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != CHOOSE || choosing < 0) {
            return;
        }
        final boolean saving = choosing == 2;
        final String[] types = choosingTypes;
        choosing = -1;
        final ArrayList<Uri> documents = new ArrayList<Uri>();
        if (result == RESULT_OK && data != null) {
            ClipData several = data.getClipData();
            if (several != null) {
                for (int i = 0; i < several.getItemCount(); i++) {
                    documents.add(several.getItemAt(i).getUri());
                }
            } else if (data.getData() != null) {
                documents.add(data.getData());
            }
        }
        if (documents.isEmpty()) {
            onChosen(new String[0]);
            return;
        }
        // (Copying a large file takes a while, and this thread is the one
        // the system talks to.)
        new Thread(new Runnable() {
            @Override
            public void run() {
                ArrayList<String> paths = new ArrayList<String>();
                for (Uri document : documents) {
                    String path = local(document, saving, types);
                    if (path != null) {
                        paths.add(path);
                    }
                }
                onChosen(paths.toArray(new String[0]));
            }
        }).start();
    }

    /** The path of the app's copy of a document, made anew; null if it can't be read. */
    private String local(Uri document, boolean saving, String[] types) {
        // The document stays the app's to read and write, in later runs too.
        try {
            getContentResolver().takePersistableUriPermission(document,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        } catch (RuntimeException e) {
            try {
                getContentResolver().takePersistableUriPermission(document, Intent.FLAG_GRANT_READ_URI_PERMISSION);
            } catch (RuntimeException readOnly) {
                // (Not every source of documents lets them be kept.)
            }
        }
        String name = nameOf(document);
        if (saving) {
            name = withType(name, types);
        }
        // (The same document gets the same path each time it's chosen.)
        File folder = new File(new File(getCacheDir(), DOCUMENTS), Integer.toHexString(document.toString().hashCode()));
        folder.mkdirs();
        File copy = new File(folder, name);
        if (saving) {
            copy.delete();
        } else {
            try (InputStream in = getContentResolver().openInputStream(document); OutputStream out = new FileOutputStream(copy)) {
                if (in == null) {
                    return null;
                }
                pour(in, out);
            } catch (IOException | RuntimeException e) {
                return null;
            }
        }
        getSharedPreferences(DOCUMENTS, MODE_PRIVATE).edit().putString(copy.getPath(), document.toString()).apply();
        return copy.getPath();
    }

    /** The app wrote the file at `path`: if it's a document's copy, the document gets what's in it. */
    public boolean store(String path) {
        SharedPreferences known = getSharedPreferences(DOCUMENTS, MODE_PRIVATE);
        String document = known.getString(path, null);
        if (document == null) {
            return false;
        }
        try (InputStream in = new FileInputStream(path);
                OutputStream out = getContentResolver().openOutputStream(Uri.parse(document), "wt")) {
            if (out == null) {
                return false;
            }
            pour(in, out);
            return true;
        } catch (IOException | RuntimeException e) {
            return false;
        }
    }

    private static void pour(InputStream in, OutputStream out) throws IOException {
        byte[] some = new byte[65536];
        for (int n = in.read(some); n >= 0; n = in.read(some)) {
            out.write(some, 0, n);
        }
    }

    /** A document's name, as the chooser shows it. */
    private String nameOf(Uri document) {
        String name = null;
        try (Cursor found = getContentResolver().query(document, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (found != null && found.moveToFirst()) {
                name = found.getString(0);
            }
        } catch (RuntimeException e) {
            // (The name below will do.)
        }
        if (name == null || name.isEmpty()) {
            name = document.getLastPathSegment();
        }
        if (name == null || name.isEmpty()) {
            name = "file";
        }
        return name.replace('/', '_').replace(':', '_');
    }

    /** `name`, ending in one of the types: the first, if it ends in none. */
    private static String withType(String name, String[] types) {
        String lower = name.toLowerCase();
        for (String type : types) {
            if (lower.endsWith("." + type.toLowerCase())) {
                return name;
            }
        }
        return types.length == 0 ? name : name + "." + types[0];
    }

    /** What the system calls the kind of file `name` is, by its extension; null if it doesn't know. */
    private static String mimeOf(String name) {
        int dot = name.lastIndexOf('.');
        if (dot < 0) {
            return null;
        }
        return MimeTypeMap.getSingleton().getMimeTypeFromExtension(name.substring(dot + 1).toLowerCase());
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

    /**
     * What the keyboard types into.
     *
     * The app draws and keeps its own text. For the keyboard to see the
     * words around the caret, and correct them, this holds a copy of the
     * field's text, which the keyboard edits as it would any text field's;
     * each change is passed on to the app (`onEdit`), and the app says what
     * became of its text (`text`). Code is only typed, with no copy.
     */
    private static final class InputView extends View {
        int kind;
        /** The word being composed, shown but not typed yet (when there's no copy). */
        String composing = "";
        /** The copy of the field's text; null when the keyboard only types. */
        Editable text;
        /** The text and selection as the app was last told, or told of. */
        String told = "";
        int toldStart;
        int toldEnd;
        /** Which field of the app's, and how many changes of it were passed on. */
        int field;
        int serial;

        InputView(Context context) {
            super(context);
            setFocusable(true);
            setFocusableInTouchMode(true);
        }

        /** Starts over from the app's text (null: no copy). */
        void give(String from, int selectStart, int selectEnd) {
            if (from == null) {
                text = null;
                told = "";
                return;
            }
            text = new SpannableStringBuilder(from);
            toldStart = Math.max(0, Math.min(selectStart, from.length()));
            toldEnd = Math.max(0, Math.min(selectEnd, from.length()));
            Selection.setSelection(text, toldStart, toldEnd);
            told = from;
        }

        /** The keyboard did something to the copy: the app is told what changed. */
        void changed() {
            if (text == null) {
                return;
            }
            String now = text.toString();
            int selectStart = Math.max(0, Selection.getSelectionStart(text));
            int selectEnd = Math.max(0, Selection.getSelectionEnd(text));
            if (!now.equals(told)) {
                // What's the same at both ends didn't change. (A pair of
                // characters that make one, like an emoji's, isn't split.)
                int start = 0;
                int most = Math.min(now.length(), told.length());
                while (start < most && now.charAt(start) == told.charAt(start)) {
                    start++;
                }
                if (start > 0 && Character.isHighSurrogate(now.charAt(start - 1))) {
                    start--;
                }
                int same = 0;
                while (same < most - start && now.charAt(now.length() - 1 - same) == told.charAt(told.length() - 1 - same)) {
                    same++;
                }
                if (same > 0 && Character.isLowSurrogate(now.charAt(now.length() - same))) {
                    same--;
                }
                serial++;
                onEdit(field, serial, start, told.length() - same, now.substring(start, now.length() - same), selectStart, selectEnd);
            } else if (selectStart != toldStart || selectEnd != toldEnd) {
                serial++;
                onEdit(field, serial, 0, 0, "", selectStart, selectEnd);
            } else {
                return;
            }
            told = now;
            toldStart = selectStart;
            toldEnd = selectEnd;
            InputMethodManager manager = (InputMethodManager) getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
            manager.updateSelection(this, selectStart, selectEnd, BaseInputConnection.getComposingSpanStart(text),
                    BaseInputConnection.getComposingSpanEnd(text));
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
            if (text != null) {
                info.initialSelStart = toldStart;
                info.initialSelEnd = toldEnd;
                return new Editor(this);
            }
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

    /** The keyboard's hold on the copy of a field's text: it edits the copy, and the app hears of it. */
    private static final class Editor extends BaseInputConnection {
        private final InputView view;
        /** How many sets of changes are open: the app hears of a set as one. */
        private int open;

        Editor(InputView view) {
            super(view, true);
            this.view = view;
        }

        @Override
        public Editable getEditable() {
            return view.text;
        }

        private boolean done(boolean result) {
            if (open == 0) {
                view.changed();
            }
            return result;
        }

        @Override
        public boolean beginBatchEdit() {
            open++;
            return true;
        }

        @Override
        public boolean endBatchEdit() {
            if (open > 0) {
                open--;
            }
            done(true);
            return open > 0;
        }

        @Override
        public boolean commitText(CharSequence text, int newCursorPosition) {
            if (view.text == null) {
                return false;
            }
            if (text.toString().equals("\n")) {
                // (Enter is a key to the app, whatever it does there.)
                done(super.finishComposingText());
                sendKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER));
                sendKeyEvent(new KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_ENTER));
                return true;
            }
            return done(super.commitText(text, newCursorPosition));
        }

        @Override
        public boolean setComposingText(CharSequence text, int newCursorPosition) {
            return view.text != null && done(super.setComposingText(text, newCursorPosition));
        }

        @Override
        public boolean setComposingRegion(int start, int end) {
            return view.text != null && done(super.setComposingRegion(start, end));
        }

        @Override
        public boolean finishComposingText() {
            return view.text != null && done(super.finishComposingText());
        }

        @Override
        public boolean deleteSurroundingText(int before, int after) {
            return view.text != null && done(super.deleteSurroundingText(before, after));
        }

        @Override
        public boolean deleteSurroundingTextInCodePoints(int before, int after) {
            return view.text != null && done(super.deleteSurroundingTextInCodePoints(before, after));
        }

        @Override
        public boolean setSelection(int start, int end) {
            return view.text != null && done(super.setSelection(start, end));
        }
    }
}
