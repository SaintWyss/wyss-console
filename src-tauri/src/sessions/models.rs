//! What the managed Codex runtime offers (F6a, MV-02/11/12): one read of its catalogue, with
//! what the runtime's own listing says about each entry, and where and when it was read.
//!
//! A listing is not entitlement: the runtime lists models it can name, and nothing here says
//! an account may use one. A default, an effort or a replacement is shown **as the runtime
//! marked it**, never as what a session uses; that is the observed model, read elsewhere.
//! The runtime's own prose (upgrade copy, descriptions) is never carried.
use super::{
    service::SessionService,
    view::{AgentMode, RuntimeFacts, SessionError, SessionFailure},
};
use crate::contracts::control::{
    ActualState, ControlError, ModelOffer, ResultValue, RuntimeModels, RuntimeTarget,
};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

pub const CATALOGUE_SCHEMA_VERSION: u8 = 1;
/// Protocol pages one read walks at most (twenty entries each; the authority holds at most fifty).
const WALK_PAGES: usize = 4;

/// Whether the managed runtime that was asked is there to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogueState {
    /// Ready: the entries are what it listed when it started.
    Listed,
    /// Not running or not ready. A catalogue of a runtime that has stopped is never shown.
    NotReady,
}

/// One model the runtime listed. `None` for default, efforts and replacement means "the
/// authority did not report it" (an earlier version), which is not "no".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogueEntry {
    pub model_id: String,
    pub display_name: Option<String>,
    pub provider_id: Option<String>,
    /// The runtime marks it as its default.
    pub is_default: Option<bool>,
    /// The effort the runtime marks as this model's default.
    pub default_effort: Option<String>,
    /// The efforts the runtime says this model supports.
    pub efforts: Option<Vec<String>>,
    /// The model the runtime marks as replacing this one, as an identifier.
    pub replaced_by: Option<String>,
    /// The date the runtime gives for retirement, as it gave it.
    pub retires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelCatalogue {
    pub schema_version: u8,
    pub state: CatalogueState,
    /// The mode of the generation that listed it, while it is ready.
    pub mode: Option<AgentMode>,
    /// The runtime's own version string, as it reported it.
    pub runtime_version: Option<String>,
    /// `Some(false)`: the runtime's listing continued past what the authority took.
    /// `None`: not reported.
    pub complete: Option<bool>,
    pub entries: Vec<CatalogueEntry>,
    /// When this desktop read it from the authority (Unix seconds).
    pub read_at: u64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// The descriptor's fields by name: its own accessors are not public, its encoding is.
fn entry_of(model: &serde_json::Value, offer: Option<&ModelOffer>) -> Option<CatalogueEntry> {
    let text = |key: &str| {
        model
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    Some(CatalogueEntry {
        model_id: text("model_id")?,
        display_name: text("display_name"),
        provider_id: text("provider_id"),
        is_default: offer.map(|o| o.is_default),
        default_effort: offer.and_then(|o| o.default_effort.clone()),
        efforts: offer.map(|o| o.efforts.clone()),
        replaced_by: offer.and_then(|o| o.replaced_by.clone()),
        retires_at: offer.and_then(|o| o.retires_at.clone()),
    })
}

impl SessionService {
    /// One reading of the managed runtime's catalogue: its state first, then its pages.
    pub fn model_catalogue(&self) -> Result<ModelCatalogue, SessionError> {
        self.call(|client| {
            let status = client.runtime_status().map_err(SessionError::of)?;
            let facts = RuntimeFacts::of(&status);
            if status.actual_state != ActualState::Ready {
                return Ok(ModelCatalogue {
                    schema_version: CATALOGUE_SCHEMA_VERSION,
                    state: CatalogueState::NotReady,
                    mode: None,
                    runtime_version: None,
                    complete: None,
                    entries: Vec::new(),
                    read_at: now(),
                });
            }
            let target = RuntimeTarget::codex();
            let mut entries = Vec::new();
            let mut complete = None;
            let mut cursor = None;
            for _ in 0..WALK_PAGES {
                let page = client.runtime_models(RuntimeModels {
                    adapter_id: target.adapter_id.clone(),
                    runtime_kind: target.runtime_kind,
                    cursor: cursor.take(),
                    limit: 20,
                });
                let page = match page {
                    Ok(page) => page,
                    // The runtime stopped between the status and the page: nothing is shown
                    // of a catalogue whose runtime has gone, and the rest is not guessed.
                    Err(ControlError::InvalidTransition | ControlError::StaleRevision) => {
                        return Ok(ModelCatalogue {
                            schema_version: CATALOGUE_SCHEMA_VERSION,
                            state: CatalogueState::NotReady,
                            mode: None,
                            runtime_version: None,
                            complete: None,
                            entries: Vec::new(),
                            read_at: now(),
                        });
                    }
                    Err(error) => return Err(SessionError::of(error)),
                };
                let ResultValue::RuntimeModels {
                    models,
                    offers,
                    complete: whole,
                    next,
                    ..
                } = page
                else {
                    return Err(SessionError::new(SessionFailure::Unavailable));
                };
                complete = whole;
                for (index, model) in models.iter().enumerate() {
                    let value = serde_json::to_value(model)
                        .map_err(|_| SessionError::new(SessionFailure::Unavailable))?;
                    if let Some(entry) = entry_of(&value, offers.get(index)) {
                        entries.push(entry);
                    }
                }
                match next {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
            // More pages than the walk takes is an incomplete catalogue, said as such.
            if cursor.is_some() {
                complete = Some(false);
            }
            Ok(ModelCatalogue {
                schema_version: CATALOGUE_SCHEMA_VERSION,
                state: CatalogueState::Listed,
                mode: facts.mode,
                runtime_version: status.runtime_version.clone(),
                complete,
                entries,
                read_at: now(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_entry_carries_what_the_runtime_marked_and_none_of_its_prose() {
        let model = json!({"schema_version":1,"provider_id":"custom","model_id":"m-1",
            "display_name":"M One","availability":"unknown"});
        let offer = ModelOffer {
            is_default: true,
            default_effort: Some("medium".into()),
            efforts: vec!["low".into(), "medium".into()],
            replaced_by: Some("m-2".into()),
            retires_at: Some("2027-01-31T00:00:00Z".into()),
        };
        let entry = entry_of(&model, Some(&offer)).unwrap();
        assert_eq!(entry.model_id, "m-1");
        assert_eq!(entry.display_name.as_deref(), Some("M One"));
        assert_eq!(entry.provider_id.as_deref(), Some("custom"));
        assert_eq!(entry.is_default, Some(true));
        assert_eq!(entry.efforts.as_deref().map(<[_]>::len), Some(2));
        assert_eq!(entry.replaced_by.as_deref(), Some("m-2"));
        // Availability is not carried at all: nothing says the account may use it.
        assert!(
            !serde_json::to_string(&entry)
                .unwrap()
                .contains("availability")
        );
        // An authority that reported no offer says nothing, which is not "not the default".
        let bare = entry_of(&model, None).unwrap();
        assert_eq!(
            (bare.is_default, bare.efforts, bare.default_effort),
            (None, None, None)
        );
        // No identifier, no entry.
        assert!(entry_of(&json!({"display_name":"x"}), None).is_none());
    }
}

#[cfg(test)]
mod service_tests {
    use super::*;
    use crate::contracts::control::Operation;
    use crate::sessions::tests::{Peer, answer, runtime_status, uuid};
    use serde_json::json;

    fn descriptor(name: &str) -> crate::contracts::agent_adapter::AgentModelDescriptorV1 {
        crate::contracts::agent_adapter::AgentModelDescriptorV1::from_value(json!({
            "schema_version": 1, "provider_id": null, "model_id": name,
            "display_name": format!("{name} name"), "availability": "unknown"
        }))
        .unwrap()
    }
    fn offer(default: bool) -> ModelOffer {
        ModelOffer {
            is_default: default,
            default_effort: Some("medium".into()),
            efforts: vec!["low".into(), "medium".into()],
            replaced_by: None,
            retires_at: None,
        }
    }

    #[test]
    fn a_runtime_that_is_not_ready_has_no_catalogue_and_no_page_is_asked_for() {
        let peer = Peer::canonical(|op| match op {
            Operation::RuntimeStatus(_) => answer(Ok(runtime_status(None, None))),
            other => panic!("{other:?}"),
        });
        let catalogue = peer.service().model_catalogue().unwrap();
        assert_eq!(catalogue.state, CatalogueState::NotReady);
        assert!(catalogue.entries.is_empty() && catalogue.complete.is_none());
        assert_eq!(peer.methods(), vec!["runtime/status".to_owned()]);
    }

    #[test]
    fn the_pages_of_a_ready_runtime_are_read_with_their_offers_and_say_whether_they_are_whole() {
        let id =
            crate::contracts::identities::AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
        let mut served = 0u16;
        let peer = Peer::canonical(move |op| match op {
            Operation::RuntimeStatus(_) => {
                answer(Ok(runtime_status(Some("codex_inference_v1"), None)))
            }
            Operation::RuntimeModels(p) => {
                assert_eq!(p.limit, 20);
                let (names, next): (Vec<String>, _) = if p.cursor.is_none() {
                    (
                        (0..20).map(|n| format!("m{n}")).collect(),
                        Some(crate::contracts::control::ModelCursor {
                            runtime_instance_id: id.clone(),
                            index: 20,
                        }),
                    )
                } else {
                    served += 1;
                    (vec!["last".to_owned()], None)
                };
                answer(Ok(ResultValue::RuntimeModels {
                    runtime_instance_id: id.clone(),
                    offers: names.iter().map(|n| offer(n == "m0")).collect(),
                    models: names.iter().map(|n| descriptor(n)).collect(),
                    complete: Some(false),
                    next,
                }))
            }
            other => panic!("{other:?} {served}"),
        });
        let catalogue = peer.service().model_catalogue().unwrap();
        assert_eq!(catalogue.state, CatalogueState::Listed);
        assert_eq!(catalogue.mode, Some(AgentMode::Inference));
        assert_eq!(catalogue.runtime_version.as_deref(), Some("0.159.0"));
        assert_eq!(catalogue.entries.len(), 21);
        assert_eq!(catalogue.entries[0].is_default, Some(true));
        assert_eq!(catalogue.entries[1].is_default, Some(false));
        assert_eq!(catalogue.entries[20].model_id, "last");
        assert_eq!(
            catalogue.entries[0].display_name.as_deref(),
            Some("m0 name")
        );
        // The runtime's own listing continued past what the authority took: said, not hidden.
        assert_eq!(catalogue.complete, Some(false));
        // Everything about it is a read: status, then two pages, nothing else.
        assert_eq!(
            peer.methods(),
            vec!["runtime/status", "runtime/model/list", "runtime/model/list"]
        );
    }

    #[test]
    fn an_authority_that_reports_no_offers_gives_entries_that_claim_nothing() {
        let id =
            crate::contracts::identities::AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
        let peer = Peer::canonical(move |op| match op {
            Operation::RuntimeStatus(_) => {
                answer(Ok(runtime_status(Some("codex_inference_v1"), None)))
            }
            Operation::RuntimeModels(_) => answer(Ok(ResultValue::RuntimeModels {
                runtime_instance_id: id.clone(),
                models: vec![descriptor("only")],
                offers: vec![],
                complete: None,
                next: None,
            })),
            other => panic!("{other:?}"),
        });
        let catalogue = peer.service().model_catalogue().unwrap();
        let entry = &catalogue.entries[0];
        assert_eq!(
            (entry.is_default, &entry.efforts, &entry.default_effort),
            (None, &None, &None)
        );
        assert_eq!(catalogue.complete, None);
    }

    #[test]
    fn a_runtime_that_stops_between_the_status_and_a_page_leaves_no_catalogue() {
        let peer = Peer::canonical(|op| match op {
            Operation::RuntimeStatus(_) => {
                answer(Ok(runtime_status(Some("codex_inference_v1"), None)))
            }
            Operation::RuntimeModels(_) => answer(Err(ControlError::InvalidTransition)),
            other => panic!("{other:?}"),
        });
        let catalogue = peer.service().model_catalogue().unwrap();
        assert_eq!(catalogue.state, CatalogueState::NotReady);
        assert!(catalogue.entries.is_empty());
    }

    #[test]
    fn a_listing_that_never_ends_is_walked_to_its_bound_and_called_incomplete() {
        let id =
            crate::contracts::identities::AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
        let peer = Peer::canonical(move |op| match op {
            Operation::RuntimeStatus(_) => {
                answer(Ok(runtime_status(Some("codex_inference_v1"), None)))
            }
            Operation::RuntimeModels(_) => answer(Ok(ResultValue::RuntimeModels {
                runtime_instance_id: id.clone(),
                models: vec![descriptor("x")],
                offers: vec![],
                complete: Some(true),
                next: Some(crate::contracts::control::ModelCursor {
                    runtime_instance_id: id.clone(),
                    index: 1,
                }),
            })),
            other => panic!("{other:?}"),
        });
        let catalogue = peer.service().model_catalogue().unwrap();
        assert_eq!(catalogue.entries.len(), WALK_PAGES);
        assert_eq!(catalogue.complete, Some(false));
        assert_eq!(peer.methods().len(), 1 + WALK_PAGES);
    }

    #[test]
    fn the_catalogue_serializes_exactly_the_windows_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/model-catalogue-v1.json"
        ))
        .unwrap();
        // One entry, built from the same parts the fixture names.
        struct Offer<'a> {
            default: bool,
            effort: &'a str,
            efforts: &'a [&'a str],
            replaced_by: Option<&'a str>,
            retires_at: Option<&'a str>,
        }
        let entry = |id: &str, name: Option<&str>, provider: Option<&str>, offer: Option<Offer>| {
            CatalogueEntry {
                model_id: id.into(),
                display_name: name.map(Into::into),
                provider_id: provider.map(Into::into),
                is_default: offer.as_ref().map(|o| o.default),
                default_effort: offer.as_ref().map(|o| o.effort.to_owned()),
                efforts: offer
                    .as_ref()
                    .map(|o| o.efforts.iter().map(|e| (*e).to_owned()).collect()),
                replaced_by: offer.as_ref().and_then(|o| o.replaced_by.map(Into::into)),
                retires_at: offer.as_ref().and_then(|o| o.retires_at.map(Into::into)),
            }
        };
        let listed = ModelCatalogue {
            schema_version: CATALOGUE_SCHEMA_VERSION,
            state: CatalogueState::Listed,
            mode: Some(AgentMode::Inference),
            runtime_version: Some("0.159.0".into()),
            complete: Some(true),
            entries: vec![
                entry(
                    "model-a",
                    Some("Model A"),
                    None,
                    Some(Offer {
                        default: true,
                        effort: "medium",
                        efforts: &["low", "medium", "high"],
                        replaced_by: None,
                        retires_at: None,
                    }),
                ),
                entry(
                    "model-b",
                    Some("Model B (older)"),
                    Some("custom"),
                    Some(Offer {
                        default: false,
                        effort: "high",
                        efforts: &["high"],
                        replaced_by: Some("model-a"),
                        retires_at: Some("2027-01-31T00:00:00Z"),
                    }),
                ),
            ],
            read_at: 1_792_466_400,
        };
        assert_eq!(serde_json::to_value(&listed).unwrap(), fixture["listed"]);
        let partial = ModelCatalogue {
            schema_version: CATALOGUE_SCHEMA_VERSION,
            state: CatalogueState::Listed,
            mode: Some(AgentMode::LocalTools),
            runtime_version: None,
            complete: Some(false),
            entries: vec![entry("model-only", None, None, None)],
            read_at: 1_792_466_401,
        };
        assert_eq!(
            serde_json::to_value(&partial).unwrap(),
            fixture["partial_without_offers"]
        );
        let not_ready = ModelCatalogue {
            schema_version: CATALOGUE_SCHEMA_VERSION,
            state: CatalogueState::NotReady,
            mode: None,
            runtime_version: None,
            complete: None,
            entries: vec![],
            read_at: 1_792_466_402,
        };
        assert_eq!(
            serde_json::to_value(&not_ready).unwrap(),
            fixture["not_ready"]
        );
    }
}
