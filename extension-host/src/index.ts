// ─── Extension Host Index ────────────────────────────────
//
// Main entry point for the isolated TypeScript extension host.
// Loads and runs extensions in a sandboxed environment.

import { ExtensionRPC } from './rpc';
import { Limits } from './limits';

export interface ExtensionManifest {
  id: string;
  name: string;
  version: string;
  kind: 'plugin' | 'mcp_server' | 'skill' | 'hook' | 'command';
  description?: string;
  permissions: string[];
  entry: string;
}

export interface ExtensionInstance {
  manifest: ExtensionManifest;
  rpc: ExtensionRPC;
  limits: Limits;
}

export class ExtensionHost {
  private extensions: Map<string, ExtensionInstance> = new Map();
  private rpc: ExtensionRPC;

  constructor() {
    this.rpc = new ExtensionRPC();
  }

  /**
   * Load and validate an extension manifest.
   * Returns the manifest if valid, or throws if denied.
   */
  async loadExtension(manifest: ExtensionManifest): Promise<ExtensionInstance> {
    // Validate manifest
    if (!manifest.id || !manifest.name || !manifest.version) {
      throw new Error(`Invalid manifest: missing required fields`);
    }

    // Check permissions against allowed list
    const allowedPermissions = ['read', 'search', 'list', 'patch'];
    for (const perm of manifest.permissions) {
      if (!allowedPermissions.includes(perm)) {
        throw new Error(`Permission '${perm}' is not allowed for extensions`);
      }
    }

    // Create limits
    const limits = new Limits({
      timeoutMs: 30_000,
      maxOutputBytes: 1_048_576,
      maxMemoryMb: 128,
    });

    const instance: ExtensionInstance = {
      manifest,
      rpc: this.rpc,
      limits,
    };

    this.extensions.set(manifest.id, instance);
    return instance;
  }

  /**
   * Unload an extension.
   */
  unloadExtension(id: string): void {
    this.extensions.delete(id);
  }

  /**
   * Get a loaded extension by ID.
   */
  getExtension(id: string): ExtensionInstance | undefined {
    return this.extensions.get(id);
  }

  /**
   * List all loaded extensions.
   */
  listExtensions(): ExtensionManifest[] {
    return Array.from(this.extensions.values()).map(e => e.manifest);
  }

  /**
   * Get the RPC interface.
   */
  getRPC(): ExtensionRPC {
    return this.rpc;
  }
}