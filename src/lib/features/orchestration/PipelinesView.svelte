<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type {
    CoordinatorSnapshot,
    HandoffConfirmationV1,
    HandoffEnvelopeV1,
    HandoffItemV1,
    OrchestrationPlanV1,
    PlanReviewV1,
    ProviderLane,
    StepPlanV1,
    WorkspaceSnapshot,
  } from '../../contracts/orchestration';
  import { chooseWorkspace } from '../../desktop/sessions-client';
  import { t } from '../../i18n/index.svelte';
  import {
    listRuns,
    OrchestrationCallError,
    reconcileStep,
    reviewPlan,
    runAction,
    sealHandoff,
    submitHandoff,
    submitPlan,
    verifyStep,
    workspaceSnapshot,
  } from '../../desktop/orchestration-client';
  import {
    prepareSession,
    readSession,
    readSessions,
  } from '../../desktop/f8-client';
  import type { SessionRow, SessionsView } from '../../contracts/f8';
  import { readNativeSessions } from '../../desktop/native-client';
  import type { NativeSession } from '../../contracts/native';

  type Tab = 'pipelines' | 'handoff';
  type StepDraft = {
    step_id: string;
    required: boolean;
    provider: ProviderLane;
    effect: StepPlanV1['effect'];
    managed_session_id: string;
    prompt: string;
    dependencies: string[];
  };
  type SourceObservation =
    | {
        plane: 'managed';
        session_id: string;
        generation: number;
        workspace_root: string;
        session_revision: number;
      }
    | {
        plane: 'native';
        session_id: string;
        generation: number;
        workspace_root: string;
        started_at: number;
        restarts: number;
      };

  let {
    visible,
    f8Active,
    onruntimes,
  }: {
    visible: boolean;
    f8Active: boolean;
    onruntimes: () => void;
  } = $props();

  function tabKey(event: KeyboardEvent, index: number) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? 1 : 1 - index;
    tab = next === 0 ? 'pipelines' : 'handoff';
    const bar = (event.currentTarget as HTMLElement).parentElement;
    bar?.querySelectorAll<HTMLButtonElement>('[role=tab]')[next]?.focus();
  }

  const now = () => Math.floor(Date.now() / 1000);
  const newId = () => crypto.randomUUID();
  const initialStep = (): StepDraft => ({
    step_id: newId(),
    required: true,
    provider: 'managed_codex',
    effect: 'managed_inference',
    managed_session_id: '',
    prompt: '',
    dependencies: [],
  });
  let tab = $state<Tab>('pipelines');
  let busy = $state(false);
  let loadingRuns = $state(false);
  let lastError = $state('');
  let runsError = $state('');
  let sessionsError = $state('');
  let runs = $state<CoordinatorSnapshot[]>([]);
  let managedSessions = $state<SessionRow[]>([]);
  let nativeSessions = $state<NativeSession[]>([]);
  let planWorkspaceRoot = $state('');
  let planWorkspace = $state<WorkspaceSnapshot | null>(null);
  let planProjectId = $state('local-project');
  let objective = $state('');
  let durationMinutes = $state<number | undefined>(30);
  const budgetInvalid = $derived(
    !Number.isInteger(durationMinutes) ||
      durationMinutes === undefined ||
      durationMinutes < 1 ||
      durationMinutes > 60,
  );
  let steps = $state<StepDraft[]>([initialStep()]);
  let reviewedPlan = $state<OrchestrationPlanV1 | null>(null);
  let planReview = $state<PlanReviewV1 | null>(null);
  let handoffSourceRoot = $state('');
  let handoffSourceWorkspace = $state<WorkspaceSnapshot | null>(null);
  let handoffSourceSession = $state('');
  let handoffSourceGeneration = $state(1);
  let handoffSourcePlane = $state<'native' | 'managed'>('native');
  let handoffSourceProject = $state('local-project');
  let handoffSourceObservation = $state<SourceObservation | null>(null);
  let handoffTargetRoot = $state('');
  let handoffTargetWorkspace = $state<WorkspaceSnapshot | null>(null);
  let handoffTargetProject = $state('local-project');
  let handoffTargetProvider = $state<ProviderLane>('managed_codex');
  let handoffTargetEffect = $state<
    'managed_inference' | 'managed_local_tools' | 'native_full_account'
  >('managed_inference');
  let handoffTargetSession = $state('');
  let freshReceiverId = $state<string | null>(null);
  let receiverIntentId = $state<string | null>(null);
  let receiverReady = $state(false);
  let receiverStatus = $state(
    'Primero prepara una sesión nueva Managed para este handoff.',
  );
  let receiverSummary = $state('');
  let handoffNativeProfile = $state('');
  let handoffObjective = $state('');
  let handoffContext = $state('');
  let handoffFiles = $state('');
  let handoffQuestions = $state('');
  let handoffProvenance = $state<HandoffItemV1['provenance']>('human_entered');
  let handoffPreview = $state<{
    envelope: HandoffEnvelopeV1;
    first_message: string;
  } | null>(null);
  let handoffRun = $state<CoordinatorSnapshot | null>(null);
  let copied = $state(false);
  let observedWorkspace = $state<Record<string, WorkspaceSnapshot>>({});
  let runWorkspaceRoots = $state<Record<string, string>>({});
  const sessionOptions = $derived(
    managedSessions.filter(
      (session) =>
        session.state === 'prepared' &&
        session.runtime.actual === 'ready' &&
        session.runtime.instance !== null,
    ),
  );

  const ERROR_TEXT: Record<string, string> = {
    unavailable: 'El coordinador local no está disponible.',
    invalid_request: 'La solicitud no coincide con el contrato M6.',
    invalid_plan: 'El plan no es válido o supera los límites finitos.',
    invalid_handoff: 'El handoff está incompleto, vencido o cambió.',
    handoff_objective_too_large:
      'El objetivo supera 2.048 bytes; reduce el texto antes de sellarlo.',
    handoff_prompt_too_large:
      'El mensaje generado supera 8 KiB; reduce los extractos o preguntas antes de sellarlo.',
    review_stale:
      'El contenido cambió después de la revisión; vuelve a revisarlo.',
    trust_boundary_blocked:
      'Bloqueado: la frontera de confianza de Native no está demostrada.',
    provider_blocked:
      'El proveedor o el host no está admitido para este handoff.',
    receiver_not_fresh:
      'F8 confirmó que el receptor tiene trabajo previo o no está preparado como sesión nueva.',
    receiver_freshness_unproven:
      'No se pudo probar desde la evidencia F8 que el receptor esté nuevo y sin historial.',
    source_freshness_unproven:
      'No se pudo probar la identidad o generación actual de la sesión Managed de origen.',
    permission_denied: 'La autoridad Managed denegó el inicio.',
    resource_busy: 'El recurso Managed está ocupado; no se inició otro turno.',
    workspace_conflict:
      'El workspace cambió o no coincide con la revisión aprobada.',
    journal_unavailable: 'El registro local no pudo guardar el estado.',
    authority_unavailable:
      'La autoridad Managed no respondió; el resultado puede requerir revisión.',
    unknown_run: 'No se encontró esa ejecución.',
    invalid_transition: 'La acción no está disponible para el estado actual.',
    expired: 'La revisión o la ejecución venció.',
    effect_unknown:
      'El efecto es incierto y queda detenido para revisión humana.',
  };

  function errorText(error: unknown): string {
    return error instanceof OrchestrationCallError
      ? (ERROR_TEXT[error.code] ?? 'Falló la operación M6.')
      : 'No se pudo completar la operación M6.';
  }

  function showError(error: unknown) {
    lastError = errorText(error);
  }

  async function loadSessions() {
    sessionsError = '';
    const managed = f8Active ? readSessions(null) : Promise.resolve(null);
    const [managedResult, nativeResult] = await Promise.allSettled([
      managed,
      readNativeSessions(false),
    ]);
    if (managedResult.status === 'fulfilled' && managedResult.value) {
      const page: SessionsView = managedResult.value;
      managedSessions = page.sessions;
    } else {
      managedSessions = [];
      if (f8Active)
        sessionsError = 'No se pudieron leer sesiones Managed desde F8.';
    }
    nativeSessions =
      nativeResult.status === 'fulfilled' ? nativeResult.value : [];
    if (!f8Active && !sessionsError)
      sessionsError =
        'La autoridad Managed está inactiva; se muestran solo las sesiones Native observadas.';
  }

  async function refreshRuns() {
    if (loadingRuns) return;
    loadingRuns = true;
    try {
      runs = await listRuns();
      runsError = '';
    } catch (error) {
      runsError = errorText(error);
    } finally {
      loadingRuns = false;
    }
  }

  $effect(() => {
    if (!visible) return;
    // These operations toggle their own loading state. Keep those writes from
    // becoming dependencies of the visibility effect or the monitor will
    // continually reschedule itself.
    untrack(() => {
      void refreshRuns();
      void loadSessions();
    });
    const timer = window.setInterval(() => void refreshRuns(), 1500);
    return () => window.clearInterval(timer);
  });

  function invalidatePlanReview() {
    reviewedPlan = null;
    planReview = null;
    lastError = '';
  }

  function updateStep(id: string, patch: Partial<StepDraft>) {
    steps = steps.map((step) =>
      step.step_id === id ? { ...step, ...patch } : step,
    );
    invalidatePlanReview();
  }

  function changeProvider(step: StepDraft, provider: ProviderLane) {
    const effect =
      provider === 'hermes_planner'
        ? 'proposal_only'
        : provider === 'native_codex' || provider === 'native_claude'
          ? 'native_full_account'
          : step.effect === 'managed_local_tools'
            ? 'managed_local_tools'
            : 'managed_inference';
    updateStep(step.step_id, {
      provider,
      effect,
      managed_session_id:
        provider === 'managed_codex' ? step.managed_session_id : '',
    });
  }

  async function pickPlanWorkspace() {
    lastError = '';
    try {
      const path = await chooseWorkspace();
      if (path === null) return;
      planWorkspaceRoot = path;
      planWorkspace = await workspaceSnapshot(path);
      invalidatePlanReview();
    } catch (error) {
      showError(error);
    }
  }

  function planFromDraft(): OrchestrationPlanV1 {
    if (!planWorkspace || !planWorkspaceRoot || !objective.trim())
      throw new Error('draft');
    const created = now();
    if (budgetInvalid || durationMinutes === undefined)
      throw new Error('budget');
    const safeMinutes = durationMinutes;
    const planSteps: StepPlanV1[] = steps.map((step) => ({
      step_id: step.step_id,
      dependencies: step.dependencies.map((step_id) => ({
        step_id,
        allow_missing_output:
          steps.find((candidate) => candidate.step_id === step_id)?.required ===
          false,
      })),
      required: step.required,
      provider: step.provider,
      effect: step.effect,
      managed_session_id:
        step.provider === 'managed_codex'
          ? step.managed_session_id || null
          : null,
      model_id: null,
      prompt: step.prompt,
      max_attempts: 1,
      max_duration_seconds: Math.min(1800, safeMinutes * 60),
    }));
    return {
      schema_version: 1,
      plan_id: newId(),
      revision: 1,
      host_id: 'local',
      project_id: planProjectId.trim(),
      workspace_id: planWorkspace.workspace_id,
      workspace_root: planWorkspaceRoot,
      workspace_revision: planWorkspace.workspace_revision,
      objective: objective.trim(),
      created_at_unix: created,
      expires_at_unix: created + safeMinutes * 60,
      limits: {
        max_steps: 16,
        max_parallelism: 1,
        max_total_seconds: safeMinutes * 60,
        stop_policy: 'cancel_current_managed_turn',
      },
      steps: planSteps,
    };
  }

  async function reviewCurrentPlan() {
    lastError = '';
    busy = true;
    try {
      if (budgetInvalid) {
        lastError =
          'El presupuesto debe ser un número entero entre 1 y 60 minutos.';
        return;
      }
      const plan = planFromDraft();
      const current = await workspaceSnapshot(plan.workspace_root);
      if (
        current.workspace_id !== plan.workspace_id ||
        current.workspace_revision !== plan.workspace_revision
      ) {
        planWorkspace = current;
        throw new OrchestrationCallError('workspace_conflict');
      }
      const reviewed = await reviewPlan(plan);
      reviewedPlan = plan;
      planReview = reviewed;
    } catch (error) {
      if (error instanceof OrchestrationCallError) showError(error);
      else
        lastError =
          'Completa el workspace, objetivo, pasos, dependencias y sesiones antes de revisar.';
    } finally {
      busy = false;
    }
  }

  async function confirmPlan() {
    if (!reviewedPlan || !planReview) return;
    busy = true;
    lastError = '';
    try {
      const run = await submitPlan(reviewedPlan, planReview);
      runs = [run, ...runs.filter((item) => item.run_id !== run.run_id)];
      invalidatePlanReview();
      await refreshRuns();
    } catch (error) {
      showError(error);
    } finally {
      busy = false;
    }
  }

  function makeItems(
    text: string,
    kind: HandoffItemV1['kind'],
    revision: string,
  ): HandoffItemV1[] {
    return text
      .split('\n')
      .map((item) => item.trim())
      .filter(Boolean)
      .map((item) => ({
        item_id: newId(),
        source_revision: revision,
        kind,
        provenance: handoffProvenance,
        text: item,
      }));
  }

  async function pickHandoffWorkspace(side: 'source' | 'target') {
    lastError = '';
    try {
      const path = await chooseWorkspace();
      if (path === null) return;
      const snapshot = await workspaceSnapshot(path);
      if (side === 'source') {
        handoffSourceRoot = path;
        handoffSourceWorkspace = snapshot;
      } else {
        handoffTargetRoot = path;
        handoffTargetWorkspace = snapshot;
      }
      handoffPreview = null;
      handoffRun = null;
    } catch (error) {
      showError(error);
    }
  }

  function clearHandoffSource() {
    handoffSourceRoot = '';
    handoffSourceWorkspace = null;
    handoffSourceSession = '';
    handoffSourceGeneration = 1;
    handoffSourceObservation = null;
    handoffPreview = null;
  }

  async function selectManagedSource(sessionId: string) {
    clearHandoffSource();
    if (!sessionId) return;
    lastError = '';
    busy = true;
    try {
      const detail = await readSession(sessionId as SessionRow['id']);
      const root = detail.binding.workspace;
      const generation = detail.session.runtime.generation;
      if (
        detail.session.state !== 'prepared' ||
        detail.binding.state !== 'bound_attached' ||
        !root ||
        generation < 1
      ) {
        throw new OrchestrationCallError('workspace_conflict');
      }
      const snapshot = await workspaceSnapshot(root);
      handoffSourceSession = sessionId;
      handoffSourceGeneration = generation;
      handoffSourceRoot = root;
      handoffSourceWorkspace = snapshot;
      handoffSourceObservation = {
        plane: 'managed',
        session_id: sessionId,
        generation,
        workspace_root: root,
        session_revision: detail.session.revision,
      };
      handoffSourcePlane = 'managed';
    } catch (error) {
      showError(error);
    } finally {
      busy = false;
    }
  }

  async function selectNativeSource(sessionId: string) {
    clearHandoffSource();
    if (!sessionId) return;
    lastError = '';
    const session = nativeSessions.find(
      (candidate) => candidate.id === sessionId,
    );
    if (!session || session.state !== 'running') {
      lastError =
        'La sesión Native ya no está activa; actualiza y selecciona una sesión en ejecución.';
      return;
    }
    busy = true;
    try {
      const root = session.project;
      const snapshot = await workspaceSnapshot(root);
      const generation = session.restarts + 1;
      handoffSourceSession = session.id;
      handoffSourceGeneration = generation;
      handoffSourceRoot = root;
      handoffSourceWorkspace = snapshot;
      handoffSourceObservation = {
        plane: 'native',
        session_id: session.id,
        generation,
        workspace_root: root,
        started_at: session.started_at,
        restarts: session.restarts,
      };
      handoffSourcePlane = 'native';
    } catch (error) {
      showError(error);
    } finally {
      busy = false;
    }
  }

  async function prepareHandoffReceiver() {
    if (
      !f8Active ||
      handoffTargetProvider !== 'managed_codex' ||
      !handoffTargetRoot
    ) {
      receiverStatus =
        'Selecciona workspace de destino y habilita F8 Managed antes de preparar el receptor.';
      return;
    }
    busy = true;
    lastError = '';
    receiverReady = false;
    try {
      receiverIntentId ??= newId();
      const receipt = await prepareSession(receiverIntentId);
      handoffTargetSession = receipt.id;
      freshReceiverId = receipt.id;
      receiverStatus =
        'Sesión Managed nueva preparada. Autoriza su binding, runtime y lease desde Runtimes.';
      receiverSummary = `Receptor nuevo ${receipt.id}`;
      handoffPreview = null;
      await loadSessions();
    } catch {
      receiverStatus =
        'F8 no confirmó la preparación. No se crea otra sesión con un intent diferente; vuelve a intentar el mismo pedido.';
      lastError = 'No se pudo preparar el receptor Managed mediante F8.';
    } finally {
      busy = false;
    }
  }

  async function checkHandoffReceiver(): Promise<boolean> {
    receiverReady = false;
    if (
      !f8Active ||
      handoffTargetProvider !== 'managed_codex' ||
      !handoffTargetSession ||
      !handoffTargetRoot
    ) {
      receiverStatus =
        'Falta una sesión nueva Managed, F8 activo o workspace destino.';
      return false;
    }
    if (handoffTargetSession !== freshReceiverId) {
      receiverStatus =
        'El receptor no fue creado para este handoff. Prepara una sesión Managed nueva.';
      return false;
    }
    try {
      const detail = await readSession(
        handoffTargetSession as SessionRow['id'],
      );
      const expectedProfile =
        handoffTargetEffect === 'managed_local_tools'
          ? 'codex_local_tools_v1'
          : 'codex_inference_v1';
      const expectedAttachment =
        handoffTargetEffect === 'managed_local_tools'
          ? 'local_tools_v1'
          : 'inference_only_v1';
      const gate = detail.gates.find((item) => item.action === 'turn_start');
      const lease = detail.lease;
      const ready =
        detail.session.state === 'prepared' &&
        detail.turns.length === 0 &&
        !detail.turns_partial &&
        detail.binding.state === 'bound_attached' &&
        detail.binding.workspace === handoffTargetRoot &&
        detail.binding.mode === expectedAttachment &&
        detail.binding.attached_instance !== null &&
        detail.session.runtime.actual === 'ready' &&
        detail.session.runtime.profile === expectedProfile &&
        detail.session.runtime.instance === detail.binding.attached_instance &&
        lease?.state === 'active' &&
        lease.scopes.includes('turn_start') &&
        Date.parse(lease.expires_at) > Date.now() &&
        gate?.reason === 'allowed';
      receiverSummary = `${detail.session.id} · generation ${detail.session.runtime.generation} · ${detail.session.runtime.profile ?? 'perfil desconocido'} · lease ${lease?.state ?? 'ausente'}`;
      receiverReady = ready;
      receiverStatus = ready
        ? 'Receptor nuevo, sin historial, adjunto al workspace y habilitado por la lease F8 turn_start.'
        : 'Receptor aún no admitido. En Runtimes, configura el perfil solicitado, enlaza el workspace destino y concede una lease turn_start; no se enviará el handoff hasta confirmar todo.';
      return ready;
    } catch {
      receiverStatus =
        'No se pudo observar el receptor desde F8; no se asume que esté preparado.';
      return false;
    }
  }

  async function revalidateHandoffSource() {
    const observed = handoffSourceObservation;
    if (!observed) throw new OrchestrationCallError('workspace_conflict');
    if (observed.plane === 'managed') {
      const detail = await readSession(observed.session_id as SessionRow['id']);
      if (
        detail.session.state !== 'prepared' ||
        detail.binding.state !== 'bound_attached' ||
        detail.binding.workspace !== observed.workspace_root ||
        detail.session.runtime.generation !== observed.generation ||
        detail.session.revision !== observed.session_revision
      ) {
        throw new OrchestrationCallError('workspace_conflict');
      }
    } else {
      const fresh = await readNativeSessions(false);
      const session = fresh.find(
        (candidate) => candidate.id === observed.session_id,
      );
      if (
        !session ||
        session.state !== 'running' ||
        session.started_at !== observed.started_at ||
        session.restarts !== observed.restarts ||
        session.project !== observed.workspace_root ||
        session.restarts + 1 !== observed.generation
      ) {
        throw new OrchestrationCallError('workspace_conflict');
      }
    }
    const current = await workspaceSnapshot(observed.workspace_root);
    if (
      !handoffSourceWorkspace ||
      current.workspace_id !== handoffSourceWorkspace.workspace_id ||
      current.workspace_revision !== handoffSourceWorkspace.workspace_revision
    ) {
      handoffSourceWorkspace = current;
      throw new OrchestrationCallError('workspace_conflict');
    }
  }

  async function reviewHandoff() {
    lastError = '';
    busy = true;
    handoffPreview = null;
    try {
      if (
        handoffTargetProvider === 'managed_claude' ||
        handoffTargetProvider === 'hermes_planner'
      ) {
        throw new OrchestrationCallError('provider_blocked');
      }
      if (handoffTargetProvider === 'managed_codex' && !freshReceiverId) {
        receiverStatus =
          'Prepara primero un receptor Managed nuevo para este handoff.';
        throw new Error('receiver');
      }
      if (
        !handoffSourceWorkspace ||
        !handoffTargetWorkspace ||
        !handoffSourceRoot ||
        !handoffTargetRoot
      )
        throw new Error('workspace');
      await revalidateHandoffSource();
      if (
        handoffTargetProvider === 'managed_codex' &&
        !(await checkHandoffReceiver())
      ) {
        throw new Error('receiver');
      }
      const [currentSource, currentTarget] = await Promise.all([
        workspaceSnapshot(handoffSourceRoot),
        workspaceSnapshot(handoffTargetRoot),
      ]);
      if (
        currentSource.workspace_revision !==
          handoffSourceWorkspace.workspace_revision ||
        currentTarget.workspace_revision !==
          handoffTargetWorkspace.workspace_revision
      ) {
        handoffSourceWorkspace = currentSource;
        handoffTargetWorkspace = currentTarget;
        throw new OrchestrationCallError('workspace_conflict');
      }
      const created = now();
      const envelope: HandoffEnvelopeV1 = {
        schema_version: 1,
        handoff_id: newId(),
        revision: 1,
        source: {
          host_id: 'local',
          session_id: handoffSourceSession.trim(),
          session_generation: Math.max(1, Math.floor(handoffSourceGeneration)),
          plane: handoffSourcePlane,
          project_id: handoffSourceProject.trim(),
          workspace_id: currentSource.workspace_id,
          workspace_revision: currentSource.workspace_revision,
          workspace_root: handoffSourceRoot,
        },
        target: {
          host_id: 'local',
          project_id: handoffTargetProject.trim(),
          workspace_id: currentTarget.workspace_id,
          workspace_revision: currentTarget.workspace_revision,
          workspace_root: handoffTargetRoot,
          provider: handoffTargetProvider,
          effect:
            handoffTargetProvider === 'native_codex' ||
            handoffTargetProvider === 'native_claude'
              ? 'native_full_account'
              : handoffTargetEffect,
          managed_session_id:
            handoffTargetProvider === 'managed_codex'
              ? handoffTargetSession.trim() || null
              : null,
          native_profile_id:
            handoffTargetProvider === 'native_codex' ||
            handoffTargetProvider === 'native_claude'
              ? handoffNativeProfile.trim() || null
              : null,
          profile_revision: null,
        },
        objective: handoffObjective.trim(),
        selected_context_items: makeItems(
          handoffContext,
          'excerpt',
          currentSource.workspace_revision,
        ),
        selected_file_references: makeItems(
          handoffFiles,
          'file_reference',
          currentSource.workspace_revision,
        ),
        open_questions: makeItems(
          handoffQuestions,
          'open_question',
          currentSource.workspace_revision,
        ),
        created_at_unix: created,
        expires_at_unix: created + 15 * 60,
        content_digest: '',
      };
      handoffPreview = await sealHandoff(envelope);
    } catch (error) {
      if (error instanceof OrchestrationCallError) showError(error);
      else
        lastError =
          'Completa origen, destino, sesión, objetivo y revisa ambas revisiones de workspace.';
    } finally {
      busy = false;
    }
  }

  async function confirmHandoff() {
    if (!handoffPreview) return;
    if (
      handoffPreview.envelope.target.provider === 'native_codex' ||
      handoffPreview.envelope.target.provider === 'native_claude'
    ) {
      lastError =
        'BLOCKED: el destino Native requiere una frontera de confianza aceptada. El texto se puede copiar para el flujo manual existente.';
      return;
    }
    busy = true;
    lastError = '';
    const confirmation: HandoffConfirmationV1 = {
      handoff_id: handoffPreview.envelope.handoff_id,
      revision: handoffPreview.envelope.revision,
      reviewed_digest: handoffPreview.envelope.content_digest,
      confirmed_at_unix: now(),
    };
    try {
      await revalidateHandoffSource();
      if (
        handoffPreview.envelope.target.provider === 'managed_codex' &&
        !(await checkHandoffReceiver())
      ) {
        lastError = receiverStatus;
        busy = false;
        return;
      }
      handoffRun = await submitHandoff(handoffPreview.envelope, confirmation);
      runs = [
        handoffRun,
        ...runs.filter((item) => item.run_id !== handoffRun?.run_id),
      ];
      receiverIntentId = null;
      freshReceiverId = null;
      handoffTargetSession = '';
      await refreshRuns();
    } catch (error) {
      showError(error);
    } finally {
      busy = false;
    }
  }

  async function copyHandoff() {
    if (!handoffPreview) return;
    try {
      await navigator.clipboard.writeText(handoffPreview.first_message);
      copied = true;
      window.setTimeout(() => (copied = false), 1800);
    } catch {
      lastError =
        'El portapapeles no está disponible; selecciona y copia el texto revisado.';
    }
  }

  async function updateRun(runId: string, action: 'pause' | 'resume' | 'stop') {
    busy = true;
    lastError = '';
    try {
      const next = await runAction(action, runId);
      runs = runs.map((run) => (run.run_id === runId ? next : run));
    } catch (error) {
      showError(error);
    } finally {
      busy = false;
    }
  }

  async function chooseRunWorkspace(run: CoordinatorSnapshot) {
    try {
      const root = await chooseWorkspace();
      if (!root) return;
      const snapshot = await workspaceSnapshot(root);
      if (snapshot.workspace_id !== run.workspace_id)
        throw new OrchestrationCallError('workspace_conflict');
      runWorkspaceRoots = { ...runWorkspaceRoots, [run.run_id]: root };
      observedWorkspace = { ...observedWorkspace, [run.run_id]: snapshot };
    } catch (error) {
      showError(error);
    }
  }

  async function reviewedWorkspaceRevision(
    run: CoordinatorSnapshot,
    root: string,
  ): Promise<string> {
    // Use only the revision already displayed when the owner selected the workspace.
    // Re-sampling at confirmation must never silently promote unseen changes.
    const observed = observedWorkspace[run.run_id];
    if (
      !observed ||
      runWorkspaceRoots[run.run_id] !== root ||
      observed.workspace_id !== run.workspace_id
    )
      throw new OrchestrationCallError('workspace_conflict');
    const current = await workspaceSnapshot(root);
    if (
      current.workspace_id !== observed.workspace_id ||
      current.workspace_revision !== observed.workspace_revision
    ) {
      const remaining = { ...observedWorkspace };
      delete remaining[run.run_id];
      observedWorkspace = remaining;
      throw new OrchestrationCallError('workspace_conflict');
    }
    return observed.workspace_revision;
  }

  async function confirmStep(
    run: CoordinatorSnapshot,
    stepId: string,
    decision:
      | 'confirmed_completed'
      | 'confirmed_no_effect'
      | 'confirmed_cancelled'
      | 'unknown',
    root?: string,
  ) {
    busy = true;
    lastError = '';
    try {
      let revision: string | null = null;
      if (decision === 'confirmed_completed') {
        const chosenRoot = root ?? runWorkspaceRoots[run.run_id];
        if (!chosenRoot) throw new Error('workspace');
        revision = await reviewedWorkspaceRevision(run, chosenRoot);
        root = chosenRoot;
      }
      const next = await reconcileStep(
        run.run_id,
        stepId,
        decision,
        root ?? null,
        revision,
      );
      runs = runs.map((item) => (item.run_id === next.run_id ? next : item));
    } catch (error) {
      if (error instanceof OrchestrationCallError) showError(error);
      else
        lastError =
          'Selecciona el workspace y confirma la revisión actual antes de declarar un efecto completado.';
    } finally {
      busy = false;
    }
  }

  async function confirmWorkspaceOutput(
    run: CoordinatorSnapshot,
    stepId: string,
    root: string,
  ) {
    busy = true;
    lastError = '';
    try {
      const revision = await reviewedWorkspaceRevision(run, root);
      const next = await verifyStep(run.run_id, stepId, revision);
      runs = runs.map((item) => (item.run_id === next.run_id ? next : item));
    } catch (error) {
      showError(error);
    } finally {
      busy = false;
    }
  }

  function statusText(state: string): string {
    return state.replaceAll('_', ' ');
  }

  onMount(() => {
    // Volatile handoff contents and prompts are deliberately not restored from disk.
    return () => {
      reviewedPlan = null;
      planReview = null;
      handoffPreview = null;
      handoffContext = '';
      handoffFiles = '';
      handoffQuestions = '';
    };
  });
</script>

<section
  class="pipelines-feature"
  aria-label={t('Pipelines and handoff')}
  hidden={!visible}
  inert={!visible}
>
  <header class="m6-toolbar">
    <div>
      <h2>Pipelines</h2>
      <p class="m6-muted">
        OL3 supervisado · DAG finito · concurrencia Managed fijada en 1
      </p>
    </div>
    <button
      class="m6-button"
      type="button"
      disabled={busy || loadingRuns}
      onclick={() => void refreshRuns()}>Actualizar</button
    >
  </header>

  <div class="m6-tabs" role="tablist" aria-label="Coordinación">
    <button
      class:active={tab === 'pipelines'}
      role="tab"
      id="coordination-plan-tab"
      aria-controls="coordination-plan-panel"
      tabindex={tab === 'pipelines' ? 0 : -1}
      onkeydown={(event) => tabKey(event, 0)}
      aria-selected={tab === 'pipelines'}
      onclick={() => (tab = 'pipelines')}>Plan y ejecuciones</button
    >
    <button
      class:active={tab === 'handoff'}
      role="tab"
      id="coordination-handoff-tab"
      aria-controls="coordination-handoff-panel"
      tabindex={tab === 'handoff' ? 0 : -1}
      onkeydown={(event) => tabKey(event, 1)}
      aria-selected={tab === 'handoff'}
      onclick={() => (tab = 'handoff')}>{t('Structured handoff')}</button
    >
  </div>

  {#if runsError}
    <p class="m6-alert" role="alert">{runsError}</p>
  {/if}

  {#if lastError}
    <p class="m6-alert" role="alert">{lastError}</p>
  {/if}

  {#if tab === 'pipelines'}
    <div
      class="m6-layout"
      id="coordination-plan-panel"
      role="tabpanel"
      aria-labelledby="coordination-plan-tab"
    >
      <section class="m6-panel" aria-labelledby="plan-title">
        <div class="m6-panel-head">
          <div>
            <h3 id="plan-title">Crear un plan revisable</h3>
            <p>
              Los prompts permanecen en memoria volátil hasta que el dueño
              revisa y confirma el digest.
            </p>
          </div>
        </div>
        <div class="m6-form">
          <label
            >Workspace
            <span class="m6-inline"
              ><input
                readonly
                value={planWorkspaceRoot}
                placeholder="Elige una carpeta"
              /><button
                class="m6-button"
                type="button"
                onclick={() => void pickPlanWorkspace()}>Elegir…</button
              ></span
            >
          </label>
          {#if planWorkspace}<p class="m6-meta">
              Workspace <code>{planWorkspace.workspace_id.slice(0, 16)}…</code>
              · revisión
              <code>{planWorkspace.workspace_revision.slice(0, 16)}…</code>
            </p>{/if}
          <label
            >Project ID (etiqueta, no ruta)<input
              bind:value={planProjectId}
              maxlength="128"
              oninput={invalidatePlanReview}
            /></label
          >
          <label
            >Objetivo<input
              bind:value={objective}
              maxlength="2048"
              oninput={invalidatePlanReview}
            /></label
          >
          <label
            >Presupuesto total en minutos<input
              type="number"
              min="1"
              max="60"
              step="1"
              aria-invalid={budgetInvalid}
              aria-describedby={budgetInvalid ? 'plan-budget-error' : undefined}
              bind:value={durationMinutes}
              oninput={invalidatePlanReview}
            /></label
          >
          {#if budgetInvalid}
            <p id="plan-budget-error" class="m6-alert" role="alert">
              El presupuesto debe ser un número entero entre 1 y 60 minutos.
            </p>
          {/if}
          <div class="m6-section-title">
            <h4>Pasos · máximo 16</h4>
            <button
              class="m6-button"
              type="button"
              disabled={steps.length >= 16}
              onclick={() => {
                steps = [...steps, initialStep()];
                invalidatePlanReview();
              }}>Añadir paso</button
            >
          </div>
          {#each steps as step, index (step.step_id)}
            <article class="m6-step-editor">
              <header>
                <strong>Paso {index + 1}</strong><button
                  class="m6-button subtle"
                  type="button"
                  disabled={steps.length <= 1}
                  onclick={() => {
                    steps = steps
                      .filter((candidate) => candidate.step_id !== step.step_id)
                      .map((candidate) => ({
                        ...candidate,
                        dependencies: candidate.dependencies.filter(
                          (id) => id !== step.step_id,
                        ),
                      }));
                    invalidatePlanReview();
                  }}>Quitar</button
                >
              </header>
              <label
                >Proveedor
                <select
                  value={step.provider}
                  onchange={(event) =>
                    changeProvider(
                      step,
                      event.currentTarget.value as ProviderLane,
                    )}
                >
                  <option value="managed_codex"
                    >{t('Managed Codex · F8 authority')}</option
                  >
                  <option value="managed_claude"
                    >Managed Claude · adapter bloqueado</option
                  >
                  <option value="native_codex"
                    >Native Codex · bloqueado por frontera</option
                  >
                  <option value="native_claude"
                    >Native Claude · bloqueado por frontera</option
                  >
                  <option value="hermes_planner"
                    >Hermes planner · bloqueado</option
                  >
                </select>
              </label>
              {#if step.provider === 'managed_codex' || step.provider === 'managed_claude'}
                <label
                  >Sesión Managed
                  <select
                    value={step.managed_session_id}
                    onchange={(event) =>
                      updateStep(step.step_id, {
                        managed_session_id: event.currentTarget.value,
                      })}
                  >
                    <option value="">Selecciona una sesión lista</option>
                    {#each sessionOptions as session (session.id)}<option
                        value={session.id}
                        >{session.id.slice(0, 8)} · {session.runtime.profile ??
                          'perfil no reportado'} · {session.runtime.workspace ??
                          'workspace del binding'}</option
                      >{/each}
                  </select>
                </label>
                <label
                  >Efecto
                  <select
                    value={step.effect}
                    onchange={(event) =>
                      updateStep(step.step_id, {
                        effect: event.currentTarget
                          .value as StepDraft['effect'],
                      })}
                  >
                    <option value="managed_inference">Managed inference</option>
                    <option value="managed_local_tools"
                      >Managed local tools · aprobación F8 por acción</option
                    >
                  </select>
                </label>
              {:else if step.provider === 'native_codex' || step.provider === 'native_claude'}
                <p class="m6-blocked">
                  BLOCKED_TRUST_BOUNDARY · no se lanzará un proceso Native desde
                  OL3.
                </p>
              {:else}
                <p class="m6-blocked">
                  BLOCKED_PROVIDER · el planner no tiene autoridad de ejecución.
                </p>
              {/if}
              {#if index > 0}
                <fieldset>
                  <legend>Dependencias</legend>
                  {#each steps.slice(0, index) as predecessor (predecessor.step_id)}
                    <label class="m6-check"
                      ><input
                        type="checkbox"
                        checked={step.dependencies.includes(
                          predecessor.step_id,
                        )}
                        onchange={(event) =>
                          updateStep(step.step_id, {
                            dependencies: event.currentTarget.checked
                              ? [...step.dependencies, predecessor.step_id]
                              : step.dependencies.filter(
                                  (id) => id !== predecessor.step_id,
                                ),
                          })}
                      />
                      Paso {steps.findIndex(
                        (candidate) =>
                          candidate.step_id === predecessor.step_id,
                      ) + 1}
                      {predecessor.required
                        ? '(obligatorio)'
                        : '(opcional)'}</label
                    >
                  {/each}
                </fieldset>
              {/if}
              <label class="m6-check"
                ><input
                  type="checkbox"
                  checked={step.required}
                  onchange={(event) =>
                    updateStep(step.step_id, {
                      required: event.currentTarget.checked,
                    })}
                /> Paso obligatorio</label
              >
              <label
                >Entrada exacta del paso<textarea
                  rows="4"
                  maxlength="8192"
                  bind:value={step.prompt}
                  oninput={invalidatePlanReview}
                  placeholder="Describe el trabajo de este paso. El texto no concede autoridad ni aprobaciones."
                ></textarea></label
              >
            </article>
          {/each}
          <p class="m6-note">
            M6 fija un turno Managed global a la vez. Native full-account,
            Claude adapter y Hermes planner permanecen visibles pero bloqueados.
          </p>
          <button
            class="m6-button primary"
            type="button"
            disabled={busy}
            onclick={() => void reviewCurrentPlan()}
            >Revisar el plan completo</button
          >
        </div>
        {#if reviewedPlan && planReview}
          <div class="m6-review" aria-label="Plan revisado">
            <h4>
              Revisión explícita · digest <code>{planReview.digest}</code>
            </h4>
            <p>
              Revisa objetivo, workspace, orden DAG, proveedor, efectos,
              prompts, presupuesto y vencimiento. Confirmar permite solo el plan
              exacto anterior.
            </p>
            <details>
              <summary>Ver plan completo y entradas exactas</summary>
              <pre>{JSON.stringify(reviewedPlan, null, 2)}</pre>
            </details>
            <button
              class="m6-button primary"
              type="button"
              disabled={busy}
              onclick={() => void confirmPlan()}
              >Confirmar y enviar al coordinador</button
            >
          </div>
        {/if}
      </section>

      <section class="m6-panel" aria-labelledby="runs-title">
        <div class="m6-panel-head">
          <div>
            <h3 id="runs-title">Ejecuciones locales</h3>
            <p>
              Solo metadatos durables. Sin prompts ni salida de proveedor en el
              registro local.
            </p>
          </div>
        </div>
        {#if runs.length === 0}
          <p class="m6-empty">
            {loadingRuns
              ? 'Leyendo registro local…'
              : 'No hay ejecuciones registradas.'}
          </p>
        {:else}
          <div class="m6-runs">
            {#each runs as run (run.run_id)}
              <article class="m6-run">
                <header>
                  <div>
                    <strong class="m6-status" data-state={run.state}
                      >{statusText(run.state)}</strong
                    >
                    <p><code>{run.run_id}</code></p>
                  </div>
                  <span class="m6-meta"
                    >r{run.revision} · digest
                    <code>{run.plan_digest.slice(0, 12)}…</code></span
                  >
                </header>
                {#if run.failure}<p class="m6-alert compact">
                    {statusText(run.failure)}
                  </p>{/if}
                <ul class="m6-step-list">
                  {#each run.steps as step (step.step_id)}
                    <li>
                      <div>
                        <strong>{statusText(step.state)}</strong><span>
                          {step.provider} · {step.effect} · intentos {step.attempt_count}</span
                        >{#if step.failure}<small
                            >{statusText(step.failure)}</small
                          >{/if}
                      </div>
                      {#if step.state === 'verifying'}
                        <div class="m6-recovery">
                          <p>
                            El turno terminó. Inspecciona el workspace antes de
                            marcar el paso completado.
                          </p>
                          <button
                            class="m6-button"
                            type="button"
                            disabled={busy}
                            onclick={() => void chooseRunWorkspace(run)}
                            >Elegir y observar workspace</button
                          >
                          {#if runWorkspaceRoots[run.run_id]}<button
                              class="m6-button"
                              type="button"
                              disabled={busy}
                              onclick={() =>
                                void confirmWorkspaceOutput(
                                  run,
                                  step.step_id,
                                  runWorkspaceRoots[run.run_id],
                                )}
                              >Confirmar salida tras revisar la revisión actual</button
                            >{/if}
                          {#if observedWorkspace[run.run_id]}<small
                              >Revisión observada <code
                                >{observedWorkspace[run.run_id]
                                  .workspace_revision}</code
                              ></small
                            >{/if}
                        </div>
                      {:else if step.state === 'needs_review'}
                        <div class="m6-recovery">
                          <p>
                            Resultado incierto o coordinador reiniciado. No se
                            reenvía el turno. Confirma evidencia antes de cerrar
                            este paso.
                          </p>
                          <button
                            class="m6-button"
                            type="button"
                            disabled={busy}
                            onclick={() => void chooseRunWorkspace(run)}
                            >Elegir y observar workspace</button
                          >
                          {#if runWorkspaceRoots[run.run_id]}
                            <button
                              class="m6-button"
                              type="button"
                              disabled={busy}
                              onclick={() =>
                                void confirmStep(
                                  run,
                                  step.step_id,
                                  'confirmed_completed',
                                )}
                              >Confirmo que completó · revisar revisión actual</button
                            >
                          {/if}
                          <button
                            class="m6-button"
                            type="button"
                            disabled={busy}
                            onclick={() =>
                              void confirmStep(
                                run,
                                step.step_id,
                                'confirmed_no_effect',
                              )}
                            >Confirmo que no tuvo efecto · cerrar como fallido</button
                          >
                          <button
                            class="m6-button danger"
                            type="button"
                            disabled={busy}
                            onclick={() =>
                              void confirmStep(
                                run,
                                step.step_id,
                                'confirmed_cancelled',
                              )}>Confirmo cancelación</button
                          >
                          {#if observedWorkspace[run.run_id]}<small
                              >Workspace observado <code
                                >{observedWorkspace[
                                  run.run_id
                                ].workspace_id.slice(0, 12)}…</code
                              >
                              · revisión
                              <code
                                >{observedWorkspace[run.run_id]
                                  .workspace_revision}</code
                              ></small
                            >{/if}
                        </div>
                      {/if}
                    </li>
                  {/each}
                </ul>
                <footer>
                  {#if run.state === 'running'}<button
                      class="m6-button"
                      type="button"
                      disabled={busy}
                      onclick={() => void updateRun(run.run_id, 'pause')}
                      >Pausar</button
                    >{/if}
                  {#if run.state === 'paused'}<button
                      class="m6-button"
                      type="button"
                      disabled={busy}
                      onclick={() => void updateRun(run.run_id, 'resume')}
                      >Reanudar</button
                    >{/if}
                  {#if ['running', 'paused', 'pausing', 'cancelling'].includes(run.state)}<button
                      class="m6-button danger"
                      type="button"
                      disabled={busy || run.state === 'cancelling'}
                      onclick={() => void updateRun(run.run_id, 'stop')}
                      >Detener/cancelar</button
                    >{/if}
                </footer>
              </article>
            {/each}
          </div>
        {/if}
      </section>
    </div>
  {:else}
    <div
      class="m6-layout handoff-layout"
      id="coordination-handoff-panel"
      role="tabpanel"
      aria-labelledby="coordination-handoff-tab"
    >
      <section class="m6-panel" aria-labelledby="handoff-title">
        <div class="m6-panel-head">
          <div>
            <h3 id="handoff-title">Preparar handoff estructurado v1</h3>
            <p>
              El contenido del origen se selecciona explícitamente; nunca se
              heredan conversación, credenciales, permisos ni aprobaciones.
            </p>
          </div>
        </div>
        <div class="m6-form">
          <fieldset>
            <legend>Origen</legend>
            <label
              >Plano de origen<select
                value={handoffSourcePlane}
                onchange={(event) => {
                  handoffSourcePlane = event.currentTarget.value as
                    'native' | 'managed';
                  clearHandoffSource();
                }}
                ><option value="native">Native</option><option
                  value="managed"
                  disabled={!f8Active}>Managed</option
                ></select
              ></label
            >
            {#if handoffSourcePlane === 'managed'}
              <label
                >Sesión Managed de origen<select
                  value={handoffSourceSession}
                  onchange={(event) =>
                    void selectManagedSource(event.currentTarget.value)}
                  ><option value=""
                    >Selecciona sesión para observar generación y workspace</option
                  >{#each managedSessions as session (session.id)}<option
                      value={session.id}
                      >{session.id.slice(0, 8)} · {session.state} · generation {session
                        .runtime.generation}</option
                    >{/each}</select
                ></label
              >
            {:else}
              <label
                >Sesión Native de origen<select
                  value={handoffSourceSession}
                  onchange={(event) =>
                    void selectNativeSource(event.currentTarget.value)}
                  ><option value="">Selecciona sesión Native activa</option
                  >{#each nativeSessions.filter((session) => session.state === 'running') as session (session.id)}<option
                      value={session.id}
                      >{session.agent_name} · {session.id.slice(0, 8)} · reinicios
                      {session.restarts}</option
                    >{/each}</select
                ></label
              >
            {/if}
            <label
              >Workspace de origen<input
                readonly
                value={handoffSourceRoot}
                placeholder="Se toma de la sesión observada"
              /></label
            >
            {#if handoffSourceWorkspace}<p class="m6-meta">
                Revisión de origen <code
                  >{handoffSourceWorkspace.workspace_revision.slice(
                    0,
                    18,
                  )}…</code
                >
              </p>{/if}
            {#if handoffSourceObservation}<p class="m6-meta">
                Session <code>{handoffSourceSession}</code> · generation
                observada {handoffSourceGeneration}
              </p>{/if}
            <label
              >Project ID (etiqueta, no ruta)<input
                bind:value={handoffSourceProject}
                maxlength="128"
                oninput={() => (handoffPreview = null)}
              /></label
            >
          </fieldset>
          <fieldset>
            <legend>Destino</legend>
            <label
              >Workspace de destino <span class="m6-inline"
                ><input
                  readonly
                  value={handoffTargetRoot}
                  placeholder="Elige la carpeta receptora"
                /><button
                  class="m6-button"
                  type="button"
                  onclick={() => void pickHandoffWorkspace('target')}
                  >Elegir…</button
                ></span
              ></label
            >
            {#if handoffTargetWorkspace}<p class="m6-meta">
                Revisión de destino <code
                  >{handoffTargetWorkspace.workspace_revision.slice(
                    0,
                    18,
                  )}…</code
                >
              </p>{/if}
            <label
              >Project ID (etiqueta, no ruta)<input
                bind:value={handoffTargetProject}
                maxlength="128"
                oninput={() => (handoffPreview = null)}
              /></label
            >
            <label
              >Proveedor receptor<select
                bind:value={handoffTargetProvider}
                onchange={() => {
                  handoffPreview = null;
                  receiverReady = false;
                }}
              >
                <option value="managed_codex"
                  >Managed Codex · admitido por F8</option
                >
                <option value="managed_claude"
                  >Managed Claude · bloqueado</option
                >
                <option value="native_codex"
                  >Native Codex · bloqueado, solo copia manual</option
                >
                <option value="native_claude"
                  >Native Claude · bloqueado, solo copia manual</option
                >
                <option value="hermes_planner"
                  >Hermes planner · bloqueado</option
                >
              </select></label
            >
            {#if handoffTargetProvider === 'managed_codex'}
              <p class="m6-meta">
                Cada handoff requiere un receptor Managed nuevo, sin turnos ni
                contexto previo. El receptor no hereda permisos.
              </p>
              <button
                class="m6-button"
                type="button"
                disabled={busy || !f8Active || !handoffTargetRoot}
                onclick={() => void prepareHandoffReceiver()}
                >Preparar receptor Managed nuevo mediante F8</button
              >
              {#if receiverSummary}<p class="m6-meta">
                  <code>{receiverSummary}</code>
                </p>{/if}
              <p
                class="m6-status"
                data-state={receiverReady ? 'completed' : 'needs_review'}
                role="status"
              >
                {receiverStatus}
              </p>
              {#if freshReceiverId && !receiverReady}<button
                  class="m6-button"
                  type="button"
                  disabled={busy || !f8Active}
                  onclick={onruntimes}>Configurar receptor en Runtimes</button
                >{/if}
              <label
                >Efecto <select
                  bind:value={handoffTargetEffect}
                  onchange={() => {
                    handoffPreview = null;
                    receiverReady = false;
                  }}
                  ><option value="managed_inference">Managed inference</option
                  ><option value="managed_local_tools"
                    >Managed local tools con aprobación F8</option
                  ></select
                ></label
              >
            {:else if handoffTargetProvider === 'managed_claude'}
              <p class="m6-blocked">
                BLOCKED_PROVIDER · no hay adaptador Managed Claude admitido por
                la autoridad F8 de este M6.
              </p>
            {:else if handoffTargetProvider === 'native_codex' || handoffTargetProvider === 'native_claude'}
              <label
                >Perfil Native (referencia)<input
                  bind:value={handoffNativeProfile}
                  maxlength="128"
                  oninput={() => (handoffPreview = null)}
                /></label
              >
              <p class="m6-blocked">
                BLOCKED_TRUST_BOUNDARY · no se iniciará automáticamente un
                destino Native. El flujo manual existente sigue disponible.
              </p>
            {:else}<p class="m6-blocked">
                BLOCKED_PROVIDER · Hermes no recibe autoridad para ejecutar.
              </p>{/if}
          </fieldset>
          <fieldset>
            <legend>Contenido seleccionado</legend>
            <label
              >Objetivo confirmado por el dueño<input
                bind:value={handoffObjective}
                maxlength="2048"
                oninput={() => (handoffPreview = null)}
              /></label
            >
            <label
              >Procedencia<select
                bind:value={handoffProvenance}
                onchange={() => (handoffPreview = null)}
                ><option value="human_entered">{t('Human entered')}</option
                ><option value="provider_reported"
                  >{t('Provider reported')}</option
                ><option value="provider_output_untrusted"
                  >{t('Provider output · untrusted')}</option
                ></select
              ></label
            >
            <label
              >Extractos elegidos (una línea por elemento)<textarea
                rows="4"
                bind:value={handoffContext}
                oninput={() => (handoffPreview = null)}></textarea></label
            >
            <label
              >Referencias de archivo como etiquetas relativas (una por línea)<textarea
                rows="3"
                bind:value={handoffFiles}
                oninput={() => (handoffPreview = null)}
                placeholder="src/module.ts"></textarea></label
            >
            <label
              >Preguntas abiertas (una por línea)<textarea
                rows="3"
                bind:value={handoffQuestions}
                oninput={() => (handoffPreview = null)}></textarea></label
            >
          </fieldset>
          <button
            class="m6-button primary"
            type="button"
            disabled={busy}
            onclick={() => void reviewHandoff()}
            >Sellar y revisar bytes exactos</button
          >
        </div>
      </section>
      <section class="m6-panel" aria-labelledby="handoff-review-title">
        <div class="m6-panel-head">
          <div>
            <h3 id="handoff-review-title">Revisión y entrega</h3>
            <p>
              Todo texto citado es entrada no confiable. El digest se confirma
              contra origen, destino, revisiones y selección.
            </p>
          </div>
        </div>
        {#if !f8Active}<p class="m6-blocked">
            M5/F8 está inactivo. Handoff Managed no puede iniciar hasta que la
            autoridad aprobada esté activa.
          </p>{/if}
        {#if sessionsError}<p class="m6-blocked">{sessionsError}</p>{/if}
        {#if handoffPreview}
          <div class="m6-review">
            <p>
              Handoff <code>{handoffPreview.envelope.handoff_id}</code> ·
              revisión {handoffPreview.envelope.revision}
            </p>
            <p>
              Digest confirmado <code
                >{handoffPreview.envelope.content_digest}</code
              >
            </p>
            <p>
              Origen {handoffPreview.envelope.source.plane} · destino {handoffPreview
                .envelope.target.provider} · workspace
              <code
                >{handoffPreview.envelope.target.workspace_id.slice(
                  0,
                  16,
                )}…</code
              >
            </p>
            <p>
              Destino de sesión <code
                >{handoffPreview.envelope.target.managed_session_id ??
                  handoffPreview.envelope.target.native_profile_id ??
                  'propuesta sin sesión'}</code
              >
            </p>
            <label
              >Primer mensaje exacto, delimitado como contexto no confiable<textarea
                class="m6-message"
                readonly
                rows="18"
                value={handoffPreview.first_message}></textarea></label
            >
            <details>
              <summary>Revisar sobre completo</summary>
              <pre>{JSON.stringify(handoffPreview.envelope, null, 2)}</pre>
            </details>
            <button
              class="m6-button"
              type="button"
              onclick={() => void copyHandoff()}
              >{copied ? 'Copiado' : 'Copiar mensaje revisado'}</button
            >
            {#if handoffPreview.envelope.target.provider === 'native_codex' || handoffPreview.envelope.target.provider === 'native_claude'}
              <p class="m6-blocked">
                BLOCKED_TRUST_BOUNDARY · el digest no autoriza el lanzamiento
                Native; usa el flujo manual y su confirmación local.
              </p>
            {:else if handoffPreview.envelope.target.provider !== 'managed_codex'}
              <p class="m6-blocked">
                BLOCKED_PROVIDER · el adaptador del destino no está admitido
                para envío.
              </p>
            {:else}
              <button
                class="m6-button primary"
                type="button"
                disabled={busy || !f8Active}
                onclick={() => void confirmHandoff()}
                >Confirmar digest y entregar al Managed nuevo</button
              >
            {/if}
          </div>
        {:else}
          <p class="m6-empty">
            El contenido del handoff es volátil. Completa selección y workspace
            para generar la revisión.
          </p>
        {/if}
        {#if handoffRun}
          <div class="m6-alert" role="status">
            Ejecución <code>{handoffRun.run_id}</code> · {statusText(
              handoffRun.state,
            )}. La entrega no implica que el destinatario haya terminado
            correctamente.
          </div>
        {/if}
      </section>
    </div>
  {/if}
</section>

<style>
  .pipelines-feature {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow: auto;
    padding: 2px 2px 20px;
  }
  .m6-toolbar,
  .m6-panel-head,
  .m6-section-title,
  .m6-step-editor > header,
  .m6-run > header,
  .m6-run > footer,
  .m6-inline {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .m6-toolbar {
    position: sticky;
    top: 0;
    z-index: 2;
    padding: 6px 0;
    background: var(--bg);
  }
  h2 {
    margin: 0;
    font-size: var(--text-lg);
  }
  h3 {
    margin: 0;
    font-size: var(--text-md);
  }
  h4 {
    margin: 0;
    font-size: var(--text-sm);
  }
  p {
    margin: 0;
  }
  .m6-muted,
  .m6-meta,
  .m6-panel-head p,
  .m6-note {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .m6-meta code,
  code {
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }
  .m6-tabs {
    display: flex;
    gap: 8px;
    border-bottom: 1px solid var(--line-strong);
  }
  .m6-tabs button {
    min-height: 36px;
    border: 0;
    border-radius: 8px 8px 0 0;
    color: var(--muted);
  }
  .m6-tabs button.active {
    color: var(--accent-strong);
    background: var(--accent-soft);
  }
  .m6-layout {
    display: grid;
    grid-template-columns: minmax(300px, 0.92fr) minmax(340px, 1.08fr);
    align-items: start;
    gap: 12px;
  }
  .handoff-layout {
    grid-template-columns: minmax(320px, 1fr) minmax(320px, 0.9fr);
  }
  .m6-panel {
    min-width: 0;
    border: 1px solid var(--line);
    background: var(--surface);
    border-radius: var(--radius-md);
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 13px;
  }
  .m6-panel-head {
    align-items: flex-start;
  }
  .m6-form {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .m6-form > label,
  .m6-step-editor > label,
  .m6-review > label,
  fieldset > label {
    display: flex;
    flex-direction: column;
    gap: 5px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  input,
  select,
  textarea {
    width: 100%;
    min-height: 38px;
    padding: 8px 10px;
    color: var(--text);
    background: var(--terminal-bg);
    border: 1px solid var(--line-strong);
    border-radius: 8px;
    font: inherit;
    font-size: var(--text-sm);
  }
  input[readonly] {
    color: var(--muted);
  }
  textarea {
    resize: vertical;
    line-height: 1.5;
  }
  .m6-inline > input {
    flex: 1;
    min-width: 0;
  }
  .m6-button {
    min-height: 34px;
    padding: 5px 11px;
    border: 1px solid var(--line-strong);
    border-radius: 8px;
    white-space: nowrap;
    font-size: var(--text-xs);
  }
  .m6-button:hover:not(:disabled) {
    background: var(--surface-strong);
  }
  .m6-button:disabled {
    opacity: 0.5;
  }
  .m6-button.primary {
    border-color: var(--accent-line);
    background: var(--accent-soft);
    color: var(--accent-strong);
    font-weight: 650;
  }
  .m6-button.danger {
    border-color: var(--error-line);
    background: var(--error-soft);
    color: var(--error-text);
  }
  .m6-button.subtle {
    min-height: 28px;
  }
  .m6-section-title {
    margin-top: 4px;
  }
  .m6-step-editor,
  .m6-run,
  .m6-review {
    border: 1px solid var(--line);
    background: var(--surface-subtle);
    border-radius: 9px;
    padding: 11px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .m6-check {
    display: flex !important;
    flex-direction: row !important;
    align-items: center;
    gap: 8px !important;
    color: var(--text) !important;
  }
  .m6-check input {
    width: 16px;
    min-height: 16px;
  }
  fieldset {
    min-width: 0;
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  legend {
    padding: 0 5px;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .m6-blocked {
    padding: 9px 10px;
    color: var(--warn);
    background: var(--warn-soft);
    border: 1px solid var(--warn-line);
    border-radius: 8px;
    font-size: var(--text-xs);
  }
  .m6-note {
    line-height: 1.5;
  }
  .m6-review {
    overflow: hidden;
  }
  .m6-review > p {
    font-size: var(--text-xs);
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  pre {
    max-height: 360px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--terminal-bg);
    padding: 10px;
    border-radius: 8px;
    font: var(--text-xs) / 1.5 var(--font-mono);
  }
  details summary {
    cursor: pointer;
    color: var(--accent-strong);
    font-size: var(--text-xs);
  }
  .m6-empty {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: 15px 3px;
  }
  .m6-alert {
    border: 1px solid var(--error-line);
    background: var(--error-soft);
    color: var(--error-text);
    border-radius: 8px;
    padding: 9px 11px;
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
  }
  .m6-alert.compact {
    padding: 5px 8px;
  }
  .m6-runs {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .m6-run {
    gap: 10px;
  }
  .m6-run > header {
    align-items: flex-start;
  }
  .m6-run > header p {
    margin-top: 4px;
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
  }
  .m6-status {
    display: inline-block;
    padding: 2px 7px;
    border-radius: 99px;
    background: var(--surface-raised);
    text-transform: uppercase;
    font-size: var(--text-xs);
    letter-spacing: 0.04em;
  }
  .m6-status[data-state='completed'] {
    color: var(--ok);
    background: var(--ok-soft);
  }
  .m6-status[data-state='needs_review'],
  .m6-status[data-state='blocked'] {
    color: var(--warn);
    background: var(--warn-soft);
  }
  .m6-status[data-state='failed'],
  .m6-status[data-state='expired'] {
    color: var(--error-text);
    background: var(--error-soft);
  }
  .m6-step-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .m6-step-list li {
    border-top: 1px solid var(--line);
    padding-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 7px;
    font-size: var(--text-xs);
  }
  .m6-step-list li > div:first-child > span,
  .m6-step-list small {
    color: var(--muted);
  }
  .m6-step-list small {
    display: block;
    overflow-wrap: anywhere;
  }
  .m6-recovery {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    padding: 8px;
    border-radius: 8px;
    background: var(--warn-soft);
  }
  .m6-recovery p {
    width: 100%;
    color: var(--warn);
  }
  .m6-recovery small {
    width: 100%;
  }
  .m6-run > footer {
    justify-content: flex-end;
  }
  .m6-message {
    min-height: 260px;
    color: var(--text);
  }
  @media (max-width: 1100px) {
    .m6-layout,
    .handoff-layout {
      grid-template-columns: 1fr;
    }
  }
</style>
