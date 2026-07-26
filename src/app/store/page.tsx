'use client';

/**
 * /store deep link 占位。
 * 联网商店按 ADR-0012 属 P2 后置；本地「个人创意」是唯一表面。
 * useShellState 把 '/store' 映射为 'modules' 视图，由 WorkshopPage 渲染，
 * 本路由子节点不会被显示。此前这里有一份不可达的 StorePage（约 270 行），已删除。
 */
export default function StoreRoute() {
  return null;
}
