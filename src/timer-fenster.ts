// Gemeinsames der kleinen Timer-Fenster (Menüleiste D-47, Ablauf D-46): Fenster-Grundform und die IPC-Wege zurück zur Web-App.

import { BrowserWindow, ipcMain } from 'electron';
import path from 'path';
import { jetztLesen, sendeBefehl } from './timer-bruecke';

/** Was ein Fenster beim Öffnen sofort braucht — je Fenster vom Ersteller hinterlegt. */
const ersteDaten = new Map<number, () => unknown>();

export function kleinesFenster(datei: string, optionen: Electron.BrowserWindowConstructorOptions, daten: () => unknown): BrowserWindow {
  const win = new BrowserWindow({
    frame: false,
    transparent: true,
    hasShadow: false, // Schatten kommt aus dem CSS der Karte
    resizable: false,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    skipTaskbar: true,
    show: false,
    ...optionen,
    webPreferences: {
      preload: path.join(__dirname, 'fenster-preload.js'),
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
    },
  });
  const id = win.webContents.id;
  ersteDaten.set(id, daten);
  win.on('closed', () => ersteDaten.delete(id));
  void win.loadFile(path.join(__dirname, '..', 'resources', datei));
  return win;
}

/** Schickt Daten an ein Fenster, falls es noch lebt. */
export function sende(win: BrowserWindow | null, daten: unknown): void {
  if (win && !win.isDestroyed()) win.webContents.send('timer-daten', daten);
}

export function registriereTimerIpc(holeAppNachVorn: () => void): void {
  ipcMain.on('timer-fenster-bereit', (event) => {
    const daten = ersteDaten.get(event.sender.id);
    if (daten) event.sender.send('timer-daten', daten());
  });

  ipcMain.handle('timer-befehl', async (_event, befehl: unknown) => {
    if (typeof befehl !== 'string') return 'ungueltig';
    const ergebnis = await sendeBefehl(befehl);
    if (ergebnis === 'app') holeAppNachVorn();
    await jetztLesen();
    return ergebnis;
  });

  ipcMain.handle('app-oeffnen', () => holeAppNachVorn());

  ipcMain.on('timer-fenster-hoehe', (event, px: unknown) => {
    const win = BrowserWindow.fromWebContents(event.sender);
    if (!win || typeof px !== 'number' || !Number.isFinite(px)) return;
    const [breite] = win.getSize();
    win.setSize(breite, Math.min(400, Math.max(80, Math.ceil(px))), false);
  });

  ipcMain.on('timer-fenster-schliessen', (event) => {
    BrowserWindow.fromWebContents(event.sender)?.hide();
  });
}
