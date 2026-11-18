package de.musiker15.sshcraft;

import android.app.Activity;
import android.hardware.biometrics.BiometricManager;
import android.hardware.biometrics.BiometricPrompt;
import android.os.CancellationSignal;
import java.util.concurrent.ConcurrentHashMap;

/**
 * The system's biometric prompt for SSHCraft's app lock: a fingerprint, the face, or the device
 * PIN, pattern or password when the user prefers it. Rust starts a prompt with {@link #ask} and
 * reads its answer with {@link #poll}; the prompt's callback is Java, which Rust cannot provide.
 * Built into classes.dex by {@code cargo xtask android}.
 */
public final class Biometric {
    private static final int AUTHENTICATORS =
            BiometricManager.Authenticators.BIOMETRIC_STRONG | BiometricManager.Authenticators.DEVICE_CREDENTIAL;

    public static final int PENDING = -1;
    public static final int VERIFIED = 0;
    public static final int DECLINED = 1;

    /** Answers by token until they are read; a token is asked once. */
    private static final ConcurrentHashMap<Long, Integer> ANSWERS = new ConcurrentHashMap<>();

    private Biometric() {}

    /** Whether the device can confirm the user: a strong biometric or a screen lock is set up. */
    public static boolean available(Activity activity) {
        BiometricManager manager = activity.getSystemService(BiometricManager.class);
        return manager != null && manager.canAuthenticate(AUTHENTICATORS) == BiometricManager.BIOMETRIC_SUCCESS;
    }

    /** Shows the prompt on the activity's thread. Anything but a success is declined. */
    public static void ask(final Activity activity, final String title, final String subtitle, final long token) {
        ANSWERS.put(token, PENDING);
        activity.runOnUiThread(() -> {
            try {
                BiometricPrompt prompt = new BiometricPrompt.Builder(activity)
                        .setTitle(title)
                        .setSubtitle(subtitle)
                        .setAllowedAuthenticators(AUTHENTICATORS)
                        .build();
                prompt.authenticate(new CancellationSignal(), activity.getMainExecutor(), new BiometricPrompt.AuthenticationCallback() {
                    @Override
                    public void onAuthenticationSucceeded(BiometricPrompt.AuthenticationResult result) {
                        ANSWERS.put(token, VERIFIED);
                    }

                    @Override
                    public void onAuthenticationError(int code, CharSequence message) {
                        ANSWERS.put(token, DECLINED);
                    }
                });
            } catch (RuntimeException e) {
                ANSWERS.put(token, DECLINED);
            }
        });
    }

    /** The answer for token: PENDING while the prompt is open; an unknown token is declined. */
    public static int poll(long token) {
        Integer answer = ANSWERS.get(token);
        if (answer == null) {
            return DECLINED;
        }
        if (answer != PENDING) {
            ANSWERS.remove(token);
        }
        return answer;
    }
}
