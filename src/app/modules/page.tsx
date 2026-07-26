'use client';

/**
 * /modules deep link 占位。
 * 实际渲染由 ShellLayout 完成：useShellState 把 '/modules' 映射为 'modules' 视图，
 * MainContent 对该视图渲染 WorkshopPage（个人创意单一表面），本路由子节点不会被显示。
 * 此前这里有一份独立的 ModulesPage（约 240 行），与 WorkshopPage 功能重复且不可达，已删除。
 */
export default function ModulesRoute() {
  return null;
}
