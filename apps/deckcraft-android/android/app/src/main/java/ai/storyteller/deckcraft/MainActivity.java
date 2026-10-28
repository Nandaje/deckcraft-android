package ai.storyteller.deckcraft;

import android.Manifest;
import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.Settings;

/**
 * DeckCraft's activity: a {@link NativeActivity} running the Rust app ({@code libdeckcraft_android.so}),
 * plus what Rust can't do on its own: asking for file access.
 */
public class MainActivity extends NativeActivity {
    private static final int STORAGE_REQUEST = 1;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        if (!hasStorageAccess()) {
            requestStorageAccess();
        }
    }

    /** Whether DeckCraft can read and write shared storage. */
    public boolean hasStorageAccess() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return Environment.isExternalStorageManager();
        }
        return checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED;
    }

    /**
     * Shows the system's "All files access" page for DeckCraft (Android 11+), or the storage
     * permission prompt (Android 8-10). Called at start and, through JNI, from the file browser's
     * "Grant file access" button, so it may run on any thread.
     */
    public void requestStorageAccess() {
        runOnUiThread(() -> {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                try {
                    Intent intent = new Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                            Uri.parse("package:" + getPackageName()));
                    startActivity(intent);
                } catch (ActivityNotFoundException e) {
                    startActivity(new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION));
                }
            } else {
                requestPermissions(new String[] {
                        Manifest.permission.READ_EXTERNAL_STORAGE,
                        Manifest.permission.WRITE_EXTERNAL_STORAGE,
                }, STORAGE_REQUEST);
            }
        });
    }

    /**
     * Back leaves DeckCraft running in the background instead of finishing the activity: the Rust
     * event loop can only start once per process. Open documents stay open (AutoRecover also
     * keeps them if Android ends the process).
     */
    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        moveTaskToBack(true);
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        // A later launch must start a fresh process, for the same reason as above.
        android.os.Process.killProcess(android.os.Process.myPid());
    }
}
