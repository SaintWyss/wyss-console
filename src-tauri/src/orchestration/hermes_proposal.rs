//! Closed, memory-only boundary for an untrusted Hermes planning proposal.
//!
//! This intentionally accepts descriptive plan data only. It has no shell/action/tool field,
//! performs no provider call, cannot create an OL3 plan and cannot reach an execution adapter.
//! Effectful planner admission remains blocked until the selected Wyss Agent boundary is proven.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const PROPOSAL_SCHEMA_VERSION: u8 = 1;
pub const MAX_PROPOSAL_BYTES: usize = 24 * 1024;
pub const MAX_PROPOSAL_STEPS: usize = 16;
pub const MAX_PROPOSAL_TEXT_BYTES: usize = 16 * 1024;
pub const MAX_STEP_TEXT_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HermesProposalV1 {
    pub schema_version: u8,
    pub objective: String,
    pub steps: Vec<ProposedStepV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedStepV1 {
    /// Proposal-local label; it does not select a provider, session or execution authority.
    pub step_id: String,
    pub summary: String,
    pub rationale: String,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalError {
    TooLarge,
    InvalidJson,
    InvalidContract,
    Cycle,
}

/// Parses provider output as a candidate for explicit human review. It is not a policy decision.
pub fn parse_untrusted_candidate(bytes: &[u8]) -> Result<HermesProposalV1, ProposalError> {
    if bytes.is_empty() || bytes.len() > MAX_PROPOSAL_BYTES {
        return Err(ProposalError::TooLarge);
    }
    let proposal: HermesProposalV1 =
        serde_json::from_slice(bytes).map_err(|_| ProposalError::InvalidJson)?;
    validate(&proposal)?;
    Ok(proposal)
}

fn validate(proposal: &HermesProposalV1) -> Result<(), ProposalError> {
    if proposal.schema_version != PROPOSAL_SCHEMA_VERSION
        || proposal.objective.trim().is_empty()
        || proposal.objective.contains('\0')
        || proposal.objective.len() > MAX_STEP_TEXT_BYTES
        || proposal.steps.is_empty()
        || proposal.steps.len() > MAX_PROPOSAL_STEPS
    {
        return Err(ProposalError::InvalidContract);
    }
    let mut total = proposal.objective.len();
    let mut ids = BTreeSet::new();
    for step in &proposal.steps {
        if !label_ok(&step.step_id, 64)
            || !ids.insert(step.step_id.as_str())
            || !text_ok(&step.summary)
            || !text_ok(&step.rationale)
            || step.depends_on.len() > MAX_PROPOSAL_STEPS
        {
            return Err(ProposalError::InvalidContract);
        }
        total = total
            .saturating_add(step.summary.len())
            .saturating_add(step.rationale.len());
        let mut deps = BTreeSet::new();
        for dependency in &step.depends_on {
            if !label_ok(dependency, 64)
                || dependency == &step.step_id
                || !deps.insert(dependency.as_str())
            {
                return Err(ProposalError::InvalidContract);
            }
        }
    }
    if total > MAX_PROPOSAL_TEXT_BYTES {
        return Err(ProposalError::TooLarge);
    }
    let by_id: BTreeMap<&str, &ProposedStepV1> = proposal
        .steps
        .iter()
        .map(|step| (step.step_id.as_str(), step))
        .collect();
    let mut indegree: BTreeMap<&str, usize> = ids.iter().map(|id| (*id, 0)).collect();
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for step in &proposal.steps {
        for dependency in &step.depends_on {
            if !by_id.contains_key(dependency.as_str()) {
                return Err(ProposalError::InvalidContract);
            }
            *indegree
                .get_mut(step.step_id.as_str())
                .ok_or(ProposalError::InvalidContract)? += 1;
            children
                .entry(dependency.as_str())
                .or_default()
                .push(step.step_id.as_str());
        }
    }
    let mut ready: VecDeque<&str> = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect();
    let mut visited = 0;
    while let Some(id) = ready.pop_front() {
        visited += 1;
        if let Some(next) = children.get(id) {
            for child in next {
                let degree = indegree
                    .get_mut(child)
                    .ok_or(ProposalError::InvalidContract)?;
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(child);
                }
            }
        }
    }
    if visited != proposal.steps.len() {
        return Err(ProposalError::Cycle);
    }
    Ok(())
}

fn text_ok(text: &str) -> bool {
    !text.trim().is_empty() && !text.contains('\0') && text.len() <= MAX_STEP_TEXT_BYTES
}

fn label_ok(text: &str, max: usize) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &[u8] = br#"{
      "schema_version": 1,
      "objective": "Review the change",
      "steps": [
        {"step_id":"inspect","summary":"Inspect files","rationale":"Find relevant code","depends_on":[]},
        {"step_id":"report","summary":"Report findings","rationale":"Summarize observations","depends_on":["inspect"]}
      ]
    }"#;

    #[test]
    fn accepts_only_bounded_descriptive_dag_data() {
        let proposal = parse_untrusted_candidate(VALID).unwrap();
        assert_eq!(proposal.steps.len(), 2);
        assert_eq!(proposal.steps[1].depends_on, ["inspect"]);
    }

    #[test]
    fn rejects_tool_fields_unknown_fields_and_effectful_extensions() {
        for hostile in [
            br#"{"schema_version":1,"objective":"x","steps":[],"tool_call":"shell.run"}"#.as_slice(),
            br#"{"schema_version":1,"objective":"x","steps":[{"step_id":"x","summary":"run this","rationale":"x","depends_on":[],"command":"rm -rf /"}]}"#.as_slice(),
        ] {
            assert_eq!(
                parse_untrusted_candidate(hostile),
                Err(ProposalError::InvalidJson)
            );
        }
    }

    #[test]
    fn rejects_dangling_duplicate_and_cyclic_dependencies() {
        let dangling = br#"{"schema_version":1,"objective":"x","steps":[{"step_id":"a","summary":"a","rationale":"a","depends_on":["missing"]}]}"#;
        let duplicate = br#"{"schema_version":1,"objective":"x","steps":[{"step_id":"a","summary":"a","rationale":"a","depends_on":["a"]}]}"#;
        let cycle = br#"{"schema_version":1,"objective":"x","steps":[{"step_id":"a","summary":"a","rationale":"a","depends_on":["b"]},{"step_id":"b","summary":"b","rationale":"b","depends_on":["a"]}]}"#;
        assert_eq!(
            parse_untrusted_candidate(dangling),
            Err(ProposalError::InvalidContract)
        );
        assert_eq!(
            parse_untrusted_candidate(duplicate),
            Err(ProposalError::InvalidContract)
        );
        assert_eq!(parse_untrusted_candidate(cycle), Err(ProposalError::Cycle));
    }

    #[test]
    fn enforces_exact_size_boundaries_before_json_work() {
        assert_eq!(parse_untrusted_candidate(&[]), Err(ProposalError::TooLarge));
        let oversized = vec![b' '; MAX_PROPOSAL_BYTES + 1];
        assert_eq!(
            parse_untrusted_candidate(&oversized),
            Err(ProposalError::TooLarge)
        );
        let objective = "x".repeat(MAX_STEP_TEXT_BYTES + 1);
        let json = serde_json::to_vec(&HermesProposalV1 {
            schema_version: 1,
            objective,
            steps: vec![ProposedStepV1 {
                step_id: "s".into(),
                summary: "summary".into(),
                rationale: "why".into(),
                depends_on: vec![],
            }],
        })
        .unwrap();
        assert_eq!(
            parse_untrusted_candidate(&json),
            Err(ProposalError::InvalidContract)
        );
    }
}
