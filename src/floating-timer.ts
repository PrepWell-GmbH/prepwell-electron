// Schwebe-Timer — Stufe 2, Plan B: natives Zweitfenster (ADR-0070-Fallback).
//
// Document PiP ist in Electron nicht implementiert (electron/electron#39633).
// Auf macOS UND Linux vermessen: requestWindow() löst auf, ~3ms später feuert
// pagehide+unload, das Fenster existiert nie sichtbar. Deshalb greift der im
// ADR vorgesehene Fallback: ein kleines, rahmenloses BrowserWindow, das der
// Main-Prozess selbst besitzt — Schweben ist damit garantiert statt erhofft,
// und es funktioniert auf jeder Plattform.
//
// Automatik: Fokus-Verlust des Hauptfensters öffnet (nur wenn ein Timer
// aktiv ist), Fokus-Rückkehr schließt. Minimieren/Verstecken zählt als
// Fokus-Verlust. Das Schwebe-Fenster wird mit showInactive() gezeigt und
// stiehlt der App, zu der der Nutzer wechselt, nicht den Fokus — und weil
// nur der 'focus' des HAUPTfensters schließt, gibt es keine Rückkopplung,
// wenn der Nutzer das Schwebe-Fenster selbst anfasst.
//
// Der Frontend-Hook (window.__prepwellFloatingTimer) liefert nur noch den
// Timer-Zustand. Seine open()/close()-Methoden (Document PiP) werden nicht
// mehr aufgerufen. Kontrakt für den Anzeige-Text: getDisplayState() —
// optional, die Shell fällt ohne ihn auf "Timer aktiv" zurück.

import { app, BrowserWindow, screen } from 'electron';
import path from 'path';

const LOG = '[FloatingTimer]';
const IS_DEV = !app.isPackaged;

// Karte im Look des Dashboard-FloatingWidget, kompakt. Im Fenstermaß stecken
// 10px Rand pro Seite für den CSS-Schatten (das Fenster selbst ist transparent).
const WIDTH = 280;
const HEIGHT = 80;
const MARGIN = 16;
const POLL_MS = 1000;

/** Was die Zustands-Probe aus dem Hauptfenster zurückmeldet. */
interface DisplayState {
  hookPresent: boolean;
  active: boolean;
  /** Anzeige-Text vom Frontend (z.B. "24:31"), sonst null. */
  text: string | null;
  label: string | null;
}

const EMPTY_STATE: DisplayState = { hookPresent: false, active: false, text: null, label: null };

let mainWindow: BrowserWindow | null = null;
let floatWin: BrowserWindow | null = null;
let poll: ReturnType<typeof setInterval> | null = null;
let inactiveReads = 0;

// Läuft im Renderer des Hauptfensters. Tolerant gegenüber dem heutigen Hook
// (nur isTimerActive) und dem künftigen getDisplayState(): { active, text,
// label } — sobald das Frontend den Getter liefert, zeigt das Fenster die
// echte Zeit, ohne dass die Shell sich ändern muss.
const STATE_PROBE = `(() => {
  const hook = window.__prepwellFloatingTimer;
  if (!hook) return { hookPresent: false, active: false, text: null, label: null };
  let active = false;
  try { active = !!hook.isTimerActive(); } catch (e) {}
  let text = null, label = null;
  try {
    if (typeof hook.getDisplayState === 'function') {
      const s = hook.getDisplayState();
      if (s) {
        text = typeof s.text === 'string' ? s.text : null;
        label = typeof s.label === 'string' ? s.label : null;
        if (typeof s.active === 'boolean') active = s.active;
      }
    }
  } catch (e) {}
  return { hookPresent: true, active, text, label };
})()`;

async function readState(): Promise<DisplayState> {
  if (!mainWindow || mainWindow.isDestroyed()) return EMPTY_STATE;
  try {
    return (await mainWindow.webContents.executeJavaScript(STATE_PROBE)) as DisplayState;
  } catch {
    return EMPTY_STATE;
  }
}

function pushState(state: DisplayState): void {
  if (!floatWin || floatWin.isDestroyed()) return;
  const payload = JSON.stringify(state);
  void floatWin.webContents
    .executeJavaScript(`window.__update && window.__update(${payload})`)
    .catch(() => {});
}

function startPoll(): void {
  if (poll) return;
  inactiveReads = 0;
  poll = setInterval(
    () =>
      void readState().then((state) => {
        // Hysterese: Während Navigation/Reload im Hauptfenster fehlt der Hook
        // kurz und die Probe liefert "inaktiv". Erst drei Inaktiv-Lesungen in
        // Folge schalten die Karte auf idle — sonst flackert sie mitten in
        // der Session auf "Kein Timer aktiv".
        if (state.active) {
          inactiveReads = 0;
        } else {
          inactiveReads++;
          if (inactiveReads < 3) return;
        }
        pushState(state);
      }),
    POLL_MS,
  );
}

function stopPoll(): void {
  if (poll) {
    clearInterval(poll);
    poll = null;
  }
}

function createFloatWindow(): BrowserWindow {
  // Oben rechts auf dem Bildschirm, auf dem der Nutzer gerade arbeitet.
  const display = screen.getDisplayNearestPoint(screen.getCursorScreenPoint());
  const { x, y, width } = display.workArea;

  const win = new BrowserWindow({
    x: x + width - WIDTH - MARGIN,
    y: y + MARGIN,
    width: WIDTH,
    height: HEIGHT,
    frame: false,
    transparent: true,
    hasShadow: false, // Schatten kommt aus dem CSS der Karte
    resizable: false,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    skipTaskbar: true,
    alwaysOnTop: true,
    show: false,
    webPreferences: {
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  // 'screen-saver' ist die Stufe, die auf macOS auch über Vollbild-Apps liegt.
  win.setAlwaysOnTop(true, 'screen-saver');
  if (process.platform === 'darwin') {
    win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
  }

  void win.loadFile(path.join(__dirname, '..', 'resources', 'floating-timer.html'));
  win.webContents.on('did-finish-load', () => void readState().then(pushState));
  // showInactive: sichtbar werden, ohne der Ziel-App den Fokus zu stehlen.
  win.once('ready-to-show', () => win.showInactive());
  win.on('closed', () => {
    floatWin = null;
    stopPoll();
  });
  return win;
}

/**
 * Öffnet den Schwebe-Timer.
 *
 * @param force true = manueller Aufruf (Menü): öffnet auch ohne aktiven
 *              Timer und zeigt die Idle-Card. Die Automatik ruft ohne force
 *              und öffnet nur, wenn das Frontend einen aktiven Timer meldet —
 *              sonst würde bei jedem App-Wechsel ein Fenster aufpoppen.
 */
export async function openFloatingTimer(force = false): Promise<void> {
  const state = await readState();
  if (!force && !state.active) {
    // Nur Dev: sichtbar machen, WARUM nichts aufgeht — fehlender Hook heißt
    // "Setting aus oder nicht eingeloggt", active=false heißt "kein Timer".
    if (IS_DEV) console.log(`${LOG} skip — hook=${state.hookPresent}, active=${state.active}`);
    return;
  }

  if (!floatWin || floatWin.isDestroyed()) {
    floatWin = createFloatWindow();
    if (IS_DEV) {
      console.log(
        `${LOG} open → Zweitfenster #${floatWin.id} (hook=${state.hookPresent}, active=${state.active}, force=${force})`,
      );
    }
  }
  pushState(state);
  startPoll();
}

export async function closeFloatingTimer(): Promise<void> {
  stopPoll();
  if (floatWin && !floatWin.isDestroyed()) {
    if (IS_DEV) console.log(`${LOG} close → Zweitfenster #${floatWin.id}`);
    floatWin.close();
  }
  floatWin = null;
}

/**
 * Meldet das Hauptfenster an und verdrahtet die Automatik.
 * MUSS nach dem Erzeugen des Hauptfensters aufgerufen werden.
 */
export function setupFloatingTimer(win: BrowserWindow): void {
  mainWindow = win;

  // Stufe-2-Automatik. 'blur' feuert auch, wenn der Nutzer das Schwebe-
  // Fenster selbst anklickt — openFloatingTimer ist idempotent, das ist ok.
  win.on('blur', () => void openFloatingTimer());
  win.on('minimize', () => void openFloatingTimer());
  win.on('hide', () => void openFloatingTimer());
  win.on('focus', () => void closeFloatingTimer());
  win.on('closed', () => {
    mainWindow = null;
    void closeFloatingTimer();
  });

  if (!IS_DEV) return;

  // Dev-Diagnose: Event-Protokoll für die Verifikation am Bildschirm.
  for (const event of ['blur', 'focus', 'minimize', 'restore', 'hide', 'show']) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (win as any).on(event, () => {
      const f = floatWin && !floatWin.isDestroyed() ? `#${floatWin.id} visible=${floatWin.isVisible()}` : 'zu';
      console.log(`${LOG} [event] ${event} — Schwebe-Fenster: ${f}`);
    });
  }
}
