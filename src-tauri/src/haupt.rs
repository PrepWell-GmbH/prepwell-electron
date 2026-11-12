// Hauptfenster: lädt die Web-App, hält sie auf ihrer Domain, öffnet alles andere im Browser. Dazu die Menüzeile der App.

use std::{
  collections::BTreeMap,
  path::{Path, PathBuf},
  sync::{Mutex, OnceLock},
};

use tauri::{
  menu::{Menu, MenuEvent, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder},
  webview::{DownloadEvent, NewWindowResponse, PageLoadEvent, PermissionResponse},
  AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent, Wry,
};
use tauri_plugin_opener::OpenerExt;

use crate::{anmeldung, menueleiste, schwebe, IS_DEV};

pub const LABEL: &str = "haupt";
const PROD_URL: &str = "https://app.prepwell.de";
const DEV_URL: &str = "http://localhost:3000";
/// Auth-Server der Web-App (Supabase hinter eigener Domain) und die Anmelde-Anbieter, zu denen er weiterleitet.
pub const AUTH_SERVER: &str = "api.prepwell.de";
const ANMELDE_ANBIETER: [&str; 2] = ["accounts.google.com", "appleid.apple.com"];
/// Was die Web-App als `electronAPI.platform` sieht (Electrons `process.platform`).
const PLATTFORM: &str = if cfg!(target_os = "macos") { "darwin" } else { "linux" };

/// Läuft im Hauptfenster vor jedem Skript der Seite und stellt `window.electronAPI` bereit.
const HAUPT_BRUECKE: &str = include_str!("../js/haupt-bruecke.js");

/// Die Befehle, die die Web-App rufen darf — dieselben wie in capabilities/haupt.json.
const RECHTE_DER_WEB_APP: [&str; 6] =
  ["allow-open-external", "allow-update-ui-ready", "allow-get-pending-update", "allow-install-update", "allow-fenster-ziehen", "allow-fenster-zoomen"];

/// Adresse der Web-App.
///
/// Dev-Override, um die Shell gegen eine andere Umgebung zu prüfen, ohne das Frontend auf :3000 zu starten:
///   PREPWELL_URL=https://app.prepwell.de npm run dev
/// Bewusst nur im Dev-Lauf wirksam — eine ausgelieferte App lässt sich damit nicht umlenken,
/// sonst wäre der Navigations-Wächter über eine Umgebungsvariable aushebelbar.
pub fn app_url() -> &'static Url {
  static ADRESSE: OnceLock<Url> = OnceLock::new();
  ADRESSE.get_or_init(|| {
    let dev = IS_DEV.then(|| std::env::var("PREPWELL_URL").ok().and_then(|adresse| Url::parse(&adresse).ok()).unwrap_or_else(|| Url::parse(DEV_URL).unwrap()));
    dev.unwrap_or_else(|| Url::parse(PROD_URL).unwrap())
  })
}

pub fn fenster(app: &AppHandle) -> Option<WebviewWindow> {
  app.get_webview_window(LABEL)
}

fn gleiche_herkunft(a: &Url, b: &Url) -> bool {
  a.scheme() == b.scheme() && a.host_str() == b.host_str() && a.port_or_known_default() == b.port_or_known_default()
}

/// Liegt die Adresse auf der Herkunft der Web-App?
pub fn ist_app_herkunft(url: &Url) -> bool {
  gleiche_herkunft(url, app_url())
}

/// Gehört die Adresse zur App? Geprüft wird die Herkunft, nicht der Textanfang —
/// `https://app.prepwell.de.evil.example` gehört nicht dazu.
fn gehoert_zur_app(url: &Url) -> bool {
  match url.scheme() {
    // Mitgelieferte Seiten (Start-/Offline-Seite).
    "tauri" => true,
    "about" => true,
    // blob:-Adressen tragen die Herkunft der Seite, die sie erzeugt hat.
    "blob" => Url::parse(url.path()).is_ok_and(|innen| gleiche_herkunft(&innen, app_url())),
    _ => gleiche_herkunft(url, app_url()),
  }
}

/// Stationen der Anmeldung über Google oder Apple, falls sie im Fenster läuft: Auth-Server → Anbieter → zurück
/// auf `/auth/oauth`. Der Normalfall ist der Browser des Nutzers (anmeldung.rs); das hier ist der Rückfall, wenn
/// der sich nicht starten ließ. Dann müssen alle Stationen im Fenster bleiben — einzeln in den Browser geschickt
/// bräche die Anmeldung ab.
fn gehoert_zur_anmeldung(url: &Url) -> bool {
  if url.scheme() != "https" {
    return false;
  }
  match url.host_str() {
    Some(AUTH_SERVER) => url.path().starts_with("/auth/v1/"),
    Some(host) => ANMELDE_ANBIETER.contains(&host) || ist_google_konten(host),
    None => false,
  }
}

/// Google setzt nach der Anmeldung seine Sitzung auch auf der Länder-Domain (z. B. `accounts.google.de/accounts/SetSID`)
/// und leitet erst danach zurück. Erlaubt sind `accounts.google.<land>` sowie `.co.<land>` und `.com.<land>` —
/// nicht `accounts.google.irgendwas.de`, das jedem gehören könnte.
fn ist_google_konten(host: &str) -> bool {
  let Some(rest) = host.strip_prefix("accounts.google.") else {
    return host == "accounts.youtube.com";
  };
  let land = |teil: &str| (2..=3).contains(&teil.len()) && teil.bytes().all(|b| b.is_ascii_lowercase());
  match rest.split_once('.') {
    None => land(rest),
    Some((zweite, oberste)) => matches!(zweite, "co" | "com") && land(oberste),
  }
}

/// Laufende Downloads: Adresse → Zieldatei.
static DOWNLOADS: Mutex<BTreeMap<String, PathBuf>> = Mutex::new(BTreeMap::new());

/// Zieldatei im Downloads-Ordner unter dem vorgeschlagenen Namen; ist er vergeben, mit „ (2)", „ (3)", …
fn download_ziel(app: &AppHandle, vorschlag: &Path) -> Option<PathBuf> {
  let ordner = app.path().download_dir().ok()?;
  let name = vorschlag.file_name().and_then(|n| n.to_str()).filter(|n| !n.is_empty()).unwrap_or("PrepWell-Download");
  Some(freier_name(&ordner, name))
}

fn freier_name(ordner: &Path, name: &str) -> PathBuf {
  let erster = ordner.join(name);
  if !erster.exists() {
    return erster;
  }
  let (stamm, endung) = match name.rsplit_once('.') {
    Some((stamm, endung)) if !stamm.is_empty() => (stamm, format!(".{endung}")),
    _ => (name, String::new()),
  };
  (2..).map(|nr| ordner.join(format!("{stamm} ({nr}){endung}"))).find(|pfad| !pfad.exists()).unwrap_or(erster)
}

fn im_browser_oeffnen(app: &AppHandle, url: &Url) {
  if matches!(url.scheme(), "http" | "https" | "mailto") {
    let _ = app.opener().open_url(url.as_str(), None::<&str>);
  }
}

/// Im Dev-Lauf darf zusätzlich die Dev-Adresse der Web-App die Befehle der electronAPI rufen.
pub fn erlaube_dev_adresse(app: &AppHandle) -> tauri::Result<()> {
  if !IS_DEV {
    return Ok(());
  }
  let adresse = app_url();
  let herkunft = match adresse.port() {
    Some(port) => format!("{}://{}:{port}", adresse.scheme(), adresse.host_str().unwrap_or_default()),
    None => format!("{}://{}", adresse.scheme(), adresse.host_str().unwrap_or_default()),
  };
  let rechte = serde_json::json!({
    "identifier": "haupt-dev",
    "windows": [LABEL],
    "remote": { "urls": [format!("{herkunft}/*")] },
    "permissions": RECHTE_DER_WEB_APP,
  });
  app.add_capability(rechte.to_string())
}

pub fn erzeuge_fenster(app: &AppHandle) -> tauri::Result<WebviewWindow> {
  erzeuge_fenster_auf(app, None)
}

/// Wie `erzeuge_fenster`, aber mit einer Seite der Web-App als erster Adresse statt der Startseite
/// (Rückkehr aus der Anmeldung im Browser, anmeldung.rs).
pub fn erzeuge_fenster_auf(app: &AppHandle, erste_adresse: Option<Url>) -> tauri::Result<WebviewWindow> {
  let (wache, neue_fenster) = (app.clone(), app.clone());
  let start = erste_adresse.filter(ist_app_herkunft).map_or_else(|| WebviewUrl::App("offline.html".into()), WebviewUrl::External);
  // Die Startseite (ui/offline.html) prüft die Verbindung und wechselt dann zur Web-App;
  // ohne Netz bleibt sie stehen und zeigt den Offline-Hinweis.
  let bauer = WebviewWindowBuilder::new(app, LABEL, start)
    .title("PrepWell")
    .inner_size(1200.0, 800.0)
    .min_inner_size(800.0, 600.0);
  // Die Web-App zeichnet ihre Kopfzeile unter die Ampel-Knöpfe; unter Linux bleibt die Titelleiste des Systems.
  #[cfg(target_os = "macos")]
  let bauer = bauer
    .title_bar_style(tauri::TitleBarStyle::Overlay)
    .hidden_title(true)
    .traffic_light_position(tauri::LogicalPosition::new(16.0, 16.0));
  let fenster = bauer
    .initialization_script(HAUPT_BRUECKE.replace("__APP_URL__", app_url().as_str()).replace("__PLATTFORM__", PLATTFORM))
    // Navigations-Wächter: das Fenster bleibt auf der App-Domain, alles andere geht in den Browser.
    // WebKit fragt hier auch für eingebettete Rahmen; die Web-App bettet per CSP nur die eigene Herkunft ein.
    .on_navigation(move |url| {
      // Anmeldung über Google/Apple läuft im Browser des Nutzers; nur wenn das scheitert, im Fenster.
      if anmeldung::ist_start(url) && anmeldung::starte(&wache, url) {
        return false;
      }
      if gehoert_zur_app(url) || gehoert_zur_anmeldung(url) {
        return true;
      }
      im_browser_oeffnen(&wache, url);
      false
    })
    // window.open / target=_blank: eigene Seiten dürfen ein Fenster öffnen, fremde Links gehen in den Browser.
    .on_new_window(move |url, _| {
      if gehoert_zur_app(&url) {
        return NewWindowResponse::Allow;
      }
      im_browser_oeffnen(&neue_fenster, &url);
      NewWindowResponse::Deny
    })
    // Alle Berechtigungs-Anfragen (Kamera, Mikrofon, …) werden verweigert. Standort weist haupt-bruecke.js ab.
    .on_permission_request(|_, art| {
      println!("[Berechtigungen] verweigert: {art:?}");
      PermissionResponse::Deny
    })
    // Downloads (Kalender-Export, …): ohne diesen Handler verpufft der Klick. Die Datei kommt in den
    // Downloads-Ordner und wird dort gezeigt — die App hat keine eigene Download-Anzeige.
    .on_download(|webview, ereignis| {
      match ereignis {
        DownloadEvent::Requested { url, destination } => {
          let Some(ziel) = download_ziel(webview.app_handle(), destination) else { return false };
          DOWNLOADS.lock().unwrap().insert(url.to_string(), ziel.clone());
          *destination = ziel;
        }
        DownloadEvent::Finished { url, path, success } => {
          // macOS nennt den Pfad hier nicht — deshalb der gemerkte.
          let gemerkt = DOWNLOADS.lock().unwrap().remove(url.as_str());
          match path.or(gemerkt).filter(|_| success) {
            Some(datei) => {
              let _ = webview.app_handle().opener().reveal_item_in_dir(datei);
            }
            None => eprintln!("[Download] fehlgeschlagen"),
          }
        }
        _ => {}
      }
      true
    })
    .on_page_load(|_, ereignis| {
      if IS_DEV && ereignis.event() == PageLoadEvent::Finished {
        println!("[Hauptfenster] geladen: {}", ereignis.url());
      }
    })
    .build()?;

  // Schwebe-Timer-Automatik (ADR-0070): Fokus weg → öffnen, Fokus zurück → schließen.
  // Minimieren und Verstecken nehmen dem Fenster ebenfalls den Fokus.
  let handle = app.clone();
  fenster.on_window_event(move |ereignis| match ereignis {
    WindowEvent::Focused(false) => {
      tauri::async_runtime::spawn(schwebe::oeffnen(handle.clone(), false));
    }
    WindowEvent::Focused(true) => schwebe::schliessen(&handle),
    WindowEvent::Destroyed => {
      schwebe::schliessen(&handle);
      // Ohne Dock und Menüleisten-Symbol gäbe es keinen Weg zurück: Hauptfenster zu heißt App zu.
      // (Die versteckten Timer-Fenster hielten sie sonst am Leben.)
      #[cfg(not(target_os = "macos"))]
      handle.exit(0);
    }
    _ => {}
  });

  #[cfg(debug_assertions)]
  fenster.open_devtools();

  Ok(fenster)
}

/// Hauptfenster nach vorn — aus Menüleiste und Ablauf-Fenster („PrepWell öffnen") und beim zweiten Start der App.
pub fn hole_app_nach_vorn(app: &AppHandle) {
  let Some(fenster) = fenster(app) else {
    if let Err(fehler) = erzeuge_fenster(app) {
      eprintln!("[Hauptfenster] ließ sich nicht öffnen: {fehler}");
    }
    return;
  };
  if fenster.is_minimized().unwrap_or(false) {
    let _ = fenster.unminimize();
  }
  let _ = fenster.show();
  let _ = fenster.set_focus();
}

/// Öffnet eine URL im Standardbrowser (Stripe, OAuth, …). Nur https — alles andere wird still verworfen.
#[tauri::command]
pub fn open_external(app: AppHandle, url: String) {
  if Url::parse(&url).is_ok_and(|adresse| adresse.scheme() == "https") {
    let _ = app.opener().open_url(url, None::<&str>);
  }
}

// ─── Menüzeile ──────────────────────────────────────────

static ZOOM: Mutex<f64> = Mutex::new(1.0);
const ZOOM_SCHRITT: f64 = 1.2;
/// Titel des Eintrags — main.rs legt sein Kürzel unter macOS auf die Plus-Taste (mac::kuerzel_nach_zeichen).
pub const MENUE_VERGROESSERN: &str = "Vergrößern";

pub fn menue(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
  let app_menue = SubmenuBuilder::new(app, "PrepWell")
    .item(&PredefinedMenuItem::about(app, Some("Über PrepWell"), None)?)
    .separator()
    .item(&PredefinedMenuItem::hide(app, Some("PrepWell ausblenden"))?)
    .item(&PredefinedMenuItem::hide_others(app, Some("Andere ausblenden"))?)
    .item(&PredefinedMenuItem::show_all(app, Some("Alle einblenden"))?)
    .separator()
    .item(&PredefinedMenuItem::quit(app, Some("PrepWell beenden"))?)
    .build()?;

  let bearbeiten = SubmenuBuilder::new(app, "Bearbeiten")
    .item(&PredefinedMenuItem::undo(app, Some("Rückgängig"))?)
    .item(&PredefinedMenuItem::redo(app, Some("Wiederholen"))?)
    .separator()
    .item(&PredefinedMenuItem::cut(app, Some("Ausschneiden"))?)
    .item(&PredefinedMenuItem::copy(app, Some("Kopieren"))?)
    .item(&PredefinedMenuItem::paste(app, Some("Einfügen"))?)
    .item(&PredefinedMenuItem::select_all(app, Some("Alles auswählen"))?)
    .build()?;

  let mut ansicht = SubmenuBuilder::new(app, "Ansicht")
    .item(&MenuItemBuilder::with_id("neu-laden", "Neu laden").accelerator("CmdOrCtrl+R").build(app)?)
    .item(&MenuItemBuilder::with_id("neu-laden-erzwingen", "Erzwingen Neu laden").accelerator("CmdOrCtrl+Shift+R").build(app)?)
    .separator()
    .item(&MenuItemBuilder::with_id("zoom-zurueck", "Zoom zurücksetzen").accelerator("CmdOrCtrl+0").build(app)?)
    .item(&MenuItemBuilder::with_id("zoom-groesser", MENUE_VERGROESSERN).accelerator("CmdOrCtrl+=").build(app)?)
    .item(&MenuItemBuilder::with_id("zoom-kleiner", "Verkleinern").accelerator("CmdOrCtrl+-").build(app)?)
    .separator()
    .item(&PredefinedMenuItem::fullscreen(app, Some("Vollbild"))?);
  // Nur im Dev-Lauf: Handgriffe zum Prüfen der Timer-Fenster, ohne Maus auf der Menüleiste.
  if IS_DEV {
    ansicht = ansicht
      .separator()
      .item(&MenuItemBuilder::with_id("schwebe-oeffnen", "Schwebe-Timer öffnen").accelerator("CmdOrCtrl+Shift+F").build(app)?)
      .item(&MenuItemBuilder::with_id("schwebe-schliessen", "Schwebe-Timer schließen").accelerator("CmdOrCtrl+Shift+G").build(app)?)
      .item(&MenuItemBuilder::with_id("menueleisten-timer", "Menüleisten-Timer öffnen").build(app)?)
      .separator()
      .item(&MenuItemBuilder::with_id("entwicklertools", "Entwicklertools").accelerator("Alt+CmdOrCtrl+I").build(app)?);
  }
  let ansicht = ansicht.build()?;

  let fenster = SubmenuBuilder::new(app, "Fenster")
    .item(&PredefinedMenuItem::minimize(app, Some("Minimieren"))?)
    .item(&PredefinedMenuItem::close_window(app, Some("Schließen"))?)
    .separator()
    .item(&PredefinedMenuItem::bring_all_to_front(app, Some("Alle nach vorne bringen"))?)
    .build()?;

  Menu::with_items(app, &[&app_menue, &bearbeiten, &ansicht, &fenster])
}

pub fn menue_ereignis(app: &AppHandle, ereignis: MenuEvent) {
  let zoome = |faktor: f64| {
    if let Some(fenster) = fenster(app) {
      *ZOOM.lock().unwrap() = faktor;
      let _ = fenster.set_zoom(faktor);
    }
  };
  let zoom = *ZOOM.lock().unwrap();
  match ereignis.id().as_ref() {
    "neu-laden" | "neu-laden-erzwingen" => {
      if let Some(fenster) = fenster(app) {
        let _ = fenster.reload();
      }
    }
    "zoom-zurueck" => zoome(1.0),
    "zoom-groesser" => zoome((zoom * ZOOM_SCHRITT).min(5.0)),
    "zoom-kleiner" => zoome((zoom / ZOOM_SCHRITT).max(0.25)),
    // Manuell öffnet auch ohne aktiven Timer (Leerlauf-Karte) — so ist das Schweben ohne Login prüfbar.
    "schwebe-oeffnen" => {
      tauri::async_runtime::spawn(schwebe::oeffnen(app.clone(), true));
    }
    "schwebe-schliessen" => schwebe::schliessen(app),
    // Wie ein Klick auf das Menüleisten-Symbol (D-47).
    "menueleisten-timer" => menueleiste::umschalten(app, None),
    #[cfg(debug_assertions)]
    "entwicklertools" => {
      if let Some(fenster) = fenster(app) {
        fenster.open_devtools();
      }
    }
    _ => {}
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn anmeldung_bleibt_im_fenster() {
    let ja = ["https://api.prepwell.de/auth/v1/callback?code=x", "https://accounts.google.com/v3/signin", "https://accounts.google.de/accounts/SetSID", "https://accounts.google.co.uk/x", "https://accounts.youtube.com/x", "https://appleid.apple.com/auth"];
    let nein = ["https://api.prepwell.de/rest/v1/x", "http://accounts.google.com/", "https://accounts.google.evil.de/", "https://accounts.google.com.evil.example/", "https://google.de/", "https://accounts.google.de.evil.example/"];
    for adresse in ja {
      assert!(gehoert_zur_anmeldung(&Url::parse(adresse).unwrap()), "{adresse}");
    }
    for adresse in nein {
      assert!(!gehoert_zur_anmeldung(&Url::parse(adresse).unwrap()), "{adresse}");
    }
  }
}
