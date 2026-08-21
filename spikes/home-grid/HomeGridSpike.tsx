'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import { Grip, Moon, PanelLeftClose, PanelLeftOpen, Sun } from 'lucide-react';
import {
  Responsive,
  noCompactor,
  useContainerWidth,
  type Layout,
  type ResponsiveLayouts,
} from 'react-grid-layout';
import {
  BREAKPOINTS,
  COLUMNS,
  FIXTURE_COUNTS,
  createFixtureLayouts,
  fixtureWidgetId,
  normalizeLayout,
  resolveBreakpoint,
  type FixtureCount,
  type HomeBreakpoint,
} from './layoutModel';

const STABLE_COMPACTOR = { ...noCompactor, preventCollision: true };

type Theme = 'light' | 'dark';

function useRuntimeSignals() {
  const [longTasks, setLongTasks] = useState(0);
  const [maxLongTask, setMaxLongTask] = useState(0);
  const [runtimeErrors, setRuntimeErrors] = useState(0);

  useEffect(() => {
    const onError = () => setRuntimeErrors((count) => count + 1);
    const onRejection = () => setRuntimeErrors((count) => count + 1);
    window.addEventListener('error', onError);
    window.addEventListener('unhandledrejection', onRejection);

    let observer: PerformanceObserver | undefined;
    if (PerformanceObserver.supportedEntryTypes.includes('longtask')) {
      observer = new PerformanceObserver((list) => {
        const entries = list.getEntries();
        setLongTasks((count) => count + entries.length);
        setMaxLongTask((duration) =>
          Math.max(duration, ...entries.map((entry) => entry.duration)),
        );
      });
      observer.observe({ entryTypes: ['longtask'] });
    }

    return () => {
      window.removeEventListener('error', onError);
      window.removeEventListener('unhandledrejection', onRejection);
      observer?.disconnect();
    };
  }, []);

  return { longTasks, maxLongTask, runtimeErrors };
}

export function HomeGridSpike() {
  const [fixtureCount, setFixtureCount] = useState<FixtureCount>(5);
  const [layouts, setLayouts] = useState<ResponsiveLayouts<HomeBreakpoint>>(() =>
    createFixtureLayouts(5),
  );
  const [editMode, setEditMode] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [theme, setTheme] = useState<Theme>('light');
  const [stopCommits, setStopCommits] = useState(0);
  const [activeBreakpoint, setActiveBreakpoint] = useState<HomeBreakpoint>('lg');
  const { width, containerRef, mounted } = useContainerWidth({
    measureBeforeMount: true,
    initialWidth: 1000,
  });
  const signals = useRuntimeSignals();

  const widgetIds = useMemo(
    () => Array.from({ length: fixtureCount }, (_, index) => fixtureWidgetId(index)),
    [fixtureCount],
  );

  const selectFixture = useCallback((count: FixtureCount) => {
    setFixtureCount(count);
    setLayouts(createFixtureLayouts(count));
    setStopCommits(0);
  }, []);

  const commitStoppedLayout = useCallback(
    (layout: Layout) => {
      setLayouts((current) => ({
        ...current,
        [activeBreakpoint]: normalizeLayout(layout, activeBreakpoint, fixtureCount),
      }));
      setStopCommits((count) => count + 1);
    },
    [activeBreakpoint, fixtureCount],
  );

  return (
    <main className="spike-shell" data-theme={theme}>
      <aside className={sidebarCollapsed ? 'spike-sidebar collapsed' : 'spike-sidebar'}>
        <strong className="brand">N</strong>
        {!sidebarCollapsed && <span className="brand-name">AiNative</span>}
        <button
          className="icon-button sidebar-toggle"
          type="button"
          aria-label={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          onClick={() => setSidebarCollapsed((collapsed) => !collapsed)}
        >
          {sidebarCollapsed ? <PanelLeftOpen /> : <PanelLeftClose />}
        </button>
        <div className="sidebar-spacer" />
        <span className="avatar">LD</span>
      </aside>

      <section className="workspace">
        <div className="toolbar">
          <div className="segmented-control" aria-label="Fixture size">
            {FIXTURE_COUNTS.map((count) => (
              <button
                key={count}
                type="button"
                aria-pressed={fixtureCount === count}
                onClick={() => selectFixture(count)}
              >
                {count}
              </button>
            ))}
          </div>

          <label className="switch-control">
            <input
              type="checkbox"
              checked={editMode}
              onChange={(event) => setEditMode(event.target.checked)}
            />
            <span>Edit</span>
          </label>

          <button
            className="icon-button"
            type="button"
            aria-label={theme === 'light' ? 'Use dark theme' : 'Use light theme'}
            onClick={() => setTheme((current) => (current === 'light' ? 'dark' : 'light'))}
          >
            {theme === 'light' ? <Moon /> : <Sun />}
          </button>
        </div>

        <dl className="telemetry" aria-label="Grid telemetry">
          <div><dt>Container</dt><dd data-testid="container-width">{Math.round(width)}px</dd></div>
          <div><dt>Breakpoint</dt><dd data-testid="breakpoint">{resolveBreakpoint(width)}</dd></div>
          <div><dt>Stop commits</dt><dd data-testid="stop-commits">{stopCommits}</dd></div>
          <div><dt>Long tasks</dt><dd>{signals.longTasks}</dd></div>
          <div><dt>Max task</dt><dd>{signals.maxLongTask.toFixed(1)}ms</dd></div>
          <div><dt>Runtime errors</dt><dd data-testid="runtime-errors">{signals.runtimeErrors}</dd></div>
        </dl>

        <div className="grid-viewport" ref={containerRef} data-testid="grid-container">
          {mounted ? (
            <Responsive<HomeBreakpoint>
              width={width}
              layouts={layouts}
              breakpoints={BREAKPOINTS}
              cols={COLUMNS}
              rowHeight={32}
              margin={[12, 12]}
              containerPadding={[24, 24]}
              compactor={STABLE_COMPACTOR}
              dragConfig={{
                enabled: editMode,
                bounded: true,
                handle: '.widget-drag-handle',
                cancel: '.widget-content',
                threshold: 3,
              }}
              resizeConfig={{ enabled: editMode, handles: ['se'] }}
              onBreakpointChange={(breakpoint) => setActiveBreakpoint(breakpoint)}
              onDragStop={commitStoppedLayout}
              onResizeStop={commitStoppedLayout}
            >
              {widgetIds.map((id, index) => (
                <article className={editMode ? 'fixture-widget editing' : 'fixture-widget'} key={id}>
                  <div className="widget-drag-handle" aria-hidden={!editMode}>
                    {editMode && <Grip />}
                  </div>
                  <div className="widget-content">
                    <span>Fixture</span>
                    <strong>{index + 1}</strong>
                    <small>{id}</small>
                  </div>
                </article>
              ))}
            </Responsive>
          ) : (
            <div className="measuring">Measuring container</div>
          )}
        </div>
      </section>
    </main>
  );
}
