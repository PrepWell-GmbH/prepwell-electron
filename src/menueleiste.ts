// Timer in der Mac-Menüleiste (D-47): Symbol rechts oben, daneben die Restzeit, solange ein Timer läuft.
// Klick öffnet darunter ein kleines Fenster: Restzeit und Phase, Pause/Weiter/Beenden, Timer starten, PrepWell öffnen.

import { Tray, nativeImage, screen, type BrowserWindow } from 'electron';
import path from 'path';
import { aufStand, LEER, type TimerStand } from './timer-bruecke';
import { kleinesFenster, sende } from './timer-fenster';
import { sperreSchwebeTimer } from './floating-timer';

const BREITE = 300;
const HOEHE = 236;

let tray: Tray | null = null;
let fenster: BrowserWindow | null = null;
let stand: TimerStand = LEER;

function titel(s: TimerStand): string {
  if (!s.aktiv || !s.text) return '';
  // Kurz neben dem Symbol: „18min“ statt „18min verbleibend“ bzw. „18min gelernt“ (Stoppuhr).
  const kurz = s.text.replace(/\s*(verbleibend|gelernt)$/, '');
  return s.pausiert ? ` ${kurz} ‖` : ` ${kurz}`;
}

function position(): { x: number; y: number } {
  const knopf = tray?.getBounds() ?? { x: 0, y: 0, width: 0, height: 0 };
  const flaeche = screen.getDisplayNearestPoint({ x: knopf.x, y: knopf.y }).workArea;
  const x = Math.round(Math.min(Math.max(knopf.x + knopf.width / 2 - BREITE / 2, flaeche.x + 4), flaeche.x + flaeche.width - BREITE - 4));
  return { x, y: Math.round(knopf.y + knopf.height + 2) };
}

function erzeugeFenster(): BrowserWindow {
  const win = kleinesFenster('menueleiste.html', { width: BREITE, height: HOEHE, alwaysOnTop: true }, () => stand);
  win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
  // Wie ein Menü: Klick daneben schließt.
  win.on('blur', () => win.hide());
  win.on('hide', () => sperreSchwebeTimer(false));
  win.on('closed', () => { fenster = null; });
  return win;
}

export function umschalten(): void {
  if (fenster && !fenster.isDestroyed() && fenster.isVisible()) {
    fenster.hide();
    return;
  }
  sperreSchwebeTimer(true);
  if (!fenster || fenster.isDestroyed()) fenster = erzeugeFenster();
  const { x, y } = position();
  fenster.setPosition(x, y, false);
  sende(fenster, stand);
  fenster.show();
  fenster.focus();
}

export function setupMenueleiste(): void {
  const bild = nativeImage.createFromPath(path.join(__dirname, '..', 'resources', 'trayTemplate.png'));
  bild.setTemplateImage(true);
  tray = new Tray(bild);
  tray.setToolTip('PrepWell Timer');
  tray.on('click', umschalten);
  tray.on('right-click', umschalten);

  aufStand((neu) => {
    stand = neu;
    tray?.setTitle(titel(neu), { fontType: 'monospacedDigit' });
    if (fenster && !fenster.isDestroyed() && fenster.isVisible()) sende(fenster, neu);
  });
}
