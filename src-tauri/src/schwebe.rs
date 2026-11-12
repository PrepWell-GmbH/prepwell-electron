// Schwebe-Timer (ADR-0070): kleines, rahmenloses Fenster oben rechts, das über allem liegt, solange ein Timer läuft und PrepWell nicht vorn ist.
//
// Automatik: Fokus-Verlust des Hauptfensters öffnet (nur wenn ein Timer aktiv ist), Fokus-Rückkehr schließt.
// Minimieren und Verstecken zählen als Fokus-Verlust. Das Schwebe-Fenster wird inaktiv gezeigt und stiehlt der App,
// zu der der Nutzer wechselt, nicht den Fokus — und weil nur der Fokus des HAUPTfensters schließt, gibt es keine
// Rückkopplung, wenn der Nutzer das Schwebe-Fenster selbst anfasst.
//
// Der Frontend-Hook (window.__prepwellFloatingTimer) liefert nur den Timer-Zustand; gelesen wird er in bruecke.rs.
//
// Das Fenster wird beim ersten Öffnen erzeugt und danach nur noch aus- und eingeblendet (siehe mac.rs).

use std::sync::Mutex;

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow};

use crate::{
  bruecke::{self, SchwebeStand, TimerStand},
  fenster, haupt, mac, menueleiste, IS_DEV,
};

pub const LABEL: &str = "schwebe";
const LOG: &str = "[Schwebe-Timer]";

/// Linux hat kein Symbol in der Menüleiste (siehe menueleiste.rs). Dort zeigt die Schwebe-Karte deshalb
/// dieselbe Karte wie das Menüleisten-Fenster am Mac (D-47): Modus, Restzeit, Fortschritt, Pause/Beenden, „Anpassen".
pub const WIE_MENUELEISTE: bool = cfg!(not(target_os = "macos"));
const SEITE: &str = if WIE_MENUELEISTE { "menueleiste.html" } else { "floating-timer.html" };

// Mac: Karte im Look des Dashboard-FloatingWidget, kompakt. Im Fenstermaß stecken 10px Rand pro Seite
// für den CSS-Schatten (das Fenster selbst ist durchsichtig). Linux: Maße des Menüleisten-Fensters;
// die Höhe meldet die Seite danach selbst (fenster.rs → timer_fenster_hoehe).
pub const BREITE: f64 = if WIE_MENUELEISTE { menueleiste::BREITE } else { 280.0 };
const HOEHE: f64 = if WIE_MENUELEISTE { menueleiste::HOEHE } else { 80.0 };
const RAND: f64 = 16.0;

struct Zustand {
  /// Solange das Menüleisten-Fenster offen ist (D-47), bleibt die Karte zu — sie läge rechts oben genau darüber.
  gesperrt: bool,
  /// Inaktiv-Lesungen in Folge, seit die Karte offen ist.
  inaktiv_gelesen: u32,
}

static ZUSTAND: Mutex<Zustand> = Mutex::new(Zustand { gesperrt: false, inaktiv_gelesen: 0 });

fn gesperrt() -> bool {
  ZUSTAND.lock().unwrap().gesperrt
}

fn fenster(app: &AppHandle) -> Option<WebviewWindow> {
  app.get_webview_window(LABEL)
}

fn offen(app: &AppHandle) -> Option<WebviewWindow> {
  fenster(app).filter(|fenster| fenster.is_visible().unwrap_or(false))
}

/// Oben rechts auf dem Bildschirm, auf dem der Nutzer gerade arbeitet.
fn setze_an_platz(app: &AppHandle, fenster: &WebviewWindow) {
  let zeiger = app.cursor_position().unwrap_or_default();
  let bildschirm = app.monitor_from_point(zeiger.x, zeiger.y).ok().flatten().or(app.primary_monitor().ok().flatten());
  if let Some(bildschirm) = bildschirm {
    let (flaeche, faktor) = (bildschirm.work_area(), bildschirm.scale_factor());
    let x = f64::from(flaeche.position.x) + f64::from(flaeche.size.width) - (BREITE + RAND) * faktor;
    let y = f64::from(flaeche.position.y) + RAND * faktor;
    let _ = fenster.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
  }
}

/// Reicht den Stand in die Karte. Die Menüleisten-Karte (Linux) liest den vollen Timer-Stand, die kompakte Karte den Schwebe-Stand.
fn schiebe(fenster: &WebviewWindow, schwebe: &SchwebeStand, stand: &TimerStand) {
  if WIE_MENUELEISTE {
    fenster::sende(fenster, stand);
  } else if let Ok(json) = serde_json::to_string(schwebe) {
    let _ = fenster.eval(format!("window.__update && window.__update({json})"));
  }
}

fn erzeuge(app: &AppHandle) -> tauri::Result<WebviewWindow> {
  let handle = app.clone();
  let fenster = fenster::kleines_fenster(app, LABEL, SEITE, BREITE, HOEHE, mac::EBENE_GANZ_OBEN, move |fenster| {
    // Seite geladen: aktuellen Stand hineinreichen, dann sichtbar werden.
    let (handle, fenster) = (handle.clone(), fenster.clone());
    tauri::async_runtime::spawn(async move {
      if WIE_MENUELEISTE {
        // Die Menüleisten-Karte hängt am Mac fest unter dem Symbol; als Schwebe-Karte lässt sie sich ziehen.
        let _ = fenster.eval("document.getElementById('karte')?.setAttribute('data-ziehen', '')");
      }
      let lesung = bruecke::lesen(&handle).await;
      schiebe(&fenster, &lesung.schwebe, &lesung.stand);
      mac::zeige_inaktiv(&fenster);
    });
  })?;
  setze_an_platz(app, &fenster);
  Ok(fenster)
}

/// Öffnet den Schwebe-Timer.
///
/// `force` = manueller Aufruf (Menü): öffnet auch ohne aktiven Timer und zeigt die Leerlauf-Karte.
/// Die Automatik ruft ohne `force` und öffnet nur, wenn das Frontend einen aktiven Timer meldet —
/// sonst würde bei jedem App-Wechsel ein Fenster aufpoppen.
pub async fn oeffnen(app: AppHandle, force: bool) {
  if !force && gesperrt() {
    return;
  }
  let lesung = bruecke::lesen(&app).await;
  let stand = &lesung.schwebe;
  if !force {
    // Während des Lesens kann sich die Lage geändert haben: Menüleisten-Fenster auf, oder der Nutzer ist schon zurück.
    let wieder_vorn = haupt::fenster(&app).is_some_and(|f| f.is_focused().unwrap_or(false));
    if gesperrt() || wieder_vorn {
      return;
    }
    if !stand.active {
      // Nur Dev: sichtbar machen, WARUM nichts aufgeht — fehlender Hook heißt
      // "Einstellung aus oder nicht eingeloggt", active=false heißt "kein Timer".
      if IS_DEV {
        println!("{LOG} übersprungen — hook={}, active={}", stand.hook_present, stand.active);
      }
      return;
    }
  }

  if let Some(fenster) = offen(&app) {
    schiebe(&fenster, stand, &lesung.stand);
    return;
  }
  ZUSTAND.lock().unwrap().inaktiv_gelesen = 0;
  match fenster(&app) {
    // Schon einmal erzeugt: frischen Stand hinein, an den Platz setzen, zeigen.
    Some(fenster) => {
      schiebe(&fenster, stand, &lesung.stand);
      setze_an_platz(&app, &fenster);
      mac::zeige_inaktiv(&fenster);
    }
    // Das neue Fenster zeigt sich selbst, sobald seine Seite geladen ist.
    None => {
      if let Err(fehler) = erzeuge(&app) {
        eprintln!("{LOG} Fenster ließ sich nicht öffnen: {fehler}");
        return;
      }
    }
  }
  if IS_DEV {
    println!("{LOG} geöffnet (hook={}, active={}, force={force})", stand.hook_present, stand.active);
  }
}

pub fn schliessen(app: &AppHandle) {
  if let Some(fenster) = offen(app) {
    if IS_DEV {
      println!("{LOG} geschlossen");
    }
    let _ = fenster.hide();
  }
}

/// Menüleisten-Fenster offen: Schwebe-Timer zu und gesperrt. Wieder zu: Automatik wie vorher —
/// ist PrepWell dann nicht vorn, kommt die Karte zurück.
pub fn sperre(app: &AppHandle, an: bool) {
  ZUSTAND.lock().unwrap().gesperrt = an;
  if an {
    schliessen(app);
  } else if haupt::fenster(app).is_some_and(|f| !f.is_focused().unwrap_or(true)) {
    tauri::async_runtime::spawn(oeffnen(app.clone(), false));
  }
}

/// Sekundentakt der Brücke: hält die offene Karte aktuell.
pub fn auf_takt(app: &AppHandle, stand: &SchwebeStand) {
  let Some(fenster) = offen(app) else { return };
  // Hysterese: Während Navigation/Neuladen im Hauptfenster fehlt der Hook kurz und die Lesung meldet "inaktiv".
  // Erst drei Inaktiv-Lesungen in Folge schalten die Karte auf Leerlauf — sonst flackert sie mitten in der
  // Sitzung auf "Kein Timer aktiv".
  {
    let mut zustand = ZUSTAND.lock().unwrap();
    if stand.active {
      zustand.inaktiv_gelesen = 0;
    } else {
      zustand.inaktiv_gelesen += 1;
      if zustand.inaktiv_gelesen < 3 {
        return;
      }
    }
  }
  // Den vollen Stand hat menueleiste::auf_stand im selben Takt gerade abgelegt (bruecke.rs → jetzt_lesen).
  schiebe(&fenster, stand, &menueleiste::stand());
}
