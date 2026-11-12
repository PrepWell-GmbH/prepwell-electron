// Auto-Update.
// Ein fertig geladenes Update wird dem Nutzer ANGEBOTEN, statt still beim Beenden eingespielt zu werden.
// Der Hinweis erscheint bewusst erst nach dem Download — der Klick landet dann sofort im neuen Stand.
//
// Zwei Wege für die Anzeige:
//   1. Das Frontend zeigt den Hinweis selbst (meldet sich per `update_ui_ready`).
//   2. Meldet sich niemand, zeigt die Shell einen nativen Dialog.
// So funktioniert das Update-Angebot auch gegen ein Frontend, das die Update-UI noch nicht kennt, und auf der Offline-Seite.
// Wer den Hinweis wegklickt, bekommt das Update beim Beenden der App.

use std::{sync::Mutex, time::Duration};

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{haupt, IS_DEV};

const LOG: &str = "[Updater]";
/// Stündlich nachfassen.
const PRUEF_ABSTAND: Duration = Duration::from_secs(60 * 60);
/// Karenz, damit ein noch ladendes Frontend die Anzeige übernehmen kann.
const KARENZ_FRONTEND: Duration = Duration::from_secs(10);

/// Ein heruntergeladenes, geprüftes Update, das auf die Installation wartet.
struct Bereit {
  update: Update,
  daten: Vec<u8>,
}

struct Zustand {
  /// Version des heruntergeladenen Updates, sonst None.
  version: Option<String>,
  bereit: Option<Bereit>,
  /// Wird true, sobald das Frontend die Update-Anzeige übernommen hat.
  frontend_zeigt: bool,
}

static ZUSTAND: Mutex<Zustand> = Mutex::new(Zustand { version: None, bereit: None, frontend_zeigt: false });

fn bereite_version() -> Option<String> {
  ZUSTAND.lock().unwrap().version.clone()
}

pub fn setup(app: &AppHandle) {
  // Kein Auto-Update im Dev-Lauf. Unter Linux gibt es kein Update-Paket — dort wird von Hand neu gebaut.
  if IS_DEV || cfg!(not(target_os = "macos")) {
    return;
  }
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    loop {
      if let Err(fehler) = pruefe(&app).await {
        eprintln!("{LOG} Fehler: {fehler}");
      }
      tokio::time::sleep(PRUEF_ABSTAND).await;
    }
  });
}

/// Fragt nach einem Update und lädt es im Hintergrund herunter.
async fn pruefe(app: &AppHandle) -> tauri_plugin_updater::Result<()> {
  let Some(update) = app.updater()?.check().await? else {
    return Ok(());
  };
  if bereite_version().as_deref() == Some(update.version.as_str()) {
    return Ok(()); // liegt schon bereit
  }
  println!("{LOG} Version {} verfügbar — lädt im Hintergrund...", update.version);
  let daten = update.download(|_, _| {}, || {}).await?;
  let version = update.version.clone();
  {
    let mut zustand = ZUSTAND.lock().unwrap();
    zustand.version = Some(version.clone());
    zustand.bereit = Some(Bereit { update, daten });
  }
  println!("{LOG} Version {version} bereit.");
  melde(app, version).await;
  Ok(())
}

/// Meldet ein bereitliegendes Update ans Frontend — oder zeigt den nativen Dialog.
async fn melde(app: &AppHandle, version: String) {
  if sende_ans_frontend(app, &version) {
    return;
  }
  // Das Update ist erst nach dem Download fertig, das Frontend also längst geladen.
  // Trotzdem kurz warten, falls gerade neu geladen wird.
  tokio::time::sleep(KARENZ_FRONTEND).await;
  if sende_ans_frontend(app, &version) {
    return;
  }
  zeige_dialog(app, &version);
}

fn sende_ans_frontend(app: &AppHandle, version: &str) -> bool {
  if !ZUSTAND.lock().unwrap().frontend_zeigt {
    return false;
  }
  let Some(fenster) = haupt::fenster(app) else {
    return false;
  };
  let version = serde_json::Value::String(version.into());
  fenster.eval(format!("window.__prepwellUpdateBereit && window.__prepwellUpdateBereit({version})")).is_ok()
}

fn zeige_dialog(app: &AppHandle, version: &str) {
  let Some(fenster) = haupt::fenster(app) else {
    return;
  };
  let handle = app.clone();
  app
    .dialog()
    .message(format!("Version {version} steht bereit. PrepWell startet dafür kurz neu."))
    .title("Eine neuere Version ist vorhanden")
    .kind(MessageDialogKind::Info)
    .buttons(MessageDialogButtons::OkCancelCustom("Jetzt aktualisieren".into(), "Später".into()))
    .parent(&fenster)
    .show(move |jetzt| {
      if jetzt {
        installiere(&handle);
      }
    });
}

/// Spielt das bereitliegende Update ein und startet die App neu. Liefert false, wenn keines bereitliegt.
fn installiere(app: &AppHandle) -> bool {
  let Some(bereit) = ZUSTAND.lock().unwrap().bereit.take() else {
    return false;
  };
  let app = app.clone();
  tauri::async_runtime::spawn_blocking(move || match bereit.update.install(&bereit.daten) {
    Ok(()) => app.restart(),
    Err(fehler) => {
      eprintln!("{LOG} Installation fehlgeschlagen: {fehler}");
      // Zurücklegen: der nächste Versuch (Klick oder Beenden) hat das Update noch.
      ZUSTAND.lock().unwrap().bereit = Some(bereit);
    }
  });
  true
}

/// Beim Beenden der App: ein liegengebliebenes Update einspielen (ohne Neustart).
pub fn beim_beenden() {
  let Some(bereit) = ZUSTAND.lock().unwrap().bereit.take() else {
    return;
  };
  if let Err(fehler) = bereit.update.install(&bereit.daten) {
    eprintln!("{LOG} Installation beim Beenden fehlgeschlagen: {fehler}");
  }
}

// ─── Befehle der Web-App (electronAPI) ──────────────────

/// Das Frontend übernimmt die Anzeige — ab jetzt kein nativer Dialog mehr.
/// Liegt schon ein Update bereit (z. B. weil es während eines Neuladens fertig wurde), kommt dessen Version zurück.
#[tauri::command]
pub fn update_ui_ready() -> Option<String> {
  let mut zustand = ZUSTAND.lock().unwrap();
  zustand.frontend_zeigt = true;
  zustand.version.clone()
}

/// Für den Fall, dass die Komponente erst nach der Meldung mountet — dann fragt sie den Stand aktiv ab.
#[tauri::command]
pub fn get_pending_update() -> Option<String> {
  bereite_version()
}

/// Klick auf „Jetzt aktualisieren". Nur wirksam, wenn wirklich ein Update bereitliegt.
#[tauri::command]
pub fn install_update(app: AppHandle) -> bool {
  installiere(&app)
}
