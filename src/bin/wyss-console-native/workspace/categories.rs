//! Skill categories and their display order in Workspace.
use crate::t;
use wyss_console::arsenal::store::Summary;

const RULES: &[(&str, &str, &[&str])] = &[
    (
        t!("General"),
        t!("Security"),
        &["security", "cso", "guard", "careful", "freeze"],
    ),
    (t!("General"), t!("Fixing bugs"), &["debug", "investigate"]),
    (
        t!("By type of work"),
        t!("Content and SEO"),
        &["content-strategy", "social-media"],
    ),
    (
        t!("By type of work"),
        t!("Animation and video"),
        &[
            "gsap",
            "anime",
            "animation",
            "lottie",
            "three",
            "typegpu",
            "waapi",
            "hyperframes",
            "remotion",
            "contribute-catalog",
        ],
    ),
    (
        t!("By type of work"),
        t!("Design and interface"),
        &[
            "design",
            "-ui",
            "ui-",
            "ux",
            "frontend",
            "tailwind",
            "shadcn",
            "accessibility",
            "image-to-code",
            "taste",
            "dashboard",
        ],
    ),
    (
        t!("Own topics"),
        t!("Study and knowledge"),
        &["tutor", "quiz", "cursos", "second-brain", "inbox", "learn"],
    ),
    (
        t!("General"),
        t!("Decisions"),
        &[
            "devils-advocate",
            "fool",
            "junta",
            "council",
            "ceo-review",
            "advisor",
        ],
    ),
    (
        t!("General"),
        t!("Research"),
        &[
            "research",
            "mapper",
            "graphify",
            "scrape",
            "discovery",
            "analyzer",
            "intel",
            "profiler",
        ],
    ),
    (
        t!("General"),
        t!("Review and testing"),
        &[
            "review",
            "verif",
            "checker",
            "auditor",
            "qa",
            "test",
            "eval",
            "health",
            "benchmark",
            "canary",
            "playwright",
        ],
    ),
    (
        t!("By type of work"),
        t!("Docs and writing"),
        &[
            "doc-",
            "document",
            "markdown",
            "humanizer",
            "pdf",
            "prompt",
            "skill-creator",
            "skillify",
            "retro",
            "report",
        ],
    ),
    (
        t!("General"),
        t!("Planning and method"),
        &[
            "plan",
            "roadmap",
            "spec",
            "office-hours",
            "framework-selector",
            "synthesizer",
            "curator",
        ],
    ),
    (
        t!("By type of work"),
        t!("Building"),
        &[
            "executor",
            "fixer",
            "ship",
            "deploy",
            "fastapi",
            "solid",
            "typescript",
            "react",
            "vite",
            "pair-agent",
        ],
    ),
    (
        t!("Environment"),
        t!("System and setup"),
        &[
            "cachyos",
            "workstation",
            "doctor",
            "setup",
            "upgrade",
            "ios-",
            "context-",
            "sync-",
            "browser",
        ],
    ),
];
pub fn category(item: &Summary) -> (&'static str, &'static str) {
    classify(&item.name, &item.description)
}

pub fn classify(name: &str, description: &str) -> (&'static str, &'static str) {
    let name = name.to_lowercase();
    RULES
        .iter()
        .find(|(_, category, words)| {
            (*category == "Content and SEO" && name.starts_with("seo-"))
                || (*category == "Planning and method"
                    && ["arsenal", "combo", "loops", "orient"].contains(&name.as_str()))
                || words.iter().any(|word| name.contains(word))
                || *category == "Building"
                    && ["implement", "build", "scaffold"]
                        .iter()
                        .any(|word| description.to_lowercase().contains(word))
        })
        .map(|(section, label, _)| (*section, *label))
        .unwrap_or((t!("Own topics"), t!("Other")))
}
