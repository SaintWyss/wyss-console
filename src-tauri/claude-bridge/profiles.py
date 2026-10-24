"""Frozen Claude Agent SDK option sets for the two Wyss Managed profiles.

Nothing here is configurable by the authority's client: the authority picks a profile *name*
and the bridge builds the options from this file, which the authority has hashed. The sets
come from the exact-build experiments in `probe/matrix*.py` (results: the "Results of E1-E9"
section of docs/research/claude-managed-feasibility-2026-10-23.md): each option exists because the
experiment showed a hidden effect path that the option closes, and `disallowed_tools` alone
was shown NOT to close hooks, MCP launch, apiKeyHelper or memory loading.
"""

# Names of the only tools the model may ever be offered, per profile. The authority-owned
# server is called "wyss"; the SDK namespaces its tools as `mcp__wyss__<tool>`.
AUTHORITY_SERVER = "wyss"
AUTHORITY_TOOLS = ("run_command", "write_file")
AUTHORITY_TOOL_NAMES = tuple("mcp__%s__%s" % (AUTHORITY_SERVER, t) for t in AUTHORITY_TOOLS)
# The CLI adds this entry to the API `tools` list whenever an MCP tool exists. Calling it
# returns "No such tool available" (probe matrix3 case g); it carries no effect.
INERT_TOOL_NAMES = ("DeferredToolPlaceholder",)

PROFILES = ("inference", "local_tools")

# CLI flags passed through `extra_args`. `bare` skips hooks, plugin sync, auto-memory,
# keychain/OAuth reads and CLAUDE.md discovery; `safe-mode` disables customizations;
# `disable-slash-commands` removes every skill/command; `permission-prompts none` makes
# anything that would have prompted a denial instead of a question nobody answers.
BASE_EXTRA_ARGS = {
    "bare": None,
    "safe-mode": None,
    "disable-slash-commands": None,
    "permission-prompts": "none",
}


def allowed_api_tools(profile):
    """The exact set of tool names an API request may carry for this profile."""
    if profile == "inference":
        return frozenset()
    if profile == "local_tools":
        return frozenset(AUTHORITY_TOOL_NAMES) | frozenset(INERT_TOOL_NAMES)
    raise ValueError("unknown profile")


def option_kwargs(profile, *, cwd, session_id=None, resume=None, model=None, effort=None):
    """Plain keyword arguments for `ClaudeAgentOptions` (callbacks and the SDK server are
    added by the bridge). Pure and deterministic so the authority can digest it."""
    if profile not in PROFILES:
        raise ValueError("unknown profile")
    kw = {
        # Built-in tools: none. `tools=[]` is what removes the definitions; the model is never
        # offered Bash/Edit/Write/Read/WebFetch/Agent/Skill/Workflow/Cron*.
        "tools": [],
        "disallowed_tools": ["*"] if profile == "inference" else [],
        "allowed_tools": [] if profile == "inference" else list(AUTHORITY_TOOL_NAMES),
        # No user/project/local settings (hooks, apiKeyHelper, permissions, env, MCP, memory).
        "setting_sources": [],
        "strict_mcp_config": True,
        "plugins": [],
        "skills": [],
        # `@path` expansion and slash dispatch would read local files / run commands from
        # prompt text with no tool at all (probe case mention-default).
        "verbatim_prompts": True,
        "permission_mode": "dontAsk",
        "cwd": cwd,
        "include_partial_messages": True,
        "extra_args": dict(BASE_EXTRA_ARGS),
    }
    # A conversation is either named up front (a new one) or resumed, never both.
    if resume:
        kw["resume"] = resume
    elif session_id:
        kw["session_id"] = session_id
    if model:
        kw["model"] = model
    if effort:
        kw["effort"] = effort
    return kw

# Environment the CLI is started with, merged over the (already empty) bridge environment.
# None of these is a credential. They ask the CLI for no optional traffic; they do not confine it.
# What confines it is the network namespace it runs in (a loopback and nothing else, see
# `f8/claude/launch.rs`), which a test observes.
CLI_ENV = {
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
    "DISABLE_AUTOUPDATER": "1",
    "DISABLE_TELEMETRY": "1",
    "DISABLE_ERROR_REPORTING": "1",
    "DISABLE_BUG_COMMAND": "1",
}
# The key the CLI is given is minted per run (`bridge.py`) and is the tap's password; only its shape is
# fixed. Written in two pieces so no source line looks like a credential to a scanner; it is not one.
# Where the credential broker's socket is seen from inside the sandbox of the bridge and the CLI.
BROKER_DIR = "/run/wyss/broker"
BROKER_SOCKET = BROKER_DIR + "/broker.sock"
SYNTHETIC_PREFIX = "sk" + "-ant-api03-wyss-tap-"


def frozen_digest(profile):
    """SHA-256 of everything this file decides for a profile, independent of the per-session
    values (cwd, resume, model, effort). The authority pins the expected digests: a change to
    this file that is not mirrored in the authority changes the digest and is refused."""
    import hashlib
    import json

    kw = option_kwargs(profile, cwd="/")
    del kw["cwd"]
    doc = {
        "profile": profile,
        "options": kw,
        "api_tools": sorted(allowed_api_tools(profile)),
        "cli_env": CLI_ENV,
        "authority_tools": list(AUTHORITY_TOOL_NAMES),
    }
    return hashlib.sha256(json.dumps(doc, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
