// Gemeinsames der kleinen Timer-Fenster (Menüleiste D-47, Ablauf D-46, Schwebe-Timer ADR-0070):
// Fenster-Grundform und die Befehle, mit denen die Fenster zurück zur Web-App sprechen.

use serde::Serialize;
use serde_json::Value;
use tauri::{webview::PageLoadEvent, AppHandle, LogicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::{ablauf, bruecke, haupt, mac, menueleiste, schwebe};

/// Läuft in jedem kleinen Fenster vor dem Skript der Seite und stellt `window.prepwellTimer` bereit.
const FENSTER_BRUECKE: &str = include_str!("../js/fenster-bruecke.js");

/// Erzeugt ein kleines, rahmenloses, durchsichtiges Fenster — zunächst unsichtbar.
/// Diese Fenster werden nie zerstört, nur aus- und eingeblendet (Grund in mac.rs).
/// `nach_laden` läuft, sobald die Seite geladen ist (Electrons ready-to-show).
pub fn kleines_fenster(
  app: &AppHandle,
  label: &str,
  datei: &str,
  breite: f64,
  hoehe: f64,
  ebene: isize,
  nach_laden: impl Fn(&WebviewWindow) + Send + Sync + 'static,
) -> tauri::Result<WebviewWindow> {
  let fenster = WebviewWindowBuilder::new(app, label, WebviewUrl::App(datei.into()))
    .title("PrepWell Timer")
    .inner_size(breite, hoehe)
    .decorations(false)
    .transparent(true)
    .shadow(false) // Schatten kommt aus dem CSS der Karte
    .resizable(false)
    .minimizable(false)
    .maximizable(false)
    .skip_taskbar(true)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .visible(false)
    .focused(false)
    .accept_first_mouse(true) // der erste Klick trifft den Knopf, statt nur das Fenster zu wecken
    .initialization_script(FENSTER_BRUECKE)
    .on_page_load(move |fenster, ereignis| {
      if ereignis.event() == PageLoadEvent::Finished {
        nach_laden(&fenster);
      }
    })
    .build()?;
  mac::als_panel(&fenster, ebene);
  Ok(fenster)
}

/// Schickt Daten an ein Fenster (kommt in der Seite bei `prepwellTimer.aufDaten` an).
pub fn sende(fenster: &WebviewWindow, daten: &impl Serialize) {
  if let Ok(json) = serde_json::to_string(daten) {
    let _ = fenster.eval(format!("window.__prepwellDaten && window.__prepwellDaten({json})"));
  }
}

// ─── Befehle der kleinen Fenster ────────────────────────

/// Das Fenster ist bereit und holt, was es sofort anzeigen soll.
#[tauri::command]
pub fn timer_fenster_bereit(window: WebviewWindow) -> Value {
  match window.label() {
    menueleiste::LABEL => serde_json::to_value(menueleiste::stand()).unwrap_or(Value::Null),
    // Linux: die Schwebe-Karte ist dieselbe Seite wie das Menüleisten-Fenster.
    schwebe::LABEL if schwebe::WIE_MENUELEISTE => serde_json::to_value(menueleiste::stand()).unwrap_or(Value::Null),
    ablauf::LABEL => serde_json::to_value(ablauf::aktuell()).unwrap_or(Value::Null),
    _ => Value::Null,
  }
}

/// Timer-Befehl an die Web-App. Meldet sie 'app', kommt das Hauptfenster nach vorn.
#[tauri::command]
pub async fn timer_befehl(app: AppHandle, befehl: Value) -> &'static str {
  let Some(befehl) = befehl.as_str() else {
    return "ungueltig";
  };
  // Hauptfenster geschlossen (am Mac lebt die App weiter): es gibt keine Web-App, die den Befehl ausführen könnte.
  // Statt eines Knopfs ohne Wirkung öffnet sich PrepWell.
  if haupt::fenster(&app).is_none() {
    haupt::hole_app_nach_vorn(&app);
    return "app";
  }
  let ergebnis = bruecke::sende_befehl(&app, befehl).await;
  if ergebnis == "app" {
    haupt::hole_app_nach_vorn(&app);
  }
  bruecke::jetzt_lesen(&app).await;
  ergebnis
}

/// Hauptfenster nach vorn holen („PrepWell öffnen").
#[tauri::command]
pub fn app_oeffnen(app: AppHandle) {
  haupt::hole_app_nach_vorn(&app);
}

/// Die Seite meldet die Höhe ihres Inhalts — das Fenster passt sich an.
#[tauri::command]
pub fn timer_fenster_hoehe(window: WebviewWindow, px: f64) {
  let breite = match window.label() {
    menueleiste::LABEL => menueleiste::BREITE,
    schwebe::LABEL if schwebe::WIE_MENUELEISTE => schwebe::BREITE,
    ablauf::LABEL => ablauf::BREITE,
    _ => return,
  };
  if !px.is_finite() {
    return;
  }
  let _ = window.set_size(LogicalSize::new(breite, px.ceil().clamp(80.0, 400.0)));
}

/// Das Fenster schließt sich selbst.
#[tauri::command]
pub fn timer_fenster_schliessen(app: AppHandle, window: WebviewWindow) {
  match window.label() {
    menueleiste::LABEL => menueleiste::verstecken(&app),
    schwebe::LABEL => schwebe::schliessen(&app),
    _ => {
      let _ = window.hide();
    }
  }
}

/// Doppelklick auf die Titelleiste des Hauptfensters: vergrößern bzw. zurück (haupt-bruecke.js).
#[tauri::command]
pub fn fenster_zoomen(window: WebviewWindow) {
  if window.label() != haupt::LABEL {
    return;
  }
  let _ = if window.is_maximized().unwrap_or(false) { window.unmaximize() } else { window.maximize() };
}

/// Das Fenster wird mit der Maus gezogen (Ersatz für `-webkit-app-region: drag`): die Karten der kleinen Fenster
/// und der Titelleisten-Streifen des Hauptfensters.
#[tauri::command]
pub fn fenster_ziehen(window: WebviewWindow) {
  let _ = window.start_dragging();
}
