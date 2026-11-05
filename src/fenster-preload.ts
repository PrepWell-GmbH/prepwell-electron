// Preload der kleinen Timer-Fenster (Menüleiste, Ablauf): nur Stand empfangen und Befehle schicken — kein Zugriff auf Node oder die Web-App.

import { contextBridge, ipcRenderer, IpcRendererEvent } from 'electron';

contextBridge.exposeInMainWorld('prepwellTimer', {
  /** Stand bzw. Ablauf vom Main-Prozess; liefert die Abmeldefunktion. */
  aufDaten: (callback: (daten: unknown) => void): (() => void) => {
    const listener = (_event: IpcRendererEvent, daten: unknown): void => callback(daten);
    ipcRenderer.on('timer-daten', listener);
    ipcRenderer.send('timer-fenster-bereit');
    return () => ipcRenderer.removeListener('timer-daten', listener);
  },
  /** Timer-Befehl an die Web-App: pause, weiter, beenden, start-pomodoro, start-countdown, start-stoppuhr, timer-oeffnen. */
  befehl: (befehl: string): Promise<string> => ipcRenderer.invoke('timer-befehl', befehl),
  /** Hauptfenster nach vorn holen. */
  appOeffnen: (): Promise<void> => ipcRenderer.invoke('app-oeffnen'),
  /** Höhe des Inhalts (px) — das Fenster passt sich an. */
  hoehe: (px: number): void => ipcRenderer.send('timer-fenster-hoehe', px),
  /** Dieses Fenster schließen. */
  schliessen: (): void => ipcRenderer.send('timer-fenster-schliessen'),
});
