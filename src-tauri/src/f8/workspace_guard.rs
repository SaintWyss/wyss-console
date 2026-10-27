//! Authority-owned workspace directory pins for the guarded M6 path.
//!
//! File handles are used only to compare live objects while F8 retains the corresponding
//! descriptors. They are not a recovery credential. A process restart loses the pin registry
//! and therefore leaves persisted claims unproven.
#[cfg(target_os = "linux")]
mod linux {
    use crate::contracts::control::{ControlError as E, ControlResult};
    use nix::{
        dir::Dir,
        errno::Errno,
        fcntl::{OFlag, openat, readlinkat},
        sys::stat::Mode,
        sys::statfs::{BTRFS_SUPER_MAGIC, EXT4_SUPER_MAGIC, fstatfs},
    };
    use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeMap,
        ffi::CString,
        fs::File,
        io::Read,
        os::{
            fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
            unix::{
                ffi::{OsStrExt, OsStringExt},
                fs::MetadataExt,
            },
        },
        path::{Path, PathBuf},
        sync::Arc,
        time::{Duration, Instant},
    };
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    const MAX_ANCESTORS: usize = 64;
    // Released history does not occupy a live run slot. All other claim phases do,
    // including verified-but-unreleased and quarantined ownership.
    const MAX_UNRELEASED_CLAIMS: i64 = 128;
    const MAX_UNCLAIMED_ROOTS: i64 = 128;
    const MAX_HANDLE_BYTES: usize = 128;
    const MAX_SNAPSHOT_ENTRIES: usize = 20_000;
    const MAX_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
    const MAX_SNAPSHOT_DEPTH: usize = 64;
    const MAX_SNAPSHOT_DURATION: Duration = Duration::from_secs(10);
    const AT_EMPTY_PATH: i32 = 0x1000;
    const RESOLVE_NO_MAGICLINKS: u64 = 0x02;

    type VerificationObservationRow = (String, i64, i64, String, String, String, String, String);

    #[repr(C)]
    struct OpenHow {
        flags: u64,
        mode: u64,
        resolve: u64,
    }

    // Linux's flexible-array `struct file_handle`, with a bounded buffer. The handle bytes
    // remain opaque; this code never interprets their filesystem-specific layout.
    #[repr(C)]
    struct FileHandle {
        handle_bytes: u32,
        handle_type: i32,
        bytes: [u8; MAX_HANDLE_BYTES],
    }

    #[derive(Clone, PartialEq, Eq)]
    struct ObjectIdentity {
        filesystem: [u8; 32],
        object: [u8; 32],
        mount_id: i32,
    }

    /// A live F8-owned directory object and the bounded ancestry proof observed with it.
    /// The locator is transient in-memory state and is never serialized by this module.
    #[derive(Clone)]
    pub struct RootPin {
        fd: Arc<File>,
        pub root_binding_id: String,
        pub authority_store_id: String,
        pub authority_epoch: String,
        pub proof_revision: u64,
        locator: PathBuf,
        root: ObjectIdentity,
        ancestry: Vec<ObjectIdentity>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Overlap {
        Conflict,
        Disjoint,
        Unproven,
    }

    impl RootPin {
        /// Resolve a locator once with magic links refused, then retain the opened directory.
        /// Ordinary symlinks resolve to their target; subsequent checks compare the resulting
        /// live object and never treat the path text as identity evidence.
        pub fn open(
            locator: &Path,
            authority_store_id: impl Into<String>,
            authority_epoch: impl Into<String>,
            root_binding_id: impl Into<String>,
        ) -> ControlResult<Self> {
            let fd = open_directory(locator)?;
            let fd = Arc::new(fd);
            let ancestry = ancestry(&fd)?;
            let root = ancestry.first().cloned().ok_or(E::Unsupported)?;
            let authority_store_id = authority_store_id.into();
            let authority_epoch = authority_epoch.into();
            let root_binding_id = root_binding_id.into();
            for id in [&authority_store_id, &authority_epoch, &root_binding_id] {
                super::super::protocol::canonical(id)?;
            }
            Ok(Self {
                fd,
                root_binding_id,
                authority_store_id,
                authority_epoch,
                proof_revision: 1,
                locator: locator.to_path_buf(),
                root,
                ancestry,
            })
        }

        /// Duplicate the retained descriptor for a trusted launch boundary.
        pub fn duplicate_fd(&self) -> ControlResult<File> {
            self.fd.try_clone().map_err(|_| E::StorageUnavailable)
        }

        /// The current locator still resolves to the object F8 pinned. Failure is closed;
        /// callers must seal the claim gate and retain the pin.
        pub fn locator_matches(&self) -> ControlResult<bool> {
            let current = open_directory(&self.locator)?;
            let identity = object_identity(&current)?;
            Ok(identity.object == self.root.object
                && identity.filesystem == self.root.filesystem
                && identity.mount_id == self.root.mount_id
                && self.current_ancestry()? == self.ancestry)
        }

        /// Re-observe the parents of the retained object, never of a replacement locator.
        /// Two equal bounded walks reject topology changing during the observation. This is
        /// cooperating-writer evidence, not a lock against arbitrary host filesystem actors.
        fn current_ancestry(&self) -> ControlResult<Vec<ObjectIdentity>> {
            let first = ancestry(&self.fd)?;
            let second = ancestry(&self.fd)?;
            if first != second || first.first() != Some(&self.root) {
                return Err(E::WorkspaceInvalid);
            }
            Ok(first)
        }

        /// Hash the reviewed tree through the retained directory descriptor. This deliberately
        /// does not reopen the locator: a renamed pinned root remains reviewable by its owner,
        /// while a replacement at the old path cannot supply the reviewed bytes.
        pub fn content_digest(&self) -> ControlResult<String> {
            let mut digest = Sha256::new();
            let mut entries_seen = 0usize;
            let mut bytes_seen = 0u64;
            let deadline = Instant::now() + MAX_SNAPSHOT_DURATION;
            snapshot_directory(
                &self.fd,
                Path::new(""),
                0,
                &mut digest,
                &mut entries_seen,
                &mut bytes_seen,
                deadline,
            )?;
            Ok(hex(&digest.finalize()))
        }

        pub fn same_live_object(&self, other: &Self) -> bool {
            self.root.filesystem == other.root.filesystem && self.root.object == other.root.object
        }

        /// Parent/child roots conflict; siblings on the same qualified mount remain independent.
        /// Different mounts and unsupported topology are not assumed disjoint.
        pub fn overlap(&self, other: &Self) -> Overlap {
            let (Ok(ours), Ok(theirs)) = (self.current_ancestry(), other.current_ancestry()) else {
                return Overlap::Unproven;
            };
            if self.same_live_object(other)
                || ours.iter().any(|ancestor| {
                    ancestor.filesystem == other.root.filesystem
                        && ancestor.object == other.root.object
                })
                || theirs.iter().any(|ancestor| {
                    ancestor.filesystem == self.root.filesystem
                        && ancestor.object == self.root.object
                })
            {
                return Overlap::Conflict;
            }
            if self.root.filesystem != other.root.filesystem
                || self.root.mount_id != other.root.mount_id
            {
                return Overlap::Unproven;
            }
            Overlap::Disjoint
        }

        /// Exact locator reuse is a conflict while this pin remains unresolved, including after
        /// the name is replaced by an equal-content directory.
        pub fn same_locator(&self, other: &Self) -> bool {
            self.locator == other.locator
        }

        pub fn object_fingerprint(&self) -> String {
            hex(&self.root.object)
        }

        pub fn filesystem_fingerprint(&self) -> String {
            hex(&self.root.filesystem)
        }

        pub fn mount_id(&self) -> i32 {
            self.root.mount_id
        }

        pub fn ancestry_fingerprint(&self) -> String {
            let mut digest = Sha256::new();
            for item in &self.ancestry {
                digest.update(item.filesystem);
                digest.update(item.object);
                digest.update(item.mount_id.to_le_bytes());
            }
            hex(&digest.finalize())
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct ImportedClaim {
        pub claim_id: String,
        pub run_id: String,
        pub claim_generation: u64,
        pub source_revision: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Claim {
        pub claim_id: String,
        pub root_binding_id: String,
        pub claim_generation: u64,
        pub claim_revision: u64,
        pub gate_revision: u64,
        pub phase: String,
        pub gate_state: String,
        pub settlement_id: Option<String>,
        pub coordinator_revision: u64,
    }

    #[derive(Debug, Clone)]
    pub struct PreparedStart {
        pub receipt_id: String,
        pub receipt_revision: u64,
        pub expires_at: String,
        pub tuple_digest: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PreparedVerification {
        pub observation_id: String,
        pub claim_id: String,
        pub claim_generation: u64,
        pub claim_revision: u64,
        pub root_binding_id: String,
        pub content_digest: String,
    }

    /// F8's process-local pins and durable claim ledger. The locator remains only in each live
    /// `RootPin`; it is never written to SQLite. Constructing a new guard means the authority
    /// descriptor set was lost, so every persisted nonterminal claim is quarantined atomically.
    pub struct WorkspaceGuard {
        store_id: String,
        epoch: String,
        pins: BTreeMap<String, RootPin>,
        prepared_start_deadlines: BTreeMap<String, (Instant, String)>,
    }

    impl WorkspaceGuard {
        pub fn open(conn: &mut Connection, epoch: impl Into<String>) -> ControlResult<Self> {
            let epoch = epoch.into();
            super::super::protocol::canonical(&epoch)?;
            super::super::v10::validate_v10(conn)?;
            let store_id: String = conn
                .query_row(
                    "SELECT authority_store_id FROM workspace_guard_state WHERE id=1",
                    [],
                    |row| row.get(0),
                )
                .map_err(|_| E::StorageIncompatible)?;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            tx.execute(
                "INSERT INTO authority_epoch_history(authority_epoch,started_at) VALUES(?1,?2)",
                params![epoch, now()?],
            )
            .map_err(|_| E::StorageIncompatible)?;
            tx.execute(
                "UPDATE guarded_start_receipts SET state='invalidated',receipt_revision=receipt_revision+1 WHERE state='prepared'",
                [],
            )
            .map_err(|_| E::StorageUnavailable)?;
            // An observation or fully closed history needs no physical proof to retire.
            // Anything still referenced by uncertain work keeps its proof-loss barrier.
            retire_roots_in(&tx, true)?;
            tx.execute(
                "UPDATE workspace_root_bindings SET proof_state='identity_unproven',proof_revision=proof_revision+1 WHERE proof_state='pinned'",
                [],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.execute(
                "UPDATE guarded_attempts SET send_state='needs_review' WHERE send_state IN ('reserved','possibly_sent','verifying')",
                [],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.execute(
                "UPDATE workspace_claims SET phase='quarantined',gate_state='quarantined',quarantine_reason='authority_identity_lost',claim_revision=claim_revision+1,gate_revision=gate_revision+1 WHERE phase<>'released'",
                [],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            Ok(Self {
                store_id,
                epoch,
                pins: BTreeMap::new(),
                prepared_start_deadlines: BTreeMap::new(),
            })
        }

        pub fn store_id(&self) -> &str {
            &self.store_id
        }

        pub fn epoch(&self) -> &str {
            &self.epoch
        }

        /// Drop a live directory reference only after all authority-owned uses are closed.
        /// Rows remain immutable identity history; retirement never reconstructs a pin.
        pub(crate) fn retire_released_roots(&mut self, conn: &mut Connection) -> ControlResult<()> {
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            tx.execute(
                "UPDATE guarded_start_receipts SET state='invalidated',receipt_revision=receipt_revision+1 WHERE state='prepared' AND claim_id IN (SELECT claim_id FROM workspace_claims WHERE phase='released')",
                [],
            ).map_err(|_|E::StorageUnavailable)?;
            let retired = retire_roots_in(&tx, false)?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            for root in retired {
                self.pins.remove(&root);
            }
            Ok(())
        }

        /// Locator/topology drift revokes dispatch, not the original directory reference.
        /// Commit this fence independently of a subsequently refused start transaction.
        pub(crate) fn refresh_dispatch_proofs(&self, conn: &mut Connection) -> ControlResult<()> {
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            for (root_id, pin) in &self.pins {
                let live: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM workspace_root_bindings WHERE root_binding_id=?1 AND authority_store_id=?2 AND authority_epoch=?3 AND proof_state='pinned' AND proof_revision=?4)",
                    params![root_id,self.store_id,self.epoch,pin.proof_revision as i64],
                    |row|row.get(0),
                ).map_err(|_|E::StorageUnavailable)?;
                if live && !pin.locator_matches().unwrap_or(false) {
                    tx.execute(
                        "UPDATE workspace_root_bindings SET proof_revision=proof_revision+1 WHERE root_binding_id=?1 AND proof_revision=?2",
                        params![root_id,pin.proof_revision as i64],
                    ).map_err(|_|E::StorageUnavailable)?;
                    tx.execute(
                        "UPDATE workspace_claims SET gate_state='stopped',claim_revision=claim_revision+1,gate_revision=gate_revision+1 WHERE root_binding_id=?1 AND phase NOT IN ('released','quarantined')",
                        [root_id],
                    ).map_err(|_|E::StorageUnavailable)?;
                    tx.execute(
                        "UPDATE guarded_start_receipts SET state='invalidated',receipt_revision=receipt_revision+1 WHERE root_binding_id=?1 AND state='prepared'",
                        [root_id],
                    ).map_err(|_|E::StorageUnavailable)?;
                }
            }
            tx.commit().map_err(|_| E::StorageUnavailable)
        }

        fn require_disjoint_claims(
            &self,
            conn: &Connection,
            root_id: &str,
            owning_claim: Option<&str>,
        ) -> ControlResult<()> {
            let pin = self.pins.get(root_id).ok_or(E::GateDenied)?;
            let mut stmt = conn
                .prepare(
                    "SELECT claim_id,root_binding_id FROM workspace_claims WHERE phase<>'released'",
                )
                .map_err(|_| E::StorageUnavailable)?;
            let claims = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|_| E::StorageUnavailable)?;
            for claim in claims {
                let (claim_id, other_root) = claim.map_err(|_| E::StorageUnavailable)?;
                if owning_claim == Some(claim_id.as_str()) {
                    continue;
                }
                let other = self.pins.get(&other_root).ok_or(E::GateDenied)?;
                if pin.same_locator(other) || pin.overlap(other) != Overlap::Disjoint {
                    return Err(E::WorkspaceInvalid);
                }
                if !other.locator_matches().unwrap_or(false) {
                    // A replacement at a retained locator is a conservative conflict
                    // domain too. Compare its live object, so spelling an alias of that
                    // replacement cannot bypass the fence. This observation never
                    // replaces the original pin or grants execution authority.
                    let locator_now = RootPin::open(
                        &other.locator,
                        self.store_id.clone(),
                        self.epoch.clone(),
                        crate::control::store::uuid().as_str().to_owned(),
                    )?;
                    if pin.overlap(&locator_now) != Overlap::Disjoint {
                        return Err(E::WorkspaceInvalid);
                    }
                }
            }
            Ok(())
        }

        /// Import the complete M6 unresolved-claim snapshot exactly once. Imported workspace
        /// identity is deliberately unproven; any nonempty snapshot keeps admission disabled.
        pub fn import_claims(
            &self,
            conn: &mut Connection,
            coordinator_store_id: &str,
            snapshot_revision: u64,
            claims: &[ImportedClaim],
        ) -> ControlResult<String> {
            super::super::protocol::canonical(coordinator_store_id)?;
            if snapshot_revision == 0 || claims.len() > 2048 {
                return Err(E::InvalidRequest);
            }
            let mut ordered = claims.to_vec();
            ordered.sort_by(|a, b| {
                (&a.claim_id, &a.claim_generation).cmp(&(&b.claim_id, &b.claim_generation))
            });
            let mut seen = std::collections::BTreeSet::new();
            let mut digest = Sha256::new();
            digest.update(coordinator_store_id.as_bytes());
            digest.update(snapshot_revision.to_le_bytes());
            for claim in &ordered {
                super::super::protocol::canonical(&claim.claim_id)?;
                super::super::protocol::canonical(&claim.run_id)?;
                if claim.claim_generation == 0
                    || claim.source_revision == 0
                    || !seen.insert((claim.claim_id.clone(), claim.claim_generation))
                {
                    return Err(E::InvalidRequest);
                }
                digest.update(claim.claim_id.as_bytes());
                digest.update(claim.run_id.as_bytes());
                digest.update(claim.claim_generation.to_le_bytes());
                digest.update(claim.source_revision.to_le_bytes());
            }
            let digest = hex(&digest.finalize());
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            let state: (String, Option<String>, Option<String>, Option<i64>) = tx
                .query_row(
                    "SELECT import_state,coordinator_store_id,snapshot_digest,snapshot_revision FROM workspace_guard_state WHERE id=1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(|_| E::StorageIncompatible)?;
            if state.0 == "complete" {
                if state.1.as_deref() == Some(coordinator_store_id)
                    && state.2.as_deref() == Some(digest.as_str())
                    && state.3 == Some(snapshot_revision as i64)
                {
                    tx.commit().map_err(|_| E::StorageUnavailable)?;
                    return Ok(digest);
                }
                return Err(E::IntentConflict);
            }
            if state.0 != "required" {
                return Err(E::StorageIncompatible);
            }
            for claim in &ordered {
                tx.execute(
                    "INSERT INTO workspace_imported_claims(coordinator_store_id,claim_id,run_id,claim_generation,identity_state,source_revision) VALUES(?1,?2,?3,?4,'identity_unproven',?5)",
                    params![coordinator_store_id, claim.claim_id, claim.run_id, claim.claim_generation as i64, claim.source_revision as i64],
                )
                .map_err(|_| E::StorageUnavailable)?;
            }
            tx.execute(
                "UPDATE workspace_guard_state SET import_state='complete',coordinator_store_id=?1,snapshot_digest=?2,snapshot_revision=?3 WHERE id=1 AND import_state='required'",
                params![coordinator_store_id, digest, snapshot_revision as i64],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            Ok(digest)
        }

        /// True only after a complete import and while no legacy or lost-identity claim can
        /// overlap a new root. Active claims with live F8 pins do not disable independent roots.
        pub fn capability_ready(&self, conn: &Connection) -> ControlResult<bool> {
            let (import_state, imported, unproven, quarantined): (String, i64, i64, i64) = conn
                .query_row(
                    "SELECT s.import_state,(SELECT count(*) FROM workspace_imported_claims),(SELECT count(*) FROM workspace_root_bindings WHERE proof_state='identity_unproven'),(SELECT count(*) FROM workspace_claims WHERE phase='quarantined') FROM workspace_guard_state s WHERE id=1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(|_| E::StorageIncompatible)?;
            Ok(import_state == "complete" && imported == 0 && unproven == 0 && quarantined == 0)
        }

        /// Permit a pre-existing ordinary F8 turn only when its live runtime root is
        /// demonstrably disjoint from every unresolved M6 claim. If import is incomplete,
        /// a binding is missing, or the filesystem relationship is unproven, fail closed.
        pub fn ordinary_start_allowed(
            &self,
            conn: &mut Connection,
            runtime_instance_id: &str,
        ) -> ControlResult<bool> {
            self.refresh_dispatch_proofs(conn)?;
            if !self.capability_ready(conn)? {
                return Ok(false);
            }
            let active_claim_roots = {
                let mut statement = conn
                    .prepare("SELECT root_binding_id FROM workspace_claims WHERE phase<>'released'")
                    .map_err(|_| E::StorageUnavailable)?;
                statement
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|_| E::StorageUnavailable)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| E::StorageUnavailable)?
            };
            super::super::protocol::canonical(runtime_instance_id)?;
            let candidate_root_id: Option<String> = conn
                .query_row(
                    "SELECT root_binding_id FROM managed_runtime_workspace_bindings WHERE runtime_instance_id=?1 AND authority_store_id=?2 AND authority_epoch=?3",
                    params![runtime_instance_id, self.store_id, self.epoch],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|_| E::StorageUnavailable)?;
            let Some(candidate_root_id) = candidate_root_id else {
                // Existing unguarded profiles retain their policy only in an empty claim
                // domain. A pinned runtime must validate its proof even after claim release.
                return Ok(active_claim_roots.is_empty());
            };
            let candidate = self.pins.get(&candidate_root_id).ok_or(E::GateDenied)?;
            let candidate_proof: Option<(String, String, String, i64)> = conn
                .query_row(
                    "SELECT proof_state,authority_store_id,authority_epoch,proof_revision FROM workspace_root_bindings WHERE root_binding_id=?1",
                    [&candidate_root_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()
                .map_err(|_| E::StorageUnavailable)?;
            if !candidate_proof.is_some_and(|(state, store, epoch, revision)| {
                state == "pinned"
                    && store == self.store_id
                    && epoch == self.epoch
                    && revision as u64 == candidate.proof_revision
            }) || !candidate.locator_matches()?
            {
                return Ok(false);
            }

            match self.require_disjoint_claims(conn, &candidate_root_id, None) {
                Ok(()) => Ok(true),
                Err(error @ (E::StorageUnavailable | E::StorageIncompatible)) => Err(error),
                Err(_) => Ok(false),
            }
        }

        /// Executable approvals keep the same live root/claim fence as their admitted turn.
        /// Decline/cancel do not use this path and remain available for containment.
        pub(crate) fn check_executable_approval(
            &self,
            conn: &mut Connection,
            reference: &super::super::protocol::RuntimeRef,
            turn_id: &str,
        ) -> ControlResult<()> {
            self.refresh_dispatch_proofs(conn)?;
            let root_id = self.runtime_root_binding(
                conn,
                reference.managed_session_id.as_str(),
                reference.runtime_instance_id.as_str(),
                reference.generation,
            )?;
            let owning_claim: Option<String> = conn.query_row(
                "SELECT c.claim_id FROM guarded_attempts a JOIN workspace_claims c USING(claim_id) WHERE a.turn_id=?1 AND c.root_binding_id=?2 AND c.phase='unsettled' AND c.gate_state='open'",
                params![turn_id,root_id],
                |row|row.get(0),
            ).optional().map_err(|_|E::StorageUnavailable)?;
            self.require_disjoint_claims(conn, &root_id, owning_claim.as_deref())
        }

        /// Bind a transient path to an open directory object and persist only opaque, path-free
        /// fingerprints. Observation alone does not create execution authority.
        pub fn observe_root(
            &mut self,
            conn: &mut Connection,
            locator: &Path,
        ) -> ControlResult<String> {
            self.retire_released_roots(conn)?;
            if !self.capability_ready(conn)? {
                return Err(E::GateDenied);
            }
            let root_id = crate::control::store::uuid().as_str().to_owned();
            let pin = RootPin::open(
                locator,
                self.store_id.clone(),
                self.epoch.clone(),
                root_id.clone(),
            )?;
            // Repeated observation of the same still-bound object must reuse its immutable
            // binding. Otherwise runtime launch would see multiple matching pins and could not
            // prove which one owns the execution root. A moved or retargeted locator never
            // refreshes an old pin: it remains a review case, not a new binding shortcut.
            let mut existing = None;
            for (binding_id, retained) in &self.pins {
                if retained.same_live_object(&pin) {
                    if existing.is_some() || !retained.locator_matches()? {
                        return Err(E::WorkspaceInvalid);
                    }
                    existing = Some(binding_id.clone());
                }
            }
            if let Some(binding_id) = existing {
                return Ok(binding_id);
            }
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            let (store, import): (String, String) = tx
                .query_row(
                    "SELECT authority_store_id,import_state FROM workspace_guard_state WHERE id=1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| E::StorageIncompatible)?;
            if store != self.store_id || import != "complete" {
                return Err(E::StaleRevision);
            }
            let (live_roots, unclaimed_roots): (i64,i64) = tx.query_row(
                "SELECT count(*),COALESCE(sum(NOT EXISTS(SELECT 1 FROM workspace_claims c WHERE c.root_binding_id=r.root_binding_id AND c.phase<>'released')),0) FROM workspace_root_bindings r WHERE r.authority_store_id=?1 AND r.authority_epoch=?2 AND r.proof_state='pinned'",
                params![self.store_id,self.epoch],
                |row|Ok((row.get(0)?,row.get(1)?)),
            ).map_err(|_|E::StorageUnavailable)?;
            if unclaimed_roots >= MAX_UNCLAIMED_ROOTS
                || live_roots >= MAX_UNRELEASED_CLAIMS + MAX_UNCLAIMED_ROOTS
            {
                return Err(E::CapacityExceeded);
            }
            tx.execute(
                "INSERT INTO workspace_root_bindings(root_binding_id,authority_store_id,authority_epoch,proof_state,proof_revision,filesystem_fingerprint,object_fingerprint,ancestry_fingerprint,mount_id,created_at) VALUES(?1,?2,?3,'pinned',1,?4,?5,?6,?7,?8)",
                params![root_id, self.store_id, self.epoch, pin.filesystem_fingerprint(), pin.object_fingerprint(), pin.ancestry_fingerprint(), pin.mount_id(), now()?],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            self.pins.insert(root_id.clone(), pin);
            Ok(root_id)
        }

        /// Admit a durable workspace claim under BEGIN IMMEDIATE. Pairwise root conflicts and
        /// missing descriptors fail closed; M6 scheduler observations are not authority.
        pub fn acquire_claim(
            &self,
            conn: &mut Connection,
            coordinator_store_id: &str,
            run_id: &str,
            plan_revision: u64,
            plan_digest: &str,
            root_binding_id: &str,
        ) -> ControlResult<Claim> {
            super::super::protocol::canonical(coordinator_store_id)?;
            super::super::protocol::canonical(run_id)?;
            super::super::protocol::canonical(root_binding_id)?;
            if plan_revision == 0
                || plan_digest.len() != 64
                || !plan_digest.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(E::InvalidRequest);
            }
            self.refresh_dispatch_proofs(conn)?;
            let pin = self.pins.get(root_binding_id).ok_or(E::GateDenied)?;
            if pin.authority_store_id != self.store_id
                || pin.authority_epoch != self.epoch
                || !pin.locator_matches()?
            {
                return Err(E::StaleRevision);
            }
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            let (import, imported): (String, i64) = tx
                .query_row(
                    "SELECT import_state,(SELECT count(*) FROM workspace_imported_claims) FROM workspace_guard_state WHERE id=1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| E::StorageIncompatible)?;
            if import != "complete" || imported != 0 {
                return Err(E::GateDenied);
            }
            let root_state: (String, String, String, i64) = tx
                .query_row(
                    "SELECT proof_state,authority_store_id,authority_epoch,proof_revision FROM workspace_root_bindings WHERE root_binding_id=?1",
                    [root_binding_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if root_state.0 != "pinned"
                || root_state.1 != self.store_id
                || root_state.2 != self.epoch
                || root_state.3 != 1
            {
                return Err(E::StaleRevision);
            }
            if let Some(row) = tx
                .query_row(
                    "SELECT claim_id,root_binding_id,claim_generation,claim_revision,gate_revision,phase,gate_state,plan_revision,plan_digest,settlement_id,coordinator_revision FROM workspace_claims WHERE coordinator_store_id=?1 AND run_id=?2 ORDER BY claim_generation DESC LIMIT 1",
                    params![coordinator_store_id, run_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, i64>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?, row.get::<_, i64>(7)?, row.get::<_, String>(8)?, row.get::<_, Option<String>>(9)?, row.get::<_, i64>(10)?)),
                )
                .optional()
                .map_err(|_| E::StorageUnavailable)?
            {
                if row.1 == root_binding_id && row.7 == plan_revision as i64 && row.8 == plan_digest && row.5 != "released" {
                    tx.commit().map_err(|_| E::StorageUnavailable)?;
                    return Ok(Claim { claim_id: row.0, root_binding_id: row.1, claim_generation: row.2 as u64, claim_revision: row.3 as u64, gate_revision: row.4 as u64, phase: row.5, gate_state: row.6, settlement_id: row.9, coordinator_revision: row.10 as u64 });
                }
                if row.5 != "released" {
                    return Err(E::IntentConflict);
                }
            }
            let unreleased: i64 = tx
                .query_row(
                    "SELECT count(*) FROM workspace_claims WHERE phase<>'released'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|_| E::StorageUnavailable)?;
            if unreleased >= MAX_UNRELEASED_CLAIMS {
                return Err(E::CapacityExceeded);
            }
            self.require_disjoint_claims(&tx, root_binding_id, None)?;
            // An ordinary Managed turn may have been admitted before this claim arrived.
            // Refuse overlapping or identity-unproven active work so its provider approvals
            // cannot continue to mutate a workspace after M6 acquires ownership.
            let active_turn_roots = {
                let mut stmt = tx
                    .prepare(
                        "SELECT DISTINCT b.root_binding_id FROM managed_turns t LEFT JOIN managed_runtime_workspace_bindings b ON b.runtime_instance_id=t.runtime_instance_id WHERE t.state IN ('starting','running','indeterminate')",
                    )
                    .map_err(|_| E::StorageUnavailable)?;
                stmt.query_map([], |row| row.get::<_, Option<String>>(0))
                    .map_err(|_| E::StorageUnavailable)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| E::StorageUnavailable)?
            };
            for active_root_id in active_turn_roots {
                let Some(active_root_id) = active_root_id else {
                    return Err(E::WorkspaceInvalid);
                };
                let active_pin = self.pins.get(&active_root_id).ok_or(E::GateDenied)?;
                if pin.same_locator(active_pin) {
                    return Err(E::WorkspaceInvalid);
                }
                match pin.overlap(active_pin) {
                    Overlap::Disjoint => {}
                    Overlap::Conflict | Overlap::Unproven => {
                        return Err(E::WorkspaceInvalid);
                    }
                }
            }
            let generation: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(claim_generation),0)+1 FROM workspace_claims WHERE coordinator_store_id=?1 AND run_id=?2",
                    params![coordinator_store_id, run_id],
                    |row| row.get(0),
                )
                .map_err(|_| E::StorageUnavailable)?;
            let claim_id = crate::control::store::uuid().as_str().to_owned();
            tx.execute(
                "INSERT INTO workspace_claims(claim_id,coordinator_store_id,run_id,plan_revision,plan_digest,root_binding_id,claim_generation,claim_revision,gate_state,gate_revision,phase,quarantine_reason,settlement_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,1,'open',1,'held',NULL,NULL,?8)",
                params![claim_id, coordinator_store_id, run_id, plan_revision as i64, plan_digest, root_binding_id, generation, now()?],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            Ok(Claim {
                claim_id,
                root_binding_id: root_binding_id.to_owned(),
                claim_generation: generation as u64,
                claim_revision: 1,
                gate_revision: 1,
                phase: "held".to_owned(),
                gate_state: "open".to_owned(),
                settlement_id: None,
                coordinator_revision: 0,
            })
        }

        /// Move a claim gate with compare-and-swap revisions. Stop and pause retain ownership.
        #[allow(clippy::too_many_arguments)] // Explicit CAS inputs mirror the authority contract.
        pub fn set_gate(
            &self,
            conn: &mut Connection,
            claim_id: &str,
            expected_claim_revision: u64,
            expected_gate_revision: u64,
            gate: &str,
            settlement_id: Option<&str>,
            coordinator_revision: Option<u64>,
        ) -> ControlResult<Claim> {
            super::super::protocol::canonical(claim_id)?;
            if !matches!(gate, "open" | "paused" | "stopped") {
                return Err(E::InvalidRequest);
            }
            if let Some(settlement_id) = settlement_id {
                super::super::protocol::canonical(settlement_id)?;
            }
            if coordinator_revision
                .is_some_and(|revision| revision == 0 || revision > i64::MAX as u64)
            {
                return Err(E::InvalidRequest);
            }
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            let (root_id, generation, revision, gate_revision, phase, current, current_settlement, current_coordinator_revision): (String, i64, i64, i64, String, String, Option<String>, i64) = tx
                .query_row(
                    "SELECT root_binding_id,claim_generation,claim_revision,gate_revision,phase,gate_state,settlement_id,coordinator_revision FROM workspace_claims WHERE claim_id=?1",
                    [claim_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if revision as u64 != expected_claim_revision
                || gate_revision as u64 != expected_gate_revision
            {
                return Err(E::StaleRevision);
            }
            let acknowledge_settlement = phase == "verified";
            if phase == "released"
                || phase == "quarantined"
                || (gate == "open" && phase != "held" && !acknowledge_settlement)
                || (acknowledge_settlement
                    && (settlement_id.is_none()
                        || settlement_id != current_settlement.as_deref()
                        || coordinator_revision.is_none()
                        || coordinator_revision.is_some_and(|revision| {
                            revision <= current_coordinator_revision as u64
                        })))
                || (!acknowledge_settlement
                    && (settlement_id.is_some() || coordinator_revision.is_some()))
            {
                return Err(E::InvalidTransition);
            }
            if current == gate && !acknowledge_settlement {
                tx.commit().map_err(|_| E::StorageUnavailable)?;
                return Ok(Claim {
                    claim_id: claim_id.to_owned(),
                    root_binding_id: root_id,
                    claim_generation: generation as u64,
                    claim_revision: revision as u64,
                    gate_revision: gate_revision as u64,
                    phase,
                    gate_state: current,
                    settlement_id: current_settlement,
                    coordinator_revision: current_coordinator_revision as u64,
                });
            }
            tx.execute(
                "UPDATE workspace_claims SET gate_state=?1,phase=CASE WHEN phase='verified' THEN 'held' ELSE phase END,coordinator_revision=CASE WHEN phase='verified' THEN ?2 ELSE coordinator_revision END,claim_revision=claim_revision+1,gate_revision=gate_revision+1 WHERE claim_id=?3 AND claim_revision=?4 AND gate_revision=?5",
                params![gate, coordinator_revision.unwrap_or(current_coordinator_revision as u64) as i64, claim_id, expected_claim_revision as i64, expected_gate_revision as i64],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            Ok(Claim {
                claim_id: claim_id.to_owned(),
                root_binding_id: root_id,
                claim_generation: generation as u64,
                claim_revision: revision as u64 + 1,
                gate_revision: gate_revision as u64 + 1,
                phase: if acknowledge_settlement {
                    "held".to_owned()
                } else {
                    phase
                },
                gate_state: gate.to_owned(),
                settlement_id: current_settlement,
                coordinator_revision: coordinator_revision
                    .unwrap_or(current_coordinator_revision as u64),
            })
        }

        pub fn claim_status(&self, conn: &Connection, claim_id: &str) -> ControlResult<Claim> {
            super::super::protocol::canonical(claim_id)?;
            conn.query_row(
                "SELECT claim_id,root_binding_id,claim_generation,claim_revision,gate_revision,phase,gate_state,settlement_id,coordinator_revision FROM workspace_claims WHERE claim_id=?1",
                [claim_id],
                |row| {
                    Ok(Claim {
                        claim_id: row.get(0)?,
                        root_binding_id: row.get(1)?,
                        claim_generation: row.get::<_, i64>(2)? as u64,
                        claim_revision: row.get::<_, i64>(3)? as u64,
                        gate_revision: row.get::<_, i64>(4)? as u64,
                        phase: row.get(5)?,
                        gate_state: row.get(6)?,
                        settlement_id: row.get(7)?,
                        coordinator_revision: row.get::<_, i64>(8)? as u64,
                    })
                },
            )
            .map_err(|_| E::GateDenied)
        }

        /// Prepare a root-backed observation against the live F8 pin. The supplied path is
        /// merely a selector; content is read descriptor-relatively from the retained object.
        pub fn prepare_verification(
            &self,
            conn: &mut Connection,
            request: &super::super::guard_protocol::VerificationPrepare,
        ) -> ControlResult<PreparedVerification> {
            let claim = self.claim_status(conn, &request.claim_id)?;
            if claim.claim_generation != request.claim_generation
                || claim.root_binding_id != request.root_binding_id
            {
                return Err(E::StaleRevision);
            }
            if claim.phase == "verified"
                && claim.settlement_id.as_deref() == Some(&request.observation_id)
            {
                let settled: Option<(i64, String, String, String, String)> = conn
                    .query_row(
                        "SELECT claim_revision,attempt_id,authority_epoch,content_digest,state FROM verification_observations WHERE observation_id=?1 AND claim_id=?2 AND claim_generation=?3 AND root_binding_id=?4",
                        params![request.observation_id,request.claim_id,request.claim_generation as i64,request.root_binding_id],
                        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
                    )
                    .optional()
                    .map_err(|_| E::StorageUnavailable)?;
                let Some((revision, attempt, epoch, digest, state)) = settled else {
                    return Err(E::StorageIncompatible);
                };
                if revision as u64 != request.expected_claim_revision
                    || attempt != request.attempt_id
                    || epoch != self.epoch
                    || digest != request.reviewed_digest
                    || state != "settled"
                {
                    return Err(E::IntentConflict);
                }
                return Ok(PreparedVerification {
                    observation_id: request.observation_id.clone(),
                    claim_id: request.claim_id.clone(),
                    claim_generation: request.claim_generation,
                    claim_revision: request.expected_claim_revision,
                    root_binding_id: request.root_binding_id.clone(),
                    content_digest: digest,
                });
            }
            if claim.claim_revision != request.expected_claim_revision || claim.phase != "unsettled"
            {
                return Err(E::StaleRevision);
            }
            let pin = self
                .pins
                .get(&request.root_binding_id)
                .filter(|pin| {
                    pin.authority_store_id == self.store_id && pin.authority_epoch == self.epoch
                })
                .ok_or(E::GateDenied)?;
            let candidate = RootPin::open(
                &request.locator,
                self.store_id.clone(),
                self.epoch.clone(),
                crate::control::store::uuid().as_str().to_owned(),
            )?;
            if !pin.same_live_object(&candidate) {
                return Err(E::WorkspaceInvalid);
            }
            let actual_digest = pin.content_digest()?;
            if actual_digest != request.reviewed_digest {
                return Err(E::StaleRevision);
            }
            let send_state =
                require_quiescent_attempt(conn, &request.claim_id, &request.attempt_id)?;
            if !matches!(
                send_state.as_str(),
                "reserved" | "verifying" | "known_no_send"
            ) {
                return Err(E::GateDenied);
            }

            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            let existing: Option<VerificationObservationRow> = tx
                .query_row(
                    "SELECT claim_id,claim_generation,claim_revision,root_binding_id,authority_epoch,attempt_id,content_digest,state FROM verification_observations WHERE observation_id=?1 AND state IN ('prepared','settled')",
                    [&request.observation_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
                )
                .optional()
                .map_err(|_| E::StorageUnavailable)?;
            if let Some(row) = existing {
                if row.0 == request.claim_id
                    && row.1 as u64 == request.claim_generation
                    && row.2 as u64 == request.expected_claim_revision
                    && row.3 == request.root_binding_id
                    && row.4 == self.epoch
                    && row.5 == request.attempt_id
                    && row.6 == actual_digest
                    && row.7 == "prepared"
                {
                    tx.commit().map_err(|_| E::StorageUnavailable)?;
                    return Ok(PreparedVerification {
                        observation_id: request.observation_id.clone(),
                        claim_id: request.claim_id.clone(),
                        claim_generation: request.claim_generation,
                        claim_revision: request.expected_claim_revision,
                        root_binding_id: request.root_binding_id.clone(),
                        content_digest: actual_digest,
                    });
                }
                return Err(E::IntentConflict);
            }
            tx.execute(
                "INSERT INTO verification_observations(observation_id,claim_id,claim_generation,claim_revision,root_binding_id,authority_epoch,attempt_id,content_digest,state,decision,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'prepared',NULL,?9)",
                params![request.observation_id, request.claim_id, request.claim_generation as i64, request.expected_claim_revision as i64, request.root_binding_id, self.epoch, request.attempt_id, actual_digest, now()?],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            Ok(PreparedVerification {
                observation_id: request.observation_id.clone(),
                claim_id: request.claim_id.clone(),
                claim_generation: request.claim_generation,
                claim_revision: request.expected_claim_revision,
                root_binding_id: request.root_binding_id.clone(),
                content_digest: actual_digest,
            })
        }

        /// Settle only the exact prepared review while the original F8 pin is live and the
        /// admitted turn is quiescent. Owner input alone never changes claim ownership.
        pub fn commit_verification(
            &self,
            conn: &mut Connection,
            request: &super::super::guard_protocol::VerificationCommit,
        ) -> ControlResult<Claim> {
            let observation: (String, i64, i64, String, String, String, String, String, Option<String>) = conn
                .query_row(
                    "SELECT claim_id,claim_generation,claim_revision,root_binding_id,authority_epoch,attempt_id,content_digest,state,decision FROM verification_observations WHERE observation_id=?1 AND state IN ('prepared','settled')",
                    [&request.observation_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if observation.0 != request.claim_id
                || observation.1 as u64 != request.claim_generation
                || observation.2 as u64 != request.expected_claim_revision
                || observation.4 != self.epoch
                || observation.5 != request.attempt_id
                || observation.6 != request.reviewed_digest
            {
                return Err(E::StaleRevision);
            }
            use super::super::guard_protocol::VerificationDecision as D;
            let decision = match request.decision {
                D::ConfirmedCompleted => "confirmed_completed",
                D::ConfirmedNoEffect => "confirmed_no_effect",
                D::ConfirmedCancelled => "confirmed_cancelled",
            };
            let claim = self.claim_status(conn, &request.claim_id)?;
            if observation.7 == "settled" {
                if observation.8.as_deref() != Some(decision)
                    || claim.claim_generation != request.claim_generation
                    || claim.phase != "verified"
                    || claim.settlement_id.as_deref() != Some(&request.observation_id)
                {
                    return Err(E::IntentConflict);
                }
                return Ok(claim);
            }
            if claim.claim_generation != request.claim_generation
                || claim.claim_revision != request.expected_claim_revision
                || claim.root_binding_id != observation.3
                || claim.phase != "unsettled"
            {
                return Err(E::StaleRevision);
            }
            let pin = self.pins.get(&observation.3).ok_or(E::GateDenied)?;
            let candidate = RootPin::open(
                &request.locator,
                self.store_id.clone(),
                self.epoch.clone(),
                crate::control::store::uuid().as_str().to_owned(),
            )?;
            if !pin.same_live_object(&candidate) || pin.content_digest()? != observation.6 {
                return Err(E::StaleRevision);
            }
            let send_state =
                require_quiescent_attempt(conn, &request.claim_id, &request.attempt_id)?;
            match request.decision {
                D::ConfirmedCompleted if send_state != "verifying" => {
                    return Err(E::GateDenied);
                }
                D::ConfirmedNoEffect
                    if !matches!(send_state.as_str(), "reserved" | "known_no_send") =>
                {
                    return Err(E::GateDenied);
                }
                D::ConfirmedCancelled
                    if !matches!(send_state.as_str(), "verifying" | "known_no_send") =>
                {
                    return Err(E::GateDenied);
                }
                _ => {}
            }
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            let decision = match request.decision {
                D::ConfirmedCompleted => "confirmed_completed",
                D::ConfirmedNoEffect => "confirmed_no_effect",
                D::ConfirmedCancelled => "confirmed_cancelled",
            };
            let changed = tx
                .execute(
                    "UPDATE verification_observations SET state='settled',decision=?1 WHERE observation_id=?2 AND claim_id=?3 AND claim_generation=?4 AND claim_revision=?5 AND authority_epoch=?6 AND attempt_id=?7 AND content_digest=?8 AND state='prepared'",
                    params![decision, request.observation_id, request.claim_id, request.claim_generation as i64, request.expected_claim_revision as i64, self.epoch, request.attempt_id, request.reviewed_digest],
                )
                .map_err(|_| E::StorageUnavailable)?;
            if changed != 1 {
                return Err(E::StaleRevision);
            }
            let changed = tx
                .execute(
                    "UPDATE workspace_claims SET phase='verified',gate_state='stopped',claim_revision=claim_revision+1,gate_revision=gate_revision+1,settlement_id=?1 WHERE claim_id=?2 AND claim_generation=?3 AND claim_revision=?4 AND phase='unsettled'",
                    params![request.observation_id, request.claim_id, request.claim_generation as i64, request.expected_claim_revision as i64],
                )
                .map_err(|_| E::StorageUnavailable)?;
            if changed != 1 {
                return Err(E::StaleRevision);
            }
            if request.decision == D::ConfirmedNoEffect {
                let changed = tx.execute(
                    "UPDATE guarded_attempts SET send_state='known_no_send' WHERE attempt_id=?1 AND claim_id=?2 AND send_state IN ('reserved','known_no_send')",
                    params![request.attempt_id,request.claim_id],
                ).map_err(|_| E::StorageUnavailable)?;
                if changed != 1 {
                    return Err(E::StaleRevision);
                }
            } else if send_state == "verifying" {
                let changed = tx.execute(
                    "UPDATE guarded_attempts SET send_state='terminal' WHERE attempt_id=?1 AND claim_id=?2 AND send_state='verifying'",
                    params![request.attempt_id,request.claim_id],
                ).map_err(|_| E::StorageUnavailable)?;
                if changed != 1 {
                    return Err(E::StaleRevision);
                }
            }
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            self.claim_status(conn, &request.claim_id)
        }

        /// Release is a separate, idempotent operation after M6 has durably committed its
        /// terminal revision. Unsettled/unknown attempts are never force-released.
        pub fn release_claim(
            &mut self,
            conn: &mut Connection,
            request: &super::super::guard_protocol::ClaimRelease,
        ) -> ControlResult<Claim> {
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            let current: (i64, i64, String, String, Option<String>, i64, i64) = tx
                .query_row(
                    "SELECT claim_generation,claim_revision,phase,gate_state,settlement_id,coordinator_revision,gate_revision FROM workspace_claims WHERE claim_id=?1",
                    [&request.claim_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if current.0 as u64 != request.claim_generation
                || current.4.as_deref() != Some(request.settlement_id.as_str())
            {
                return Err(E::StaleRevision);
            }
            if current.2 == "released" && current.5 as u64 == request.coordinator_revision {
                tx.commit().map_err(|_| E::StorageUnavailable)?;
                self.retire_released_roots(conn)?;
                return self.claim_status(conn, &request.claim_id);
            }
            if current.1 as u64 != request.expected_claim_revision
                || current.2 != "verified"
                || current.3 == "open"
                || request.coordinator_revision <= current.5 as u64
            {
                return Err(E::InvalidTransition);
            }
            let unresolved: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM guarded_attempts WHERE claim_id=?1 AND send_state NOT IN ('terminal','known_no_send'))",
                    [&request.claim_id],
                    |row| row.get(0),
                )
                .map_err(|_| E::StorageUnavailable)?;
            if unresolved {
                return Err(E::GateDenied);
            }
            let changed = tx
                .execute(
                    "UPDATE workspace_claims SET phase='released',gate_state='stopped',claim_revision=claim_revision+1,gate_revision=gate_revision+1,coordinator_revision=?1 WHERE claim_id=?2 AND claim_generation=?3 AND claim_revision=?4 AND phase='verified' AND settlement_id=?5",
                    params![request.coordinator_revision as i64, request.claim_id, request.claim_generation as i64, request.expected_claim_revision as i64, request.settlement_id],
                )
                .map_err(|_| E::StorageUnavailable)?;
            if changed != 1 {
                return Err(E::StaleRevision);
            }
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            self.retire_released_roots(conn)?;
            self.claim_status(conn, &request.claim_id)
        }

        /// Release a terminal M6 claim only when F8 proves no start receipt or attempt was
        /// ever admitted. The same writer transaction fences a racing start reservation.
        pub fn release_unused_claim(
            &mut self,
            conn: &mut Connection,
            request: &super::super::guard_protocol::ClaimReleaseUnused,
        ) -> ControlResult<Claim> {
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            let current: (i64, i64, i64, String, String, Option<String>, i64) = tx
                .query_row(
                    "SELECT claim_generation,claim_revision,gate_revision,phase,gate_state,settlement_id,coordinator_revision FROM workspace_claims WHERE claim_id=?1",
                    [&request.claim_id],
                    |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if current.0 as u64 != request.claim_generation || current.5.is_some() {
                return Err(E::StaleRevision);
            }
            if current.3 == "released" && current.6 as u64 == request.coordinator_revision {
                tx.commit().map_err(|_| E::StorageUnavailable)?;
                self.retire_released_roots(conn)?;
                return self.claim_status(conn, &request.claim_id);
            }
            if current.1 as u64 != request.expected_claim_revision
                || current.2 as u64 != request.expected_gate_revision
                || current.3 != "held"
                || request.coordinator_revision <= current.6 as u64
            {
                return Err(E::InvalidTransition);
            }
            let has_attempt: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM guarded_attempts WHERE claim_id=?1) OR EXISTS(SELECT 1 FROM guarded_start_receipts WHERE claim_id=?1 AND state IN ('prepared','consumed'))",
                    [&request.claim_id],
                    |row| row.get(0),
                )
                .map_err(|_| E::StorageUnavailable)?;
            if has_attempt {
                return Err(E::GateDenied);
            }
            let changed = tx
                .execute(
                    "UPDATE workspace_claims SET phase='released',gate_state='stopped',claim_revision=claim_revision+1,gate_revision=gate_revision+1,coordinator_revision=?1 WHERE claim_id=?2 AND claim_generation=?3 AND claim_revision=?4 AND gate_revision=?5 AND phase='held' AND settlement_id IS NULL",
                    params![request.coordinator_revision as i64,request.claim_id,request.claim_generation as i64,request.expected_claim_revision as i64,request.expected_gate_revision as i64],
                )
                .map_err(|_| E::StorageUnavailable)?;
            if changed != 1 {
                return Err(E::StaleRevision);
            }
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            self.retire_released_roots(conn)?;
            self.claim_status(conn, &request.claim_id)
        }

        /// Prepare one bounded, one-use receipt. This is only a short-lived observation;
        /// execution authority is granted later by `Writer::admit_guarded_turn_ex` at L1.
        pub fn prepare_start(
            &mut self,
            conn: &mut Connection,
            tuple: &super::super::guard_protocol::StartTuple,
            reference: &super::super::protocol::RuntimeRef,
            attachment: &super::super::engine::Attachment,
        ) -> ControlResult<PreparedStart> {
            use super::super::guard_protocol::{StartEffect, StartProfile, StartPurpose};
            tuple.validate()?;
            reference.validate()?;
            self.refresh_dispatch_proofs(conn)?;
            if tuple.authority_store_id != self.store_id
                || tuple.authority_epoch != self.epoch
                || tuple.session_id != reference.managed_session_id.as_str()
                || tuple.runtime_instance_id != reference.runtime_instance_id.as_str()
                || tuple.runtime_generation != reference.generation
                || tuple.model_id.is_some()
                || tuple.approval_reference.is_some()
                || attachment.revision != tuple.attachment_revision
                || attachment.workspace.is_empty()
                || attachment.profile
                    != match tuple.model_profile {
                        StartProfile::CodexInferenceV1 => "codex_inference_v1",
                        StartProfile::CodexLocalToolsV1 => "codex_local_tools_v1",
                    }
                || (tuple.effect_class == StartEffect::ManagedInference
                    && tuple.model_profile != StartProfile::CodexInferenceV1)
                || (tuple.effect_class == StartEffect::ManagedLocalTools
                    && tuple.model_profile != StartProfile::CodexLocalToolsV1)
            {
                return Err(E::GateDenied);
            }
            let monotonic_now = Instant::now();
            self.prepared_start_deadlines
                .retain(|_, (deadline, _)| *deadline > monotonic_now);
            if self.prepared_start_deadlines.len() >= 2048 {
                return Err(E::CapacityExceeded);
            }
            let pin = self.pins.get(&tuple.root_binding_id).ok_or(E::GateDenied)?;
            if pin.authority_store_id != self.store_id
                || pin.authority_epoch != self.epoch
                || !pin.locator_matches()?
            {
                return Err(E::StaleRevision);
            }

            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            self.require_disjoint_claims(&tx, &tuple.root_binding_id, Some(&tuple.claim_id))?;
            let root: (String, String, String, i64) = tx
                .query_row(
                    "SELECT proof_state,authority_store_id,authority_epoch,proof_revision FROM workspace_root_bindings WHERE root_binding_id=?1",
                    [&tuple.root_binding_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if root.0 != "pinned"
                || root.1 != self.store_id
                || root.2 != self.epoch
                || root.3 as u64 != tuple.root_observation_revision
            {
                return Err(E::StaleRevision);
            }
            let claim: (String, String, String, i64, String, String, i64, i64, String, i64) = tx
                .query_row(
                    "SELECT coordinator_store_id,run_id,plan_digest,plan_revision,root_binding_id,phase,claim_generation,claim_revision,gate_state,gate_revision FROM workspace_claims WHERE claim_id=?1",
                    [&tuple.claim_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if claim.0 != tuple.coordinator_store_id
                || claim.1 != tuple.run_id
                || claim.2 != tuple.plan_digest
                || claim.3 as u64 != tuple.plan_revision
                || claim.4 != tuple.root_binding_id
                || claim.5 != "held"
                || claim.6 as u64 != tuple.claim_generation
                || claim.7 as u64 != tuple.claim_revision
                || claim.8 != "open"
                || claim.9 as u64 != tuple.dispatch_gate_revision
            {
                return Err(E::StaleRevision);
            }
            let runtime_binding: (String, i64, String, String, String) = tx
                .query_row(
                    "SELECT managed_session_id,generation,authority_store_id,authority_epoch,root_binding_id FROM managed_runtime_workspace_bindings WHERE runtime_instance_id=?1",
                    [&tuple.runtime_instance_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if runtime_binding.0 != tuple.session_id
                || runtime_binding.1 as u64 != tuple.runtime_generation
                || runtime_binding.2 != self.store_id
                || runtime_binding.3 != self.epoch
                || runtime_binding.4 != tuple.root_binding_id
            {
                return Err(E::StaleRevision);
            }
            let session: (String, i64) = tx
                .query_row(
                    "SELECT control_state,revision FROM managed_sessions WHERE session_id=?1",
                    [&tuple.session_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| E::SessionNotFound)?;
            let binding: (i64, String) = tx
                .query_row(
                    "SELECT binding_revision,workspace_root FROM managed_session_bindings WHERE managed_session_id=?1",
                    [&tuple.session_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| E::BindingNotFound)?;
            if session.0 != "prepared"
                || session.1 as u64 != tuple.session_revision
                || binding.0 as u64 != tuple.attachment_revision
                || binding.1 != attachment.workspace
            {
                return Err(E::StaleRevision);
            }
            let freshness: (String, Option<String>, i64) = tx
                .query_row(
                    "SELECT freshness_state,authority_epoch,ever_started FROM managed_session_freshness WHERE session_id=?1",
                    [&tuple.session_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if freshness.0 != "proven" || freshness.1.as_deref() != Some(self.epoch.as_str()) {
                return Err(E::GateDenied);
            }
            if tuple.purpose == StartPurpose::HandoffFreshReceiver && freshness.2 != 0 {
                return Err(E::GateDenied);
            }
            let lease: (String, i64, String, String) = tx
                .query_row(
                    "SELECT state,revision,expires_at,scopes FROM execution_leases WHERE lease_id=?1 AND session_id=?2",
                    params![tuple.lease_id, tuple.session_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(|_| E::LeaseNotFound)?;
            if lease.0 != "active" || lease.1 as u64 != tuple.lease_revision {
                return Err(E::StaleRevision);
            }
            let scopes: Vec<String> =
                serde_json::from_str(&lease.3).map_err(|_| E::StorageIncompatible)?;
            if !scopes.iter().any(|scope| scope == "turn_start") {
                return Err(E::GateDenied);
            }
            let now = OffsetDateTime::now_utc();
            let lease_expiry =
                OffsetDateTime::parse(&lease.2, &Rfc3339).map_err(|_| E::StorageIncompatible)?;
            let remaining_ms = (lease_expiry - now).whole_milliseconds();
            if remaining_ms <= 0 {
                return Err(E::GateDenied);
            }
            let ttl_ms = remaining_ms.min(30_000) as u64;
            let deadline = Instant::now() + Duration::from_millis(ttl_ms);
            let expires_at = (now + time::Duration::milliseconds(ttl_ms as i64))
                .format(&Rfc3339)
                .map_err(|_| E::InternalError)?;
            let tuple_digest = tuple.digest()?;

            let prior: Option<(String, i64, String)> = tx
                .query_row(
                    "SELECT receipt_id,receipt_revision,expires_at FROM guarded_start_receipts WHERE claim_id=?1 AND tuple_digest=?2 AND state='prepared' AND expires_at>?3 ORDER BY receipt_revision DESC LIMIT 1",
                    params![tuple.claim_id, tuple_digest, now.format(&Rfc3339).map_err(|_| E::InternalError)?],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(|_| E::StorageUnavailable)?;
            if let Some((receipt_id, revision, prior_expiry)) = prior {
                if let Some((existing_deadline, existing_digest)) =
                    self.prepared_start_deadlines.get(&receipt_id)
                    && *existing_deadline > Instant::now()
                    && existing_digest == &tuple_digest
                {
                    tx.commit().map_err(|_| E::StorageUnavailable)?;
                    return Ok(PreparedStart {
                        receipt_id,
                        receipt_revision: revision as u64,
                        expires_at: prior_expiry,
                        tuple_digest,
                    });
                }
                tx.execute(
                    "UPDATE guarded_start_receipts SET state='invalidated',receipt_revision=receipt_revision+1 WHERE receipt_id=?1 AND state='prepared'",
                    [&receipt_id],
                )
                .map_err(|_| E::StorageUnavailable)?;
            }
            let receipt_id = crate::control::store::uuid().as_str().to_owned();
            let outstanding: i64 = tx
                .query_row(
                    "SELECT count(*) FROM guarded_start_receipts WHERE state='prepared' AND expires_at>?1",
                    [&now.format(&Rfc3339).map_err(|_| E::InternalError)?],
                    |row| row.get(0),
                )
                .map_err(|_| E::StorageUnavailable)?;
            if outstanding >= 2048 {
                return Err(E::CapacityExceeded);
            }
            tx.execute(
                "INSERT INTO guarded_start_receipts(receipt_id,authority_store_id,authority_epoch,claim_id,claim_generation,claim_revision,root_binding_id,tuple_digest,purpose,expires_at,receipt_revision,state,attempt_id,turn_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,1,'prepared',NULL,NULL)",
                params![receipt_id, self.store_id, self.epoch, tuple.claim_id, tuple.claim_generation as i64, tuple.claim_revision as i64, tuple.root_binding_id, tuple_digest, match tuple.purpose { StartPurpose::PipelineStep => "pipeline_step", StartPurpose::HandoffFreshReceiver => "handoff_fresh_receiver" }, expires_at],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            self.prepared_start_deadlines
                .insert(receipt_id.clone(), (deadline, tuple_digest.clone()));
            Ok(PreparedStart {
                receipt_id,
                receipt_revision: 1,
                expires_at,
                tuple_digest,
            })
        }

        /// A durable timestamp is not enough: prepared receipts are usable only while the
        /// exact process-local monotonic deadline and tuple digest remain in memory.
        pub fn prepared_start_is_live(&self, receipt_id: &str, tuple_digest: &str) -> bool {
            self.prepared_start_deadlines
                .get(receipt_id)
                .is_some_and(|(deadline, digest)| {
                    *deadline > Instant::now() && digest == tuple_digest
                })
        }

        /// Consume a prepared receipt, reserve its claim and link the F8 turn/attempt while
        /// the caller's single `Writer` transaction is still open.
        pub fn consume_prepared_start_in(
            &mut self,
            tx: &rusqlite::Transaction<'_>,
            tuple: &super::super::guard_protocol::StartTuple,
            receipt_id: &str,
            receipt_revision: u64,
            tuple_digest: &str,
            turn_id: &str,
        ) -> ControlResult<()> {
            use super::super::guard_protocol::StartPurpose;
            tuple.validate()?;
            for id in [receipt_id, turn_id] {
                super::super::protocol::canonical(id)?;
            }
            if !self.prepared_start_is_live(receipt_id, tuple_digest)
                || tuple.authority_store_id != self.store_id
                || tuple.authority_epoch != self.epoch
            {
                return Err(E::GateDenied);
            }
            let pin = self.pins.get(&tuple.root_binding_id).ok_or(E::GateDenied)?;
            if pin.authority_store_id != self.store_id
                || pin.authority_epoch != self.epoch
                || !pin.locator_matches()?
            {
                return Err(E::StaleRevision);
            }
            let receipt: (String, String, String, i64, String, i64, i64, String, String, String, String) = tx
                .query_row(
                    "SELECT authority_store_id,authority_epoch,claim_id,claim_generation,root_binding_id,claim_revision,receipt_revision,tuple_digest,purpose,state,expires_at FROM guarded_start_receipts WHERE receipt_id=?1",
                    [receipt_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?)),
                )
                .map_err(|_| E::GateDenied)?;
            let wall_now = OffsetDateTime::now_utc();
            let expiry =
                OffsetDateTime::parse(&receipt.10, &Rfc3339).map_err(|_| E::StorageIncompatible)?;
            if receipt.0 != self.store_id
                || receipt.1 != self.epoch
                || receipt.2 != tuple.claim_id
                || receipt.3 as u64 != tuple.claim_generation
                || receipt.4 != tuple.root_binding_id
                || receipt.5 as u64 != tuple.claim_revision
                || receipt.6 as u64 != receipt_revision
                || receipt.7 != tuple_digest
                || receipt.8
                    != match tuple.purpose {
                        StartPurpose::PipelineStep => "pipeline_step",
                        StartPurpose::HandoffFreshReceiver => "handoff_fresh_receiver",
                    }
                || receipt.9 != "prepared"
                || wall_now >= expiry
            {
                return Err(E::StaleRevision);
            }
            let claim: (String, String, String, i64, String, i64, i64, String, i64, String) = tx
                .query_row(
                    "SELECT coordinator_store_id,run_id,plan_digest,plan_revision,root_binding_id,claim_generation,claim_revision,gate_state,gate_revision,phase FROM workspace_claims WHERE claim_id=?1",
                    [&tuple.claim_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if claim.0 != tuple.coordinator_store_id
                || claim.1 != tuple.run_id
                || claim.2 != tuple.plan_digest
                || claim.3 as u64 != tuple.plan_revision
                || claim.4 != tuple.root_binding_id
                || claim.5 as u64 != tuple.claim_generation
                || claim.6 as u64 != tuple.claim_revision
                || claim.7 != "open"
                || claim.8 as u64 != tuple.dispatch_gate_revision
                || claim.9 != "held"
            {
                return Err(E::StaleRevision);
            }
            let runtime_binding: (String, i64, String, String, String) = tx
                .query_row(
                    "SELECT managed_session_id,generation,authority_store_id,authority_epoch,root_binding_id FROM managed_runtime_workspace_bindings WHERE runtime_instance_id=?1",
                    [&tuple.runtime_instance_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if runtime_binding.0 != tuple.session_id
                || runtime_binding.1 as u64 != tuple.runtime_generation
                || runtime_binding.2 != self.store_id
                || runtime_binding.3 != self.epoch
                || runtime_binding.4 != tuple.root_binding_id
            {
                return Err(E::StaleRevision);
            }
            let prior_attempt: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM guarded_attempts WHERE claim_id=?1 AND send_state NOT IN ('terminal','known_no_send'))",
                    [&tuple.claim_id],
                    |row| row.get(0),
                )
                .map_err(|_| E::StorageUnavailable)?;
            if prior_attempt {
                return Err(E::GateDenied);
            }
            self.reserve_claim_in(
                tx,
                &tuple.claim_id,
                tuple.claim_generation,
                tuple.claim_revision,
                tuple.dispatch_gate_revision,
            )?;
            let changed = tx
                .execute(
                    "UPDATE guarded_start_receipts SET state='consumed',receipt_revision=receipt_revision+1,attempt_id=?1,turn_id=?2 WHERE receipt_id=?3 AND receipt_revision=?4 AND state='prepared'",
                    params![tuple.attempt_id, turn_id, receipt_id, receipt_revision as i64],
                )
                .map_err(|_| E::StorageUnavailable)?;
            if changed != 1 {
                return Err(E::StaleRevision);
            }
            tx.execute(
                "INSERT INTO guarded_attempts(attempt_id,intent_id,claim_id,receipt_id,turn_id,tuple_digest,send_state,created_at) VALUES(?1,?2,?3,?4,?5,?6,'reserved',?7)",
                params![tuple.attempt_id, tuple.intent_id, tuple.claim_id, receipt_id, turn_id, tuple_digest, now()?],
            )
            .map_err(|_| E::IntentConflict)?;
            Ok(())
        }

        /// L2 linearization point. A one-shot reserved attempt becomes possibly sent only
        /// immediately before a non-buffering provider `send_critical` path.
        pub fn mark_guarded_attempt_possibly_sent(
            &self,
            conn: &mut Connection,
            tuple: &super::super::guard_protocol::StartTuple,
        ) -> ControlResult<()> {
            self.refresh_dispatch_proofs(conn)?;
            let pin = self.pins.get(&tuple.root_binding_id).ok_or(E::GateDenied)?;
            if pin.authority_store_id != self.store_id
                || pin.authority_epoch != self.epoch
                || !pin.locator_matches()?
            {
                return Err(E::StaleRevision);
            }
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            self.require_disjoint_claims(&tx, &tuple.root_binding_id, Some(&tuple.claim_id))?;
            let root: (String, String, i64) = tx
                .query_row(
                    "SELECT proof_state,authority_epoch,proof_revision FROM workspace_root_bindings WHERE root_binding_id=?1 AND authority_store_id=?2",
                    params![tuple.root_binding_id, self.store_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if root.0 != "pinned"
                || root.1 != self.epoch
                || root.2 as u64 != tuple.root_observation_revision
            {
                return Err(E::StaleRevision);
            }
            let claim: (i64, i64, String, String, i64, String) = tx
                .query_row(
                    "SELECT claim_revision,gate_revision,gate_state,phase,claim_generation,root_binding_id FROM workspace_claims WHERE claim_id=?1",
                    [&tuple.claim_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if claim.0 as u64 != tuple.claim_revision.saturating_add(1)
                || claim.1 as u64 != tuple.dispatch_gate_revision
                || claim.2 != "open"
                || claim.3 != "unsettled"
                || claim.4 as u64 != tuple.claim_generation
                || claim.5 != tuple.root_binding_id
            {
                return Err(E::StaleRevision);
            }
            let runtime_binding: (String, i64, String, String, String) = tx
                .query_row(
                    "SELECT managed_session_id,generation,authority_store_id,authority_epoch,root_binding_id FROM managed_runtime_workspace_bindings WHERE runtime_instance_id=?1",
                    [&tuple.runtime_instance_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if runtime_binding.0 != tuple.session_id
                || runtime_binding.1 as u64 != tuple.runtime_generation
                || runtime_binding.2 != self.store_id
                || runtime_binding.3 != self.epoch
                || runtime_binding.4 != tuple.root_binding_id
            {
                return Err(E::StaleRevision);
            }
            let session: (String, i64, i64, String) = tx
                .query_row(
                    "SELECT s.control_state,s.revision,b.binding_revision,b.workspace_root FROM managed_sessions s JOIN managed_session_bindings b ON b.managed_session_id=s.session_id WHERE s.session_id=?1",
                    [&tuple.session_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if session.0 != "prepared"
                || session.1 as u64 != tuple.session_revision
                || session.2 as u64 != tuple.attachment_revision
            {
                return Err(E::StaleRevision);
            }
            let lease: (String, i64, String) = tx
                .query_row(
                    "SELECT state,revision,expires_at FROM execution_leases WHERE lease_id=?1 AND session_id=?2",
                    params![tuple.lease_id, tuple.session_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(|_| E::LeaseNotFound)?;
            if lease.0 != "active"
                || lease.1 as u64 != tuple.lease_revision
                || OffsetDateTime::parse(&lease.2, &Rfc3339).map_err(|_| E::StorageIncompatible)?
                    <= OffsetDateTime::now_utc()
            {
                return Err(E::GateDenied);
            }
            let changed = tx
                .execute(
                    "UPDATE guarded_attempts SET send_state='possibly_sent' WHERE attempt_id=?1 AND intent_id=?2 AND claim_id=?3 AND tuple_digest=?4 AND send_state='reserved'",
                    params![tuple.attempt_id, tuple.intent_id, tuple.claim_id, tuple.digest()?],
                )
                .map_err(|_| E::StorageUnavailable)?;
            if changed != 1 {
                return Err(E::StaleRevision);
            }
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            Ok(())
        }

        pub fn mark_guarded_attempt_known_no_send(
            &self,
            conn: &mut Connection,
            attempt_id: &str,
        ) -> ControlResult<()> {
            let changed = conn
                .execute(
                    "UPDATE guarded_attempts SET send_state='known_no_send' WHERE attempt_id=?1 AND send_state='reserved'",
                    [attempt_id],
                )
                .map_err(|_| E::StorageUnavailable)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(E::StaleRevision)
            }
        }

        /// F8's actual writer uses this only inside the same transaction that reserves a turn.
        /// Standalone callers cannot turn an observation into execution authorization.
        pub(super) fn reserve_claim_in(
            &self,
            tx: &rusqlite::Transaction<'_>,
            claim_id: &str,
            generation: u64,
            revision: u64,
            gate_revision: u64,
        ) -> ControlResult<()> {
            let root_id: String = tx
                .query_row(
                    "SELECT root_binding_id FROM workspace_claims WHERE claim_id=?1 AND claim_generation=?2 AND claim_revision=?3 AND gate_revision=?4 AND phase='held' AND gate_state='open'",
                    params![claim_id, generation as i64, revision as i64, gate_revision as i64],
                    |row| row.get(0),
                )
                .map_err(|_| E::StaleRevision)?;
            let pin = self.pins.get(&root_id).ok_or(E::GateDenied)?;
            if pin.authority_store_id != self.store_id
                || pin.authority_epoch != self.epoch
                || !pin.locator_matches()?
            {
                return Err(E::StaleRevision);
            }
            let live: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM workspace_root_bindings WHERE root_binding_id=?1 AND authority_store_id=?2 AND authority_epoch=?3 AND proof_state='pinned' AND proof_revision=?4)",
                params![root_id,self.store_id,self.epoch,pin.proof_revision as i64],
                |row|row.get(0),
            ).map_err(|_|E::StorageUnavailable)?;
            if !live {
                return Err(E::StaleRevision);
            }
            self.require_disjoint_claims(tx, &root_id, Some(claim_id))?;
            let changed = tx.execute(
                "UPDATE workspace_claims SET phase='unsettled',claim_revision=claim_revision+1 WHERE claim_id=?1 AND claim_generation=?2 AND claim_revision=?3 AND gate_revision=?4 AND phase='held' AND gate_state='open'",
                params![claim_id, generation as i64, revision as i64, gate_revision as i64],
            ).map_err(|_| E::StorageUnavailable)?;
            if changed != 1 {
                return Err(E::StaleRevision);
            }
            Ok(())
        }

        /// The descriptor is available only to trusted F8 code; it is never serialized or
        /// returned by a protocol operation.
        pub fn duplicate_root_fd(&self, root_binding_id: &str) -> ControlResult<File> {
            self.pins
                .get(root_binding_id)
                .filter(|pin| {
                    pin.authority_store_id == self.store_id && pin.authority_epoch == self.epoch
                })
                .ok_or(E::GateDenied)?
                .duplicate_fd()
        }

        /// Resolve a launch locator to exactly one retained live root and return a duplicate of
        /// that pinned descriptor. The path is only a selector: it is opened for comparison,
        /// then the retained F8 descriptor is passed to the launcher as the bind source.
        /// Replaced, retargeted, ambiguous, stale, or unsupported roots fail closed.
        pub fn duplicate_root_fd_for_locator(
            &self,
            conn: &mut Connection,
            locator: &Path,
        ) -> ControlResult<(String, File)> {
            self.refresh_dispatch_proofs(conn)?;
            let candidate = RootPin::open(
                locator,
                self.store_id.clone(),
                self.epoch.clone(),
                crate::control::store::uuid().as_str().to_owned(),
            )?;
            let mut matched: Option<&RootPin> = None;
            for pin in self.pins.values() {
                if !pin.same_live_object(&candidate) {
                    continue;
                }
                if matched.is_some() || !pin.locator_matches()? {
                    return Err(E::WorkspaceInvalid);
                }
                matched = Some(pin);
            }
            let pin = matched.ok_or(E::GateDenied)?;
            let (store, epoch, state, revision): (String, String, String, i64) = conn
                .query_row(
                    "SELECT authority_store_id,authority_epoch,proof_state,proof_revision FROM workspace_root_bindings WHERE root_binding_id=?1",
                    [&pin.root_binding_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(|_| E::StorageUnavailable)?;
            if store != self.store_id
                || epoch != self.epoch
                || state != "pinned"
                || revision as u64 != pin.proof_revision
            {
                return Err(E::StaleRevision);
            }
            Ok((pin.root_binding_id.clone(), pin.duplicate_fd()?))
        }

        /// Record that one successfully admitted runtime generation was spawned with the
        /// retained root descriptor. This row is immutable history; a later F8 epoch cannot
        /// reuse it because the live pin registry is intentionally process-local.
        pub fn bind_runtime_root(
            &self,
            conn: &mut Connection,
            session_id: &str,
            runtime_instance_id: &str,
            generation: u64,
            root_binding_id: &str,
            authority_instance_id: &str,
        ) -> ControlResult<()> {
            self.refresh_dispatch_proofs(conn)?;
            for id in [
                session_id,
                runtime_instance_id,
                root_binding_id,
                authority_instance_id,
            ] {
                super::super::protocol::canonical(id)?;
            }
            if generation == 0 {
                return Err(E::InvalidRequest);
            }
            let pin = self.pins.get(root_binding_id).ok_or(E::GateDenied)?;
            if pin.authority_store_id != self.store_id
                || pin.authority_epoch != self.epoch
                || !pin.locator_matches()?
            {
                return Err(E::StaleRevision);
            }
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| E::StorageUnavailable)?;
            let launch_exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM managed_runtime_launches WHERE managed_session_id=?1 AND runtime_instance_id=?2 AND generation=?3 AND authority_instance_id=?4)",
                    params![session_id, runtime_instance_id, generation as i64, authority_instance_id],
                    |row| row.get(0),
                )
                .map_err(|_| E::StorageUnavailable)?;
            if !launch_exists {
                return Err(E::StaleRevision);
            }
            let existing: Option<(String, String, i64, String, String, String)> = tx
                .query_row(
                    "SELECT managed_session_id,runtime_instance_id,generation,authority_store_id,authority_epoch,root_binding_id FROM managed_runtime_workspace_bindings WHERE runtime_instance_id=?1",
                    [runtime_instance_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
                )
                .optional()
                .map_err(|_| E::StorageUnavailable)?;
            if let Some(existing) = existing {
                if existing
                    == (
                        session_id.to_owned(),
                        runtime_instance_id.to_owned(),
                        generation as i64,
                        self.store_id.clone(),
                        self.epoch.clone(),
                        root_binding_id.to_owned(),
                    )
                {
                    tx.commit().map_err(|_| E::StorageUnavailable)?;
                    return Ok(());
                }
                return Err(E::IntentConflict);
            }
            tx.execute(
                "INSERT INTO managed_runtime_workspace_bindings(runtime_instance_id,managed_session_id,generation,authority_store_id,authority_epoch,root_binding_id,bound_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![runtime_instance_id, session_id, generation as i64, self.store_id, self.epoch, root_binding_id, now()?],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
            Ok(())
        }

        /// Return only a binding from this live authority epoch whose root pin is still held.
        pub fn runtime_root_binding(
            &self,
            conn: &Connection,
            session_id: &str,
            runtime_instance_id: &str,
            generation: u64,
        ) -> ControlResult<String> {
            let row: (String, i64, String, String, String) = conn
                .query_row(
                    "SELECT managed_session_id,generation,authority_store_id,authority_epoch,root_binding_id FROM managed_runtime_workspace_bindings WHERE runtime_instance_id=?1",
                    [runtime_instance_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                )
                .map_err(|_| E::GateDenied)?;
            if row.0 != session_id
                || row.1 as u64 != generation
                || row.2 != self.store_id
                || row.3 != self.epoch
            {
                return Err(E::StaleRevision);
            }
            let pin = self.pins.get(&row.4).ok_or(E::GateDenied)?;
            if pin.authority_store_id != self.store_id
                || pin.authority_epoch != self.epoch
                || !pin.locator_matches()?
            {
                return Err(E::StaleRevision);
            }
            let live: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM workspace_root_bindings WHERE root_binding_id=?1 AND authority_store_id=?2 AND authority_epoch=?3 AND proof_state='pinned' AND proof_revision=?4)",
                params![row.4,self.store_id,self.epoch,pin.proof_revision as i64],
                |row|row.get(0),
            ).map_err(|_|E::StorageUnavailable)?;
            if !live {
                return Err(E::StaleRevision);
            }
            Ok(row.4)
        }
    }

    fn retire_roots_in(
        conn: &Connection,
        include_observations: bool,
    ) -> ControlResult<Vec<String>> {
        // A persisted owned-reap audit event is required for every referencing launch.
        // A stopped desired state, missing ACK, or absent runtime binding is not that proof.
        let mut stmt = conn.prepare(
            "SELECT r.root_binding_id FROM workspace_root_bindings r
             WHERE r.proof_state IN ('pinned','identity_unproven')
             AND (?1 OR EXISTS(SELECT 1 FROM workspace_claims c WHERE c.root_binding_id=r.root_binding_id AND c.phase='released'))
             AND NOT EXISTS(SELECT 1 FROM workspace_claims c WHERE c.root_binding_id=r.root_binding_id AND c.phase<>'released')
             AND NOT EXISTS(SELECT 1 FROM guarded_start_receipts s WHERE s.root_binding_id=r.root_binding_id AND s.state='prepared')
             AND NOT EXISTS(SELECT 1 FROM guarded_start_receipts s LEFT JOIN guarded_attempts a USING(receipt_id) WHERE s.root_binding_id=r.root_binding_id AND s.state='consumed' AND (a.attempt_id IS NULL OR a.send_state NOT IN ('terminal','known_no_send')))
             AND NOT EXISTS(SELECT 1 FROM managed_runtime_workspace_bindings b WHERE b.root_binding_id=r.root_binding_id AND NOT EXISTS(SELECT 1 FROM control_events e WHERE json_extract(e.envelope,'$.event_type')='authority.runtime.instance_exited' AND json_extract(e.envelope,'$.payload.runtime_instance_id')=b.runtime_instance_id))
             AND NOT EXISTS(SELECT 1 FROM managed_runtime_launches l WHERE NOT EXISTS(SELECT 1 FROM managed_runtime_workspace_bindings b WHERE b.runtime_instance_id=l.runtime_instance_id) AND NOT EXISTS(SELECT 1 FROM control_events e WHERE json_extract(e.envelope,'$.event_type')='authority.runtime.instance_exited' AND json_extract(e.envelope,'$.payload.runtime_instance_id')=l.runtime_instance_id))
             AND NOT EXISTS(SELECT 1 FROM managed_turns t JOIN managed_runtime_workspace_bindings b ON b.runtime_instance_id=t.runtime_instance_id WHERE b.root_binding_id=r.root_binding_id AND t.state IN ('starting','running','indeterminate'))
             AND NOT EXISTS(SELECT 1 FROM managed_approvals a JOIN managed_runtime_workspace_bindings b ON b.runtime_instance_id=a.runtime_instance_id WHERE b.root_binding_id=r.root_binding_id AND (a.state IN ('pending','indeterminate') OR (a.state='accepted' AND a.effect='dispatched')))",
        ).map_err(|_|E::StorageUnavailable)?;
        let roots = stmt
            .query_map([include_observations], |row| row.get::<_, String>(0))
            .map_err(|_| E::StorageUnavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| E::StorageUnavailable)?;
        drop(stmt);
        for root in &roots {
            conn.execute("UPDATE workspace_root_bindings SET proof_state='released',proof_revision=proof_revision+1 WHERE root_binding_id=?1",[root])
                .map_err(|_|E::StorageUnavailable)?;
        }
        Ok(roots)
    }

    fn now() -> ControlResult<String> {
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| E::StorageUnavailable)
    }

    fn open_directory(path: &Path) -> ControlResult<File> {
        if !path.is_absolute() {
            return Err(E::WorkspaceInvalid);
        }
        let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| E::WorkspaceInvalid)?;
        let how = OpenHow {
            flags: (nix::libc::O_PATH | nix::libc::O_DIRECTORY | nix::libc::O_CLOEXEC) as u64,
            mode: 0,
            resolve: RESOLVE_NO_MAGICLINKS,
        };
        // SAFETY: both pointers refer to live, correctly laid out values for the syscall.
        let fd = unsafe {
            nix::libc::syscall(
                nix::libc::SYS_openat2,
                nix::libc::AT_FDCWD,
                path.as_ptr(),
                &how as *const OpenHow,
                std::mem::size_of::<OpenHow>(),
            )
        };
        if fd < 0 {
            let error = Errno::last();
            return Err(
                if matches!(error, Errno::ENOSYS | Errno::EOPNOTSUPP | Errno::EINVAL) {
                    E::Unsupported
                } else {
                    E::UnsafePath
                },
            );
        }
        // SAFETY: the successful openat2 call returned a fresh owned descriptor.
        let owned = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        Ok(File::from(owned))
    }

    fn ancestry(root: &File) -> ControlResult<Vec<ObjectIdentity>> {
        let mut chain = Vec::with_capacity(MAX_ANCESTORS);
        let mut current = root.try_clone().map_err(|_| E::StorageUnavailable)?;
        let root_mount = object_identity(&current)?.mount_id;
        for _ in 0..MAX_ANCESTORS {
            let identity = object_identity(&current)?;
            if identity.mount_id != root_mount {
                return Err(E::Unsupported);
            }
            let at_root = chain.last().is_some_and(|last: &ObjectIdentity| {
                last.filesystem == identity.filesystem && last.object == identity.object
            });
            chain.push(identity.clone());
            let parent = openat(
                Some(current.as_raw_fd()),
                "..",
                OFlag::O_PATH | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| E::Unsupported)?;
            // SAFETY: `openat` returned a fresh descriptor owned by this call.
            let parent = unsafe { OwnedFd::from_raw_fd(parent) };
            let parent = File::from(parent);
            let parent_identity = object_identity(&parent)?;
            if parent_identity.mount_id != root_mount {
                // This descriptor is the boundary of the pinned mount. Keep ancestry within
                // one qualified mount; comparisons across mount IDs remain `Unproven` below.
                return Ok(chain);
            }
            if at_root
                || (parent_identity.filesystem == identity.filesystem
                    && parent_identity.object == identity.object)
            {
                return Ok(chain);
            }
            current = parent;
        }
        Err(E::Unsupported)
    }

    fn snapshot_directory(
        directory: &File,
        relative: &Path,
        depth: usize,
        digest: &mut Sha256,
        entries_seen: &mut usize,
        bytes_seen: &mut u64,
        deadline: Instant,
    ) -> ControlResult<()> {
        if depth > MAX_SNAPSHOT_DEPTH || Instant::now() > deadline {
            return Err(E::WorkspaceInvalid);
        }
        let before = directory.metadata().map_err(|_| E::StorageUnavailable)?;
        if !before.is_dir() {
            return Err(E::WorkspaceInvalid);
        }
        // The authority root is retained as O_PATH. Open `.` relative to that exact object for
        // enumeration; this never resolves the replaceable external locator.
        let read_fd = openat(
            Some(directory.as_raw_fd()),
            ".",
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| E::StorageUnavailable)?;
        // SAFETY: `openat` returned a new descriptor owned by this call.
        let read_dir = unsafe { File::from_raw_fd(read_fd) };
        let mut dir = Dir::from(read_dir).map_err(|_| E::StorageUnavailable)?;
        let mut names = Vec::new();
        for entry in dir.iter() {
            let entry = entry.map_err(|_| E::StorageUnavailable)?;
            let name = entry.file_name().to_bytes();
            if name != b"." && name != b".." {
                names.push(std::ffi::OsString::from_vec(name.to_vec()));
            }
        }
        names.sort_by(|a, b| a.as_os_str().as_bytes().cmp(b.as_os_str().as_bytes()));

        for name in names {
            *entries_seen = (*entries_seen).saturating_add(1);
            if *entries_seen > MAX_SNAPSHOT_ENTRIES || Instant::now() > deadline {
                return Err(E::WorkspaceInvalid);
            }
            let child_relative = relative.join(&name);
            let metadata_fd = openat(
                Some(directory.as_raw_fd()),
                name.as_os_str(),
                OFlag::O_PATH | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| E::UnsafePath)?;
            // SAFETY: `openat` returned a new descriptor owned by this call.
            let metadata_fd = unsafe { File::from_raw_fd(metadata_fd) };
            let metadata = metadata_fd.metadata().map_err(|_| E::StorageUnavailable)?;
            if name == ".git" && metadata.is_dir() {
                continue;
            }

            hash_field(digest, child_relative.as_os_str().as_bytes());
            digest.update((metadata.mode() & 0o7777).to_le_bytes());
            if metadata.is_dir() {
                digest.update(*b"d");
                let child_fd = openat(
                    Some(directory.as_raw_fd()),
                    name.as_os_str(),
                    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|_| E::UnsafePath)?;
                // SAFETY: `openat` returned a new descriptor owned by this call.
                let child = unsafe { File::from_raw_fd(child_fd) };
                let opened = child.metadata().map_err(|_| E::StorageUnavailable)?;
                if !same_metadata(&metadata, &opened) {
                    return Err(E::WorkspaceInvalid);
                }
                snapshot_directory(
                    &child,
                    &child_relative,
                    depth + 1,
                    digest,
                    entries_seen,
                    bytes_seen,
                    deadline,
                )?;
            } else if metadata.file_type().is_symlink() {
                digest.update(*b"l");
                // Linux supports readlinkat(fd, "") for an O_PATH descriptor to the symlink
                // itself. This avoids resolving the child's name after enumeration.
                let target = readlinkat(Some(metadata_fd.as_raw_fd()), c"")
                    .map_err(|_| E::WorkspaceInvalid)?;
                hash_field(digest, target.as_os_str().as_bytes());
                let after = metadata_fd.metadata().map_err(|_| E::StorageUnavailable)?;
                if !same_metadata(&metadata, &after) {
                    return Err(E::WorkspaceInvalid);
                }
            } else if metadata.is_file() {
                digest.update(*b"f");
                if metadata.len() > MAX_SNAPSHOT_BYTES.saturating_sub(*bytes_seen) {
                    return Err(E::WorkspaceInvalid);
                }
                let file_fd = openat(
                    Some(directory.as_raw_fd()),
                    name.as_os_str(),
                    OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK,
                    Mode::empty(),
                )
                .map_err(|_| E::UnsafePath)?;
                // SAFETY: `openat` returned a new descriptor owned by this call.
                let mut file = unsafe { File::from_raw_fd(file_fd) };
                let opened = file.metadata().map_err(|_| E::StorageUnavailable)?;
                if !opened.is_file() || !same_metadata(&metadata, &opened) {
                    return Err(E::WorkspaceInvalid);
                }
                let mut buffer = [0u8; 64 * 1024];
                let mut size = 0u64;
                loop {
                    if Instant::now() > deadline {
                        return Err(E::WorkspaceInvalid);
                    }
                    let count = file.read(&mut buffer).map_err(|_| E::StorageUnavailable)?;
                    if count == 0 {
                        break;
                    }
                    size = size.saturating_add(count as u64);
                    *bytes_seen = (*bytes_seen).saturating_add(count as u64);
                    if *bytes_seen > MAX_SNAPSHOT_BYTES {
                        return Err(E::WorkspaceInvalid);
                    }
                    digest.update(&buffer[..count]);
                }
                let after = file.metadata().map_err(|_| E::StorageUnavailable)?;
                if size != metadata.len()
                    || !same_metadata(&opened, &after)
                    || metadata.len() != opened.len()
                {
                    return Err(E::WorkspaceInvalid);
                }
                digest.update(size.to_le_bytes());
            } else {
                return Err(E::WorkspaceInvalid);
            }
        }
        let after = directory.metadata().map_err(|_| E::StorageUnavailable)?;
        if !same_metadata(&before, &after) {
            return Err(E::WorkspaceInvalid);
        }
        Ok(())
    }

    fn same_metadata(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
        left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.mode() == right.mode()
            && left.len() == right.len()
            && left.mtime() == right.mtime()
            && left.mtime_nsec() == right.mtime_nsec()
            && left.ctime() == right.ctime()
            && left.ctime_nsec() == right.ctime_nsec()
    }

    fn hash_field(digest: &mut Sha256, bytes: &[u8]) {
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }

    fn require_quiescent_attempt(
        conn: &Connection,
        claim_id: &str,
        attempt_id: &str,
    ) -> ControlResult<String> {
        let (turn_id, send_state, session_id, turn_state): (String, String, String, String) = conn
            .query_row(
                "SELECT a.turn_id,a.send_state,t.managed_session_id,t.state FROM guarded_attempts a JOIN managed_turns t ON t.managed_turn_id=a.turn_id WHERE a.claim_id=?1 AND a.attempt_id=?2",
                params![claim_id, attempt_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|_| E::GateDenied)?;
        if matches!(turn_state.as_str(), "starting" | "running") {
            return Err(E::GateDenied);
        }
        let pending_approval: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM managed_approvals WHERE managed_turn_id=?1 AND (state='pending' OR (state='accepted' AND effect='dispatched')))",
                [&turn_id],
                |row| row.get(0),
            )
            .map_err(|_| E::StorageUnavailable)?;
        if pending_approval {
            return Err(E::GateDenied);
        }
        let active_turn: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM managed_turns WHERE managed_session_id=?1 AND state IN ('starting','running'))",
                [&session_id],
                |row| row.get(0),
            )
            .map_err(|_| E::StorageUnavailable)?;
        if active_turn {
            return Err(E::GateDenied);
        }
        if !matches!(
            send_state.as_str(),
            "reserved" | "possibly_sent" | "verifying" | "known_no_send" | "terminal"
        ) {
            return Err(E::GateDenied);
        }
        Ok(send_state)
    }

    fn object_identity(file: &File) -> ControlResult<ObjectIdentity> {
        let stat = fstatfs(file.as_fd()).map_err(|_error| {
            #[cfg(test)]
            eprintln!("workspace pin fstatfs failed: {_error}");
            E::Unsupported
        })?;
        let fs_type = stat.filesystem_type();
        if fs_type != BTRFS_SUPER_MAGIC && fs_type != EXT4_SUPER_MAGIC {
            #[cfg(test)]
            eprintln!("workspace pin unsupported filesystem magic: {}", fs_type.0);
            return Err(E::Unsupported);
        }
        let fsid = stat.filesystem_id();
        // fsid_t is the kernel's fixed-size filesystem-instance discriminator. It is only
        // combined with an opaque handle for live comparisons, never used alone as identity.
        let fsid_bytes = unsafe {
            std::slice::from_raw_parts(
                (&fsid as *const nix::sys::statfs::fsid_t).cast::<u8>(),
                std::mem::size_of_val(&fsid),
            )
        };
        let mut handle = FileHandle {
            handle_bytes: MAX_HANDLE_BYTES as u32,
            handle_type: 0,
            bytes: [0; MAX_HANDLE_BYTES],
        };
        let empty = c"";
        let mut mount_id: i32 = 0;
        // SAFETY: `file`, `empty`, `handle`, and `mount_id` stay alive across the syscall;
        // the kernel is given the exact bounded buffer size and AT_EMPTY_PATH uses this FD.
        let result = unsafe {
            nix::libc::syscall(
                nix::libc::SYS_name_to_handle_at,
                file.as_raw_fd(),
                empty.as_ptr(),
                &mut handle as *mut FileHandle,
                &mut mount_id as *mut i32,
                AT_EMPTY_PATH,
            )
        };
        if result != 0
            || handle.handle_bytes == 0
            || handle.handle_bytes as usize > MAX_HANDLE_BYTES
            || handle.handle_type == 0
            || mount_id <= 0
        {
            #[cfg(test)]
            eprintln!(
                "workspace pin handle query failed: errno={}, bytes={}, type={}, mount_id={mount_id}",
                Errno::last(),
                handle.handle_bytes,
                handle.handle_type
            );
            return Err(E::Unsupported);
        }
        let handle_len = handle.handle_bytes as usize;
        let mut filesystem = Sha256::new();
        filesystem.update(fs_type.0.to_le_bytes());
        filesystem.update(fsid_bytes);
        let filesystem: [u8; 32] = filesystem.finalize().into();
        let mut object = Sha256::new();
        object.update(filesystem);
        object.update(handle.handle_type.to_le_bytes());
        object.update((handle_len as u64).to_le_bytes());
        object.update(&handle.bytes[..handle_len]);
        Ok(ObjectIdentity {
            filesystem,
            object: object.finalize().into(),
            mount_id,
        })
    }

    fn hex(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for &byte in bytes {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{fs, os::unix::fs::symlink};

        struct Temp(PathBuf);
        impl Temp {
            fn new() -> Self {
                // `/tmp` may be tmpfs even when the project volume is a qualified Btrfs/ext4
                // filesystem. Keep fixtures beside the build artifacts so this test exercises
                // the actual filesystem the compiler used, without touching source files.
                let base = std::env::var_os("CARGO_TARGET_DIR")
                    .map(PathBuf::from)
                    .or_else(|| {
                        std::env::current_exe()
                            .ok()?
                            .parent()?
                            .parent()
                            .map(Path::to_path_buf)
                    })
                    .expect("cargo target directory is available");
                let base = if base.is_absolute() {
                    base
                } else {
                    std::env::current_dir().unwrap().join(base)
                };
                let path = base.join(format!("wyss-f8-pin-{}", uuid::Uuid::new_v4()));
                fs::create_dir(&path).unwrap();
                Self(path)
            }
            fn path(&self, name: &str) -> PathBuf {
                self.0.join(name)
            }
        }
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        fn pin(path: &Path) -> RootPin {
            RootPin::open(
                path,
                "00000000-0000-4000-8000-000000000001",
                "00000000-0000-4000-8000-000000000002",
                "00000000-0000-4000-8000-000000000003",
            )
            .unwrap()
        }

        fn named_pin(path: &Path, store: &str, epoch: &str, root: &str) -> RootPin {
            RootPin::open(path, store, epoch, root).unwrap()
        }

        #[test]
        fn ordinary_start_is_allowed_only_for_proven_disjoint_live_roots() {
            let tmp = Temp::new();
            let claimed_root = tmp.path("claimed");
            let independent_root = tmp.path("independent");
            fs::create_dir(&claimed_root).unwrap();
            fs::create_dir(&independent_root).unwrap();
            let store = "00000000-0000-4000-8000-000000000011";
            let epoch = "00000000-0000-4000-8000-000000000012";
            let claim_root = "00000000-0000-4000-8000-000000000013";
            let independent_binding = "00000000-0000-4000-8000-000000000014";
            let claim_pin = named_pin(&claimed_root, store, epoch, claim_root);
            let independent_pin = named_pin(&independent_root, store, epoch, independent_binding);
            let guard = WorkspaceGuard {
                store_id: store.into(),
                epoch: epoch.into(),
                pins: BTreeMap::from([
                    (claim_root.into(), claim_pin),
                    (independent_binding.into(), independent_pin),
                ]),
                prepared_start_deadlines: BTreeMap::new(),
            };
            let mut conn = Connection::open_in_memory().unwrap();
            conn.execute_batch(
                "CREATE TABLE workspace_guard_state(id INTEGER PRIMARY KEY,authority_store_id TEXT,import_state TEXT);
                 CREATE TABLE workspace_imported_claims(claim_id TEXT);
                 CREATE TABLE workspace_root_bindings(root_binding_id TEXT,proof_state TEXT,authority_store_id TEXT,authority_epoch TEXT,proof_revision INTEGER);
                 CREATE TABLE workspace_claims(claim_id TEXT,root_binding_id TEXT,phase TEXT);
                 CREATE TABLE managed_runtime_workspace_bindings(runtime_instance_id TEXT,root_binding_id TEXT,authority_store_id TEXT,authority_epoch TEXT);
                 INSERT INTO workspace_guard_state VALUES(1,'00000000-0000-4000-8000-000000000011','complete');
                 INSERT INTO workspace_root_bindings VALUES('00000000-0000-4000-8000-000000000013','pinned','00000000-0000-4000-8000-000000000011','00000000-0000-4000-8000-000000000012',1);
                 INSERT INTO workspace_root_bindings VALUES('00000000-0000-4000-8000-000000000014','pinned','00000000-0000-4000-8000-000000000011','00000000-0000-4000-8000-000000000012',1);
                 INSERT INTO workspace_claims VALUES('00000000-0000-4000-8000-000000000015','00000000-0000-4000-8000-000000000013','held');
                 INSERT INTO managed_runtime_workspace_bindings VALUES('00000000-0000-4000-8000-000000000016','00000000-0000-4000-8000-000000000014','00000000-0000-4000-8000-000000000011','00000000-0000-4000-8000-000000000012');",
            )
            .unwrap();
            assert!(
                guard
                    .ordinary_start_allowed(&mut conn, "00000000-0000-4000-8000-000000000016")
                    .unwrap()
            );

            conn.execute(
                "UPDATE managed_runtime_workspace_bindings SET root_binding_id=?1 WHERE runtime_instance_id=?2",
                params![claim_root, "00000000-0000-4000-8000-000000000016"],
            )
            .unwrap();
            assert!(
                !guard
                    .ordinary_start_allowed(&mut conn, "00000000-0000-4000-8000-000000000016")
                    .unwrap()
            );
        }

        #[test]
        fn equal_content_replacement_does_not_match_the_retained_directory_pin() {
            let tmp = Temp::new();
            let root = tmp.path("workspace");
            fs::create_dir(&root).unwrap();
            fs::write(root.join("same.txt"), b"same bytes").unwrap();
            let original = pin(&root);
            let moved = tmp.path("original-moved");
            fs::rename(&root, &moved).unwrap();
            fs::create_dir(&root).unwrap();
            fs::write(root.join("same.txt"), b"same bytes").unwrap();
            assert!(!original.locator_matches().unwrap());
            let replacement = pin(&root);
            assert!(!original.same_live_object(&replacement));
            assert!(original.same_locator(&replacement));
        }

        #[test]
        fn content_digest_uses_the_retained_root_after_locator_replacement() {
            let tmp = Temp::new();
            let root = tmp.path("workspace");
            fs::create_dir(&root).unwrap();
            fs::write(root.join("same.txt"), b"original bytes").unwrap();
            let original = pin(&root);
            let expected = crate::orchestration::workspace_revision(&root).unwrap();

            let moved = tmp.path("original-moved");
            fs::rename(&root, &moved).unwrap();
            fs::create_dir(&root).unwrap();
            fs::write(root.join("same.txt"), b"replacement bytes").unwrap();

            assert!(!original.locator_matches().unwrap());
            assert_eq!(original.content_digest().unwrap(), expected);
            assert_ne!(
                original.content_digest().unwrap(),
                crate::orchestration::workspace_revision(&root).unwrap()
            );
        }

        #[test]
        fn retargeted_symlink_does_not_rebind_an_existing_pin() {
            let tmp = Temp::new();
            let first = tmp.path("first");
            let second = tmp.path("second");
            fs::create_dir(&first).unwrap();
            fs::create_dir(&second).unwrap();
            let link = tmp.path("workspace-link");
            symlink(&first, &link).unwrap();
            let original = pin(&link);
            fs::remove_file(&link).unwrap();
            symlink(&second, &link).unwrap();
            assert!(!original.locator_matches().unwrap());
            assert!(!original.same_live_object(&pin(&link)));
        }

        #[test]
        fn parent_child_conflict_but_sibling_workspaces_are_disjoint() {
            let tmp = Temp::new();
            let parent = tmp.path("parent");
            fs::create_dir(&parent).unwrap();
            fs::create_dir(parent.join("child")).unwrap();
            fs::create_dir(parent.join("sibling-a")).unwrap();
            fs::create_dir(parent.join("sibling-b")).unwrap();
            let parent_pin = pin(&parent);
            let child_pin = pin(&parent.join("child"));
            let sibling_a = pin(&parent.join("sibling-a"));
            let sibling_b = pin(&parent.join("sibling-b"));
            assert_eq!(parent_pin.overlap(&child_pin), Overlap::Conflict);
            assert_eq!(sibling_a.overlap(&sibling_b), Overlap::Disjoint);
        }

        #[test]
        fn symlink_aliases_are_same_live_object() {
            let tmp = Temp::new();
            let root = tmp.path("workspace");
            fs::create_dir(&root).unwrap();
            let alias = tmp.path("alias");
            symlink(&root, &alias).unwrap();
            let direct = pin(&root);
            let via_alias = pin(&alias);
            assert!(direct.same_live_object(&via_alias));
            assert_eq!(direct.overlap(&via_alias), Overlap::Conflict);
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::{Claim, ImportedClaim, Overlap, PreparedStart, RootPin, WorkspaceGuard};
