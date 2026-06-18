/**
 * Natives Bridge SDK Type Definitions
 *
 * This file provides TypeScript type definitions for the `window.natives.*` API
 * available to plugins running inside Natives iframes.
 *
 * Usage:
 *   1. Copy this file to your plugin project
 *   2. Add to your tsconfig.json "files" or "include" array
 *   3. Enjoy autocomplete and type-checking!
 */

interface NativesDB {
  /** Read a value by key from the plugin's data store */
  get(key: string): Promise<string | null>;

  /** Write a key-value pair to the plugin's data store */
  set(key: string, value: string): Promise<void>;

  /** Delete a key from the plugin's data store */
  delete(key: string): Promise<void>;

  /** List all keys, optionally filtered by prefix */
  list(prefix?: string): Promise<string[]>;
}

interface NativesSettings {
  /** Get the current theme ID (e.g. 'terminal-volt', 'warm-archive') */
  getTheme(): Promise<string>;

  /** Get the current locale (e.g. 'zh', 'en') */
  getLocale(): Promise<string>;

  /** Listen for theme changes */
  onThemeChange(callback: (theme: string) => void): () => void;
}

interface NativesEnv {
  /** Get an environment variable value (requires 'env' permission) */
  get(key: string): Promise<string | undefined>;
}

interface NativesNotification {
  /** Send a notification to the host (requires 'notification' permission) */
  send(title: string, body: string, level?: 'info' | 'warning' | 'error'): Promise<void>;

  /** Update the module's badge count */
  badge(count: number): Promise<void>;
}

interface NativesIPC {
  /**
   * Send a message to another module (requires 'ipc' permission)
   * @param targetModuleId - The module to send to
   * @param payload - The message payload
   */
  send(targetModuleId: string, payload: unknown): Promise<void>;

  /**
   * Broadcast a message to all active modules (requires 'ipc' permission)
   * @param payload - The message payload
   */
  broadcast(payload: unknown): Promise<void>;

  /**
   * Subscribe to messages for this module
   * @param callback - Called when a message is received
   * @returns Unsubscribe function
   */
  onMessage(callback: (payload: unknown) => void): () => void;
}

interface NativesLifecycle {
  /**
   * Signal that the plugin is ready.
   * Must be called within 10 seconds or the plugin will be marked as timed out.
   */
  ready(): void;

  /**
   * Register a cleanup handler called when the plugin is about to be unloaded.
   * Use this to save state or release resources.
   */
  onUnload(callback: () => void): void;

  /**
   * Register an error handler called when an uncaught error occurs in the plugin.
   */
  onError(callback: (error: Error) => void): void;

  /**
   * Register a heartbeat handler. Called periodically by the host to check if the plugin is alive.
   * Return `true` to indicate the plugin is healthy.
   */
  onHeartbeat(callback: () => boolean): void;
}

interface NativesMeta {
  /** The plugin's module ID (from manifest.json) */
  moduleId: string;

  /** The plugin's version (from manifest.json) */
  version: string;

  /** The Natives Bridge API version */
  nativesVersion: string;
}

/**
 * The Natives Bridge API, available as `window.natives` inside plugin iframes.
 *
 * All methods are async and return Promises. The SDK automatically handles
 * the session token handshake with the host.
 */
interface NativesSDK {
  /** Key-value data store (scoped to this plugin) */
  db: NativesDB;

  /** Host settings (theme, locale) */
  settings: NativesSettings;

  /** Environment variables (requires 'env' permission) */
  env: NativesEnv;

  /** Notifications and badges (requires 'notification' permission) */
  notification: NativesNotification;

  /** Inter-module communication (requires 'ipc' permission) */
  ipc: NativesIPC;

  /** Plugin lifecycle management */
  lifecycle: NativesLifecycle;

  /** Plugin metadata */
  meta: NativesMeta;
}

declare global {
  interface Window {
    /** Natives Bridge SDK — available inside plugin iframes */
    natives?: NativesSDK;
  }
}

export {};
