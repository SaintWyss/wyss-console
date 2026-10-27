package org.wyss.console.remote;

/** Pure density conversion shared by native layout code and JVM regression tests. */
final class UiDimensions {
    private UiDimensions() {}

    static int dpToPixels(int dp, float density) {
        if (dp < 0 || !Float.isFinite(density) || density <= 0) {
            throw new IllegalArgumentException("invalid dp dimension or display density");
        }
        return Math.round(dp * density);
    }
}
