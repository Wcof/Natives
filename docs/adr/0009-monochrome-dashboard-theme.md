# ADR 0009: Monochrome Dashboard and Grayscale Global Brand Accent

> **Status**: Superseded in scope by [ADR 0010](./0010-global-neutral-spectrum.md). Its semantic-color and restrained-Dashboard decisions remain valid.

## Context

The Natives workspace features a Dashboard that displays system and tool usage. The current system guidelines enforce color constraints:
- Terminal Volt (dark mode): Bright Green/Aurora Green (`#00ff9c`) accents.
- Frosted Jasmine (light mode): Warm Orange/Peach (`#ff793f`) accents.

However, the product requirements for the Usage Dashboard and general UI aesthetics have evolved to target:
- A high-density, restrained, monochrome (black, white, gray) interface.
- Bold typography contrast, clear hierarchy of numbers, and tabular figures.
- Elimination of unnecessary decoration gradients, decorative glow effects, or superficial frosted glass.
- Preserving color exclusively for semantic states: error (danger), warning (warning), success (success), and info (info), and ensuring they are distinguishable.

This creates a conflict between the existing design tokens styling rules and the new monochrome Usage Dashboard.

## Decision

We will:
1. Revise the design-tokens standard to officially support monochrome as the primary branding styling.
2. Maintain standard semantic color tokens for status indicator:
   - Danger: Red (e.g., `--danger`, `--danger-soft`)
   - Warning: Orange/Yellow (e.g., `--warning`, `--warning-soft`)
   - Success: Green (e.g., `--success`, `--success-soft`)
   - Info: Blue (e.g., `--info`, `--info-soft`)
3. Prohibit the use of same gray/monochrome color for distinct semantic statuses, ensuring errors, warnings, and successes are easily recognizable.
4. Eliminate gradients, colored glows, and over-glassmorphism decorations on the Usage Dashboard page.
5. Supplement the theme engine and global stylesheet with soft semantics background variables (`--danger-soft`, `--warning-soft`, `--success-soft`, `--info-soft`) for background highlights.

## Consequences

- The global styles will be updated to include the soft semantics background tokens.
- `docs/standards/ui-ux/01-design-tokens.md` will be updated to remove color-isolated accent rules for the Dashboard.
- The Usage Dashboard will strictly use monochrome primary variables, semantic warnings, and clear layout separation.
