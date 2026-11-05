// Fenster vorn, wenn der Timer abläuft (D-46): Fokuszeit oder Pause vorbei, Pomodoro geschafft, Countdown abgelaufen.
// Es liegt über allen Fenstern (auch Vollbild) und bleibt stehen, bis man es wegklickt. Jeder Ablauf erscheint genau einmal (Zeitpunkt der Web-App).
// KEINE TÖNE (Lucas 2026-11-05, Frontend ADR-0138): kein beep, keine Wiedergabe — auch nicht abschaltbar eingebaut.

import { app, screen, type BrowserWindow } from 'electron';
import { aufStand, type Ablauf } from './timer-bruecke';
import { kleinesFenster, sende } from './timer-fenster';

const BREITE = 360;
const HOEHE = 172;
const RAND = 24;

/** Zeitpunkt des zuletzt gesehenen Ablaufs; undefined = noch nichts gelesen (der erste Stand setzt nur die Ausgangslage, kein altes Fenster beim Start). */
let gesehen: number | null | undefined;
let fenster: BrowserWindow | null = null;
let aktuell: Ablauf | null = null;

function zeige(ablauf: Ablauf): void {
  aktuell = ablauf;
  if (!fenster || fenster.isDestroyed()) {
    const flaeche = screen.getDisplayNearestPoint(screen.getCursorScreenPoint()).workArea;
    fenster = kleinesFenster('ablauf.html', {
      width: BREITE,
      height: HOEHE,
      x: Math.round(flaeche.x + flaeche.width / 2 - BREITE / 2),
      y: flaeche.y + RAND,
      alwaysOnTop: true,
    }, () => aktuell);
    // 'screen-saver' liegt auf macOS auch über Vollbild-Apps.
    fenster.setAlwaysOnTop(true, 'screen-saver');
    fenster.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
    fenster.on('closed', () => { fenster = null; });
    fenster.once('ready-to-show', () => fenster?.showInactive());
  } else {
    sende(fenster, ablauf);
    fenster.showInactive();
  }
  if (process.platform === 'darwin') app.dock?.bounce('informational');
}

export function setupAblaufFenster(): void {
  aufStand((stand) => {
    const um = stand.ablauf?.um ?? null;
    if (gesehen === undefined) { gesehen = um; return; }
    // Kein Ablauf (Leerlauf, Seite lädt neu) ändert nichts — so erscheint ein alter Ablauf nach kurzem Flackern nicht noch einmal.
    if (um === null || um === gesehen) return;
    gesehen = um;
    if (stand.ablauf) zeige(stand.ablauf);
  });
}
