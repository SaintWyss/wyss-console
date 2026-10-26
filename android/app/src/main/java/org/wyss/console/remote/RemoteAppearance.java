package org.wyss.console.remote;

/** Closed presentation palette shared by the native remote client screens. */
final class RemoteAppearance {
    private RemoteAppearance() {}

    enum Theme {
        DARK("dark"),
        LIGHT("light"),
        HIGH_CONTRAST("high_contrast");

        final String key;

        Theme(String key) {
            this.key = key;
        }

        static Theme from(String value) {
            for (Theme theme : values()) {
                if (theme.key.equals(value)) return theme;
            }
            return DARK;
        }
    }

    record Palette(int background, int surface, int text, int mutedText, int accent,
                   int border, int terminalBackground, int terminalText) {}

    static Palette palette(Theme theme) {
        return switch (theme) {
            case DARK -> new Palette(
                    0xff111820, 0xff1f2a35, 0xfff5f7fa, 0xffcfd8e3,
                    0xff9bc4ff, 0xffb8c4d1, 0xff000000, 0xfff5f7fa);
            case LIGHT -> new Palette(
                    0xfff6f4ef, 0xffffffff, 0xff1b2c39, 0xff48525b,
                    0xff124b8a, 0xff52606b, 0xff111820, 0xfff5f7fa);
            case HIGH_CONTRAST -> new Palette(
                    0xff000000, 0xff000000, 0xffffffff, 0xffffffff,
                    0xffffff00, 0xffffffff, 0xff000000, 0xffffffff);
        };
    }
}
