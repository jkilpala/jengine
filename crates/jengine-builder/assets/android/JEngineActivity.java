// The activity an Android build runs in when `settings.android.immersive` is
// on (the default). Everything about the game is NativeActivity's and
// libmain.so's; this class exists only to hide the system bars.
//
// It has to be Java: hiding the bars is a call on the activity's window, and
// Android only allows that from the UI thread, which the game never runs on -
// `android_main` has a thread of its own, and a JNI call from there throws
// CalledFromWrongThreadException. So it is done here, every time the window
// gets focus back (a notification, the recents screen and a dialog all bring
// the bars back with them).
//
// Compiled by jengine-builder (`javac`, then `d8`) into the APK's classes.dex.

package com.jengine.runtime;

import android.app.NativeActivity;
import android.os.Build;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;

public class JEngineActivity extends NativeActivity {
    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            hideSystemBars();
        }
    }

    @SuppressWarnings("deprecation")
    private void hideSystemBars() {
        if (Build.VERSION.SDK_INT >= 28) {
            // Draw under a notch or camera cut-out on the short edges too;
            // otherwise a phone letterboxes the game around it.
            WindowManager.LayoutParams attrs = getWindow().getAttributes();
            attrs.layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            getWindow().setAttributes(attrs);
        }
        if (Build.VERSION.SDK_INT >= 30) {
            getWindow().setDecorFitsSystemWindows(false);
            WindowInsetsController controller = getWindow().getInsetsController();
            if (controller != null) {
                controller.hide(WindowInsets.Type.systemBars());
                // A swipe from the edge shows them for a moment, over the
                // game, without taking the game's fingers away.
                controller.setSystemBarsBehavior(
                    WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            }
        } else {
            getWindow().getDecorView().setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                    | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_FULLSCREEN);
        }
    }
}
