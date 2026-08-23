/** React-grid-layout-compatible structured layout types. */
export interface GridLayoutItem {
  i: string;
  x: number;
  y: number;
  w: number;
  h: number;
  minW?: number;
  minH?: number;
  maxW?: number;
  maxH?: number;
  isBounded?: boolean;
}

export type GridLayouts = Record<'lg' | 'md' | 'sm', GridLayoutItem[]>;

export const GRID_BREAKPOINTS = { lg: 1200, md: 996, sm: 768 };
export const GRID_COLUMNS: Record<'lg' | 'md' | 'sm', number> = { lg: 12, md: 8, sm: 4 };
export const GRID_ROW_HEIGHT = 32;
export const GRID_MARGIN: [number, number] = [8, 8];
export const GRID_PADDING: [number, number] = [12, 12];
