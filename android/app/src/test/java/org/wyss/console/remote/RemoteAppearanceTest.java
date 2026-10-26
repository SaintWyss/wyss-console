package org.wyss.console.remote;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

public final class RemoteAppearanceTest {
    @Test public void onlyTheClosedPersistedThemeNamesAreAccepted() {
        assertEquals(RemoteAppearance.Theme.DARK, RemoteAppearance.Theme.from("dark"));
        assertEquals(RemoteAppearance.Theme.LIGHT, RemoteAppearance.Theme.from("light"));
        assertEquals(RemoteAppearance.Theme.HIGH_CONTRAST,
                RemoteAppearance.Theme.from("high_contrast"));
        assertEquals(RemoteAppearance.Theme.DARK, RemoteAppearance.Theme.from("system"));
        assertEquals(RemoteAppearance.Theme.DARK, RemoteAppearance.Theme.from("Dark"));
        assertEquals(RemoteAppearance.Theme.DARK, RemoteAppearance.Theme.from(null));
    }

    @Test public void everyThemeKeepsTextAndTerminalContrastAboveWcagAa() {
        for (RemoteAppearance.Theme theme : RemoteAppearance.Theme.values()) {
            RemoteAppearance.Palette palette = RemoteAppearance.palette(theme);
            assertAtLeast(contrast(palette.text(), palette.background()), 4.5, theme + " text/background");
            assertAtLeast(contrast(palette.mutedText(), palette.background()), 4.5, theme + " muted/background");
            assertAtLeast(contrast(palette.text(), palette.surface()), 4.5, theme + " text/surface");
            assertAtLeast(contrast(palette.accent(), palette.background()), 4.5, theme + " accent/background");
            assertAtLeast(contrast(palette.terminalText(), palette.terminalBackground()),
                    4.5, theme + " terminal");
            assertAtLeast(contrast(palette.border(), palette.background()), 3.0, theme + " border/background");
        }
    }

    private static void assertAtLeast(double actual, double expected, String label) {
        assertTrue(label + " contrast was " + actual, actual >= expected);
    }

    private static double contrast(int foreground, int background) {
        double first = luminance(foreground);
        double second = luminance(background);
        double lighter = Math.max(first, second);
        double darker = Math.min(first, second);
        return (lighter + 0.05) / (darker + 0.05);
    }

    private static double luminance(int color) {
        double red = linear((color >>> 16) & 0xff);
        double green = linear((color >>> 8) & 0xff);
        double blue = linear(color & 0xff);
        return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    }

    private static double linear(int channel) {
        double value = channel / 255.0;
        return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
    }
}
