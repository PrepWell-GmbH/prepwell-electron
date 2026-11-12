// Timer in der Mac-Menüleiste (D-47): Symbol rechts oben, daneben die Restzeit, solange ein Timer läuft.
// Klick öffnet darunter ein kleines Fenster: Restzeit und Phase, Pause/Weiter/Beenden, Timer starten, „Anpassen" (App mit Timer-Fenster).

use std::{
  sync::Mutex,
  time::{Duration, Instant},
};

use tauri::{
  image::Image,
  tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent},
  AppHandle, Manager, PhysicalPosition, Rect, WebviewWindow, WindowEvent,
};

use crate::{bruecke::TimerStand, fenster, mac, schwebe};

pub const LABEL: &str = "menueleiste";
pub const BREITE: f64 = 300.0;
pub const HOEHE: f64 = 236.0;
const TRAY_ID: &str = "timer";
/// Ein Klick aufs Symbol nimmt dem offenen Fenster erst den Fokus (es schließt sich) und meldet sich dann als Klick.
/// Innerhalb dieser Spanne gilt der Klick als „zumachen", nicht als „wieder aufmachen".
const KLICK_NACH_SCHLIESSEN: Duration = Duration::from_millis(350);

struct Zustand {
  stand: Option<TimerStand>,
  titel: String,
  zuletzt_versteckt: Option<Instant>,
}

static ZUSTAND: Mutex<Zustand> = Mutex::new(Zustand { stand: None, titel: String::new(), zuletzt_versteckt: None });

/// Letzter gelesener Timer-Stand (für das frisch geöffnete Fenster).
pub fn stand() -> TimerStand {
  ZUSTAND.lock().unwrap().stand.clone().unwrap_or_default()
}

/// Text, der gerade neben dem Symbol steht (für den Prüfstand).
#[cfg(debug_assertions)]
pub fn titel_jetzt() -> String {
  ZUSTAND.lock().unwrap().titel.clone()
}

fn fenster(app: &AppHandle) -> Option<WebviewWindow> {
  app.get_webview_window(LABEL)
}

fn sichtbar(fenster: &WebviewWindow) -> bool {
  fenster.is_visible().unwrap_or(false)
}

/// Text neben dem Symbol.
fn titel(stand: &TimerStand) -> String {
  let Some(text) = stand.text.as_deref().filter(|_| stand.aktiv) else {
    return String::new();
  };
  // Kurz neben dem Symbol: „18min" statt „18min verbleibend" bzw. „18min gelernt" (Stoppuhr).
  let kurz = text.strip_suffix("verbleibend").or_else(|| text.strip_suffix("gelernt")).map_or(text, str::trim_end);
  if stand.pausiert {
    format!(" {kurz} ‖")
  } else {
    format!(" {kurz}")
  }
}

/// Wo das Fenster hingehört: mittig unter dem Symbol, innerhalb des Bildschirms.
fn position(app: &AppHandle, knopf: Option<Rect>) -> Option<PhysicalPosition<i32>> {
  let knopf = knopf.or_else(|| app.tray_by_id(TRAY_ID)?.rect().ok().flatten())?;
  let ort = knopf.position.to_physical::<f64>(1.0);
  let groesse = knopf.size.to_physical::<f64>(1.0);
  let bildschirm = app.monitor_from_point(ort.x, ort.y).ok().flatten().or(app.primary_monitor().ok().flatten())?;
  let (flaeche, faktor) = (bildschirm.work_area(), bildschirm.scale_factor());
  let links = f64::from(flaeche.position.x) + 4.0 * faktor;
  let rechts = f64::from(flaeche.position.x) + f64::from(flaeche.size.width) - (BREITE + 4.0) * faktor;
  let x = (ort.x + groesse.width / 2.0 - BREITE * faktor / 2.0).min(rechts).max(links);
  let y = ort.y + groesse.height + 2.0 * faktor;
  Some(PhysicalPosition::new(x.round() as i32, y.round() as i32))
}

fn erzeuge(app: &AppHandle) -> tauri::Result<WebviewWindow> {
  let fenster = fenster::kleines_fenster(app, LABEL, "menueleiste.html", BREITE, HOEHE, mac::EBENE_MENUE, |_| {})?;
  // Wie ein Menü: Klick daneben schließt.
  let handle = app.clone();
  fenster.on_window_event(move |ereignis| {
    if let WindowEvent::Focused(false) = ereignis {
      verstecken(&handle);
    }
  });
  Ok(fenster)
}

/// Blendet das Fenster aus und gibt den Schwebe-Timer wieder frei.
pub fn verstecken(app: &AppHandle) {
  let Some(fenster) = fenster(app).filter(sichtbar) else { return };
  let _ = fenster.hide();
  ZUSTAND.lock().unwrap().zuletzt_versteckt = Some(Instant::now());
  schwebe::sperre(app, false);
}

/// Klick aufs Symbol: Fenster auf bzw. zu. `knopf` ist die Lage des Symbols, falls der Aufrufer sie kennt.
pub fn umschalten(app: &AppHandle, knopf: Option<Rect>) {
  if fenster(app).as_ref().is_some_and(sichtbar) {
    verstecken(app);
    return;
  }
  if ZUSTAND.lock().unwrap().zuletzt_versteckt.is_some_and(|wann| wann.elapsed() < KLICK_NACH_SCHLIESSEN) {
    return;
  }
  schwebe::sperre(app, true);
  let fenster = match fenster(app).map_or_else(|| erzeuge(app), Ok) {
    Ok(fenster) => fenster,
    Err(fehler) => {
      eprintln!("[Menüleiste] Fenster ließ sich nicht öffnen: {fehler}");
      schwebe::sperre(app, false);
      return;
    }
  };
  if let Some(ort) = position(app, knopf) {
    let _ = fenster.set_position(ort);
  }
  fenster::sende(&fenster, &stand());
  mac::zeige_mit_fokus(&fenster);
}

/// Neuer Timer-Stand von der Brücke: Text neben dem Symbol und das offene Fenster nachführen.
pub fn auf_stand(app: &AppHandle, neu: &TimerStand) {
  let neuer_titel = titel(neu);
  let titel_geaendert = {
    let mut zustand = ZUSTAND.lock().unwrap();
    zustand.stand = Some(neu.clone());
    let geaendert = zustand.titel != neuer_titel;
    if geaendert {
      zustand.titel = neuer_titel.clone();
    }
    geaendert
  };
  if titel_geaendert {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
      let _ = tray.set_title(Some(neuer_titel));
    }
  }
  if let Some(fenster) = fenster(app).filter(sichtbar) {
    fenster::sende(&fenster, neu);
  }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
  // Linux: kein Symbol. GNOME zeigt ohne Erweiterung keine Tray-Symbole, und Text neben dem Symbol gibt es dort nicht.
  if cfg!(not(target_os = "macos")) {
    let _ = app;
    return Ok(());
  }
  // Das @2x-Bild: macOS zeichnet es auf Menüleisten-Höhe herunter, so bleibt es auf Retina scharf.
  let bild = Image::from_bytes(include_bytes!("../icons/trayTemplate@2x.png"))?;
  TrayIconBuilder::with_id(TRAY_ID)
    .icon(bild)
    .icon_as_template(true)
    .tooltip("PrepWell Timer")
    .show_menu_on_left_click(false)
    .on_tray_icon_event(|tray, ereignis| {
      // Links- wie Rechtsklick öffnen dasselbe Fenster.
      if let TrayIconEvent::Click { button_state: MouseButtonState::Up, rect, .. } = ereignis {
        umschalten(tray.app_handle(), Some(rect));
      }
    })
    .build(app)?;
  Ok(())
}
