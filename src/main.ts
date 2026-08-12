import { app, BrowserWindow, shell, Menu, session, ipcMain, dialog } from 'electron';
import { autoUpdater } from 'electron-updater';
import windowStateKeeper from 'electron-window-state';
import path from 'path';
import { setupFloatingTimer, openFloatingTimer, closeFloatingTimer } from './floating-timer';

// ─── Config ─────────────────────────────────────────────
const IS_DEV = !app.isPackaged;
const PROD_URL = 'https://app.prepwell.de';
// Dev-Override, damit der Schwebe-Timer-Spike gegen den bereits in prod
// ausgerollten Hook geprüft werden kann, ohne das Frontend auf :3000 zu starten:
//   PREPWELL_URL=https://app.prepwell.de npm run dev
// Bewusst nur im Dev-Modus wirksam — ein paketierter Build lässt sich damit
// nicht umlenken, sonst wäre der Navigations-Guard über eine Umgebungsvariable
// aushebelbar.
const APP_URL = IS_DEV ? process.env.PREPWELL_URL || 'http://localhost:3000' : PROD_URL;

// Dev-Lauf bekommt einen eigenen Datenordner. Sonst teilt er sich userData —
// und damit die Single-Instance-Sperre — mit der installierten App: je nachdem,
// wer zuerst lief, beendet sich die jeweils andere beim Start WORTLOS. Der
// Ordner wird einmalig aus dem echten geklont, damit der Login erhalten bleibt.
if (IS_DEV) {
  app.setPath('userData', `${app.getPath('userData')}-dev`);
}

// ─── Single Instance Lock ───────────────────────────────
// Prevent multiple instances — focus existing window instead
const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
}

// ─── Window ─────────────────────────────────────────────
let mainWindow: BrowserWindow | null = null;

function createWindow(): void {
  // Restore previous window size/position
  const windowState = windowStateKeeper({
    defaultWidth: 1200,
    defaultHeight: 800,
  });

  mainWindow = new BrowserWindow({
    x: windowState.x,
    y: windowState.y,
    width: windowState.width,
    height: windowState.height,
    minWidth: 800,
    minHeight: 600,
    titleBarStyle: process.platform === 'darwin' ? 'hiddenInset' : 'default',
    trafficLightPosition: { x: 16, y: 16 },
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      // Security: disable node integration in renderer
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
    },
    show: false, // Show when ready to avoid flash
  });

  // Track window state (size, position)
  windowState.manage(mainWindow);

  // Schwebe-Timer (ADR-0070, Stufe 2 auf dem Fallback-Pfad: natives
  // Zweitfenster statt Document PiP). Verdrahtet die Blur/Focus-Automatik —
  // muss NACH dem Erzeugen des Hauptfensters laufen.
  setupFloatingTimer(mainWindow);

  // Show window when content is ready (no white flash)
  mainWindow.once('ready-to-show', () => {
    mainWindow?.show();
    if (IS_DEV) {
      mainWindow?.webContents.openDevTools({ mode: 'detach' });
    }
  });

  // Load the web app
  mainWindow.loadURL(APP_URL).catch(() => {
    // Offline: show error page
    mainWindow?.loadFile(path.join(__dirname, '..', 'resources', 'offline.html'));
  });

  // External links → system browser (not in Electron window)
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    // Allow same-origin navigation (app.prepwell.de)
    if (url.startsWith(APP_URL)) {
      return { action: 'allow' };
    }
    // Hinweis: Document PiP (Schwebe-Timer, ADR-0070) läuft NICHT durch diesen
    // Handler — im Spike wurde er bei requestWindow() kein einziges Mal
    // aufgerufen. Es braucht hier also keine about:blank-Ausnahme.
    // Everything else → system browser
    shell.openExternal(url);
    return { action: 'deny' };
  });

  // Navigation guard: keep window on app domain
  mainWindow.webContents.on('will-navigate', (event, url) => {
    if (!url.startsWith(APP_URL)) {
      event.preventDefault();
      shell.openExternal(url);
    }
  });

  // DevTools: F12 or Ctrl+Shift+I to open manually in dev
  // (not auto-opened to avoid second window)

  mainWindow.on('closed', () => {
    mainWindow = null;
  });
}

// ─── App Menu ───────────────────────────────────────────
function createMenu(): void {
  const isMac = process.platform === 'darwin';

  const template: Electron.MenuItemConstructorOptions[] = [
    // macOS app menu
    ...(isMac
      ? [
          {
            label: app.name,
            submenu: [
              { role: 'about' as const },
              { type: 'separator' as const },
              { role: 'hide' as const },
              { role: 'hideOthers' as const },
              { role: 'unhide' as const },
              { type: 'separator' as const },
              { role: 'quit' as const },
            ],
          },
        ]
      : []),
    // Edit menu (copy/paste etc.)
    {
      label: 'Bearbeiten',
      submenu: [
        { role: 'undo', label: 'Rückgängig' },
        { role: 'redo', label: 'Wiederholen' },
        { type: 'separator' },
        { role: 'cut', label: 'Ausschneiden' },
        { role: 'copy', label: 'Kopieren' },
        { role: 'paste', label: 'Einfügen' },
        { role: 'selectAll', label: 'Alles auswählen' },
      ],
    },
    // View menu
    {
      label: 'Ansicht',
      submenu: [
        { role: 'reload', label: 'Neu laden' },
        { role: 'forceReload', label: 'Erzwingen Neu laden' },
        { type: 'separator' },
        { role: 'resetZoom', label: 'Zoom zurücksetzen' },
        { role: 'zoomIn', label: 'Vergrößern' },
        { role: 'zoomOut', label: 'Verkleinern' },
        { type: 'separator' },
        { role: 'togglefullscreen', label: 'Vollbild' },
        // Schwebe-Timer (ADR-0070) — Stufe Spike: nur im Dev-Lauf. In die
        // ausgelieferte App kommen die Einträge erst mit Stufe 2, wenn das
        // macOS-Verhalten bestätigt ist (shell#5) — ein Menüeintrag, der
        // vielleicht nichts tut, ist schlechter als keiner.
        ...(IS_DEV
          ? [
              { type: 'separator' as const },
              {
                // force=true: manueller Aufruf öffnet auch ohne aktiven
                // Timer (Idle-Card) — so ist das Schweben ohne Login prüfbar.
                label: 'Schwebe-Timer öffnen',
                accelerator: 'CommandOrControl+Shift+F',
                click: () => void openFloatingTimer(true),
              },
              {
                label: 'Schwebe-Timer schließen',
                accelerator: 'CommandOrControl+Shift+G',
                click: () => void closeFloatingTimer(),
              },
              { type: 'separator' as const },
              { role: 'toggleDevTools' as const, label: 'Entwicklertools' },
            ]
          : []),
      ],
    },
    // Window menu
    {
      label: 'Fenster',
      submenu: [
        { role: 'minimize', label: 'Minimieren' },
        { role: 'close', label: 'Schließen' },
        ...(isMac
          ? [
              { type: 'separator' as const },
              { role: 'front' as const, label: 'Alle nach vorne bringen' },
            ]
          : []),
      ],
    },
  ];

  Menu.setApplicationMenu(Menu.buildFromTemplate(template));
}

// ─── Auto Updater ───────────────────────────────────────
// Ein fertig geladenes Update wird dem Nutzer ANGEBOTEN, statt still beim
// Beenden eingespielt zu werden. Der Hinweis erscheint bewusst erst nach dem
// Download ('update-downloaded', nicht 'update-available') — sonst wartet der
// Nutzer nach dem Klick auf ~200 MB DMG, statt sofort im neuen Stand zu landen.
//
// Zwei Wege für die Anzeige:
//   1. Das Frontend rendert den Hinweis selbst (meldet sich per 'update-ui-ready').
//   2. Meldet sich niemand, zeigt die Shell einen nativen Dialog.
// So funktioniert das Update-Angebot auch gegen ein Frontend, das die
// Update-UI noch nicht kennt, und auf der Offline-Seite.

/** Version des heruntergeladenen Updates, sonst null. */
let pendingUpdateVersion: string | null = null;
/** Wird true, sobald das Frontend die Update-UI übernommen hat. */
let rendererOwnsUpdateUi = false;

const UPDATE_CHECK_INTERVAL_MS = 60 * 60 * 1000; // stündlich nachfassen
/** Karenz, damit ein noch ladendes Frontend die UI übernehmen kann. */
const RENDERER_GRACE_MS = 10_000;

function setupAutoUpdater(): void {
  if (IS_DEV) return; // No auto-update in dev

  autoUpdater.autoDownload = true;
  // Fallback für alle, die den Hinweis wegklicken: dann eben beim Beenden.
  autoUpdater.autoInstallOnAppQuit = true;

  autoUpdater.on('update-available', (info) => {
    console.log(`[Updater] Version ${info.version} verfügbar — lädt im Hintergrund...`);
  });

  autoUpdater.on('update-downloaded', (info) => {
    pendingUpdateVersion = info.version;
    console.log(`[Updater] Version ${info.version} bereit.`);
    announceUpdate(info.version);
  });

  autoUpdater.on('error', (err) => {
    console.error('[Updater] Error:', err.message);
  });

  // checkForUpdates() statt checkForUpdatesAndNotify(): letzteres würde
  // zusätzlich eine System-Benachrichtigung zeigen, und die Anzeige gehört ab
  // jetzt uns.
  void autoUpdater.checkForUpdates();
  setInterval(() => void autoUpdater.checkForUpdates(), UPDATE_CHECK_INTERVAL_MS);
}

/** Meldet ein bereitliegendes Update ans Frontend — oder zeigt den nativen Dialog. */
function announceUpdate(version: string): void {
  if (sendToRenderer(version)) return;

  // Das Update ist erst nach einem langen Download fertig, das Frontend also
  // längst geladen. Trotzdem kurz warten, falls gerade neu geladen wird.
  setTimeout(() => {
    if (rendererOwnsUpdateUi && sendToRenderer(version)) return;
    void showNativeUpdatePrompt(version);
  }, RENDERER_GRACE_MS);
}

function sendToRenderer(version: string): boolean {
  if (!rendererOwnsUpdateUi || !mainWindow || mainWindow.isDestroyed()) return false;
  mainWindow.webContents.send('update-downloaded', version);
  return true;
}

async function showNativeUpdatePrompt(version: string): Promise<void> {
  if (!mainWindow || mainWindow.isDestroyed()) return;
  const { response } = await dialog.showMessageBox(mainWindow, {
    type: 'info',
    buttons: ['Jetzt aktualisieren', 'Später'],
    defaultId: 0,
    cancelId: 1,
    message: 'Eine neuere Version ist vorhanden',
    detail: `Version ${version} steht bereit. PrepWell startet dafür kurz neu.`,
  });
  if (response === 0) installUpdate();
}

/** Beendet die App, installiert das Update und startet neu. */
function installUpdate(): void {
  // isSilent=false → Installer-Fortschritt sichtbar; isForceRunAfter=true →
  // die App kommt danach von selbst wieder hoch.
  autoUpdater.quitAndInstall(false, true);
}

// ─── IPC Handlers ──────────────────────────────────────
// open-external: opens a URL in the system browser (Stripe, OAuth, etc.)
// Whitelist: only HTTPS URLs are allowed to prevent abuse.
ipcMain.handle('open-external', async (_event, url: string) => {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== 'https:') return;
    await shell.openExternal(url);
  } catch {
    // Invalid URL — silently ignore
  }
});

// update-ui-ready: Das Frontend übernimmt die Anzeige — ab jetzt kein nativer
// Dialog mehr. Liegt schon ein Update bereit (z.B. weil es während eines
// Reloads fertig wurde), wird es sofort nachgereicht.
ipcMain.on('update-ui-ready', (event) => {
  rendererOwnsUpdateUi = true;
  if (pendingUpdateVersion) event.sender.send('update-downloaded', pendingUpdateVersion);
});

// get-pending-update: Für den Fall, dass die Komponente erst nach dem Event
// mountet — dann fragt sie den Stand aktiv ab.
ipcMain.handle('get-pending-update', () => pendingUpdateVersion);

// install-update: Klick auf „Jetzt aktualisieren". Nur gültig, wenn wirklich
// ein Update bereitliegt — sonst würde quitAndInstall() die App nur beenden.
ipcMain.handle('install-update', () => {
  if (!pendingUpdateVersion) return false;
  installUpdate();
  return true;
});

// ─── App Lifecycle ──────────────────────────────────────
app.on('ready', () => {
  // Deny all permission requests by default (camera, microphone, geolocation, etc.)
  session.defaultSession.setPermissionRequestHandler((_webContents, permission, callback) => {
    // Log statt still schlucken: falls Document PiP (ADR-0070) doch eine
    // Permission anfordert, ist der Denial sonst unsichtbar und der Spike
    // scheitert ohne Spur.
    console.log(`[Permissions] verweigert: ${permission}`);
    callback(false);
  });

  // CSP: restrict what the renderer can load
  session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
    callback({
      responseHeaders: {
        ...details.responseHeaders,
        'Content-Security-Policy': [
          [
            "default-src 'self' https://app.prepwell.de https://*.supabase.co",
            "script-src 'self' 'unsafe-inline'",                              // Vite injects inline scripts
            "style-src 'self' 'unsafe-inline' https://fonts.googleapis.com",  // Tailwind + Google Fonts
            "font-src 'self' https://fonts.gstatic.com",
            "img-src 'self' data: blob: https:",
            "connect-src 'self' https://app.prepwell.de https://*.supabase.co wss://*.supabase.co",
          ].join('; '),
        ],
      },
    });
  });

  createMenu();
  createWindow();
  setupAutoUpdater();
});

// macOS: re-create window when dock icon is clicked
app.on('activate', () => {
  if (mainWindow === null) {
    createWindow();
  }
});

// Quit when all windows are closed (except macOS)
app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

// Single instance: focus existing window
app.on('second-instance', () => {
  if (mainWindow) {
    if (mainWindow.isMinimized()) mainWindow.restore();
    mainWindow.focus();
  }
});
