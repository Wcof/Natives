/**
 * Free Canvas grouping model (C-034).
 * Pure functions for grouping, translating group members, and ungrouping.
 */

import type { CanvasNode } from './types';
import { boundsOfNodes } from './frames';

/** Create a group node containing the selected member ids. */
export function createGroupNode(
  nodes: CanvasNode[],
  memberIds: string[],
  newGroupId: string,
): { groupNode: CanvasNode; updatedNodes: CanvasNode[] } | null {
  const members = nodes.filter((n) => memberIds.includes(n.id) && n.kind !== 'group');
  if (members.length < 2) return null;

  const bounds = boundsOfNodes(members);
  if (!bounds) return null;

  const maxZ = members.reduce((m, n) => Math.max(m, n.z), 0);

  const groupNode: CanvasNode = {
    id: newGroupId,
    kind: 'group',
    label: `Group (${members.length})`,
    x: bounds.x,
    y: bounds.y,
    w: bounds.w,
    h: bounds.h,
    z: maxZ + 1,
    members: members.map((m) => m.id),
  };

  const updatedNodes = nodes.map((n) => {
    if (memberIds.includes(n.id)) {
      return { ...n, frameId: groupNode.id };
    }
    return n;
  });

  return {
    groupNode,
    updatedNodes: [...updatedNodes, groupNode],
  };
}

/** Translate all members of a group when the group moves by (dx, dy). */
export function translateGroupMembers(
  nodes: CanvasNode[],
  groupId: string,
  dx: number,
  dy: number,
): CanvasNode[] {
  const group = nodes.find((n) => n.id === groupId && n.kind === 'group');
  if (!group || !group.members) return nodes;

  const memberSet = new Set(group.members);
  return nodes.map((node) => {
    if (memberSet.has(node.id)) {
      return { ...node, x: node.x + dx, y: node.y + dy };
    }
    if (node.id === groupId) {
      return { ...node, x: node.x + dx, y: node.y + dy };
    }
    return node;
  });
}

/** Dissolve a group node and un-parent its members. */
export function ungroupNode(nodes: CanvasNode[], groupId: string): CanvasNode[] {
  const group = nodes.find((n) => n.id === groupId && n.kind === 'group');
  if (!group) return nodes;

  const memberSet = new Set(group.members ?? []);
  return nodes
    .filter((n) => n.id !== groupId)
    .map((n) => (memberSet.has(n.id) && n.frameId === groupId ? { ...n, frameId: undefined } : n));
}
