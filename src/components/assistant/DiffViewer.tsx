'use client';

interface DiffViewerProps {
  oldContent: string;
  newContent: string;
  fileName: string;
  onRollback?: () => void;
}

export default function DiffViewer({ oldContent, newContent, fileName, onRollback }: DiffViewerProps) {
  // Simple line-based diff
  const oldLines = oldContent.split('\n');
  const newLines = newContent.split('\n');
  const maxLines = Math.max(oldLines.length, newLines.length);

  const lines = Array.from({ length: maxLines }, (_, i) => {
    const oldLine = i < oldLines.length ? oldLines[i] : null;
    const newLine = i < newLines.length ? newLines[i] : null;
    const isAdded = oldLine === null && newLine !== null;
    const isRemoved = oldLine !== null && newLine === null;
    const isChanged = oldLine !== newLine && oldLine !== null && newLine !== null;

    return {
      lineNum: i + 1,
      oldLine,
      newLine,
      type: isAdded ? 'added' as const : isRemoved ? 'removed' as const : isChanged ? 'changed' as const : 'unchanged' as const,
    };
  });

  return (
    <div className="rounded-lg border border-[var(--border-subtle)] overflow-hidden">
      <div className="flex items-center justify-between px-3 py-2 bg-[var(--surface)] border-b border-[var(--border-subtle)]">
        <span className="text-xs font-medium text-[var(--text-secondary)]">{fileName}</span>
        <div className="flex gap-3 text-[0.625rem]">
          <span className="text-green-400">+{lines.filter(l => l.type === 'added').length}</span>
          <span className="text-red-400">-{lines.filter(l => l.type === 'removed').length}</span>
          {onRollback && (
            <button
              onClick={onRollback}
              className="text-amber-400 hover:text-amber-300 underline transition-colors"
            >
              Rollback
            </button>
          )}
        </div>
      </div>
      <div className="overflow-x-auto max-h-[300px] overflow-y-auto">
        <table className="w-full text-[0.6875rem] font-mono">
          <tbody>
            {lines.map((line) => {
              const bgColor = line.type === 'added' ? 'bg-green-500/5' :
                line.type === 'removed' ? 'bg-red-500/5' :
                line.type === 'changed' ? 'bg-amber-500/5' : '';
              const textColor = line.type === 'added' ? 'text-green-400' :
                line.type === 'removed' ? 'text-red-400' :
                line.type === 'changed' ? 'text-amber-400' : 'text-[var(--text-secondary)]';

              return (
                <tr key={line.lineNum} className={`${bgColor} hover:bg-[var(--surface-hover)]`}>
                  <td className="px-2 py-0.5 text-right text-[var(--text-disabled)] select-none w-10 border-r border-[var(--border-subtle)]">
                    {line.lineNum}
                  </td>
                  <td className="px-2 py-0.5 text-[var(--text-disabled)] select-none w-6 text-center border-r border-[var(--border-subtle)]">
                    {line.type === 'added' ? '+' : line.type === 'removed' ? '-' : ' '}
                  </td>
                  <td className={`px-2 py-0.5 whitespace-pre ${textColor}`}>
                    {line.type === 'added' ? line.newLine :
                     line.type === 'removed' ? line.oldLine :
                     line.newLine || line.oldLine || ''}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
