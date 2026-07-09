'use client';

import { ReactNode } from 'react';
import { motion, useReducedMotion } from 'framer-motion';

interface ContentAreaProps {
  children?: ReactNode;
}

export default function ContentArea({ children }: ContentAreaProps) {
  const prefersReducedMotion = useReducedMotion();

  return (
    <motion.div
      className="doppelrand-outer h-full w-full"
      initial={prefersReducedMotion ? undefined : { opacity: 0, scale: 0.98 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={prefersReducedMotion ? undefined : { type: 'spring', stiffness: 80, damping: 15, mass: 0.9 }}
    >
      <div className="doppelrand-inner h-full w-full flex flex-col">
        {children || (
          <div className="flex-1 flex items-center justify-center text-[var(--text-disabled)] text-lg font-light">
            <div className="flex flex-col items-center gap-4">
              <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="opacity-30">
                <rect x="2" y="3" width="20" height="14" rx="2" ry="2"/>
                <line x1="8" y1="21" x2="16" y2="21"/>
                <line x1="12" y1="17" x2="12" y2="21"/>
              </svg>
              <span>Select a module to get started</span>
            </div>
          </div>
        )}
      </div>
    </motion.div>
  );
}
