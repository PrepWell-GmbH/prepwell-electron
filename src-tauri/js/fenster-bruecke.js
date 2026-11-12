// Brücke der kleinen Timer-Fenster (Menüleiste, Ablauf, Schwebe-Timer) — Nachfolger von Electrons fenster-preload.ts.
// Nur Stand empfangen und Befehle schicken; kein Zugriff auf die Web-App.
(() => {
  const invoke = (befehl, args) => window.__TAURI_INTERNALS__.invoke(befehl, args);

  let empfaenger = null;
  // Die Shell reicht neue Daten per eval herein (fenster.rs → sende).
  window.__prepwellDaten = (daten) => { if (empfaenger) empfaenger(daten); };

  window.prepwellTimer = {
    /** Stand bzw. Ablauf von der Shell; liefert die Abmeldefunktion. */
    aufDaten: (callback) => {
      empfaenger = callback;
      invoke('timer_fenster_bereit').then((daten) => { if (empfaenger === callback) callback(daten); }).catch(() => undefined);
      return () => { if (empfaenger === callback) empfaenger = null; };
    },
    /** Timer-Befehl an die Web-App: pause, weiter, beenden, start-pomodoro, start-countdown, start-stoppuhr, timer-oeffnen. */
    befehl: (befehl) => invoke('timer_befehl', { befehl }),
    /** Hauptfenster nach vorn holen. */
    appOeffnen: () => invoke('app_oeffnen'),
    /** Höhe des Inhalts (px) — das Fenster passt sich an. */
    hoehe: (px) => { invoke('timer_fenster_hoehe', { px }).catch(() => undefined); },
    /** Dieses Fenster schließen. */
    schliessen: () => { invoke('timer_fenster_schliessen').catch(() => undefined); },
  };

  // Fenster an der Karte ziehen: WebKit kennt `-webkit-app-region` nicht, deshalb startet die Shell das Ziehen.
  // Ziehbar ist, was `data-ziehen` trägt — Knöpfe darin ausgenommen.
  window.addEventListener('mousedown', (ereignis) => {
    if (ereignis.button !== 0 || !(ereignis.target instanceof Element)) return;
    if (ereignis.target.closest('button')) return;
    if (!ereignis.target.closest('[data-ziehen]')) return;
    invoke('fenster_ziehen').catch(() => undefined);
  });
})();
