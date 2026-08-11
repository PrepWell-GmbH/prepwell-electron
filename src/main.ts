import { app, BrowserWindow, shell, Menu, session, ipcMain } from 'electron';
import { autoUpdater } from 'electron-updater';
import windowStateKeeper from 'electron-window-state';
import path from 'path';
import { setupFloatingTimer, openFloatingTimer, closeFloatingTimer } from './floating-timer';

// ─── Config ─────────────────────────────────────────────
const IS_DEV = !app.isPackaged;
const PROD_URL = 'https://app.prepwell.de';
const APP_URL = IS_DEV ? 'http://localhost:3000' : PROD_URL;

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

  // Schwebe-Timer (ADR-0070). Muss NACH dem Hauptfenster laufen — siehe Kommentar
  // in setupFloatingTimer(). Stufe Spike: manueller Trigger + Diagnose, keine Automatik.
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
        { type: 'separator' },
        // Schwebe-Timer (ADR-0070) — Stufe Spike: manueller Trigger.
        // In Stufe 2 übernimmt die Blur/Focus-Automatik; die Menüeinträge
        // bleiben als manueller Weg bestehen.
        {
          label: 'Schwebe-Timer öffnen',
          accelerator: 'CommandOrControl+Shift+F',
          click: () => void openFloatingTimer(),
        },
        {
          label: 'Schwebe-Timer schließen',
          accelerator: 'CommandOrControl+Shift+G',
          click: () => void closeFloatingTimer(),
        },
        ...(IS_DEV
          ? [
              {
                // Umgeht den Frontend-Hook und öffnet ein eigenes PiP-Fenster.
                // Damit ist das Schwebe-Verhalten prüfbar, ohne dass das
                // Frontend auf :3000 laufen muss.
                label: 'Schwebe-Timer: Roh-Test (ohne Frontend-Hook)',
                click: () => void openFloatingTimer(true),
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
function setupAutoUpdater(): void {
  if (IS_DEV) return; // No auto-update in dev

  autoUpdater.autoDownload = true;
  autoUpdater.autoInstallOnAppQuit = true;

  autoUpdater.on('update-available', () => {
    console.log('[Updater] Update available — downloading...');
  });

  autoUpdater.on('update-downloaded', () => {
    console.log('[Updater] Update downloaded — will install on quit.');
  });

  autoUpdater.on('error', (err) => {
    console.error('[Updater] Error:', err.message);
  });

  // Check for updates after launch
  autoUpdater.checkForUpdatesAndNotify();
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
