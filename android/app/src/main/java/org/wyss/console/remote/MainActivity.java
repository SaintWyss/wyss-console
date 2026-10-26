package org.wyss.console.remote;

import android.app.Activity;
import android.content.res.ColorStateList;
import android.graphics.drawable.GradientDrawable;
import android.os.Bundle;
import android.view.View;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.RadioButton;
import android.widget.RadioGroup;
import android.widget.ScrollView;
import android.widget.TextView;

/** Native Android remote-client shell. All data screens fail closed while the transport is blocked. */
public final class MainActivity extends Activity {
    private static final String[] SCREENS = {
            "Hosts", "Pairing", "Projects", "Sessions", "Terminal", "Pipelines",
            "Approvals", "Devices", "Settings"
    };
    private RemoteGatewayClient gateway;
    private LinearLayout body;
    private String current = "Hosts";
    private RemoteAppearance.Theme theme = RemoteAppearance.Theme.DARK;

    @Override public void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_SECURE);
        theme = RemoteAppearance.Theme.from(
                getPreferences(MODE_PRIVATE).getString("appearance_theme", "dark"));
        gateway = new RemoteGatewayClient.Unavailable();
        render();
    }

    @Override protected void onStop() {
        // Remote output, approval details and terminal input are memory-only and cleared on leave.
        if (body != null) body.removeAllViews();
        super.onStop();
    }

    @Override protected void onStart() {
        super.onStart();
        if (body == null || body.getChildCount() == 0) render();
    }

    private void render() {
        RemoteAppearance.Palette palette = RemoteAppearance.palette(theme);
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(palette.background());
        root.setPadding(dp(16), dp(16), dp(16), dp(12));

        TextView title = new TextView(this);
        title.setText("WYSS REMOTE");
        title.setTextColor(palette.text());
        title.setTextSize(22);
        title.setTypeface(null, android.graphics.Typeface.BOLD);
        title.setAccessibilityHeading(true);
        root.addView(title);

        TextView status = new TextView(this);
        status.setText("Offline · transport decision OD01 is open · no host listener is contacted");
        status.setTextColor(palette.accent());
        status.setTextSize(14);
        status.setPadding(0, dp(8), 0, dp(8));
        status.setContentDescription("Connection status: blocked. No network request was sent.");
        root.addView(status);

        HorizontalScrollView navigationScroll = new HorizontalScrollView(this);
        navigationScroll.setHorizontalScrollBarEnabled(true);
        navigationScroll.setFillViewport(false);
        LinearLayout navigation = new LinearLayout(this);
        navigation.setOrientation(LinearLayout.HORIZONTAL);
        for (String screen : SCREENS) {
            Button button = new Button(this);
            button.setText(screen);
            styleButton(button, palette);
            button.setContentDescription("Open " + screen);
            button.setOnClickListener(view -> {
                current = screen;
                showScreen();
            });
            navigation.addView(button);
        }
        navigationScroll.addView(navigation);
        root.addView(navigationScroll);

        ScrollView contentScroll = new ScrollView(this);
        body = new LinearLayout(this);
        body.setOrientation(LinearLayout.VERTICAL);
        body.setPadding(0, dp(14), 0, dp(24));
        contentScroll.addView(body);
        root.addView(contentScroll, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f));

        setContentView(root);
        showScreen();
    }

    private void showScreen() {
        if (body == null) return;
        RemoteAppearance.Palette palette = RemoteAppearance.palette(theme);
        body.removeAllViews();
        heading(current);
        switch (current) {
            case "Hosts" -> {
                message("No paired hosts are stored on this device.");
                message("Pairing remains unavailable until the owner selects and independently reviews a transport. Host identity, expiry and revocation will be shown here after an authenticated connection exists.");
            }
            case "Pairing" -> {
                message("Pairing is disabled while OD01 is unresolved.");
                message("The eventual flow requires QR proof, two distinct device keys, a host fingerprint comparison, explicit host approval and scoped grants. Opening this screen does not create keys or enroll a device.");
                disabled("Scan host QR");
            }
            case "Projects" -> message("Project and workspace navigation needs an authenticated host. No local paths or remote projects are shown while disconnected.");
            case "Sessions" -> message("No session list is cached. A future connection will label host, plane, generation and observation age.");
            case "Terminal" -> {
                message("No terminal is attached. View and input grants are separate; terminal.write is full host-account control and stays disabled by default.");
                TextView terminal = new TextView(this);
                terminal.setText("Disconnected — no output available");
                terminal.setTextColor(palette.terminalText());
                terminal.setBackgroundColor(palette.terminalBackground());
                terminal.setTextSize(16);
                terminal.setPadding(dp(14), dp(18), dp(14), dp(18));
                terminal.setHorizontallyScrolling(true);
                terminal.setContentDescription("Terminal output. Disconnected. No commands are queued.");
                body.addView(terminal);
                disabled("Type");
                message("Input delivery reports accepted bytes, partial writes or OUTCOME_UNKNOWN. Lost acknowledgements are never replayed; reconnect requires new user input.");
            }
            case "Pipelines" -> message("No pipeline metadata is cached. Pipeline status and handoffs will be fetched after authenticated host negotiation; no prompts or provider output are stored here.");
            case "Approvals" -> message("Managed approvals remain owned by wyss-control. Remote decisions must call its existing typed authority API and require a fresh per-operation presence check.");
            case "Devices" -> {
                message("This device has no enrolled host and no remote identity has been created.");
                message("P-256 connection and per-use presence keys are generated only after an explicit pairing flow begins. Keystore security level alone is not attestation; this client cannot request LP4 until authenticated key evidence is verified.");
            }
            case "Settings" -> {
                addAppearanceSettings();
                message("H1 · user-configured overlay + gateway. Android requires the overlay VPN client and its OS VPN configuration. As a price reference, Tailscale currently lists Personal at $0 (up to 6 users), Standard at $8/user/month and Premium at $18/user/month. This does not establish which plan or overlay the owner uses.");
                message("H2 · iroh Kotlin/QUIC + relay. First-party Android bindings are available; peer streams use end-to-end TLS. Public relay is free but rate-limited with no uptime guarantee; hosted relay is $19/month and up; self-hosted relay has no software fee but transfers hosting, operations and availability cost to Wyss.");
                message("Neither candidate has reference-PC/phone CPU, RAM, battery, idle, latency or roaming measurements. OD01 remains an owner decision. No transport is selected, installed, enrolled or exposed. OD02 keeps full-account input from a locked host blocked.");
            }
            default -> message("Unavailable");
        }
        message("Gateway state: " + gateway.state() + ". No action was queued or sent.");
    }

    private void heading(String text) {
        RemoteAppearance.Palette palette = RemoteAppearance.palette(theme);
        TextView view = new TextView(this);
        view.setText(text);
        view.setTextColor(palette.text());
        view.setTextSize(20);
        view.setTypeface(null, android.graphics.Typeface.BOLD);
        view.setPadding(0, 0, 0, dp(8));
        view.setAccessibilityHeading(true);
        body.addView(view);
    }

    private void message(String text) {
        RemoteAppearance.Palette palette = RemoteAppearance.palette(theme);
        TextView view = new TextView(this);
        view.setText(text);
        view.setTextColor(palette.mutedText());
        view.setTextSize(16);
        view.setLineSpacing(dp(3), 1.0f);
        view.setPadding(0, dp(6), 0, dp(10));
        body.addView(view);
    }

    private void disabled(String label) {
        RemoteAppearance.Palette palette = RemoteAppearance.palette(theme);
        Button button = new Button(this);
        button.setText(label + " · unavailable");
        button.setEnabled(false);
        styleButton(button, palette);
        body.addView(button);
    }

    private void addAppearanceSettings() {
        RemoteAppearance.Palette palette = RemoteAppearance.palette(theme);
        message("Color theme · presentation only");
        RadioGroup choices = new RadioGroup(this);
        choices.setOrientation(RadioGroup.VERTICAL);
        choices.setContentDescription("Color theme");
        for (RemoteAppearance.Theme candidate : RemoteAppearance.Theme.values()) {
            RadioButton choice = new RadioButton(this);
            choice.setId(View.generateViewId());
            choice.setText(switch (candidate) {
                case DARK -> "Dark";
                case LIGHT -> "Light";
                case HIGH_CONTRAST -> "High contrast";
            });
            choice.setTextColor(palette.text());
            choice.setButtonTintList(ColorStateList.valueOf(palette.accent()));
            choice.setTag(candidate);
            choices.addView(choice);
            if (candidate == theme) choices.check(choice.getId());
        }
        choices.setOnCheckedChangeListener((group, checkedId) -> {
            View selected = group.findViewById(checkedId);
            if (!(selected instanceof RadioButton button)
                    || !(button.getTag() instanceof RemoteAppearance.Theme selectedTheme)) return;
            if (selectedTheme == theme) return;
            theme = selectedTheme;
            getPreferences(MODE_PRIVATE).edit()
                    .putString("appearance_theme", theme.key)
                    .apply();
            render();
        });
        body.addView(choices);
        message("Saved on this device. Appearance does not change provider, host or approval permissions.");
    }

    private void styleButton(Button button, RemoteAppearance.Palette palette) {
        button.setTextColor(palette.text());
        GradientDrawable background = new GradientDrawable();
        background.setColor(palette.surface());
        background.setCornerRadius(dp(4));
        background.setStroke(dp(1), palette.border());
        button.setBackground(background);
        button.setMinHeight(dp(48));
    }

    private int dp(int value) {
        return UiDimensions.dpToPixels(value, getResources().getDisplayMetrics().density);
    }
}
