use super::*;

fn entry(id: &str, alias: &str, efforts: &[&str], default: bool) -> Entry {
    Entry {
        model_id: id.into(),
        alias: Some(alias.into()),
        efforts: efforts.iter().map(|e| (*e).into()).collect(),
        is_default: default,
    }
}
fn catalogue(provider: Provider, revision: u64, observed_at: u64) -> Catalogue {
    Catalogue {
        provider,
        build: "2.1.309".into(),
        revision,
        observed_at,
        complete: true,
        source: if provider == Provider::Claude {
            Source::ClaudeServerInfo
        } else {
            Source::CodexModelList
        },
        entries: vec![
            entry("claude-opus-5-5", "opus", &["low", "high"], true),
            entry("claude-sonnet-5-5", "sonnet", &["low"], false),
        ],
    }
}
fn scope<'a>(provider: Provider, generation: u64) -> Scope<'a> {
    Scope {
        provider,
        build: "2.1.309",
        session: "S",
        runtime_instance: "R",
        generation,
    }
}
fn req(model: &str, effort: Option<&str>) -> ModelRequest {
    ModelRequest::new(model, effort).unwrap()
}
const NOW: u64 = 1_000_000;

#[test]
fn the_gate_is_closed_so_the_public_entry_points_refuse() {
    let cat = catalogue(Provider::Claude, 1, NOW);
    assert_eq!(
        validate(&scope(Provider::Claude, 1), &cat, NOW, req("opus", None)),
        Err(Refusal::GateClosed)
    );
    assert_eq!(
        validate_runtime_effort(Provider::Claude, "high"),
        Err(Refusal::GateClosed)
    );
}

#[test]
fn a_valid_request_is_bound_to_its_generation_and_catalogue() {
    let cat = catalogue(Provider::Claude, 7, NOW);
    let v = validate_with_gate(&scope(Provider::Claude, 3), &cat, NOW, req("opus", None)).unwrap();
    assert_eq!(v.resolved_model, "claude-opus-5-5");
    assert_eq!((v.bound.generation, v.bound.catalogue_revision), (3, 7));
    assert_eq!(
        (v.bound.session.as_str(), v.bound.runtime_instance.as_str()),
        ("S", "R")
    );
    // An id and its alias are the same model.
    let by_id = validate_with_gate(
        &scope(Provider::Claude, 3),
        &cat,
        NOW,
        req("claude-opus-5-5", None),
    )
    .unwrap();
    assert_eq!(by_id.resolved_model, v.resolved_model);
}

#[test]
fn unknown_models_are_refused_and_nothing_is_substituted() {
    let cat = catalogue(Provider::Claude, 1, NOW);
    for name in [
        "gpt-6",
        "Opus",
        "claude-opus",
        "opus ",
        "default",
        "claude-opus-5-5-latest",
    ] {
        let r = ModelRequest::new(name, None)
            .and_then(|r| validate_with_gate(&scope(Provider::Claude, 1), &cat, NOW, r));
        assert!(
            matches!(r, Err(Refusal::UnknownModel | Refusal::InvalidWord)),
            "{name}: {r:?}"
        );
    }
}

#[test]
fn words_are_provider_identifiers_not_sentences() {
    for bad in [
        "",
        "has space",
        "line\nbreak",
        "tab\t",
        "é",
        &"m".repeat(129),
    ] {
        assert!(ModelRequest::new(bad, None).is_err(), "{bad:?}");
    }
    assert_eq!(ModelRequest::new("", None), Err(Refusal::EmptyRequest));
    assert_eq!(
        ModelRequest::new("m", Some("two words")),
        Err(Refusal::InvalidWord)
    );
    assert_eq!(
        ModelRequest::new("m", Some(&"e".repeat(33))),
        Err(Refusal::InvalidWord)
    );
}

#[test]
fn effort_is_validated_by_what_the_provider_integration_can_do_not_by_shape() {
    // Claude: the SDK reads the effort when the CLI starts, so a per-turn effort is refused
    // even when the catalogue lists it for the model.
    let claude = catalogue(Provider::Claude, 1, NOW);
    assert_eq!(
        validate_with_gate(
            &scope(Provider::Claude, 1),
            &claude,
            NOW,
            req("opus", Some("high"))
        ),
        Err(Refusal::EffortNotPerTurn)
    );
    // Codex: per turn, and only for levels the model lists.
    let mut codex = catalogue(Provider::Codex, 1, NOW);
    codex.build = "0.159.0".into();
    let sc = Scope {
        build: "0.159.0",
        ..scope(Provider::Codex, 1)
    };
    assert!(validate_with_gate(&sc, &codex, NOW, req("opus", Some("high"))).is_ok());
    assert_eq!(
        validate_with_gate(&sc, &codex, NOW, req("sonnet", Some("high"))),
        Err(Refusal::UnsupportedEffort)
    );
    assert_eq!(
        validate_with_gate(&sc, &codex, NOW, req("opus", Some("extreme"))),
        Err(Refusal::UnsupportedEffort)
    );
}

#[test]
fn the_runtime_level_effort_of_claude_is_checked_against_the_pinned_builds_levels() {
    for ok in ["low", "medium", "high", "xhigh", "max"] {
        assert_eq!(
            validate_runtime_effort_with_gate(Provider::Claude, ok),
            Ok(()),
            "{ok}"
        );
    }
    for bad in ["", "ultra", "HIGH", "high "] {
        assert_eq!(
            validate_runtime_effort_with_gate(Provider::Claude, bad),
            Err(Refusal::UnsupportedEffort),
            "{bad:?}"
        );
    }
    // Codex takes its effort per turn, not as a runtime pin.
    assert_eq!(
        validate_runtime_effort_with_gate(Provider::Codex, "high"),
        Err(Refusal::UnsupportedEffort)
    );
}

#[test]
fn a_catalogue_that_is_stale_incomplete_or_for_another_provider_or_build_admits_nothing() {
    let sc = scope(Provider::Claude, 1);
    let old = catalogue(Provider::Claude, 1, NOW - CATALOGUE_MAX_AGE_SECS - 1);
    assert_eq!(
        validate_with_gate(&sc, &old, NOW, req("opus", None)),
        Err(Refusal::CatalogueStale)
    );
    let edge = catalogue(Provider::Claude, 1, NOW - CATALOGUE_MAX_AGE_SECS);
    assert!(validate_with_gate(&sc, &edge, NOW, req("opus", None)).is_ok());
    let mut partial = catalogue(Provider::Claude, 1, NOW);
    partial.complete = false;
    assert_eq!(
        validate_with_gate(&sc, &partial, NOW, req("opus", None)),
        Err(Refusal::CatalogueIncomplete)
    );
    let other = catalogue(Provider::Codex, 1, NOW);
    assert_eq!(
        validate_with_gate(&sc, &other, NOW, req("opus", None)),
        Err(Refusal::ProviderMismatch)
    );
    let mut build = catalogue(Provider::Claude, 1, NOW);
    build.build = "2.1.310".into();
    assert_eq!(
        validate_with_gate(&sc, &build, NOW, req("opus", None)),
        Err(Refusal::BuildMismatch)
    );
    // A clock that went backwards is not "older than the limit".
    assert!(
        validate_with_gate(
            &sc,
            &catalogue(Provider::Claude, 1, NOW + 100),
            NOW,
            req("opus", None)
        )
        .is_ok()
    );
}

#[test]
fn revalidation_before_dispatch_refuses_anything_that_changed() {
    let cat = catalogue(Provider::Claude, 5, NOW);
    let sc = scope(Provider::Claude, 2);
    let v = validate_with_gate(&sc, &cat, NOW, req("opus", None)).unwrap();
    assert_eq!(revalidate(&v, &sc, &cat, NOW + 10), Ok(()));
    // Another generation, runtime, session, build or provider.
    assert_eq!(
        revalidate(&v, &scope(Provider::Claude, 3), &cat, NOW),
        Err(Refusal::GenerationMismatch)
    );
    assert_eq!(
        revalidate(
            &v,
            &Scope {
                runtime_instance: "R2",
                ..sc
            },
            &cat,
            NOW
        ),
        Err(Refusal::RuntimeMismatch)
    );
    assert_eq!(
        revalidate(
            &v,
            &Scope {
                session: "S2",
                ..sc
            },
            &cat,
            NOW
        ),
        Err(Refusal::SessionMismatch)
    );
    assert_eq!(
        revalidate(&v, &Scope { build: "x", ..sc }, &cat, NOW),
        Err(Refusal::BuildMismatch)
    );
    assert_eq!(
        revalidate(&v, &scope(Provider::Codex, 2), &cat, NOW),
        Err(Refusal::ProviderMismatch)
    );
    // The catalogue was taken again (even with the same content) or aged out.
    let again = catalogue(Provider::Claude, 6, NOW + 5);
    assert_eq!(
        revalidate(&v, &sc, &again, NOW + 5),
        Err(Refusal::CatalogueChanged)
    );
    assert_eq!(
        revalidate(&v, &sc, &cat, NOW + CATALOGUE_MAX_AGE_SECS + 1),
        Err(Refusal::CatalogueStale)
    );
    // The model was retired from the listing under the same revision number: still refused.
    let mut retired = cat.clone();
    retired
        .entries
        .retain(|e| e.alias.as_deref() != Some("opus"));
    assert_eq!(
        revalidate(&v, &sc, &retired, NOW),
        Err(Refusal::UnknownModel)
    );
}

#[test]
fn a_provider_that_answers_with_another_model_is_shown_as_rerouted_never_as_honored() {
    let cat = catalogue(Provider::Claude, 1, NOW);
    let sc = scope(Provider::Claude, 1);
    let v = validate_with_gate(&sc, &cat, NOW, req("sonnet", None)).unwrap();
    let requested = Requested {
        turn_id: "t1".into(),
        request: v.request.clone(),
        resolved_model: v.resolved_model.clone(),
        generation: 1,
    };
    let set = |names: &[&str]| {
        names
            .iter()
            .map(|n| (*n).to_owned())
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(
        reroute(Some(&cat), None, &set(&["x"])),
        Reroute::NotRequested
    );
    assert_eq!(
        reroute(Some(&cat), Some(&requested), &set(&[])),
        Reroute::NotReported
    );
    assert_eq!(
        reroute(Some(&cat), Some(&requested), &set(&["claude-sonnet-5-5"])),
        Reroute::Honored
    );
    // The alias is the same model.
    assert_eq!(
        reroute(Some(&cat), Some(&requested), &set(&["sonnet"])),
        Reroute::Honored
    );
    assert_eq!(
        reroute(Some(&cat), Some(&requested), &set(&["claude-haiku-5-5"])),
        Reroute::Rerouted {
            requested: "claude-sonnet-5-5".into(),
            reported: vec!["claude-haiku-5-5".into()]
        }
    );
    // A second model beside the requested one is not "honored" either.
    assert!(matches!(
        reroute(
            Some(&cat),
            Some(&requested),
            &set(&["claude-sonnet-5-5", "claude-haiku-5-5"])
        ),
        Reroute::Rerouted { .. }
    ));
    // Without a catalogue the comparison is by exact name.
    assert!(matches!(
        reroute(None, Some(&requested), &set(&["sonnet"])),
        Reroute::Rerouted { .. }
    ));
}

#[test]
fn effort_evidence_keeps_requested_observed_and_reported_apart() {
    assert_eq!(effort(None, None, None), EffortEvidence::NotRequested);
    assert_eq!(
        effort(None, Some("medium"), None),
        EffortEvidence::NotRequested,
        "a default is not a request"
    );
    assert_eq!(
        effort(Some("low"), None, None),
        EffortEvidence::Requested("low".into())
    );
    assert_eq!(
        effort(Some("low"), Some("low"), None),
        EffortEvidence::ObservedInProviderRequest("low".into())
    );
    assert_eq!(
        effort(Some("xhigh"), Some("high"), None),
        EffortEvidence::Requested("xhigh".into()),
        "the provider request carried something else"
    );
    assert_eq!(
        effort(Some("low"), Some("low"), Some("low")),
        EffortEvidence::ProviderReported("low".into())
    );
    assert_eq!(
        effort(Some("xhigh"), None, Some("high")),
        EffortEvidence::ProviderReported("high".into())
    );
}

#[test]
fn the_four_facts_are_separate_and_the_catalogue_is_never_an_entitlement() {
    let cat = catalogue(Provider::Claude, 4, NOW);
    let configured = Some(req("opus", None));
    let requested = Some(Requested {
        turn_id: "t1".into(),
        request: req("sonnet", None),
        resolved_model: "claude-sonnet-5-5".into(),
        generation: 2,
    });
    let reported: BTreeSet<String> = ["claude-haiku-5-5".to_owned()].into();
    let f = facts(
        Some(&cat),
        NOW + 1,
        configured.clone(),
        requested,
        &reported,
        Some("medium"),
        None,
    );
    let offered = f.offered.as_ref().unwrap();
    assert_eq!(offered.entitlement, Entitlement::NotChecked);
    assert_eq!((offered.revision, offered.freshness), (4, Freshness::Fresh));
    assert_eq!(f.configured, configured);
    assert_eq!(
        f.requested.as_ref().unwrap().resolved_model,
        "claude-sonnet-5-5"
    );
    assert_eq!(f.reported_in_use, ["claude-haiku-5-5"]);
    assert!(matches!(f.reroute, Reroute::Rerouted { .. }));
    // Nothing requested an effort, so the default the CLI sent is not presented as chosen.
    assert_eq!(f.effort, EffortEvidence::NotRequested);
    let json = serde_json::to_string(&f).unwrap();
    assert!(
        json.contains("\"entitlement\":\"not_checked\"") && json.contains("\"state\":\"rerouted\""),
        "{json}"
    );
}

#[test]
fn facts_for_a_runtime_with_no_catalogue_yet_say_so() {
    let f = facts(None, NOW, None, None, &BTreeSet::new(), None, None);
    assert!(f.offered.is_none() && f.configured.is_none() && f.requested.is_none());
    assert_eq!(
        (f.reroute, f.effort),
        (Reroute::NotRequested, EffortEvidence::NotRequested)
    );
    assert!(f.reported_in_use.is_empty());
}

#[test]
fn a_stale_catalogue_is_shown_as_stale_not_hidden() {
    let cat = catalogue(Provider::Claude, 1, NOW);
    let f = facts(
        Some(&cat),
        NOW + CATALOGUE_MAX_AGE_SECS + 1,
        None,
        None,
        &BTreeSet::new(),
        None,
        None,
    );
    assert_eq!(f.offered.unwrap().freshness, Freshness::Stale);
}

#[test]
fn capabilities_differ_by_provider_and_the_codex_row_claims_no_support_today() {
    const {
        assert!(
            Caps::CLAUDE.model_per_turn
                && !Caps::CLAUDE.effort_per_turn
                && Caps::CLAUDE.effort_per_runtime
        );
        assert!(
            Caps::CODEX.model_per_turn
                && Caps::CODEX.effort_per_turn
                && !Caps::CODEX.effort_per_runtime
        );
    }
    assert_eq!(Caps::of(Provider::Claude), Caps::CLAUDE);
    // The capability is versioned on its own and is not the authority's protocol number.
    assert_eq!((CAPABILITY, CAPABILITY_VERSION), ("m3.model_selection", 1));
}

#[test]
fn refusal_codes_are_unique_snake_case() {
    use Refusal::*;
    let all = [
        GateClosed,
        InvalidWord,
        EmptyRequest,
        ProviderMismatch,
        BuildMismatch,
        SessionMismatch,
        RuntimeMismatch,
        GenerationMismatch,
        CatalogueIncomplete,
        CatalogueStale,
        CatalogueChanged,
        UnknownModel,
        UnsupportedEffort,
        EffortNotPerTurn,
        ModelNotPerTurn,
    ];
    let codes: BTreeSet<_> = all.iter().map(|r| r.code()).collect();
    assert_eq!(codes.len(), all.len());
    assert!(
        codes
            .iter()
            .all(|c| c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
    );
}
