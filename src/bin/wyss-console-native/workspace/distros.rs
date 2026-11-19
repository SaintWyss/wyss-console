use crate::t;
use wyss_console::launch::Variant;

pub struct Line(&'static str, &'static str, Option<&'static str>);
pub struct OptionSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub flag: &'static str,
    pub hint: &'static str,
    flags: &'static [(&'static str, &'static str)],
}

pub struct Distro {
    pub id: &'static str,
    pub label: &'static str,
    lines: &'static [Line],
    pub options: &'static [OptionSpec],
}

const FLATPAK: Line = Line("flatpak", "flatpak update", Some("flatpak"));
const CACHYOS_NOCONFIRM: OptionSpec = OptionSpec {
    id: "noconfirm",
    label: t!("No confirmation"),
    flag: "--noconfirm",
    hint: t!(
        "Adds --noconfirm to pacman and paru, and -y to flatpak: they answer yes on their own and do not stop to ask. Without it, they show what changes and wait for you."
    ),
    flags: &[
        ("pacman", "--noconfirm"),
        ("paru", "--noconfirm"),
        ("flatpak", "-y"),
    ],
};
const CACHYOS_AUR: OptionSpec = OptionSpec {
    id: "aur",
    label: t!("Include AUR"),
    flag: "paru",
    hint: t!(
        "Adds “paru -Sua”: after the official packages, updates the ones installed from the AUR (built by the community). Needs paru installed."
    ),
    flags: &[],
};
const CACHYOS_SKIPREVIEW: OptionSpec = OptionSpec {
    id: "skipreview",
    label: t!("Skip AUR review"),
    flag: "--skipreview",
    hint: t!(
        "Adds --skipreview to paru: it does not show each AUR package’s build script before building it. Faster, but you do not see what it runs."
    ),
    flags: &[("paru", "--skipreview")],
};
const CACHYOS_DEVEL: OptionSpec = OptionSpec {
    id: "devel",
    label: t!("Include AUR -git packages"),
    flag: "--devel",
    hint: t!(
        "Adds --devel to paru: also rebuilds AUR packages that follow a repository (-git, -svn…), which a plain update leaves as they are."
    ),
    flags: &[("paru", "--devel")],
};
const CACHYOS_CLEAN: OptionSpec = OptionSpec {
    id: "clean",
    label: t!("Clean the package cache"),
    flag: "paccache -rk2",
    hint: t!(
        "Adds a line that deletes old downloaded packages and keeps the last 2 versions of each, to free disk space. Needs pacman-contrib."
    ),
    flags: &[],
};
const CACHYOS_FLATPAK: OptionSpec = OptionSpec {
    id: "flatpak",
    label: t!("Update Flatpak apps"),
    flag: "flatpak update",
    hint: t!(
        "Adds “flatpak update”: also updates the apps installed from Flathub, which the package manager does not touch."
    ),
    flags: &[],
};
const CACHYOS_CACHY: OptionSpec = OptionSpec {
    id: "cachy",
    label: t!("Finish with cachy-update"),
    flag: "",
    hint: t!(
        "Adds “cachy-update” at the end: CachyOS’s own updater. It updates whatever is still pending and then offers to clean orphans and old cache, review .pacnew files and restart the services that need it. It asks before each step."
    ),
    flags: &[],
};
const DEBIAN_YES: OptionSpec = OptionSpec {
    id: "yes",
    label: t!("No confirmation"),
    flag: "-y",
    hint: t!(
        "Adds -y: answers yes to every question, so the update runs to the end without stopping. Without it, it asks before installing."
    ),
    flags: &[("upgrade", "-y"), ("autoremove", "-y"), ("flatpak", "-y")],
};
const DEBIAN_AUTOREMOVE: OptionSpec = OptionSpec {
    id: "autoremove",
    label: t!("Remove unused packages"),
    flag: "autoremove",
    hint: t!(
        "Adds a line that removes packages nothing needs any more (left behind by earlier updates)."
    ),
    flags: &[],
};
const FEDORA_YES: OptionSpec = OptionSpec {
    id: "yes",
    label: t!("No confirmation"),
    flag: "-y",
    hint: t!(
        "Adds -y: answers yes to every question, so the update runs to the end without stopping. Without it, it asks before installing."
    ),
    flags: &[("dnf", "-y"), ("autoremove", "-y"), ("flatpak", "-y")],
};
const OPENSUSE_YES: OptionSpec = OptionSpec {
    id: "yes",
    label: t!("No confirmation"),
    flag: "-y",
    hint: t!(
        "Adds -y: answers yes to every question, so the update runs to the end without stopping. Without it, it asks before installing."
    ),
    flags: &[("dup", "-y"), ("flatpak", "-y")],
};

pub const DISTROS: &[Distro] = &[
    Distro {
        id: "cachyos",
        label: "CachyOS",
        lines: &[
            Line("pacman", "sudo pacman -Syu", None),
            Line("paru", "paru -Sua", Some("aur")),
            Line("clean", "sudo paccache -rk2", Some("clean")),
            FLATPAK,
            Line("cachy", "cachy-update", Some("cachy")),
        ],
        options: &[
            CACHYOS_NOCONFIRM,
            CACHYOS_CLEAN,
            CACHYOS_AUR,
            CACHYOS_FLATPAK,
            CACHYOS_SKIPREVIEW,
            CACHYOS_CACHY,
            CACHYOS_DEVEL,
        ],
    },
    Distro {
        id: "arch",
        label: "Arch",
        lines: &[
            Line("pacman", "sudo pacman -Syu", None),
            Line("paru", "paru -Sua", Some("aur")),
            Line("clean", "sudo paccache -rk2", Some("clean")),
            FLATPAK,
        ],
        options: &[
            CACHYOS_NOCONFIRM,
            CACHYOS_AUR,
            CACHYOS_SKIPREVIEW,
            CACHYOS_DEVEL,
            CACHYOS_CLEAN,
            CACHYOS_FLATPAK,
        ],
    },
    Distro {
        id: "debian",
        label: "Ubuntu / Debian",
        lines: &[
            Line("update", "sudo apt update", None),
            Line("upgrade", "sudo apt full-upgrade", None),
            Line("autoremove", "sudo apt autoremove", Some("autoremove")),
            FLATPAK,
        ],
        options: &[DEBIAN_YES, DEBIAN_AUTOREMOVE, CACHYOS_FLATPAK],
    },
    Distro {
        id: "fedora",
        label: "Fedora",
        lines: &[
            Line("dnf", "sudo dnf upgrade --refresh", None),
            Line("autoremove", "sudo dnf autoremove", Some("autoremove")),
            FLATPAK,
        ],
        options: &[FEDORA_YES, DEBIAN_AUTOREMOVE, CACHYOS_FLATPAK],
    },
    Distro {
        id: "opensuse",
        label: "openSUSE",
        lines: &[
            Line("refresh", "sudo zypper refresh", None),
            Line("dup", "sudo zypper dup", None),
            FLATPAK,
        ],
        options: &[OPENSUSE_YES, CACHYOS_FLATPAK],
    },
];

impl Distro {
    pub fn compose(&self, variant: &mut Variant) {
        variant.commands = self
            .lines
            .iter()
            .filter(|line| {
                line.2
                    .is_none_or(|id| variant.options.iter().any(|on| on == id))
            })
            .map(|line| {
                let mut command = line.1.to_owned();
                for option in self.options {
                    if variant.options.iter().any(|on| on == option.id) {
                        for (_, flag) in option.flags.iter().filter(|(key, _)| *key == line.0) {
                            command.push(' ');
                            command.push_str(flag);
                        }
                    }
                }
                command
            })
            .collect();
        variant.manual = false;
    }

    pub fn fresh(&self, template: bool) -> Variant {
        let mut variant = Variant {
            distro: self.id.into(),
            options: if template && self.id == "cachyos" {
                vec!["aur".into()]
            } else {
                vec![]
            },
            commands: Vec::new(),
            manual: !template,
        };
        if template {
            self.compose(&mut variant);
        }
        variant
    }
}

pub fn command_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_options_match_workspace_order_and_flags() {
        let distro = &DISTROS[0];
        let mut v = distro.fresh(true);
        assert_eq!(v.commands, ["sudo pacman -Syu", "paru -Sua"]);
        v.options = distro
            .options
            .iter()
            .map(|option| option.id.to_owned())
            .collect();
        distro.compose(&mut v);
        assert_eq!(
            v.commands,
            [
                "sudo pacman -Syu --noconfirm",
                "paru -Sua --noconfirm --skipreview --devel",
                "sudo paccache -rk2",
                "flatpak update -y",
                "cachy-update",
            ]
        );
        assert!(
            DISTROS[2]
                .fresh(true)
                .commands
                .contains(&"sudo apt full-upgrade".to_owned())
        );
        assert_eq!(command_lines("  first \n\nlast "), ["first", "last"]);
    }
}
