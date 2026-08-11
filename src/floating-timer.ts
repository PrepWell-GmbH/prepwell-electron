// Schwebe-Timer — Shell-Seite von ADR-0070 (Issue prepwell-electron#1).
//
// STUFE: SPIKE. Hier hängt bewusst NUR der manuelle Trigger + Diagnose drin.
// Die Automatik (Blur/Minimize öffnet, Focus/Restore schließt, showFloatingTimer,
// Idempotenz) kommt erst, wenn am Bildschirm bestätigt ist, dass
//   a) documentPictureInPicture.requestWindow() aus dem Shell-Trigger heraus
//      überhaupt aufgeht (synthetische User-Gesture via executeJavaScript),
//   b) das PiP-Fenster über ANDEREN Anwendungen schwebt,
//   c) es das Minimieren des Hauptfensters überlebt.
// Scheitert (b) oder (c), sagt ADR-0070: Umplanung auf natives Zweitfenster —
// dann wäre jede vorab gebaute Automatik-Semantik Wegwerfcode.
//
// Die Fokus-/Minimize-Events werden trotzdem schon registriert, aber
// AUSSCHLIESSLICH als Logger. Damit ist vor dem Automatik-Bau belegt, ob das
// Event-Substrat auf der jeweiligen Plattform überhaupt trägt (auf Wayland ist
// 'minimize' erfahrungsgemäß die wacklige Stelle).
//
// ── Messergebnisse Spike-Lauf, Electron 41.2.0, Fedora (X11 + Wayland) ──
//  1. Die synthetische User-Gesture TRÄGT: executeJavaScript(code, true) lässt
//     requestWindow() aufgehen, kein NotAllowedError. Das ist der Mechanismus,
//     auf dem ADR-0070 steht — er ist damit belegt, nicht mehr nur angenommen.
//  2. Auf Linux stirbt das PiP-Fenster sofort: requestWindow() löst nach ~20ms
//     auf, ~1ms später feuert das Fenster pagehide + unload. Danach ist
//     documentPictureInPicture.window dauerhaft null. Identisch unter XWayland
//     und --ozone-platform=wayland. Video-PiP (video.requestPictureInPicture)
//     bleibt auf derselben Maschine offen — es ist also nicht die PiP-
//     Infrastruktur allgemein, sondern Document PiP im Speziellen.
//     → Die ACs "schwebt über anderen Apps" und "überlebt Minimieren" sind auf
//       Linux nicht prüfbar. Sie müssen auf macOS verifiziert werden (was zum
//       Mac-only-Entscheid des ADRs passt).
//  3. Das PiP-Fenster ist KEIN BrowserWindow: 'browser-window-created' feuert
//     nicht, BrowserWindow.fromWebContents() liefert null, getAllWindows()
//     zählt es nicht mit. Der Main-Prozess hat also keinen Griff daran und kann
//     alwaysOnTop/Fensterlevel NICHT selbst erzwingen — das Schweben muss
//     vollständig von Chromium kommen. Falls es auf macOS nicht von allein
//     oben liegt, greift der ADR-Fallback (natives Zweitfenster).
//  4. Weder setWindowOpenHandler noch der Permission-Handler werden von PiP
//     angefasst — die restriktiven Guards in main.ts stehen nicht im Weg.

import { app, BrowserWindow } from 'electron';

const LOG = '[FloatingTimer]';

/** Was der Renderer-Probe zurückmeldet. Landet als JSON im Main-Prozess-Log. */
interface ProbeResult {
  /** Welcher Weg genommen wurde: der ADR-Hook, der Roh-Fallback, oder keiner. */
  path: 'hook' | 'raw' | 'none';
  hookPresent: boolean;
  pipApiPresent: boolean;
  /** null = Hook fehlt oder isTimerActive() hat geworfen. */
  timerActive: boolean | null;
  opened: boolean;
  error: string | null;
}

let mainWindow: BrowserWindow | null = null;

/**
 * Probe, die im Main-World des Renderers läuft.
 *
 * Bevorzugt den ADR-0070-Hook (window.__prepwellFloatingTimer). Wenn der fehlt
 * — z.B. weil die Shell gegen ein Frontend ohne das Feature läuft — öffnet der
 * Roh-Fallback ein eigenes PiP-Fenster mit sichtbarer Uhr. So ist das
 * Schwebe-Verhalten prüfbar, ohne dass das Frontend auf :3000 laufen muss.
 */
function probeScript(allowRawFallback: boolean): string {
  return `(async () => {
  const out = {
    path: 'none',
    hookPresent: !!window.__prepwellFloatingTimer,
    pipApiPresent: !!window.documentPictureInPicture,
    timerActive: null,
    opened: false,
    error: null,
  };
  const isOpen = () => !!(window.documentPictureInPicture && window.documentPictureInPicture.window);
  // requestWindow() ist async; nach open() kurz nachfassen statt sofort zu urteilen.
  const settle = async () => {
    for (let i = 0; i < 20; i++) {
      if (isOpen()) return true;
      await new Promise((r) => setTimeout(r, 50));
    }
    return isOpen();
  };
  try {
    const hook = window.__prepwellFloatingTimer;
    if (hook && typeof hook.open === 'function') {
      out.path = 'hook';
      try { out.timerActive = hook.isTimerActive(); } catch (e) { out.timerActive = null; }
      await hook.open();
      out.opened = await settle();
      return out;
    }
    if (${allowRawFallback ? 'true' : 'false'} && window.documentPictureInPicture) {
      out.path = 'raw';
      const pip = await window.documentPictureInPicture.requestWindow({ width: 320, height: 180 });
      const d = pip.document;
      d.body.style.cssText = 'margin:0;height:100vh;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:6px;background:#0f172a;color:#f8fafc;font-family:system-ui,sans-serif';
      const label = d.createElement('div');
      label.textContent = 'PrepWell Spike';
      label.style.cssText = 'font-size:12px;letter-spacing:.12em;text-transform:uppercase;opacity:.6';
      const clock = d.createElement('div');
      clock.style.cssText = 'font-size:44px;font-weight:600;font-variant-numeric:tabular-nums';
      d.body.append(label, clock);
      // Laufende Uhr: belegt beim Hinsehen, dass das Fenster lebt und nicht
      // nur ein eingefrorener Screenshot ist, wenn das Hauptfenster weg ist.
      const started = performance.now();
      const tick = () => {
        const s = Math.floor((performance.now() - started) / 1000);
        clock.textContent = String(Math.floor(s / 60)).padStart(2, '0') + ':' + String(s % 60).padStart(2, '0');
      };
      tick();
      const iv = pip.setInterval(tick, 250);
      pip.addEventListener('pagehide', () => pip.clearInterval(iv));
      out.opened = true;
      return out;
    }
    out.error = out.pipApiPresent ? 'Hook fehlt (Frontend ohne Feature?)' : 'documentPictureInPicture nicht verfuegbar';
    return out;
  } catch (e) {
    out.error = (e && e.name ? e.name + ': ' : '') + (e && e.message ? e.message : String(e));
    return out;
  }
})()`;
}

const CLOSE_SCRIPT = `(() => {
  try {
    const hook = window.__prepwellFloatingTimer;
    if (hook && typeof hook.close === 'function') { hook.close(); return 'hook'; }
    if (window.documentPictureInPicture && window.documentPictureInPicture.window) {
      window.documentPictureInPicture.window.close();
      return 'raw';
    }
    return 'none';
  } catch (e) { return 'error: ' + (e && e.message ? e.message : String(e)); }
})()`;

/**
 * Öffnet den Schwebe-Timer.
 *
 * Der zweite Parameter von executeJavaScript ist der springende Punkt des
 * ganzen ADRs: er stellt die synthetische User-Gesture bereit, ohne die
 * Chromium requestWindow() mit NotAllowedError abweist.
 */
export async function openFloatingTimer(allowRawFallback = false): Promise<void> {
  if (!mainWindow || mainWindow.isDestroyed()) return;
  try {
    const result: ProbeResult = await mainWindow.webContents.executeJavaScript(
      probeScript(allowRawFallback),
      true, // userGesture
    );
    console.log(`${LOG} open →`, JSON.stringify(result));
  } catch (err) {
    console.error(`${LOG} open → executeJavaScript hat geworfen:`, err);
  }
}

export async function closeFloatingTimer(): Promise<void> {
  if (!mainWindow || mainWindow.isDestroyed()) return;
  try {
    const via = await mainWindow.webContents.executeJavaScript(CLOSE_SCRIPT, true);
    console.log(`${LOG} close → ${via}`);
  } catch (err) {
    console.error(`${LOG} close → executeJavaScript hat geworfen:`, err);
  }
}

/** Zählt die Fenster, die nach dem Hauptfenster aufgemacht wurden (PiP-Kandidaten). */
function describeExtraWindows(): string {
  const extras = BrowserWindow.getAllWindows().filter((w) => w.id !== mainWindow?.id);
  if (extras.length === 0) return 'keine Zusatzfenster';
  return extras
    .map((w) => `#${w.id} visible=${w.isVisible()} alwaysOnTop=${w.isAlwaysOnTop()}`)
    .join(', ');
}

/**
 * Registriert Diagnose-Listener und meldet das Hauptfenster an.
 *
 * MUSS nach dem Erzeugen des Hauptfensters aufgerufen werden: der
 * 'browser-window-created'-Listener wird erst hier gesetzt, damit er das
 * Hauptfenster selbst gar nicht erst zu sehen bekommt und jedes gemeldete
 * Fenster ein echter PiP-Kandidat ist.
 */
export function setupFloatingTimer(win: BrowserWindow): void {
  mainWindow = win;

  // Auf Linux feuert das für PiP NICHT (siehe Befund 3 oben). Bleibt drin, weil
  // die macOS-Verifikation noch aussteht: feuert es dort, wollen wir es sehen
  // und das Fenster gleich nach oben zwingen.
  app.on('browser-window-created', (_event, created) => {
    if (created.id === mainWindow?.id) return;
    console.log(
      `${LOG} Fenster #${created.id} erzeugt — alwaysOnTop vor Eingriff: ${created.isAlwaysOnTop()}`,
    );
    // 'screen-saver' ist die Stufe, die auf macOS auch über Vollbild-Apps liegt.
    created.setAlwaysOnTop(true, 'screen-saver');
    if (process.platform === 'darwin') {
      created.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
    }
    created.on('closed', () => console.log(`${LOG} Fenster #${created.id} geschlossen`));
  });

  // Falls PiP KEIN BrowserWindow erzeugt, taucht es hier trotzdem auf. Der
  // getType()-Wert sagt uns, womit wir es in Stufe 2 zu tun haben.
  app.on('web-contents-created', (_event, contents) => {
    if (contents === mainWindow?.webContents) return;
    const attached = BrowserWindow.fromWebContents(contents);
    console.log(
      `${LOG} webContents erzeugt — type=${contents.getType()} url=${contents.getURL() || '(leer)'} ` +
        `browserWindow=${attached ? '#' + attached.id : 'keins'}`,
    );
  });

  // NUR Logging. Hier hängt in Stufe 2 die Automatik dran — vorher wollen wir
  // wissen, ob die Events auf der Zielplattform sauber und einzeln feuern.
  const logEvent = (event: string) => () => {
    console.log(`${LOG} [event] ${event} — ${describeExtraWindows()}`);
  };

  win.on('blur', logEvent('blur'));
  win.on('focus', logEvent('focus'));
  win.on('minimize', logEvent('minimize'));
  win.on('restore', logEvent('restore'));
  win.on('hide', logEvent('hide'));
  win.on('show', logEvent('show'));
}
