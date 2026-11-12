// Jeder Befehl der Shell steht hier namentlich: nur so entstehen die Rechte `allow-<befehl>`,
// die capabilities/*.json den Fenstern einzeln zuteilen. Ohne diese Liste dürfte jede lokale Seite jeden Befehl rufen.
fn main() {
  tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
    tauri_build::AppManifest::new().commands(&[
      // Hauptfenster (Web-App)
      "open_external",
      "update_ui_ready",
      "get_pending_update",
      "install_update",
      // kleine Timer-Fenster
      "timer_fenster_bereit",
      "timer_befehl",
      "app_oeffnen",
      "timer_fenster_hoehe",
      "timer_fenster_schliessen",
      "fenster_ziehen",
    ]),
  ))
  .expect("tauri-build fehlgeschlagen");
}
