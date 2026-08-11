// Preload script — runs in isolated renderer context with node access.
// Exposes a typed API to the web app via contextBridge.
// The web app accesses this as window.electronAPI (typed in vite-env.d.ts).

import { contextBridge, ipcRenderer, IpcRendererEvent } from 'electron';

contextBridge.exposeInMainWorld('electronAPI', {
  /** Always true — used by ElectronGate to detect the Electron wrapper */
  isElectron: true,
  /** Current OS: 'darwin' | 'win32' | 'linux' */
  platform: process.platform,
  /** Opens a URL in the system default browser (for Stripe, OAuth, etc.) */
  openExternal: (url: string): Promise<void> => ipcRenderer.invoke('open-external', url),

  /**
   * Meldet an, dass das Frontend den Update-Hinweis selbst rendert, und
   * abonniert ihn. Der Callback bekommt die bereitliegende Version.
   *
   * Wichtig: Wird diese Funktion NICHT aufgerufen, zeigt die Shell stattdessen
   * einen nativen Dialog. Es gibt also nie den Fall, dass ein fertiges Update
   * unbemerkt liegen bleibt.
   *
   * Rückgabewert ist die Abmeldefunktion (für useEffect-Cleanup).
   */
  onUpdateReady: (callback: (version: string) => void): (() => void) => {
    const listener = (_event: IpcRendererEvent, version: string): void => callback(version);
    ipcRenderer.on('update-downloaded', listener);
    ipcRenderer.send('update-ui-ready');
    return () => {
      ipcRenderer.removeListener('update-downloaded', listener);
    };
  },

  /** Liegt gerade ein Update bereit? Liefert die Version oder null. */
  getPendingUpdate: (): Promise<string | null> => ipcRenderer.invoke('get-pending-update'),

  /**
   * Installiert das bereitliegende Update. Die App beendet sich dabei und
   * startet neu — nach diesem Aufruf läuft im Renderer nichts mehr.
   * Liefert false, wenn gar kein Update bereitliegt.
   */
  installUpdate: (): Promise<boolean> => ipcRenderer.invoke('install-update'),
});
