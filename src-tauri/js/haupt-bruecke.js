// Brücke des Hauptfensters — läuft vor jedem Skript der Seite (Nachfolger von Electrons preload.ts).
// Stellt der Web-App `window.electronAPI` bereit. Der Name bleibt, obwohl die Hülle jetzt Tauri ist:
// die Web-App erkennt die Desktop-App genau daran (`electronAPI.isElectron`, vite-env.d.ts).
// `__APP_URL__` und `__PLATTFORM__` ersetzt die Shell beim Start.
(() => {
  if (window.top !== window) return;

  const invoke = (befehl, args) => window.__TAURI_INTERNALS__.invoke(befehl, args);

  // Adresse der Web-App für die Startseite (ui/offline.html).
  Object.defineProperty(window, '__PREPWELL_APP_URL', { value: '__APP_URL__' });

  // „Update fertig geladen": die Shell ruft das per eval, sobald ein Update bereitliegt.
  const updateHoerer = new Set();
  Object.defineProperty(window, '__prepwellUpdateBereit', {
    value: (version) => {
      for (const hoerer of updateHoerer) {
        try { hoerer(version); } catch (e) { /* ein Hörer darf die anderen nicht stoppen */ }
      }
    },
  });

  // Berechtigungen: Kamera und Mikrofon verweigert die Shell (haupt.rs → on_permission_request).
  // Standort-Anfragen erreichen diesen Handler unter macOS nicht — deshalb werden sie hier abgewiesen.
  try {
    const verweigert = (_ok, fehler) => {
      if (typeof fehler === 'function') fehler({ code: 1, message: 'Standort ist in der Desktop-App nicht verfügbar.', PERMISSION_DENIED: 1, POSITION_UNAVAILABLE: 2, TIMEOUT: 3 });
    };
    if (navigator.geolocation) {
      navigator.geolocation.getCurrentPosition = verweigert;
      navigator.geolocation.watchPosition = (ok, fehler) => { verweigert(ok, fehler); return 0; };
    }
  } catch (e) { /* ohne Geolocation gibt es nichts abzuweisen */ }

  // Fenster ziehen (nur macOS): Die Titelleiste liegt durchsichtig über der Web-App, das Fenster hat damit von sich
  // aus keinen Griff mehr. Electron kannte dafür `-webkit-app-region: drag`, WKWebView nicht. Deshalb gilt der
  // oberste Streifen als Titelleiste: Ziehen verschiebt das Fenster, Doppelklick vergrößert es — außer auf allem,
  // was selbst klickbar ist (Links, Knöpfe, Eingaben, Elemente mit Zeiger-Cursor).
  if ('__PLATTFORM__' === 'darwin') {
    const LEISTE = 32; // px ab Oberkante
    const KLICKBAR = 'a, button, input, textarea, select, option, label, summary, [role="button"], [role="link"], [role="tab"], [role="menuitem"], [contenteditable], [draggable="true"]';
    const ist_griff = (ereignis) => {
      if (ereignis.button !== 0 || ereignis.clientY > LEISTE) return false;
      const ziel = ereignis.target instanceof Element ? ereignis.target : null;
      if (!ziel) return true;
      if (ziel.closest(KLICKBAR)) return false;
      try { return getComputedStyle(ziel).cursor !== 'pointer'; } catch (e) { return true; }
    };
    window.addEventListener('mousedown', (ereignis) => {
      if (!ist_griff(ereignis)) return;
      // Der zweite Klick eines Doppelklicks vergrößert, statt zu ziehen.
      invoke(ereignis.detail === 2 ? 'fenster_zoomen' : 'fenster_ziehen').catch(() => undefined);
    }, true);
  }

  const api = {
    /** Immer true — daran erkennt die Web-App die Desktop-App. */
    isElectron: true,
    /** 'darwin' oder 'linux' — setzt die Shell beim Start ein. */
    platform: '__PLATTFORM__',
    /** Öffnet eine URL im Standardbrowser (Stripe, OAuth, Rechtsseiten). */
    openExternal: (url) => invoke('open_external', { url }),

    /**
     * Meldet an, dass die Web-App den Update-Hinweis selbst zeigt, und abonniert ihn.
     * Bleibt der Aufruf aus, zeigt die Shell einen nativen Dialog — ein fertiges Update bleibt nie unbemerkt liegen.
     * Rückgabewert ist die Abmeldefunktion.
     */
    onUpdateReady: (callback) => {
      updateHoerer.add(callback);
      // Liegt schon ein Update bereit, kommt die Version als Antwort zurück.
      invoke('update_ui_ready')
        .then((version) => { if (version && updateHoerer.has(callback)) callback(version); })
        .catch(() => undefined);
      return () => { updateHoerer.delete(callback); };
    },

    /** Liegt gerade ein Update bereit? Liefert die Version oder null. */
    getPendingUpdate: () => invoke('get_pending_update'),

    /** Installiert das bereitliegende Update und startet neu. Liefert false, wenn keines bereitliegt. */
    installUpdate: () => invoke('install_update'),
  };

  Object.defineProperty(window, 'electronAPI', { value: Object.freeze(api) });
})();
