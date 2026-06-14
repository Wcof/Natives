'use client';

import { useState } from 'react';

interface ReplayStep {
  path: string;
  timestamp: number;
}

export default function SessionReplay() {
  const [steps] = useState<ReplayStep[]>([]);
  const [currentStep, setCurrentStep] = useState(0);

  return (
    <div style={{ padding: 8 }}>
      <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-dim)', marginBottom: 8, textTransform: 'uppercase' }}>
        Session Replay
      </div>

      {steps.length === 0 ? (
        <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
          Select a session to replay
        </div>
      ) : (
        <>
          {/* Timeline slider */}
          <input
            type="range"
            min={0}
            max={steps.length - 1}
            value={currentStep}
            onChange={(e) => setCurrentStep(Number(e.target.value))}
            style={{ width: '100%', marginBottom: 8 }}
          />
          <div style={{ fontSize: 11, color: 'var(--text-dim)', textAlign: 'center' }}>
            Step {currentStep + 1} of {steps.length}
          </div>
          {steps[currentStep] && (
            <div style={{ marginTop: 8, padding: 8, background: 'var(--bg-2)', borderRadius: 'var(--radius)', fontSize: 12 }}>
              {steps[currentStep]!.path}
            </div>
          )}
        </>
      )}
    </div>
  );
}
