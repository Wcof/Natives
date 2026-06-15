'use client';

import { useMemo } from 'react';

interface CsvTableProps {
  content: string;
  delimiter?: string;
  maxRows?: number;
}

export default function CsvTable({ content, delimiter = ',', maxRows = 500 }: CsvTableProps) {
  const { headers, rows } = useMemo(() => {
    const lines = content.split('\n').filter(l => l.trim());
    const limited = lines.slice(0, maxRows + 1); // +1 for header
    const parsed = limited.map(line => {
      const cells: string[] = [];
      let current = '';
      let inQuotes = false;
      for (let i = 0; i < line.length; i++) {
        const ch = line[i];
        if (ch === '"') {
          if (inQuotes && line[i + 1] === '"') {
            current += '"';
            i++;
          } else {
            inQuotes = !inQuotes;
          }
        } else if (ch === delimiter && !inQuotes) {
          cells.push(current);
          current = '';
        } else {
          current += ch;
        }
      }
      cells.push(current);
      return cells;
    });
    return {
      headers: parsed[0] || [],
      rows: parsed.slice(1),
    };
  }, [content, delimiter, maxRows]);

  return (
    <div style={{ overflow: 'auto', maxHeight: '100%', fontSize: 12 }}>
      <table style={{
        width: '100%',
        borderCollapse: 'collapse',
        fontFamily: 'var(--font-mono, monospace)',
      }}>
        <thead>
          <tr>
            {headers.map((h, i) => (
              <th key={i} style={{
                padding: '6px 10px',
                textAlign: 'left',
                borderBottom: '2px solid var(--border, #262920)',
                color: 'var(--text-dim, #9b9d8c)',
                fontWeight: 600,
                whiteSpace: 'nowrap',
                position: 'sticky',
                top: 0,
                background: 'var(--bg, #0b0c0a)',
              }}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, ri) => (
            <tr key={ri} style={{
              borderBottom: '1px solid var(--border, #262920)',
            }}>
              {row.map((cell, ci) => (
                <td key={ci} style={{
                  padding: '4px 10px',
                  color: 'var(--text, #f2f2ea)',
                  whiteSpace: 'nowrap',
                  maxWidth: 300,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}>
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {content.split('\n').length > maxRows + 1 && (
        <div style={{
          padding: '8px 12px',
          color: 'var(--text-faint)',
          fontSize: 11,
          textAlign: 'center',
        }}>
          Showing first {maxRows} rows
        </div>
      )}
    </div>
  );
}
