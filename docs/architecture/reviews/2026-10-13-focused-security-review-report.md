# Focused independent security review, 2026-10-13: the reviewer's report

Provenance, written by the author of the set and not by the reviewer. Everything below the
line is the reviewer's text; only its whitespace was normalised by the formatter.

| Aspect            | Fact                                                                                                                                                                                                                    |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Authorised by     | The owner, 2026-10-13: one bounded review, target 80,000 tokens, absolute limit 100,000                                                                                                                                 |
| Reviewer          | GPT-6 Luna at its highest reasoning setting, through Codex in a read-only sandbox; one pass; a fresh context with none of the author's reasoning                                                                        |
| What it was given | A literal extract of the uncommitted working tree (based on `537eabc`), built by a script that selected sections and collapsed table padding without changing a word; the ten checks the owner listed; an output format |
| What it read      | ADR 0029, ADR 0030, 27 and 28 whole; sections of 25, 07, 08, 09, 11, 14, 19, 20, 21, 22; parts of accepted ADR 0003 and ADR 0028. Its own "Coverage" section lists what was left out                                    |
| What it did not   | It opened no file: it did not check the extract against the documents. It ran nothing                                                                                                                                   |
| Cost              | **116,038 tokens: 16 % over the absolute limit.** The wrapper reports the total only at the end, so the run could not be stopped at the limit                                                                           |
| After the review  | Nothing was changed in the documents it evaluated. Its findings are not yet reconciled                                                                                                                                  |

---

## Verdict

**REJECT.** The proposal permits clear copies of configuration outside the one-vault exception it defines. Its write and recovery contracts conflict across documents, and they still allow security-critical updates to be lost without detection. Several controls depend on unverified filesystem behavior or a compromised renderer’s confirmation.

## Coverage

Reviewed the complete extract: ADRs 0029 and 0030; architectures 27 and 28; the specified sections of 25, 07, 08, 09, 11, 14, 19, 20, 21 and 22; the persistence section of ADR 0003; the specified sections and rows of accepted ADR 0028; and, after forming the findings below, the appendix of earlier findings.

Not reviewed: the other documents and sections expressly excluded in the prompt, including 07 outside §§5.4–5.5; 08 §§3.2 and 6; 09 §§3.1 and 3.3; 11 §7; 19 phases F2a and F2h; and the listed DN-22, DN-25–27 and DN-29 material. No token-driven omissions. No tools or workspace files were read.

## Findings

**F-01 | HIGH | DEFECT**

**Where:** `docs/architecture/14-DATA-PERSISTENCE-AND-RECOVERY.md` §§3, 8; `docs/architecture/27-PROTECTED-RESTORATION-VAULT.md` §§3.3, 8.2; ADR 0029 §2.

**Evidence and failure scenario:** The installation key creates the keyed entry digests; vault records store those digests and keyed neighbor anchors. “Delete history” removes the installation key but leaves vault records. If the provider file later changes, the Console can no longer compute comparable digests to recognize a returned entry or use the anchors. The records remain decryptable, but recovery and lifecycle behavior can degrade while the action claims to lose “history only.”

**Impact:** Deleting history silently changes the vault’s recovery capabilities.

**Required correction:** Give vault records a separate key with a compatible lifecycle, or prevent deletion of their digest key while records depend on it.

**Acceptance criterion:** Tests show history deletion, restart, index rebuild, changed-file restoration and reappearance detection all work with held records.

---

**F-02 | HIGH | DEFECT**

**Where:** ADR 0029 §§1, 2 categories 4–5 and §10; `docs/architecture/28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md` §§6–7.

**Evidence and failure scenario:** ADR 0029 makes the vault the _only_ exception for durably holding configuration content that may contain secrets. Yet the write protocol creates whole-file copies beside provider files, and conflict preservation permits a clear sidecar by user choice. A crash can leave a transient file indefinitely; a replicated provider directory can carry it to the other computer. A settings file containing a token therefore remains in clear outside the vault.

**Impact:** The proposed policy contradicts its own narrow exception and the owner’s constraint against unrelated secret-storage relaxations.

**Required correction:** Either ensure these copies cannot persist in clear, or explicitly define and obtain approval for a separate, bounded exception, including its consent and recovery behavior.

**Acceptance criterion:** A crash test proves no unauthorized clear copy survives anywhere the policy covers, or the accepted policy names the exact additional class and limits.

---

**F-03 | HIGH | DEFECT**

**Where:** `docs/architecture/07-CONFIGURATION-ENGINE.md` §5.4; `docs/architecture/28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md` §§3, 9; `docs/architecture/14-DATA-PERSISTENCE-AND-RECOVERY.md` §4; `docs/architecture/20-VERIFICATION-AND-ACCEPTANCE.md` §3.6; `docs/architecture/27-PROTECTED-RESTORATION-VAULT.md` §8.2.

**Evidence and failure scenario:** The 07 state machine is W-unnamed: it removes the old file’s name after verification and may roll back by copying from an open descriptor. The recommended 28 variant is W-named, which keeps the old inode named through settling. TS-WR-13 tests W-named, while 14 §4 still gives the W-unnamed recovery table and 27 §8.2 delegates to 07. A build following 07 can therefore fail the promised W-named rollback and crash behavior.

**Impact:** The implementer and tests do not share one normative write protocol.

**Required correction:** Choose one variant and make 07, 14, 27, 28 and the tests describe the same steps, outcomes and recovery rules.

**Acceptance criterion:** One state machine and matching fault-injection tests cover every stop and concurrent-write ordering for the chosen variant.

---

**F-04 | HIGH | DEFECT**

**Where:** `docs/architecture/07-CONFIGURATION-ENGINE.md` §5.5; `docs/architecture/28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md` §§4.1, 5, 9.

**Evidence and failure scenario:** 07 §5.5 says provider writes have named outcomes and “none is silent.” But 28 §4.1 case 4 and §5 L1 say a process holding the old inode can write a security-denying rule after settling; the write becomes invisible and is lost. L5 also permits a replicated version to replace a control while the Console is closed, with detection only at the next observation.

**Impact:** A security-critical provider change can be lost silently, while an in-scope document says it cannot.

**Required correction:** Remove the blanket “none is silent” claim and state the unresolved loss cases beside the applicable write contract. Keep mutations gated until DN-21 accepts those limits.

**Acceptance criterion:** The text and tests identify case 4 and L5 as possible losses and never report them as detected or preserved.

---

**F-05 | HIGH | DEFECT**

**Where:** `docs/architecture/07-CONFIGURATION-ENGINE.md` §5.4, `rolling_back`; `docs/architecture/28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md` §6; ADR 0029 §10.

**Evidence and failure scenario:** After three rollback attempts, 07 says the version that cannot be put back is written beside the target under a visible name. 28 instead recommends a vault record, then runtime storage, with a clear sidecar only by explicit choice. If rollback exhausts its attempts, the 07 path creates that clear sidecar before any such choice.

**Impact:** A conflict can leak an entire settings file—including unrelated secrets—into a replicated directory.

**Required correction:** Route the failure through DN-28’s selected preservation path; specify what happens if consent, vault custody or runtime storage is unavailable before discarding any version.

**Acceptance criterion:** Fault tests at every rollback failure show the version lands only in the chosen store, or the user explicitly chooses the warned clear sidecar.

---

**F-06 | HIGH | DEFECT**

**Where:** `docs/architecture/27-PROTECTED-RESTORATION-VAULT.md` §§3.6, 6–7; ADR 0030, Correction 1.

**Evidence and failure scenario:** `keep_copy` accepts entry identifiers and requires a confirmation in the page, while class consent is standing and native. Compromised page code can call the command directly and create copies for many entries; it can bypass the page confirmation. That contradicts the rule that each copy is made only by the user’s explicit act.

**Impact:** A compromised renderer can create durable, secret-bearing duplicates without the per-entry choice the design requires.

**Required correction:** Require a native confirmation listing the exact entries for every copy-creation request, or define an independently enforced backend authorization that cannot be supplied by renderer code.

**Acceptance criterion:** A test renderer calling `keep_copy` directly cannot create a record without the required trusted confirmation.

---

**F-07 | MEDIUM | UNKNOWN**

**Where:** ADR 0029 §2 category 2 and §1; `docs/architecture/14-DATA-PERSISTENCE-AND-RECOVERY.md` §3.

**Evidence and failure scenario:** The journal may persist hashes of files before and after changes, while ADR 0029 rejects unkeyed digests because guesses against short secrets can be confirmed. If a file hash is an ordinary unkeyed digest and an attacker knows the rest of a settings file, candidate values for a low-entropy secret can be tested against the journal.

**Impact:** A permitted journal field may reveal whether a secret guess is correct.

**Required correction:** Specify the hash construction and its keying. Do not persist an unkeyed whole-file digest that includes secret-bearing entries.

**Acceptance criterion:** A documented construction and test show journal artifacts cannot serve as a candidate-secret confirmation oracle.

---

**F-08 | MEDIUM | UNKNOWN**

**Where:** `docs/architecture/27-PROTECTED-RESTORATION-VAULT.md` §§3.3, 8.3; `docs/architecture/08-RULES-HOOKS-SKILLS-MCP.md` §3.3; `docs/architecture/20-VERIFICATION-AND-ACCEPTANCE.md` §3.7.

**Evidence and failure scenario:** When a file differs from h1, restoration uses recorded position and keyed neighbor digests. If identical entries or repeated groups create duplicate matching anchors, the extract gives no rule for choosing among matches. A hook could be reinserted into the wrong group or position, or duplicated, even though the plan is shown as a diff.

**Impact:** Changed-file restoration may alter executable behavior without a well-defined conflict outcome.

**Required correction:** Define uniqueness requirements and fail closed when anchors or positions are ambiguous.

**Acceptance criterion:** Tests with duplicate entries and repeated anchors return `restore_conflict` and write nothing unless the target is unambiguous.

---

**F-09 | MEDIUM | DEFECT**

**Where:** `docs/architecture/14-DATA-PERSISTENCE-AND-RECOVERY.md` §§3–4; `docs/architecture/27-PROTECTED-RESTORATION-VAULT.md` §8.2.

**Evidence and failure scenario:** Recovery relies on mutation intents without outcomes, but the mutation journal is bounded by 180 days or 10,000 entries. The extract does not exempt unresolved intents from retention. If cleanup removes an intent before startup repair, the repair procedure may lack the operation-to-record mapping needed to classify an interrupted vault write.

**Impact:** A crash-recovery guarantee depends on a journal entry that the retention rule may delete.

**Required correction:** Preserve unresolved intents until repair records an outcome, and define recovery-before-compaction ordering.

**Acceptance criterion:** Tests retain and repair an unresolved intent after exceeding each retention bound.

---

**F-10 | MEDIUM | UNKNOWN**

**Where:** `docs/architecture/27-PROTECTED-RESTORATION-VAULT.md` §§5.3, 12; `docs/architecture/20-VERIFICATION-AND-ACCEPTANCE.md` §3.7; OQ-39.

**Evidence and failure scenario:** The summary claims no entry content is on disk in clear after any operation, but TS-VLT-01 searches only the state, provider and runtime directories. OQ-39 leaves swap and core-dump handling open, and §5.3 calls swap protection best effort. A crash dump or swap file can therefore retain decrypted bytes outside the test’s search locations.

**Impact:** The at-rest protection claim is broader than the proposed verification and may not hold on supported systems.

**Required correction:** Narrow the guarantee to named artifacts, or specify platform controls and a verification scope that includes swap and core dumps.

**Acceptance criterion:** The supported-system contract and tests account for every location included in the stated guarantee.

---

**F-11 | MEDIUM | UNKNOWN**

**Where:** `docs/architecture/27-PROTECTED-RESTORATION-VAULT.md` §§2, 9.1–9.2; ADR 0029 §§2, 9; OQ-13 and OQ-32.

**Evidence and failure scenario:** The text says the vault is never placed where replicated, but detection only rejects known replication markers and explicitly is not proof that nothing copies the directory. A state directory under an unrecognized sync tool passes the check and is copied to the other computer. Separately, the two-device case assumes a conflict copy exists; OQ-13 leaves the tool’s behavior unverified.

**Impact:** Replication and conflict behavior are not guaranteed across tools or configurations, despite some absolute wording.

**Required correction:** Limit the claim to detected tools and supported configurations, or define a reliable location policy and enumerate the sync behaviors covered.

**Acceptance criterion:** The supported-tool matrix tests recognized and unrecognized cases, and user-facing claims match the actual detection boundary.

## The ten checks

1. **Secret exposure:** Not fully. F-02, F-05, F-06, F-07 and F-10 identify clear copies, renderer-triggered copies, digest leakage risk and untested memory remnants.

2. **Exact restoration:** Justified only when the current file still matches h1. Changed-file restoration is not byte-identical; F-08 identifies an unresolved ambiguity in anchor matching. The unqualified F2v objective needs that condition.

3. **Silent loss of provider changes:** Yes. F-04 documents silent loss of a late write to the old inode and delayed detection of replication replacing a control.

4. **Crash recovery consistency:** No. F-03 identifies incompatible W-unnamed/W-named contracts; F-09 identifies unresolved-intent retention risk; F-01 identifies a key lifecycle mismatch affecting vault records.

5. **Optional recovery copies:** They add a standing duplicate of potentially stale secrets, which the text discloses. F-06 shows the per-entry choice is not enforceable against compromised page code under the stated interface.

6. **Keyring, recovery key and consent:** Specified, but not yet enforceable or fully settled. OQ-38 and DN-25/26 remain open; F-06 also shows a consented class does not enforce an individual recovery-copy choice.

7. **Syncthing and two devices:** The per-computer vault and asymmetry are represented clearly, and replication can replace a control while the Console is closed. F-11 limits the guarantee: marker checks and conflict-copy assumptions are not universal.

8. **ADR coherence:** The proposed ADRs do not silently edit accepted ADRs, and their statuses are explicit. But ADR 0029’s vault-only exception conflicts with clear transient and conflict copies (F-02, F-05), and its permitted file hashes need clarification (F-07).

9. **Evidence vs. assumptions:** The documents label many behaviors as proposed or unverified. F-03, F-04, F-10 and F-11 show guarantees that still exceed the specified evidence or conflict with another section.

10. **Compromised renderer:** Most limits are stated honestly, but F-06 relies on the renderer’s page confirmation for `keep_copy`, so this control does rely on a trustworthy page.

## Against the earlier review

- **Resolved:** IR-02, IR-03, IR-04, IR-06, IR-07, IR-08, IR-18. The current text adds explicit vault crash states, rejects classifier-based persistence, names copy locations, specifies metadata refusal and descriptor-based checks, corrects the compromised-page claim, and updates reconciliation.
- **Still open:** IR-01, IR-05. ADR 0030 corrects ADR 0028, but 07 §5.5 retains a false “none is silent” claim; V7 is stronger, yet V4 still covers only rewrite paths that research finds.
- **Cannot be judged from this extract:** IR-09, IR-10, IR-11, IR-12, IR-13, IR-14, IR-15, IR-16, IR-17, IR-19, IR-20. Their cited profile, managed-plane, watcher, roadmap, journey, remote, distribution or interface material is outside the supplied sections.

## New problems introduced by the vault architecture

- **F-01:** Deleting history removes a key that vault records still depend on.
- **F-02 / F-05:** Whole-file copies can exist in clear outside the vault.
- **F-06:** Renderer code can create recovery copies despite the stated per-entry user choice.
- **F-07:** Permitted journal hashes may confirm guesses about secrets.
- **F-09:** Journal retention may remove unresolved recovery intents.
- **F-10 / F-11:** At-rest and replication claims exceed their proposed checks.

## Unverified claims

- `07 §5.5`: “every ordering” has a named outcome and “none is silent”; contradicted by 28 §4.1 case 4 and L1.
- `14 §8`: deleting history loses “history only”; not established while it deletes the digest key used by vault records (F-01).
- `27 §2`: the vault is “never placed where it is replicated”; §9.1 only checks known markers and disclaims proof (F-11).
- `27 §8.1`: VA1 holds “at every instant”; 28 §2 qualifies it on filesystem durability and flush behavior.
- `27 §12`: no plaintext exists “anywhere” after any operation; TS-VLT-01 searches limited directories and OQ-39 is open (F-10).
- `19 F2v objective`: the hook is restored “exactly”; that is true only when the source remains at h1 (F-08).
- `27 §9.2`: concurrent two-device changes are blocked by a conflict copy; that depends on the replication tool producing a detectable conflict artifact (F-11).
- `28 §6`: a runtime conflict version’s loss is “not silent” because the journal records it; F-09 leaves open whether that record survives retention.
- `27 §5.5`: the recovery matrix says the code rebuilds the key and makes every record readable; the wrapped-key file must also survive and authenticate, as the preceding failure case acknowledges.

## Readiness

- **ADR 0029:** No. Resolve F-01, F-02, F-05, F-06, F-07 and F-09, and reconcile its policy wording with the remaining vault and recovery decisions.
- **ADR 0030:** Only after F-04 is corrected in 07 §5.5 and the replacement text is consistent across the set.
- **Safe basis for future implementation:** No. At minimum, close F-01–F-07 and F-09–F-11, resolve F-08’s ambiguous restoration behavior, and decide DN-21 and DN-28 against the applicable provider and filesystem evidence.
