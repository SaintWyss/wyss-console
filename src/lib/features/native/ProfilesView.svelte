<script lang="ts">
  import {
    AGENT_IDS,
    HOOKS_MODES,
    PERMISSIONS,
    type AgentId,
    type HooksMode,
    type Permission,
    type Profile,
    type ProfileBody,
  } from '../../contracts/native';
  import type { NativeController, NativeState } from './controller';
  import {
    AGENT_LABEL,
    HOOKS_LABEL,
    PERMISSION_LABEL,
    emptyBody,
    failureText,
  } from './present';

  let {
    native,
    controller,
    visible,
  }: {
    native: NativeState;
    controller: NativeController;
    visible: boolean;
  } = $props();

  const selected = $derived(
    native.editing && native.editing !== 'new'
      ? (native.profiles.find((p) => p.id === native.editing) ?? null)
      : null,
  );
  const creating = $derived(native.editing === 'new');
  const readOnly = $derived(selected?.template ?? false);

  // The form is a copy of the saved body: nothing changes until Save.
  let form = $state<ProfileBody>(emptyBody());
  let skillsText = $state('');
  let loadedFor: string | null = null;
  $effect(() => {
    const key = native.editing;
    const current = selected;
    const stamp = `${key}:${current?.current ?? 0}`;
    if (stamp === loadedFor) return;
    loadedFor = stamp;
    const body: ProfileBody = current
      ? structuredClone($state.snapshot(current.body))
      : emptyBody();
    form = body;
    skillsText = body.skills_off.join('\n');
  });

  const parents = $derived(
    native.profiles.filter((p) => p.id !== native.editing),
  );
  const dirty = $derived(
    JSON.stringify({ ...form, skills_off: parse(skillsText) }) !==
      JSON.stringify(selected ? selected.body : creating ? emptyBody() : null),
  );
  function parse(text: string): string[] {
    return text
      .split('\n')
      .map((l) => l.trim())
      .filter(Boolean);
  }
  function toggle(agent: AgentId) {
    form.applies_to = form.applies_to.includes(agent)
      ? form.applies_to.filter((a) => a !== agent)
      : [...form.applies_to, agent];
  }
  async function save() {
    const body: ProfileBody = {
      ...$state.snapshot(form),
      model: form.model?.trim() ? form.model.trim() : null,
      skills_off: parse(skillsText),
    };
    await controller.saveProfile(
      creating ? null : (selected?.id ?? null),
      body,
    );
  }
  const day = (seconds: number) =>
    new Date(seconds * 1000).toLocaleString(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short',
    });
  function summary(p: Profile): string {
    return p.body.applies_to.map((a) => AGENT_LABEL[a]).join(' · ');
  }
</script>

<section
  class="native-feature"
  aria-label="Session profiles"
  hidden={!visible}
  inert={!visible}
>
  <p class="native-note">
    A profile says what one session should start with. It is applied when the
    session starts and changes no global file: your Claude Code and Codex
    settings stay as they are. A profile holds names and choices, never a
    secret.
  </p>
  {#if native.failure}
    <p class="sessions-status failed" role="alert">
      {failureText(native.failure.code)}
    </p>
  {/if}
  {#if native.notice}
    <p class="sessions-status" role="status">{native.notice}</p>
  {/if}

  <div class="native-profiles">
    <div class="native-profile-list">
      <div class="native-plan-actions">
        <button
          type="button"
          class="action primary"
          onclick={() => controller.edit('new')}>New profile</button
        >
        <button
          type="button"
          class="action"
          onclick={() => controller.importOne()}>Import…</button
        >
      </div>
      <ul class="events-list" aria-label="Profiles">
        {#each native.profiles as profile (profile.id)}
          <li>
            <button
              type="button"
              class="native-profile-item"
              aria-current={native.editing === profile.id}
              onclick={() => controller.edit(profile.id)}
            >
              <span class="event-title">{profile.body.name}</span>
              <span class="event-meta">
                {profile.template ? 'Template' : `Version ${profile.current}`} · {summary(
                  profile,
                )}
                {#if profile.refers_to_missing}
                  · refers to something not present{/if}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    </div>

    <div class="native-profile-editor">
      {#if native.editing === null}
        <div class="sessions-empty">
          <p class="empty-title">Choose a profile or make a new one</p>
          <p>Templates are read only. Make a copy to change one.</p>
        </div>
      {:else}
        <form
          class="native-card"
          onsubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <h3>
            {creating ? 'New profile' : readOnly ? 'Template' : 'Profile'}
          </h3>
          <div class="native-field">
            <label for="p-name">Name</label>
            <input
              id="p-name"
              type="text"
              maxlength="60"
              bind:value={form.name}
              disabled={readOnly}
              required
            />
          </div>
          <fieldset class="native-field" disabled={readOnly}>
            <legend>Written for</legend>
            {#each AGENT_IDS as agent (agent)}
              <label class="native-check">
                <input
                  type="checkbox"
                  checked={form.applies_to.includes(agent)}
                  onchange={() => toggle(agent)}
                />
                {AGENT_LABEL[agent]}
              </label>
            {/each}
          </fieldset>
          <div class="native-field">
            <label for="p-extends">Extends</label>
            <select
              id="p-extends"
              bind:value={form.extends}
              disabled={readOnly}
            >
              <option value={null}>Nothing</option>
              {#each parents as parent (parent.id)}
                <option value={parent.id}>{parent.body.name}</option>
              {/each}
            </select>
          </div>
          <div class="native-field">
            <label for="p-model">Model</label>
            <input
              id="p-model"
              type="text"
              maxlength="64"
              placeholder="the agent’s own default"
              bind:value={form.model}
              disabled={readOnly}
            />
          </div>
          <div class="native-field">
            <label for="p-permission">Permissions</label>
            <select
              id="p-permission"
              bind:value={form.permission}
              disabled={readOnly}
            >
              <option value={null}>As configured</option>
              {#each PERMISSIONS as word (word)}
                <option value={word as Permission}
                  >{PERMISSION_LABEL[word]}</option
                >
              {/each}
            </select>
          </div>
          <div class="native-field">
            <label for="p-hooks">Hooks</label>
            <select id="p-hooks" bind:value={form.hooks} disabled={readOnly}>
              <option value={null}>As configured</option>
              {#each HOOKS_MODES as word (word)}
                <option value={word as HooksMode}>{HOOKS_LABEL[word]}</option>
              {/each}
            </select>
          </div>
          <div class="native-field">
            <label for="p-skills"
              >Skills switched off (Claude Code), one per line</label
            >
            <textarea
              id="p-skills"
              rows="3"
              bind:value={skillsText}
              disabled={readOnly}></textarea>
          </div>
          <div class="native-field">
            <label for="p-notes">Notes</label>
            <textarea
              id="p-notes"
              rows="2"
              maxlength="500"
              bind:value={form.notes}
              disabled={readOnly}></textarea>
          </div>

          {#if selected && Object.keys(selected.from).length > 0}
            <p class="event-meta">
              Inherited values:
              {#each Object.entries(selected.from) as [field, from] (field)}
                {field} from “{from}”;
              {/each}
            </p>
          {/if}

          <div class="native-plan-actions">
            {#if !readOnly}
              <button
                type="submit"
                class="action primary"
                disabled={!dirty || form.name.trim() === ''}
              >
                {creating ? 'Create' : 'Save as a new version'}
              </button>
            {/if}
            {#if selected}
              <button
                type="button"
                class="action"
                onclick={() => selected && controller.duplicate(selected.id)}
                >Make a copy</button
              >
              <button
                type="button"
                class="action"
                onclick={() => selected && controller.exportOne(selected.id)}
                >Export…</button
              >
              {#if !readOnly}
                <button
                  type="button"
                  class="action"
                  onclick={() =>
                    selected && controller.removeProfile(selected.id)}
                  >Delete</button
                >
              {/if}
            {/if}
            <button
              type="button"
              class="action"
              onclick={() => controller.edit(null)}>Close</button
            >
          </div>

          {#if selected && !readOnly && selected.versions.length > 1}
            <h4>Versions</h4>
            <ul class="native-versions">
              {#each [...selected.versions].reverse() as version (version.n)}
                <li>
                  <span
                    >Version {version.n} · {day(version.saved_at)}{version.n ===
                    selected.current
                      ? ' · current'
                      : ''}</span
                  >
                  {#if version.n !== selected.current}
                    <button
                      type="button"
                      class="action"
                      onclick={() =>
                        selected && controller.restore(selected.id, version.n)}
                      >Bring back</button
                    >
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        </form>
      {/if}
    </div>
  </div>
</section>
