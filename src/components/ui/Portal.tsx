'use client';

import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import type { ReactNode } from 'react';

interface PortalProps {
  /**
   * Portal 的挂载目标，默认 document.body。
   * 渲染到 body 可脱离任何带 backdrop-filter / transform / filter / contain
   * 的祖先容器——这些属性会让后代的 position:fixed 失去相对视口定位，
   * 被 overflow:hidden 裁切。这是浮层（模态框、吐司、下拉菜单）的标准做法。
   */
  container?: Element;
  children: ReactNode;
}

/**
 * 超薄 Portal 基元 —— 把子节点渲染到 document.body。
 *
 * 内置 mounted 守卫，保证仅在客户端挂载后才执行 createPortal，
 * 避免 Next.js SSR/hydration 不一致。
 */
export default function Portal({ container, children }: PortalProps) {
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    setMounted(true);
  }, []);

  if (!mounted) return null;
  return createPortal(children, container ?? document.body);
}
