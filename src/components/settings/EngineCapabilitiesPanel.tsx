'use client';

import { Clock, Loader, Puzzle, RefreshCw, Server, Sparkles, Users, Zap } from 'lucide-react';
import { t } from '@/i18n';
import { hasMethod } from '@/lib/assistant-workspace/capability-gate';
import { SPACING } from '@/lib/design-tokens';
import {
  AvailabilityCard,
  ExtensionsList,
  InventoryList,
  InventoryRow,
  LoadableSection,
  Notice,
  RateLimitEditor,
  RunEvidenceCard,
  SnapshotEvidenceCards,
} from './engine-capabilities/cards';
import {
  useEngineCapabilities,
  type EngineCapabilitiesPanelProps,
} from './engine-capabilities/useEngineCapabilities';

export type { EngineCapabilitySnapshot, EngineCapabilityRun } from './engine-capabilities/model';

export default function EngineCapabilitiesPanel(props: EngineCapabilitiesPanelProps) {
  const panel = useEngineCapabilities(props);
  const { locale } = panel;

  return (
    <div data-testid="engine-capabilities-panel">
      <div className="mb-3 flex justify-end">
        <button
          type="button"
          className="btn inline-flex items-center gap-2 text-xs"
          onClick={() => void panel.reload()}
          disabled={panel.capabilities.phase === 'loading'}
        >
          {panel.capabilities.phase === 'loading' ? (
            <Loader size={12} className="animate-spin" />
          ) : (
            <RefreshCw size={12} />
          )}
          {t(locale, 'common.refresh')}
        </button>
      </div>

      <AvailabilityCard
        locale={locale}
        state={panel.capabilities}
        onRetry={() => void panel.reload()}
      />

      {props.selectedRun ? (
        <RunEvidenceCard
          locale={locale}
          run={props.selectedRun}
          snapshotLoaded={panel.snapshotLoaded}
          selectedSkillIds={panel.selectedSkillIds}
          selectedMcpIds={panel.selectedMcpIds}
          selectedExpertId={panel.selectedExpertId}
          selectedTeamId={panel.selectedTeamId}
        />
      ) : (
        <Notice
          title={t(locale, 'settings.engineCapabilities.noRunTitle')}
          description={t(locale, 'settings.engineCapabilities.noRunDesc')}
        />
      )}

      {props.selectedRun ? (
        <SnapshotEvidenceCards
          locale={locale}
          snapshotLoaded={panel.snapshotLoaded}
          toolPlan={panel.toolPlan}
          toolPlanHash={panel.toolPlanHash}
          promptPlan={panel.promptPlan}
          promptLayers={panel.promptLayers}
        />
      ) : null}

      <div className="settings-section-card mb-3">
        <h4>{t(locale, 'settings.engineCapabilities.inventoryTitle')}</h4>
        <p className="mt-1 text-xs text-[var(--text-muted)]">
          {t(locale, 'settings.engineCapabilities.inventoryDesc')}
        </p>
      </div>

      <LoadableSection
        icon={<Sparkles size={16} />}
        title={t(locale, 'settings.engineCapabilities.skillsTitle')}
        state={panel.skills}
        count={(items) => items.length}
        empty={t(locale, 'settings.engineCapabilities.skillsEmpty')}
        locale={locale}
        onRetry={() => {
          if (panel.caps) void panel.loadSkills(panel.caps);
        }}
        render={(items) => (
          <InventoryList>
            {items.map((skill) => {
              const selected = panel.selectedSkillIds.includes(skill.id);
              return (
                <InventoryRow
                  key={skill.id}
                  title={skill.name}
                  detail={skill.id}
                  locale={locale}
                  selected={selected}
                  loaded={selected && panel.snapshotLoaded}
                  enabled={skill.enabled}
                  trusted={skill.trusted}
                />
              );
            })}
          </InventoryList>
        )}
      />

      <LoadableSection
        icon={<Server size={16} />}
        title={t(locale, 'settings.engineCapabilities.mcpTitle')}
        state={panel.mcpServers}
        count={(items) => items.length}
        empty={t(locale, 'settings.engineCapabilities.mcpEmpty')}
        locale={locale}
        onRetry={() => {
          if (panel.caps) void panel.loadMcpServers(panel.caps);
        }}
        render={(items) => (
          <InventoryList>
            {items.map((server) => {
              const selected = panel.selectedMcpIds.includes(server.id);
              return (
                <InventoryRow
                  key={server.id}
                  title={server.name}
                  detail={`${server.id} · ${server.transport}`}
                  locale={locale}
                  selected={selected}
                  loaded={selected && panel.snapshotLoaded}
                  enabled={server.enabled}
                  trusted={server.trusted}
                />
              );
            })}
          </InventoryList>
        )}
      />

      <div className="grid gap-3 lg:grid-cols-2">
        <LoadableSection
          icon={<Users size={16} />}
          title={t(locale, 'settings.engineCapabilities.expertsTitle')}
          state={panel.experts}
          count={(items) => items.length}
          empty={t(locale, 'settings.engineCapabilities.expertsEmpty')}
          locale={locale}
          onRetry={() => {
            if (panel.caps) void panel.loadExperts(panel.caps);
          }}
          render={(items) => (
            <InventoryList>
              {items.map((expert) => {
                const selected = expert.id === panel.selectedExpertId;
                return (
                  <InventoryRow
                    key={expert.id}
                    title={expert.name}
                    detail={[expert.id, expert.modelId, expert.source].filter(Boolean).join(' · ')}
                    locale={locale}
                    selected={selected}
                    loaded={selected && panel.snapshotLoaded}
                    enabled={expert.enabled}
                  >
                    {expert.description ? <p className="m-0 text-xs text-[var(--text-muted)]">{expert.description}</p> : null}
                    <details className="text-xs">
                      <summary className="cursor-pointer text-[var(--text-secondary)]">{t(locale, 'settings.engineCapabilities.systemPrompt')}</summary>
                      <pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap rounded bg-[var(--background)] p-2 font-mono text-[11px] text-[var(--text-secondary)]">{expert.systemPrompt || t(locale, 'settings.engineCapabilities.promptEmpty')}</pre>
                    </details>
                  </InventoryRow>
                );
              })}
            </InventoryList>
          )}
        />

        <LoadableSection
          icon={<Users size={16} />}
          title={t(locale, 'settings.engineCapabilities.teamsTitle')}
          state={panel.teams}
          count={(items) => items.length}
          empty={t(locale, 'settings.engineCapabilities.teamsEmpty')}
          locale={locale}
          onRetry={() => {
            if (panel.caps) void panel.loadTeams(panel.caps);
          }}
          render={(items) => (
            <InventoryList>
              {items.map((team) => {
                const selected = team.id === panel.selectedTeamId;
                return (
                  <InventoryRow
                    key={team.id}
                    title={team.name}
                    detail={t(locale, 'settings.engineCapabilities.teamDetail', {
                      id: team.id,
                      count: team.members.length,
                      coordinator: team.coordinatorExpertId ?? '—',
                    })}
                    locale={locale}
                    selected={selected}
                    loaded={selected && panel.snapshotLoaded}
                    enabled={team.enabled}
                  >
                    {team.description ? <p className="m-0 text-xs text-[var(--text-muted)]">{team.description}</p> : null}
                    {team.members.length ? (
                      <div className="flex flex-wrap gap-1">
                        {team.members.map((member) => (
                          <code key={`${team.id}:${member.expertId}`} className="rounded bg-[var(--background)] px-1.5 py-0.5 text-[11px] text-[var(--text-secondary)]">
                            {member.expertId}
                          </code>
                        ))}
                      </div>
                    ) : null}
                  </InventoryRow>
                );
              })}
            </InventoryList>
          )}
        />
      </div>

      <LoadableSection
        icon={<Zap size={16} />}
        title={t(locale, 'rateLimit.title')}
        state={panel.rateLimit}
        empty={t(locale, 'settings.engineCapabilities.rateLimitEmpty')}
        locale={locale}
        onRetry={() => {
          if (panel.caps) void panel.loadRateLimit(panel.caps);
        }}
        render={(snapshot) => (
          <RateLimitEditor
            locale={locale}
            snapshot={snapshot}
            editEnabled={panel.editEnabled}
            onEditEnabledChange={panel.setEditEnabled}
            editRpm={panel.editRpm}
            onEditRpmChange={panel.setEditRpm}
            rpmValid={panel.rpmValid}
            intervalSeconds={panel.intervalSeconds}
            saving={panel.saving}
            canUpdate={hasMethod(panel.caps, 'engine.rateLimit.update')}
            onSave={() => void panel.saveRateLimit()}
          />
        )}
      />

      <LoadableSection
        icon={<Puzzle size={16} />}
        title={t(locale, 'settings.engineCapabilities.extensionsTitle')}
        state={panel.extensions}
        count={(snapshot) => snapshot.extensions.length}
        empty={t(locale, 'settings.engineCapabilities.extensionsEmpty')}
        locale={locale}
        onRetry={() => {
          if (panel.caps) void panel.loadExtensions(panel.caps);
        }}
        description={t(locale, 'settings.engineCapabilities.extensionsDesc')}
        render={(snapshot) => (
          <ExtensionsList locale={locale} snapshot={snapshot} />
        )}
      />

      <div className="settings-section-card mb-3">
        <div className="settings-section-heading settings-section-heading-with-icon">
          <span className="settings-preference-icon">
            <Clock size={16} />
          </span>
          <div>
            <h4>{t(locale, 'settings.engineCapabilities.jobsTitle')}</h4>
            <p className="text-xs text-[var(--text-muted)]">
              {t(locale, 'settings.engineCapabilities.jobsDesc')}
            </p>
          </div>
        </div>
        <div style={{ padding: SPACING.md }}>
          <a href="/jobs" className="btn text-xs">
            {t(locale, 'settings.engineCapabilities.openJobs')}
          </a>
        </div>
      </div>
    </div>
  );
}
