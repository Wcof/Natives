'use client';

import { useState, useEffect } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { useLocale, t as tr } from '@/i18n';
import {
  ChevronRight, ChevronDown, File, Folder, FolderOpen,
  Code, Box, GitBranch, Database,
} from 'lucide-react';

/** CodeGraph node from the backend */
interface CodeGraphNode {
  name: string;
  kind: string;
  path?: string;
  symbolType?: string;
  line?: number;
  children: CodeGraphNode[];
}

function getNodeIcon(node: CodeGraphNode, expanded: boolean) {
  if (node.kind === 'dir') return expanded ? <FolderOpen size={14} /> : <Folder size={14} />;
  if (node.kind === 'index') return <Database size={14} />;
  if (node.symbolType === 'function' || node.symbolType === 'method') return <Code size={14} />;
  if (node.symbolType === 'class' || node.symbolType === 'struct' || node.symbolType === 'interface') return <Box size={14} />;
  if (node.symbolType === 'dep' || node.kind === 'dep') return <GitBranch size={14} />;
  return <File size={14} />;
}

function getNodeColor(node: CodeGraphNode): string {
  if (node.kind === 'dir') return 'var(--primary)';
  if (node.kind === 'index') return 'var(--info, #8b5cf6)';
  if (node.symbolType === 'function' || node.symbolType === 'method') return 'var(--primary)';
  if (node.symbolType === 'class' || node.symbolType === 'struct' || node.symbolType === 'interface') return 'var(--warning, #f97316)';
  if (node.symbolType === 'dep' || node.kind === 'dep') return 'var(--diff-add, #22c55e)';
  return 'var(--text-secondary)';
}

function TreeNode({ node, depth = 0 }: { node: CodeGraphNode; depth?: number }) {
  const [expanded, setExpanded] = useState(depth < 2); // auto-expand top 2 levels
  const hasChildren = node.children && node.children.length > 0;

  return (
    <div>
      <div
        onClick={() => hasChildren && setExpanded(!expanded)}
        style={{
          display: 'flex', alignItems: 'center', gap: SPACING.xs,
          padding: `${SPACING.xs}px ${SPACING.xs}px`, paddingLeft: SPACING.sm + depth * 18,
          borderRadius: BORDER_RADIUS.sm, cursor: hasChildren ? 'pointer' : 'default',
          fontSize: FONT_SIZE.xs, color: 'var(--text)',
          transition: 'background 0.1s',
        }}
        onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--surface)'; }}
        onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
      >
        {/* Expand/collapse arrow */}
        <span style={{ width: 14, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
          {hasChildren ? (
            expanded ? <ChevronDown size={12} style={{ color: 'var(--text-disabled)' }} /> : <ChevronRight size={12} style={{ color: 'var(--text-disabled)' }} />
          ) : (
            <span style={{ width: 12 }} />
          )}
        </span>

        {/* Icon */}
        <span style={{ color: getNodeColor(node), flexShrink: 0, display: 'inline-flex' }}>
          {getNodeIcon(node, expanded)}
        </span>

        {/* Name */}
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {node.name}
        </span>

        {/* Type badge */}
        {node.symbolType && (
          <span style={{
            fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', background: 'var(--surface)',
            padding: `0 ${SPACING.xs}px`, borderRadius: BORDER_RADIUS.sm, lineHeight: '14px', flexShrink: 0,
          }}>
            {node.symbolType}
          </span>
        )}

        {/* Line number */}
        {node.line && (
          <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)', fontFamily: 'var(--font-mono)', flexShrink: 0 }}>
            :{node.line}
          </span>
        )}
      </div>

      {/* Children */}
      {expanded && hasChildren && (
        <div>
          {node.children.map((child, i) => (
            <TreeNode key={`${child.name}-${i}`} node={child} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  );
}

interface CodeGraphPanelProps {
  minimal?: boolean;
}

/**
 * CodeGraphPanel — Collapsible tree view of the project's CodeGraph data.
 *
 * Reads `.codegraph/` directory structure from the backend and renders
 * a hierarchical tree. Auto-expands top 2 levels for overview; deeper
 * levels are collapsed by default.
 */
export default function CodeGraphPanel({ minimal }: CodeGraphPanelProps) {
  const locale = useLocale();
  const [nodes, setNodes] = useState<CodeGraphNode[] | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    async function load() {
      try {
        const data = await (window as any).nativesAPI?.codegraph?.read?.() ?? [];
        setNodes(Array.isArray(data) ? data : []);
      } catch {
        setNodes([]);
      } finally {
        setLoading(false);
      }
    }
    load();
  }, []);

  if (loading) {
    return (
      <div style={{ padding: `${SPACING.md}px 0`, textAlign: 'center', color: 'var(--text-disabled)', fontSize: FONT_SIZE.xs }}>
        {tr(locale, 'dashboard.codeGraphLoading')}
      </div>
    );
  }

  if (!nodes || nodes.length === 0) {
    return (
      <div style={{ padding: `${SPACING.md}px 0`, textAlign: 'center', color: 'var(--text-disabled)', fontSize: FONT_SIZE.xs }}>
        {tr(locale, 'dashboard.codeGraphEmpty')}
      </div>
    );
  }

  return (
    <div style={{
      borderRadius: minimal ? undefined : BORDER_RADIUS.md,
      border: minimal ? undefined : '0.0625rem solid var(--border)',
      background: 'var(--surface)',
      padding: `${SPACING.xs}px 0`,
      maxHeight: 400,
      overflow: 'auto',
    }}>
      {nodes.map((node, i) => (
        <TreeNode key={`${node.name}-${i}`} node={node} />
      ))}
    </div>
  );
}
