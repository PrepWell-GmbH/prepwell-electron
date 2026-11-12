// PrepWell Desktop — Tauri-Shell. Verteilt wird nur macOS; unter Linux läuft eine abgespeckte Fassung (siehe linux.rs).
// Lädt die Web-App (app.prepwell.de) in ein natives Fenster und ergänzt, was nur eine Desktop-App kann:
// Timer in der Menüleiste, Schwebe-Timer, Fenster beim Ablauf, Auto-Update.

mod ablauf;
mod anmeldung;
mod bruecke;
mod fenster;
mod haupt;
#[cfg(target_os = "macos")]
mod mac;
// Unter Linux stehen an derselben Stelle einfache Entsprechungen ohne NSPanel.
#[cfg(not(target_os = "macos"))]
#[path = "linux.rs"]
mod mac;
mod menueleiste;
#[cfg(debug_assertions)]
mod pruefstand;
mod schwebe;
mod update;

use tauri::RunEvent;
use tauri_plugin_window_state::StateFlags;

/// Dev-Lauf (`npm run dev`) — entspricht Electrons `!app.isPackaged`.
pub const IS_DEV: bool = cfg!(debug_assertions);

fn main() {
  #[cfg(not(target_os = "macos"))]
  mac::vor_dem_start();
  let app = tauri::Builder::default()
    // Nur eine Instanz: ein zweiter Start holt das vorhandene Fenster nach vorn.
    .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| haupt::hole_app_nach_vorn(app)))
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    // Größe und Position des Hauptfensters merken; die kleinen Timer-Fenster setzt die Shell selbst.
    .plugin(
      tauri_plugin_window_state::Builder::new()
        .with_state_flags(StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED | StateFlags::FULLSCREEN)
        .with_denylist(&[menueleiste::LABEL, ablauf::LABEL, schwebe::LABEL])
        .build(),
    )
    .invoke_handler(tauri::generate_handler![
      haupt::open_external,
      update::update_ui_ready,
      update::get_pending_update,
      update::install_update,
      fenster::timer_fenster_bereit,
      fenster::timer_befehl,
      fenster::app_oeffnen,
      fenster::timer_fenster_hoehe,
      fenster::timer_fenster_schliessen,
      fenster::fenster_ziehen,
    ])
    .on_menu_event(haupt::menue_ereignis)
    .setup(|app| {
      let app = app.handle();
      haupt::erlaube_dev_adresse(app)?;
      app.set_menu(haupt::menue(app)?)?;
      mac::kuerzel_nach_zeichen(haupt::MENUE_VERGROESSERN, "+");
      haupt::erzeuge_fenster(app)?;
      // Menüleiste (D-47) und Ablauf-Fenster (D-46) lesen den Timer-Stand über die Brücke.
      menueleiste::setup(app)?;
      bruecke::starte(app);
      update::setup(app);
      #[cfg(debug_assertions)]
      pruefstand::starte(app);
      Ok(())
    })
    .build(tauri::generate_context!())
    .expect("PrepWell konnte nicht starten");

  // `app` braucht nur der Mac-Zweig (Reopen).
  #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
  app.run(|app, ereignis| match ereignis {
    // Wie jede Mac-App: das letzte Fenster zu schließen beendet die App nicht (nur Cmd+Q tut das).
    #[cfg(target_os = "macos")]
    RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
    // Klick aufs Dock-Symbol: Hauptfenster neu erzeugen, falls es geschlossen wurde.
    #[cfg(target_os = "macos")]
    RunEvent::Reopen { .. } => {
      if haupt::fenster(app).is_none() {
        haupt::hole_app_nach_vorn(app);
      }
    }
    RunEvent::Exit => update::beim_beenden(),
    _ => {}
  });
}
