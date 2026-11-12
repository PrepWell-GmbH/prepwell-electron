// Fenster vorn, wenn der Timer abläuft (D-46): Fokuszeit oder Pause vorbei, Pomodoro geschafft, Countdown abgelaufen.
// Es liegt über allen Fenstern (auch Vollbild) und bleibt stehen, bis man es wegklickt. Jeder Ablauf erscheint genau einmal (Zeitpunkt der Web-App).
// KEINE TÖNE (Lucas 2026-11-05, Frontend ADR-0138): kein Signalton, keine Wiedergabe — auch nicht abschaltbar eingebaut.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, PhysicalPosition, UserAttentionType, WebviewWindow};

use crate::{
  bruecke::{Ablauf, TimerStand},
  fenster, haupt, mac,
};

pub const LABEL: &str = "ablauf";
pub const BREITE: f64 = 360.0;
const HOEHE: f64 = 172.0;
const RAND: f64 = 24.0;

struct Zustand {
  /// Zeitpunkt des zuletzt gesehenen Ablaufs. Außen None = noch nichts gelesen
  /// (der erste Stand setzt nur die Ausgangslage — kein altes Fenster beim Start).
  gesehen: Option<Option<f64>>,
  aktuell: Option<Ablauf>,
}

static ZUSTAND: Mutex<Zustand> = Mutex::new(Zustand { gesehen: None, aktuell: None });

/// Der Ablauf, den das Fenster gerade zeigt (für das frisch geöffnete Fenster).
pub fn aktuell() -> Option<Ablauf> {
  ZUSTAND.lock().unwrap().aktuell.clone()
}

fn erzeuge(app: &AppHandle) -> tauri::Result<WebviewWindow> {
  // Inaktiv zeigen: das Fenster erscheint, ohne der App im Vordergrund den Fokus zu nehmen.
  let fenster = fenster::kleines_fenster(app, LABEL, "ablauf.html", BREITE, HOEHE, mac::EBENE_GANZ_OBEN, mac::zeige_inaktiv)?;
  // Oben mittig auf dem Bildschirm, auf dem der Nutzer gerade arbeitet.
  let zeiger = app.cursor_position().unwrap_or_default();
  let bildschirm = app.monitor_from_point(zeiger.x, zeiger.y).ok().flatten().or(app.primary_monitor().ok().flatten());
  if let Some(bildschirm) = bildschirm {
    let (flaeche, faktor) = (bildschirm.work_area(), bildschirm.scale_factor());
    let x = f64::from(flaeche.position.x) + f64::from(flaeche.size.width) / 2.0 - BREITE * faktor / 2.0;
    let y = f64::from(flaeche.position.y) + RAND * faktor;
    fenster.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))?;
  }
  Ok(fenster)
}

fn zeige(app: &AppHandle, ablauf: Ablauf) {
  ZUSTAND.lock().unwrap().aktuell = Some(ablauf.clone());
  match app.get_webview_window(LABEL) {
    Some(fenster) => {
      fenster::sende(&fenster, &ablauf);
      mac::zeige_inaktiv(&fenster);
    }
    // Beim ersten Ablauf: das neue Fenster holt ihn sich selbst, sobald seine Seite geladen ist (timer_fenster_bereit).
    None => {
      if let Err(fehler) = erzeuge(app) {
        eprintln!("[Ablauf] Fenster ließ sich nicht öffnen: {fehler}");
      }
    }
  }
  // Dock-Symbol hüpft einmal.
  if let Some(hauptfenster) = haupt::fenster(app) {
    let _ = hauptfenster.request_user_attention(Some(UserAttentionType::Informational));
  }
}

/// Neuer Timer-Stand von der Brücke: einen neuen Ablauf genau einmal zeigen.
pub fn auf_stand(app: &AppHandle, stand: &TimerStand) {
  let um = stand.ablauf.as_ref().map(|a| a.um);
  {
    let mut zustand = ZUSTAND.lock().unwrap();
    let Some(gesehen) = zustand.gesehen else {
      zustand.gesehen = Some(um);
      return;
    };
    // Kein Ablauf (Leerlauf, Seite lädt neu) ändert nichts — so erscheint ein alter Ablauf nach kurzem Flackern nicht noch einmal.
    if um.is_none() || um == gesehen {
      return;
    }
    zustand.gesehen = Some(um);
  }
  if let Some(ablauf) = stand.ablauf.clone() {
    zeige(app, ablauf);
  }
}
