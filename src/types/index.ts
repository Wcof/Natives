// Global type declarations for Natives

export {};

declare global {
  // HTTP server port injected by main process
  var __nativesHttpPort: number | undefined;

  interface Window {
    __nativesHttpPort?: number;
    nativesAPI?: {
      themeReady: () => void;
      db: {
        get: (key: string) => Promise<string | undefined>;
        set: (key: string, value: unknown) => Promise<{ ok: boolean }>;
        delete: (key: string) => Promise<{ ok: boolean }>;
        list: (prefix?: string) => Promise<string[]>;
      };
      terminal: {
        create: () => Promise<{ sessionId: string | null; error?: string }>;
        write: (sessionId: string, data: string) => Promise<void>;
        resize: (sessionId: string, cols: number, rows: number) => Promise<void>;
        kill: (sessionId: string) => Promise<void>;
      };
      module: {
        scan: () => Promise<unknown[]>;
        install: (pathOrZip: string) => Promise<{ success: boolean; error?: string }>;
        uninstall: (moduleId: string) => Promise<{ success: boolean; error?: string }>;
        list: () => Promise<unknown[]>;
      };
      env: {
        getVariables: (profileId: string) => Promise<Record<string, string>>;
        getDefaultProfile: () => Promise<unknown>;
        listProfiles: () => Promise<unknown[]>;
      };
      getTheme: () => Promise<string>;
      setTheme: (theme: string) => Promise<void>;
      notification: {
        send: (title: string, body: string, level?: string) => Promise<{ ok: boolean }>;
        list: (unreadOnly?: boolean) => Promise<unknown[]>;
      };
      onDbStateChanged: (callback: (event: unknown, channel: string, data: unknown) => void) => () => void;
    };
  }
}
