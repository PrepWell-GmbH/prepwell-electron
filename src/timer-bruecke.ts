// Brücke zum Timer der Web-App (Frontend: features/timer/hooks/use-desktop-timer.ts, D-46/D-47).
// Liest sekündlich `window.__prepwellDesktopTimer.getStand()` aus dem Hauptfenster und reicht Befehle an `steuern()` weiter.
// Fehlt der Hook (ältere Web-App, nicht eingeloggt, Seite lädt), gilt "kein Timer" — die Hülle bleibt still.

import type { BrowserWindow } from 'electron';

export interface Ablauf {
  nr: number;
  /** Zeitpunkt des Ablaufs (epoch ms) — Kennung eines Ablaufs, auch über ein Neuladen der Web-App hinweg. */
  um: number;
  art: 'pause' | 'fokus' | 'fertig';
  titel: string;
  text: string;
  wartet: boolean;
}

export interface TimerStand {
  aktiv: boolean;
  pausiert: boolean;
  modus: 'pomodoro' | 'countdown' | 'stoppuhr' | null;
  pause: boolean;
  text: string | null;
  label: string | null;
  fortschritt: number;
  ablauf: Ablauf | null;
}

export type BefehlErgebnis = 'ok' | 'app' | 'ungueltig';

export const LEER: TimerStand = {
  aktiv: false, pausiert: false, modus: null, pause: false, text: null, label: null, fortschritt: 0, ablauf: null,
};

const BEFEHLE = new Set(['pause', 'weiter', 'beenden', 'start-pomodoro', 'start-countdown', 'start-stoppuhr', 'timer-oeffnen']);
const TAKT_MS = 1000;

const PROBE = `(() => {
  const hook = window.__prepwellDesktopTimer;
  if (!hook || typeof hook.getStand !== 'function') return null;
  try { return hook.getStand(); } catch (e) { return null; }
})()`;

let hauptfenster: BrowserWindow | null = null;
let takt: ReturnType<typeof setInterval> | null = null;
const hoerer: Array<(stand: TimerStand) => void> = [];

/** Prüft, was aus dem Renderer kommt — fremde Eingabe. */
function alsStand(roh: unknown): TimerStand {
  if (!roh || typeof roh !== 'object') return LEER;
  const s = roh as Partial<TimerStand>;
  if (typeof s.aktiv !== 'boolean') return LEER;
  const a = s.ablauf;
  const ablauf = a && typeof a.nr === 'number' && typeof a.um === 'number' && typeof a.titel === 'string' && typeof a.text === 'string'
    ? { nr: a.nr, um: a.um, art: a.art ?? 'fertig', titel: a.titel, text: a.text, wartet: a.wartet === true }
    : null;
  return {
    aktiv: s.aktiv,
    pausiert: s.pausiert === true,
    modus: s.modus ?? null,
    pause: s.pause === true,
    text: typeof s.text === 'string' ? s.text : null,
    label: typeof s.label === 'string' ? s.label : null,
    fortschritt: typeof s.fortschritt === 'number' ? Math.min(1, Math.max(0, s.fortschritt)) : 0,
    ablauf,
  };
}

export async function leseStand(): Promise<TimerStand> {
  if (!hauptfenster || hauptfenster.isDestroyed()) return LEER;
  try {
    return alsStand(await hauptfenster.webContents.executeJavaScript(PROBE));
  } catch {
    return LEER;
  }
}

/** Schickt einen Befehl an die Web-App. Unbekannte Befehle verlassen die Hülle nicht. */
export async function sendeBefehl(befehl: string): Promise<BefehlErgebnis> {
  if (!BEFEHLE.has(befehl) || !hauptfenster || hauptfenster.isDestroyed()) return 'ungueltig';
  try {
    const ergebnis: unknown = await hauptfenster.webContents.executeJavaScript(
      `(() => { const h = window.__prepwellDesktopTimer; return h && typeof h.steuern === 'function' ? h.steuern(${JSON.stringify(befehl)}) : 'ungueltig'; })()`,
    );
    return ergebnis === 'ok' || ergebnis === 'app' ? ergebnis : 'ungueltig';
  } catch {
    return 'ungueltig';
  }
}

export function aufStand(hoererFn: (stand: TimerStand) => void): void {
  hoerer.push(hoererFn);
}

/** Sofort einmal lesen und verteilen (z. B. direkt nach einem Befehl). */
export async function jetztLesen(): Promise<void> {
  const stand = await leseStand();
  for (const h of hoerer) h(stand);
}

export function starteTimerBruecke(win: BrowserWindow): void {
  hauptfenster = win;
  if (takt) clearInterval(takt);
  takt = setInterval(() => void jetztLesen(), TAKT_MS);
  win.on('closed', () => {
    hauptfenster = null;
    if (takt) clearInterval(takt);
    takt = null;
    for (const h of hoerer) h(LEER);
  });
}
